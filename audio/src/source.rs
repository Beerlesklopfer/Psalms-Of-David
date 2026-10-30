// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads an audio file and yields decoded interleaved f32 samples (symphonia).

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

use crate::Error;
use crate::stream::{ByteQueue, QueueReader};

/// The message for a stream that is not MP3 (AAC, Ogg, HLS playlists, a web page ...).
/// The app maps this exact text to a localized one.
pub const STREAM_UNSUPPORTED: &str = "stream type not supported";

pub struct Source {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::audio::AudioDecoder>,
    track_id: u32,
    scratch: Vec<f32>,
    /// Sample rate and channel count of the file, as the first block reported them.
    pub rate: u32,
    pub channels: u32,
    /// Length of the file if the container says so (an MP3 without a header may not).
    pub duration: Option<Duration>,
}

impl Source {
    /// Opens `path` and starts at `start`. Fails if the file cannot be read or has
    /// no audio. The first block is already decoded, so rate and channels are known.
    pub fn open(path: &Path, start: Duration) -> Result<(Source, Vec<f32>), Error> {
        let file = Box::new(File::open(path)?);
        let mss = MediaSourceStream::new(file, Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| Error::Decode(format!("{}: {e}", path.display())))?;
        Self::start(format, start)
    }

    /// Opens a pushed stream (MP3). Blocks until the first block is decoded, so the
    /// caller must run it on a thread of its own; `abort` ends the wait.
    pub fn open_stream(queue: Arc<ByteQueue>, abort: Arc<AtomicBool>) -> Result<(Source, Vec<f32>), Error> {
        let mss = MediaSourceStream::new(Box::new(QueueReader::new(queue.clone(), abort)), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("mp3").mime_type("audio/mpeg");
        let format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| match queue.failure() {
                // The probe may hide a failed connection behind "no format found".
                Some(reason) => Error::Decode(reason),
                None => stream_error(e),
            })?;
        Self::start(format, Duration::ZERO)
    }

    fn start(mut format: Box<dyn FormatReader>, start: Duration) -> Result<(Source, Vec<f32>), Error> {
        let track = format.default_track(TrackType::Audio).ok_or_else(|| Error::Decode("no audio track".into()))?;
        let track_id = track.id;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| Error::Decode("no audio parameters".into()))?;
        let header_rate = params.sample_rate;
        let num_frames = track.num_frames;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|e| Error::Decode(format!("no decoder: {e}")))?;
        let duration = match (num_frames, header_rate) {
            (Some(frames), Some(rate)) if rate > 0 => Some(Duration::from_secs_f64(frames as f64 / rate as f64)),
            _ => None,
        };

        if !start.is_zero() {
            let time = Time::try_from_secs_f64(start.as_secs_f64())
                .ok_or_else(|| Error::Decode("cannot seek to that time".into()))?;
            format
                .seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(track_id) })
                .map_err(|e| Error::Decode(format!("seek failed: {e}")))?;
        }

        let mut source = Source { format, decoder, track_id, scratch: Vec::new(), rate: 0, channels: 0, duration };
        if !start.is_zero() {
            source.decoder.reset();
        }
        let first = source.next_samples()?.ok_or_else(|| Error::Decode("no audio at that position".into()))?;
        Ok((source, first))
    }

    /// Next block of interleaved samples, or `None` at the end of the file.
    pub fn next_samples(&mut self) -> Result<Option<Vec<f32>>, Error> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(e) => return Err(Error::Decode(e.to_string())),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    self.rate = buf.spec().rate();
                    self.channels = buf.spec().channels().count() as u32;
                    self.scratch.resize(buf.samples_interleaved(), f32::MID);
                    buf.copy_to_slice_interleaved(&mut self.scratch);
                    if self.scratch.is_empty() {
                        continue;
                    }
                    return Ok(Some(self.scratch.clone()));
                }
                // A damaged frame is skipped, like most players do.
                Err(DecodeError::DecodeError(_)) => continue,
                Err(e) => return Err(Error::Decode(e.to_string())),
            }
        }
    }
}

/// A failure while opening a stream: the queue's own reason if the connection
/// failed, otherwise the data was not something this engine can play.
fn stream_error(e: DecodeError) -> Error {
    match e {
        DecodeError::IoError(io) => Error::Decode(io.to_string()),
        _ => Error::Decode(STREAM_UNSUPPORTED.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::thread;

    /// A 3 s stereo 44.1 kHz sine at 128 kbit/s, made by ffmpeg (as the scripts do).
    /// `None` if ffmpeg is not installed; the test is then skipped with a note.
    fn mp3() -> Option<Vec<u8>> {
        let out = Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=3:sample_rate=44100", "-ac", "2", "-b:a", "128k", "-f", "mp3", "-"])
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)
    }

    fn drain(queue: &Arc<ByteQueue>, abort: &Arc<AtomicBool>) -> Result<(u64, u32, u32), Error> {
        let (mut source, first) = Source::open_stream(queue.clone(), abort.clone())?;
        let mut frames = (first.len() / source.channels as usize) as u64;
        while let Some(block) = source.next_samples()? {
            frames += (block.len() / source.channels as usize) as u64;
        }
        Ok((frames, source.rate, source.channels))
    }

    /// Pushes `data` in pieces of `piece` bytes the way a network connection would,
    /// waiting when the queue is full.
    fn feed(queue: Arc<ByteQueue>, data: Vec<u8>, piece: usize) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            for chunk in data.chunks(piece) {
                let mut rest = chunk;
                while !rest.is_empty() {
                    let n = queue.push(rest).unwrap();
                    rest = &rest[n..];
                    if n == 0 {
                        thread::sleep(Duration::from_millis(1));
                    }
                }
            }
            queue.end();
        })
    }

    #[test]
    fn an_mp3_pushed_in_small_pieces_decodes_completely() {
        let Some(data) = mp3() else { return eprintln!("skipped: ffmpeg not found") };
        let queue = ByteQueue::new(4096); // small: forces back pressure
        let abort = Arc::new(AtomicBool::new(false));
        let writer = feed(queue.clone(), data, 313);
        let (frames, rate, channels) = drain(&queue, &abort).expect("decodes");
        writer.join().unwrap();
        assert_eq!((rate, channels), (44100, 2));
        let seconds = frames as f64 / rate as f64;
        assert!((2.8..=3.2).contains(&seconds), "about 3 s decoded, got {seconds}");
    }

    #[test]
    fn a_stream_that_starts_in_the_middle_of_a_frame_decodes() {
        let Some(data) = mp3() else { return eprintln!("skipped: ffmpeg not found") };
        // Radio streams are joined wherever the broadcast is: cut the head off.
        let queue = ByteQueue::new(1 << 20);
        queue.push(&data[1000..]).unwrap();
        queue.end();
        let (frames, rate, _) = drain(&queue, &Arc::new(AtomicBool::new(false))).expect("decodes");
        assert!(frames as f64 / rate as f64 > 2.0);
    }

    #[test]
    fn a_gap_in_the_stream_after_a_reconnect_is_survived() {
        let Some(data) = mp3() else { return eprintln!("skipped: ffmpeg not found") };
        // First part, then the new connection continues somewhere else (bytes lost).
        let queue = ByteQueue::new(1 << 20);
        queue.push(&data[..20_000]).unwrap();
        queue.push(&data[20_777..]).unwrap();
        queue.end();
        let (frames, rate, _) = drain(&queue, &Arc::new(AtomicBool::new(false))).expect("decodes");
        assert!(frames as f64 / rate as f64 > 2.0, "most of the audio is still there");
    }

    #[test]
    fn something_that_is_not_mp3_is_called_unsupported() {
        for body in [&b"<html><body>Not a stream</body></html>"[..], &[0xFF, 0xF1, 0x50, 0x80, 0x1F, 0xFC][..], &[7u8; 5000][..]] {
            let queue = ByteQueue::new(1 << 20);
            queue.push(body).unwrap();
            queue.end();
            let err = Source::open_stream(queue, Arc::new(AtomicBool::new(false))).err().expect("must fail");
            assert_eq!(err.to_string(), format!("decode: {STREAM_UNSUPPORTED}"));
        }
    }

    #[test]
    fn a_failing_connection_reports_its_reason() {
        let queue = ByteQueue::new(1 << 20);
        queue.fail("HTTP 404");
        let err = Source::open_stream(queue, Arc::new(AtomicBool::new(false))).err().expect("must fail");
        assert!(err.to_string().contains("HTTP 404"), "{err}");
    }

    #[test]
    fn opening_ends_when_told_to_and_does_not_hang() {
        let queue = ByteQueue::new(1 << 20);
        let abort = Arc::new(AtomicBool::new(false));
        let opener = {
            let (q, a) = (queue.clone(), abort.clone());
            thread::spawn(move || Source::open_stream(q, a).err().map(|e| e.to_string()))
        };
        thread::sleep(Duration::from_millis(150));
        abort.store(true, std::sync::atomic::Ordering::Release);
        assert!(opener.join().unwrap().is_some());
        queue.close();
    }

    /// A 3 s stereo sine in the container `ext` made by ffmpeg with the encoder `codec`; `None` if ffmpeg cannot make it.
    fn encoded(ext: &str, codec: &str) -> Option<std::path::PathBuf> {
        let path = std::env::temp_dir().join(format!("psalm-audio-test-{}-{codec}.{ext}", std::process::id()));
        let ok = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3:sample_rate=44100", "-ac", "2", "-c:a", codec])
            .arg(&path)
            .status()
            .ok()?
            .success();
        ok.then_some(path)
    }

    #[test]
    fn the_formats_of_a_music_library_play_not_only_mp3() {
        // Ogg Vorbis (the Duo Camillo albums), FLAC, WAV and AAC in MP4 decode through the same path as MP3 files.
        for (ext, codec) in [("ogg", "libvorbis"), ("flac", "flac"), ("wav", "pcm_s16le"), ("m4a", "aac")] {
            let Some(path) = encoded(ext, codec) else { return eprintln!("skipped: ffmpeg cannot make {ext}") };
            let (mut source, first) = Source::open(&path, Duration::ZERO).unwrap_or_else(|e| panic!("{ext}: {e}"));
            let mut frames = (first.len() / source.channels as usize) as u64;
            while let Some(block) = source.next_samples().unwrap() {
                frames += (block.len() / source.channels as usize) as u64;
            }
            let seconds = frames as f64 / source.rate as f64;
            assert!((2.8..=3.3).contains(&seconds), "{ext}: about 3 s decoded, got {seconds}");
            let _ = std::fs::remove_file(&path);
        }
    }
}
