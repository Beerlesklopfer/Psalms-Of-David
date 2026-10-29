# Spec: audio output on Linux (PipeWire)

Status: design plus a working engine (`audio/`): load, play, pause, seek, volume and position, checked against recordings of a null sink (spike 1 and 2), a C interface and a Dart backend over `dart:ffi` that drives it (spike 3). Written after reading the `pipewire` 0.10.1 and `libspa` 0.10.1 sources and the PipeWire tutorial 4. Items marked **unverified** have not been tried yet; the results of spike 1 are at the end.

Scope: Linux only, PipeWire only (decision of the project owner). Android and iOS use `just_audio`, Windows is undecided. All of them sit behind one Dart interface, `AudioBackend`.

## What PipeWire expects from a playback client

- A stream is created with properties (`media.type=Audio`, `media.category=Playback`, `media.role=Music`) and connected with `Direction::Output`. The session manager (WirePlumber) links it to a sink.
- The client declares the format it produces (sample format, rate, channels, channel positions) as an `EnumFormat` param. The server converts to the device format, so the client does not need to resample.
- **Pull model.** PipeWire calls the `process` callback whenever it needs data. In it the client dequeues a buffer, writes frames into `datas[0]`, sets `chunk.offset/stride/size`, and the buffer is queued again when it is dropped (the Rust `Buffer` does this in `Drop`).
- With `StreamFlags::RT_PROCESS` the callback runs on the real-time thread. It must not allocate, lock, or touch files. Without the flag it runs on the loop thread, which is safer but adds jitter.
- `StreamFlags::MAP_BUFFERS` is needed to get a writable slice from `Data::data()`.
- A stream has a state (`state_changed` callback) and can be paused with `set_active(false)`; `flush(drain)` discards queued data (and calls `drained` when everything has been played).

## Consequences for the design

1. **The decoder cannot run in `process`.** Decoding MP3 allocates and reads files. So three threads cooperate:
   - **Decoder thread**: reads the file with `symphonia`, produces interleaved samples, pushes them into a lock-free single-producer/single-consumer ring buffer.
   - **PipeWire loop thread**: owns main loop, context, core and stream. `process` only copies from the ring buffer into the PipeWire buffer. If the ring is empty it writes silence and counts an underrun.
   - **Control side** (any thread, e.g. Dart through FFI): sends commands to the loop thread with `pipewire::channel` and reads state from atomics.
2. **Format.** Declare the file's own rate and channel count (the ripped audiobooks are 24 kHz stereo MP3) and let PipeWire resample. Use `F32LE` interleaved, which is what `symphonia` produces most naturally. Channel positions: FL/FR for stereo, MONO for mono.
3. **A change of rate or channels between tracks** needs a new format. Simplest rule: recreate the stream when the format differs; keep it when it does not (gapless inside one audiobook).
4. **Position.** The playback position is not "frames written". Use `Stream::time()` (RT-safe): `ticks` is the graph position at `now`, `delay` is the device delay, `queued` the bytes still queued. Position of the sample being heard ≈ frames pushed − (queued frames + device delay in frames). Extrapolate `ticks` with `pw_stream_get_nsec() - now` when a fresh value is needed. Sample the value on a timer in the loop thread and publish it in an atomic.
5. **Pause and resume**: `set_active(false)` / `set_active(true)`. **Unverified**: whether the link survives and how quickly audio resumes.
6. **Seek**: tell the decoder to seek (`symphonia` `FormatReader::seek`), discard what is already in the ring buffer and in the stream (`flush(false)`), then continue. The ring buffer must not mix old and new samples; carry a generation number with each chunk, or rebuild the ring on seek. **Unverified**: which of the two is simpler and glitch-free.
7. **Volume** through the stream control (`Stream::set_control`, property `volume`) instead of multiplying samples. Then the volume shows up in `pavucontrol` and `wpctl` and changes made there arrive through `control_info`. **Unverified** exact control ids.
8. **Output device choice** with the property `target.object` (node name or serial). AirPlay (RAOP) sinks, Bluetooth and HDMI show up as nodes, so listing and selecting them is the same code.
9. **Speed without pitch change** (1.5× for audiobooks) is not provided by PipeWire. It needs a time-stretch step in the decoder thread (WSOLA or a library such as Rubber Band or Signalsmith Stretch). Version 1 plays at 1×; the UI keeps the speed chips for the other platforms.

## Stream properties

| Key | Value | Why |
|---|---|---|
| `media.type` | `Audio` | required |
| `media.category` | `Playback` | routing |
| `media.role` | `Music` | WirePlumber policies (for example ducking) use the role |
| `application.name` / `app.name` | `Psalm` | name in mixers |
| `node.name` | `psalm` | stable name for scripts and tests |
| `media.name` | `<title> - <author>` | what `pavucontrol` shows as the stream |
| `media.title`, `media.artist` | from the track | used by some desktop widgets |
| `node.latency` | about `4096/48000` | listening does not need low latency; larger quantum saves power |
| `target.object` | optional | selects the output |

## Crates and build

- `pipewire` + `libspa` 0.10.x. Needs the system packages `libpipewire-0.3-dev` and `libspa-0.2-dev` (installed on the development machine, PipeWire 1.4.2) and `clang`.
- Cargo features `v0_3_xx` of the crate raise the minimum PipeWire version. `Time::buffered/queued_buffers` need `v0_3_50`, `Buffer::requested` needs `v0_3_49`. Use `v0_3_50` unless something needs more.
- `symphonia` 0.6.x for decoding (the API differs from 0.5; read its source first). A ring buffer crate for the SPSC queue (candidates: `rtrb`, `ringbuf`).

## Prototype and how to verify it without listening

Spike 1: play one ripped MP3 from the command line.

Checks, all scriptable:

1. **The stream exists**: `pw-dump` lists a node named `psalm` with `media.role=Music` and the right `media.name`.
2. **Audio really flows**: create a temporary null sink (`support.null-audio-sink`), set `target.object` to it, record its monitor with `pw-record`, and compare the recording with the decoded file (not silent, right duration, level and correlation). This does not touch the speakers.
3. **Timing**: position reported by the engine matches elapsed time within a small tolerance, before and after pause and seek.
4. **Underruns**: stay at zero while nothing else is loading the machine.

## Open questions

- Latency from pressing play to sound, and whether `set_active` is fast enough for pause.
- Behaviour when the default sink changes while playing (Bluetooth headset connects). `AUTOCONNECT` should re-link; verify.
- How the engine is exposed to Flutter: a C ABI in a shared library called through `dart:ffi` is the simplest; `flutter_rust_bridge` is the alternative.
- Where to build it in the Flutter Linux bundle (CMake step that runs `cargo build` and installs the `.so`).

## Spike 1: result

Code: `audio/src/lib.rs` (`play_file`), `audio/examples/play.rs`. Check: `audio/scripts/verify.sh <mp3>`.

What was built: decoder thread (symphonia 0.6) into an `rtrb` ring buffer; the `process` callback on the real-time thread copies whole frames out and pads with silence; the format is the file's own (24 kHz stereo, `F32LE`); the stream is connected with `AUTOCONNECT | MAP_BUFFERS | RT_PROCESS`.

What was measured (10 s of a ripped audiobook, played into a temporary null sink, recorded from its monitor, compared with the file decoded by ffmpeg):

| Check | Result |
|---|---|
| Stream node in PipeWire | present with `media.name`, `media.role=Music`, `media.category=Playback`, `application.name=Psalm`, state `running`, linked to the target sink (two links, FL and FR) |
| Level | recording RMS is 1.027 times the reference |
| Shape of the signal | envelope correlation 0.9978, lag 0 samples |
| Length | recording 9.89 s, reference 9.91 s |
| Underruns | 0 |
| Cleanup | the temporary sink is removed afterwards |

The correlation and level show that the right audio arrives at the right speed. They do not show how it sounds: the check is done on a null sink, so the output device, resampling to the device rate and latency to the speakers were not part of it. Nobody has listened to it yet.

Lessons:

- **Do not quit the loop when the file ends.** The first version stopped the main loop as soon as the ring buffer was empty and lost the last 0.16 s, because buffers already queued in PipeWire were never played. The fix is `Stream::flush(true)` and waiting for the `drained` callback, with a 2 s deadline so a broken stream cannot hang the player. After the fix the tail is complete.
- **User data of an `RT_PROCESS` stream must be `Send`-safe.** The `process` callback runs on the real-time thread while other callbacks run elsewhere, and the crate does not stop you from putting an `Rc` into the shared user data. Keep only atomics, `Arc`, the ring buffer consumer and plain values in it.
- **Quitting needs a second path.** The `process` callback only sets an atomic; a timer on the loop thread (50 ms) looks at it and does the work that is not real-time safe.
- The frame count handed to PipeWire is the real number of audio frames; padding is counted separately as underrun frames only while the file is still playing.

Still unverified from the design: pause and resume with `set_active`, seek with `flush(false)`, volume through stream controls, position from `Stream::time()`, switching the sink while playing, and the startup latency.

## Spike 2: the controllable engine

Code: `audio/src/engine.rs` (`Engine`), `audio/src/source.rs` (decoder), `audio/examples/control.rs`. Checks: `audio/scripts/verify-all.sh <mp3>`, which runs `verify.sh` (whole file) and `verify-controls.sh` for four scenarios. Each plays into a temporary null sink, records it, and compares the recording with the decoded file and with the position log of the engine. The speakers stay silent.

How it works: `Engine::start` runs PipeWire on its own thread; commands go in through a `pipewire::channel`, state comes out through atomics. A *session* is one file being played (decoder thread, ring buffer, stream). **Loading and seeking build a new session** (seeking is a seek in the decoder plus a fresh stream); **pausing is `set_active(false)`** on the stream; **volume is the stream control `SPA_PROP_volume`**, so it shows up as the volume of the stream in mixers. A stream that starts paused is connected with `INACTIVE` and woken with `set_active(true)`.

Results (a ripped audiobook track, 16 seconds):

| Scenario | What was checked | Result |
|---|---|---|
| whole file | as in spike 1, with the new engine | length 9.89 s against 9.91 s, correlation 0.9978, level 1.027 (the same numbers as before the rewrite) |
| pause | pause at 3 s, resume at 6 s | 2.98 s of digital silence; position held at 3072 ms while paused; sound resumes at 3.16 s of the file (stopped at 3.07 s); correlation 0.992 |
| seek | seek from 2 s to 8 s | break of **25 ms**; the sound after it is the file at 8.30 s (expected 8.30 s); position 10,048 ms after 4 s |
| volume | 0.5 at 4 s | level 1.002 of the file before, 0.515 after (0.51 of before); same sound, correlation 0.991 |
| paused start | loaded paused, play at 3 s | silence until the play command, then the file from its start, correlation 1.000 |

Settled by these checks (from the unverified list of spike 1): pause and resume with `set_active` (no link problems, no audible stall, position exact), seek by restarting the session (a 25 ms break is short enough for an audiobook), volume through the stream control, and starting paused.

Caveats:

- **Position runs ahead of what is audible**, by the amount handed to PipeWire but not yet played, about 60 ms in the pause test (position 3.07 s, the audible part ended at 3.01 s). It counts audio handed over, not audio heard. Using `Stream::time()` would close the gap; not done yet.
- **Seeking is accurate to a codec frame** (about 26 ms for MP3), because the engine does not discard the samples between the frame it lands on and the exact time. The 8.30 s result above is within that.
- **A file without a length in its header has no duration** (`Engine::duration` is `None`); the server knows the length from the library scan.
- Nothing here has been listened to. The checks cover the data that reaches the sink, not the device, the resampling to the device rate or the latency to the speakers.
- Still open: switching the sink while playing, startup latency, speed without a change of pitch, gapless transitions from one track to the next (a new track is a new session, so there is a short break), and the connection to Flutter (FFI).

## Spike 3: C interface and the Dart backend

Code: `audio/src/ffi.rs`, `audio/include/psalm_audio.h`; in the app `lib/audio/audio_backend.dart` (interface), `fake_backend.dart` (for tests), `pipewire_backend.dart` (over `dart:ffi`). Check: `audio/scripts/verify-dart.sh <mp3>`.

- The audio crate is built as `rlib` and `cdylib`; `target/release/libpsalm_audio.so` exports 13 functions (`psalm_engine_new`, `_free`, `_load`, `_play`, `_pause`, `_stop`, `_seek`, `_set_volume`, `_state`, `_position_ms`, `_duration_ms`, `_volume`, `psalm_last_error`).
- Rules of the interface: an opaque engine pointer; 0 for success and -1 for failure with the reason from `psalm_last_error` (kept per thread); UTF-8 strings that the engine copies; no panic crosses the boundary (it becomes a failure with the reason "internal error"); a missing engine is a failure, not a crash.
- The app talks only to `AudioBackend`. `PipeWireBackend.open(target:, libraryPath:)` finds the library in the argument, the variable `PSALM_AUDIO_LIB`, or the normal library search. `FakeBackend` moves its position by a clock that a test controls.
- Checked: 3 Rust tests of the interface (missing engine, error text cut to the buffer, errors per thread); 8 Dart tests of `FakeBackend`; and one Dart test that drives the real library into a null sink: a paused load keeps position 0, play moves the position (about 1.3 s after 1.3 s), pause holds it, a seek while paused stays paused, the volume is reported, a missing file throws `AudioException` and leaves the state `failed`, dispose can be called twice and then the backend refuses use. That test runs only when `PSALM_AUDIO_LIB`, `PSALM_TEST_SINK` and `PSALM_TEST_CLIP` are set, so it can never play on the speakers by accident.

Not done:

- **The library is not bundled into the Flutter Linux build.** The app finds it through `PSALM_AUDIO_LIB` or the library path; a CMake step that builds it and installs it into the bundle is missing.
- **The player of the app still uses its simulated playback** (`PlayerController`). It has not been switched to `AudioBackend`, because the demo books have no files; this needs the app-facing API (files to play) first.
- **`load` blocks the calling isolate** for the time it takes to open the file and decode its first block. Fine for a local file; for a file fetched over the network it will have to move to a background isolate.
- Android and iPhone need their own `AudioBackend` (ExoPlayer, AVPlayer); nothing of that exists.


## Volume is remembered by the system, per role

Found while checking the volume: WirePlumber stores the volume of a stream under its role (`~/.local/state/wireplumber/stream-properties`, `Output/Audio:media.role:Music`) and restores it for the next stream with that role, whatever program it is. Consequences:

- A volume set by one program is the starting volume of the next one with the same role. The engine therefore does not force a volume at the start; only a volume that the caller has set is applied.
- `Engine::volume()` follows what PipeWire reports (`control_info`), so it is right after a restore or a change in a mixer.
- Tests must not use the role `Music`. They set `PSALM_AUDIO_ROLE=Test` (all scripts do), so they cannot change what real programs start with.
- Whether the app should share its volume with other music programs, or use a role or application name of its own, is **open**.

## Live streams (internet radio, MP3)

- The network part is Dart (`app/lib/audio/radio_stream.dart`): `dart:io` `HttpClient` (TLS, proxies, redirects), request header `Icy-MetaData: 1`, `IcyFilter` removes the metadata blocks and reports `StreamTitle`. Reconnects 3 times (2 s apart) after a lost connection; a connection that delivered 16 KiB starts the count again. HTTP 4xx, TLS errors and non-MP3 content types fail at once.
- The engine takes the plain MP3 bytes: `psalm_engine_stream_open/_push/_end/_fail` (`audio/include/psalm_audio.h`). Push never blocks and returns the bytes taken (back pressure); the bytes go through a 1 MiB queue (`audio/src/stream.rs`) that symphonia reads as a media source without seek or length.
- The PipeWire stream is built when the first block is decoded (format known). The state is `buffering` (5) until 2 s of decoded audio are queued, and again whenever the ring runs dry; it resumes by itself.
- Pause tears down the PipeWire stream but keeps the connection; the queue discards its oldest bytes. Play opens at the live edge. Seek does nothing, `duration` is unknown, speed stays 1.0.
- Not supported: AAC, Ogg, HLS, playlists (`stream type not supported`), and the old Shoutcast v1 status line `ICY 200 OK` (`dart:io` cannot parse it).
- Checks: `audio/scripts/verify-stream.sh [station url]` (null sink, silent).
