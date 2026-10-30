// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The PipeWire output: the engine thread is a PipeWire main loop; commands arrive through a
//! `pw::channel`, a timer calls `Core::tick` every 50 ms, and the `process` closure of the core is
//! called from the stream's real-time callback.

use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use pipewire as pw;
use pw::core::CoreRc;
use pw::main_loop::MainLoopRc;
use pw::properties::properties;
use pw::spa;
use pw::stream::{StreamFlags, StreamListener, StreamRc};
use spa::param::audio::{AudioFormat, AudioInfoRaw, MAX_CHANNELS};
use spa::pod::Pod;

use super::{CommandSink, OutStream, Output, ProcessFn, Started, StreamFactory, StreamSpec};
use crate::Error;
use crate::core::Core;
use crate::engine::{Command, Shared};

pub struct PipewireOutput {
    pub target: Option<String>,
    pub role: Option<String>,
}

impl Output for PipewireOutput {
    fn start(self: Box<Self>, shared: Arc<Shared>) -> Result<Started, Error> {
        let (tx, rx) = pw::channel::channel::<Command>();
        let sink: CommandSink = Arc::new(move |cmd| tx.send(cmd).is_ok());
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = {
            let sink = sink.clone();
            thread::Builder::new()
                .name("psalm-pipewire".into())
                .spawn(move || run(*self, rx, shared, sink, ready_tx))
                .map_err(|e| Error::PipeWire(format!("cannot start the audio thread: {e}")))?
        };
        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Started { sink, thread }),
            Ok(Err(e)) => Err(Error::PipeWire(e)),
            Err(_) => Err(Error::PipeWire("PipeWire did not answer".into())),
        }
    }
}

fn run(options: PipewireOutput, rx: pw::channel::Receiver<Command>, shared: Arc<Shared>, sink: CommandSink, ready: mpsc::Sender<Result<(), String>>) {
    pw::init();
    let setup = || -> Result<(MainLoopRc, pw::context::ContextRc, CoreRc), pw::Error> {
        let mainloop = MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        Ok((mainloop, context, core))
    };
    let (mainloop, _context, pw_core) = match setup() {
        Ok(parts) => parts,
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return;
        }
    };
    let role = options.role.or_else(|| std::env::var("PSALM_AUDIO_ROLE").ok()).unwrap_or_else(|| "Music".to_owned());
    let factory = Box::new(PwFactory { core: pw_core, target: options.target, role });
    let core = Rc::new(Core::new(factory, shared, sink));

    let handler = core.clone();
    let quit = mainloop.clone();
    let _attached = rx.attach(mainloop.loop_(), move |cmd| {
        if matches!(cmd, Command::Shutdown) {
            handler.shutdown();
            quit.quit();
        } else {
            handler.handle(cmd);
        }
    });
    let ticker = core.clone();
    let timer = mainloop.loop_().add_timer(move |_| ticker.tick());
    let _ = timer.update_timer(Some(Duration::from_millis(50)), Some(Duration::from_millis(50)));

    let _ = ready.send(Ok(()));
    mainloop.run();
    core.shutdown();
}

struct PwFactory {
    core: CoreRc,
    target: Option<String>,
    role: String,
}

/// What the real-time callback owns.
struct PwRt {
    process: ProcessFn,
    channels: usize,
    drained: Arc<std::sync::atomic::AtomicBool>,
    /// Only used if PipeWire hands over a buffer that is not aligned for f32 (it does not, in practice).
    scratch: Vec<f32>,
}

struct PwStream {
    stream: StreamRc,
    _listener: StreamListener<PwRt>,
    /// Reports volume changes from outside. A listener of its own with its own data,
    /// because `process` runs on the real-time thread and must not share a `&mut` with it.
    _controls: StreamListener<Arc<std::sync::atomic::AtomicU32>>,
}

impl OutStream for PwStream {
    fn set_active(&self, active: bool) {
        let _ = self.stream.set_active(active);
    }
    fn flush(&self) {
        let _ = self.stream.flush(true);
    }
    fn set_volume(&self, volume: f32) {
        let _ = self.stream.set_control(spa::sys::SPA_PROP_volume, &[volume]);
    }
    fn disconnect(&self) {
        let _ = self.stream.disconnect();
    }
}

impl StreamFactory for PwFactory {
    fn open(&self, spec: StreamSpec, process: ProcessFn) -> Result<Box<dyn OutStream>, Error> {
        let mut props = properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => self.role.as_str(),
            *pw::keys::APP_NAME => "Psalms",
            *pw::keys::NODE_NAME => "psalm",
            *pw::keys::MEDIA_NAME => spec.media_name.as_str(),
        };
        if let Some(target) = &self.target {
            props.insert(*pw::keys::TARGET_OBJECT, target.as_str());
        }
        let stream = StreamRc::new(self.core.clone(), "psalm", props)?;

        let rt = PwRt { process, channels: spec.channels as usize, drained: spec.drained.clone(), scratch: Vec::new() };
        let listener = stream
            .add_local_listener_with_user_data(rt)
            .process(process_buffer)
            .drained(|_, rt| rt.drained.store(true, Ordering::Release))
            .register()?;

        let controls = stream
            .add_local_listener_with_user_data(spec.volume_report.clone())
            .control_info(|_stream, report, id, control| {
                if id != spa::sys::SPA_PROP_volume || control.is_null() {
                    return;
                }
                // SAFETY: PipeWire passes a valid control for the duration of the call.
                let control = unsafe { &*control };
                if control.n_values > 0 && !control.values.is_null() {
                    // SAFETY: `values` holds `n_values` floats.
                    let volume = unsafe { *control.values };
                    report.store(volume.to_bits(), Ordering::Release);
                }
            })
            .register()?;

        let values = format_pod(spec.rate, spec.channels)?;
        let mut params = [Pod::from_bytes(&values).ok_or_else(|| Error::PipeWire("bad format pod".into()))?];
        let mut flags = StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS;
        if !spec.active {
            flags |= StreamFlags::INACTIVE;
        }
        stream.connect(spa::utils::Direction::Output, None, flags, &mut params)?;
        // Only a volume that the caller has set is applied. Otherwise the system decides.
        if let Some(volume) = spec.volume {
            let _ = stream.set_control(spa::sys::SPA_PROP_volume, &[volume]);
        }
        Ok(Box::new(PwStream { stream, _listener: listener, _controls: controls }))
    }
}

/// The real-time callback: let the core fill the PipeWire buffer.
fn process_buffer(stream: &pw::stream::Stream, rt: &mut PwRt) {
    let Some(mut buffer) = stream.dequeue_buffer() else { return };
    let datas = buffer.datas_mut();
    let data = &mut datas[0];
    let stride = rt.channels * std::mem::size_of::<f32>();
    let mut frames = 0;
    if let Some(slice) = data.data() {
        let capacity = slice.len() / stride;
        // SAFETY: every bit pattern is a valid f32; `align_to_mut` only returns the aligned middle part.
        let (head, floats, _) = unsafe { slice.align_to_mut::<f32>() };
        if head.is_empty() && floats.len() >= capacity * rt.channels {
            frames = (rt.process)(&mut floats[..capacity * rt.channels]).frames;
        } else {
            rt.scratch.resize(capacity * rt.channels, 0.0);
            frames = (rt.process)(&mut rt.scratch).frames;
            #[allow(clippy::chunks_exact_to_as_chunks)]
            for (dst, src) in slice.chunks_exact_mut(4).zip(rt.scratch.iter()) {
                dst.copy_from_slice(&src.to_le_bytes());
            }
        }
    }
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = stride as _;
    *chunk.size_mut() = (frames * stride) as _;
}

/// The format the stream offers: the file's own rate and channel count, `F32LE`.
/// PipeWire converts to what the device wants.
fn format_pod(rate: u32, channels: u32) -> Result<Vec<u8>, Error> {
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_rate(rate);
    info.set_channels(channels);
    let mut position = [0; MAX_CHANNELS];
    match channels {
        1 => position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO,
        _ => {
            position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
            position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
        }
    }
    info.set_position(position);
    pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        }),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .map_err(|e| Error::PipeWire(format!("cannot build the format: {e:?}")))
}
