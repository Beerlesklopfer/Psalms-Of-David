<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Rebuilding the playback engine: separating the player from the UI thread

Status 2026-10-30 (end of day): stages A and B of the Dart side are done, see "Status" at the end. Original text: **plan, nothing of the big rebuild is built.** Project owner: "GUI and player run in the same thread, we must split them." Three independent
planners read the code (the Dart player, the native engine in `audio/`, the platforms and the delivery); this text joins their results. The first step (the
network side of the radio in a background isolate, `app/lib/audio/radio_isolate.dart`) is being built separately.

## Decided requirement: gapless playback is mandatory

Project owner, 2026-10-30: "Is gapless playback a requirement? **YES.**" It is not an extra of the rebuild but one of its acceptance criteria, **on every system**:

- Between two tracks of the same book or album there is no silence, no click and no repeated or lost sample (audiobooks and classical or live albums are the cases that
  matter). The next track is opened and decoded **before** the current one ends and is spliced in at the exact sample.
- **MP3 needs care:** the encoder delay and padding at the start and end of a file (LAME/Xing header, iTunSMPB) have to be removed, or even a seamless splice has a gap
  of a few tens of milliseconds. The Rust engine uses symphonia, which can trim from the gapless information (`enable_gapless`); to be verified on real files.
- **Linux (Rust engine):** `psalm_engine_enqueue_next(path)` pre-opens the next source and splices it in `feed()`; the trim of the delay is done in the decoder. It
  comes **early** in P3 (right after the non-blocking load), not as a late option. Test without a sound card: two sine files, the output has exactly the expected
  number of samples and no discontinuity (null output of P3).
- **Android, iOS, macOS, Windows (just_audio):** today `JustAudioBackend` loads every track alone (`setFilePath`/`setUrl`), which is **not** gapless. Change it to give the
  player the next track in advance (`ConcatenatingAudioSource` / a queue of the current and the prefetched next file; ExoPlayer is gapless for local files, AVQueuePlayer
  nearly, Windows to be tried). The prefetch the controller does today (`_prefetchNext`) already puts the next file on disk; the backend gets an `enqueueNext` in
  `AudioBackend` that the engines implement or ignore. The controller must keep track and position in step when the system switches by itself (`onExternalChange`).
  This is **not part of the later options**: it is phase P2b below, because it changes behaviour on the phones today.
- A track that is not on the disk yet when the previous one ends (slow network) is the exception; then the engine waits, but the cause must show in the counters of P0.

## What is wrong today

- **Linux:** every call into the native engine is made from the UI isolate (`app/lib/audio/pipewire_backend.dart`, all synchronous). `psalm_engine_load` and
  `load_stream` wait for the engine thread up to **20 seconds** (`audio/src/engine.rs`), and a slow file system freezes the window. `psalm_engine_free` joins the
  engine thread. The radio network read, the ICY filter and the copy loops run in the UI isolate (`radio_stream.dart`): a stalled UI can starve the output.
- **All systems:** `PlayerController` (about 1000 lines) runs the playback state machine, the 500 ms tick (position, report to the server, heard marks,
  prefetch), the sync of the listening and the lists in the UI isolate, and calls `notifyListeners` every tick, so large parts of the UI rebuild twice a second.
- **Mobile:** just_audio runs ExoPlayer and AVPlayer natively; there the UI isolate only sends commands. The jank is mostly a Linux/FFI matter plus the Dart-side
  work for radio, downloads and library loading.
- **Handle safety:** the engine hands out a raw pointer; a second isolate or a late timer after `dispose` is a use-after-free. `LAST_ERROR` is thread-local, which
  is wrong when calls come from changing threads.

## Decision

Three ways were weighed:

| | Idea | Cost | Risk | Result |
|---|---|---|---|---|
| A | keep today's split (just_audio on mobile and desktop, Rust on Linux only) | none | low | fixes nothing of the report |
| B | one Rust core on every system (WASAPI, CoreAudio, AAudio) | months | high | one behaviour, but the OS media sessions, AVAudioSession and ExoPlayer's HLS/AAC are lost and iOS can only be tried through TestFlight |
| **C** | **a player service above two native backends, plus the Linux engine made non-blocking** | **medium** | **low** | **recommended** |

**Recommendation: C.** Keep ExoPlayer and AVPlayer on Android and iOS (they already run off the Dart thread). On Linux move the player work into a service
isolate and make the Rust engine asynchronous. Desktop on one Rust core (Windows WASAPI, macOS CoreAudio through the `cpal` crate behind an output trait) is a
**later option** once Linux is stable; B for the phones only if C is finished and real differences still hurt.

Constraint that every plan has to respect: `audio_service`, `just_audio` and the platform channels work on the **root isolate**. On the phones the "player isolate"
can therefore only be a Dart worker for network and bookkeeping (radio, downloads, progress); the just_audio object itself stays in the UI isolate.

## The design

**Dart side** (planner 1). Split `PlayerController` into a **`PlayerEngine`** (pure Dart state machine in a service isolate: `open`, `toggle`, `seek`, `next`, the tick,
`_report`, `_prefetchNext`, `_writeLast`, the `AudioBackend`, `ServerSource` calls) and a thin **`PlayerController` facade** in the UI isolate with the same public
API (about 100 uses of `PlayerScope`, 37 test files keep compiling). Stays in the UI: library lists, downloads, the offline outbox (one writer), `PsalmAudioHandler`,
MPRIS, podcast downloads. A `PlayerTransport` (`IsolateTransport` and a **synchronous `InProcessTransport` for the tests**, so `FakeAsync` and the ~460 tests keep
working). Only sendable messages (commands, a coarse `PlayerSnapshot` with a `seq`, a `PositionTick` at 2 Hz, events). The position gets its own
`ValueListenable`, so only the seek bar, the time label and the mini player rebuild; `PlayerScope` fires on coarse changes only. Paths and credentials are resolved in the
UI and sent to the service (`path_provider` and keys need the root isolate). Supervise the isolate and restart it from the last snapshot.

**Native engine** (planner 2). (1) An `Output` trait with a `NullOutput`/`FileOutput`, so decode, seek, speed, back pressure, stale handles run in plain `cargo test`
without a sound card (`verify.sh` stays as the PipeWire smoke test). (2) A **handle registry** (slot and generation, `Arc<Engine>`): a stale handle answers "invalid handle"
instead of undefined behaviour; per-handle `last_error`; `psalm_abi_version()` and `struct_size` in out-structs. (3) **Asynchronous load**: `load` returns a request id,
`Source::open` runs on a worker, no joins on the loop thread. (4) A **status struct** (seqlock) and an **event port** (`Dart_PostCObject`) instead of per-tick FFI polling;
ICY title, track end, state change and errors arrive as events. (5) A **Rust network reader** (`net.rs`: HTTP, redirects, ICY filter, reconnect, explicit back pressure
into the ring buffer) so that the Dart `RadioStreamSession` is not needed for the engine any more. (6) **`enqueue_next` for true gapless (mandatory, early in P3)**. (7) cpal output for
Windows and macOS.

**Relation of the two radio paths:** the Dart radio isolate (now being built) is the **interim** answer and the reference for the ICY rules; if the Rust network reader
is accepted, it replaces the isolate on Linux. Without a Rust HTTP/TLS stack the isolate stays.

## Measure first (before the big work)

1. Run the Linux app with `flutter run --profile`; record a DevTools timeline during a library scroll while playing; count frames over 16 ms.
2. Add counters to the engine (ring underruns, minimum fill, decode time per chunk, callback lateness) as one `psalm_engine_stats` call, and a Dart timer on the UI
   isolate that logs its own lateness (more than 50 ms is a stall).
3. Reproduce with a local file, a radio stream and a podcast download in parallel.
4. **Decision rule:** underruns only while the UI is blocked -> the service isolate (P4). Underruns with an idle UI -> the cause is in the engine, do P3 first. Neither
   -> network; the radio isolate (P1) is probably enough.

## Phases (each can be released; acceptance that runs without a sound card or a phone)

| Phase | Content | Days | Acceptance in CI | By hand |
|---|---|---|---|---|
| P0 | counters and the stall logger | 2 | counters rise on a forced underrun (`FakeBackend`, engine test) | profile run on Linux |
| P1 | radio network read in a background isolate (**being built**) | ~2 | local TCP/ICY server, a busy UI loop in the test, no byte lost | radio for 30 min |
| P2 | `PlayerController` talks to the backend through a narrow async port; `InProcessTransport` | 4 | all existing tests green, contract tests on `FakeBackend` | - |
| P2b | **gapless on the phones and macOS/Windows:** `enqueueNext` in `AudioBackend`, `JustAudioBackend` with a queue of the current and the next file | 4-6 | `FakeBackend` contract test: the next track is queued before the end, the controller keeps track/position when the engine switches by itself | a gapless album on the emulator and on a phone (TestFlight): no gap, no click |
| P3 | Rust engine: output trait, handle registry, async load, **`enqueue_next` (gapless)**, event port, status struct | 10-14 | `cargo test` with the null output vs an f32 reference, stale handle, load while loading | PipeWire listening |
| P4 | player service in an isolate on Linux, `PositionBuilder` in the UI | 5-7 | the same service tests with a blocked UI isolate, isolate restart | Linux app |
| P5 | car browse tree in `PsalmAudioHandler`, `automotive_app_desc.xml` | 4 (+3 CarPlay, blocked by Apple) | `getChildren('root')` for a fake library with visibility and age filter | Android emulator + Desktop Head Unit |
| P6 | voice (App Intents, Google): `docs/spec-voice.md`, Siri first | 8-10 | server-side intent resolution tests | TestFlight, emulator |

Order: **P0 and P1, then the measurement decides** between P3 and P4. P2 is useful in every case (one contract for both plans). P5 and P6 are independent and may run
beside them. Rough total for the Linux rebuild (P2 to P4): 3 to 5 weeks.

## Risks

Tests that need `FakeAsync` break if the transport is not synchronous in tests; the offline outbox and `PodcastDownloads` are tightly coupled to the controller; a state
mirror in the UI can go stale (a `seq` on every snapshot, stale commands are dropped); credentials and the certificate pin in a second isolate; `Dart_PostCObject`
(API_DL) in the Flutter bundle build; seqlock torn reads; thread joins at shutdown; TLS and the CA store per OS if the reader moves into Rust; two network paths during the
transition; iOS and CarPlay can only be tried by hand; Windows has never run on a real machine.

## Open questions for the project owner

1. Linux and desktop first, with just_audio kept on the phones (the plan above), or are the phones in scope? 
2. May the engine depend on a Rust HTTP/TLS stack (binary size, licences are AGPL compatible)? If not, networking stays in Dart.
3. ~~Is true gapless a requirement?~~ **Decided: yes, mandatory** (see above). Open: is crossfade wanted as well (it needs the same queue, plus a volume ramp)?
4. May the C interface of the engine break once? There are no outside callers of the library.
5. Is it acceptable to lose up to about 5 seconds of the listening progress when the player isolate crashes?
6. Is 2 Hz position streaming enough, or should the UI interpolate for a smooth seek bar?
7. Is Windows a supported target (then it needs a system media handler, SMTC) or only built in CI?
8. What exactly happens when the jank shows: scrolling, a screen change, a download, radio? How many albums?
9. May a PipeWire job (a null sink works in a container) be added to the CI?

## Status (2026-10-30, Dart side P2)

Done, all in-process, no isolate yet, playback behaviour unchanged (analyze clean; 529 tests passed, 12 skip, before P2b; 605 pass with P2b and the parallel work):

- **Stage A, position apart from the coarse state.** `PlayerController.positionListenable` (a `ValueNotifier` behind `position`); the 500 ms tick no longer calls
  `notifyListeners`, only a coarse change does (book, track, playing, loading, error, speed, stream title/details, unplayable, heard marks). Seek bar, the time of the
  mini player and the labels of the player screen are `ValueListenableBuilder`s; `PsalmAudioHandler` also listens to the position listenable so the system
  "now playing" check runs at the old rate; MPRIS never announced the position. Test: `test/position_listenable_test.dart`.
- **Stage B, the engine behind a transport.** `lib/player/state_machine.dart` (pure `transition`, explicit states, three media kinds, effects),
  `lib/player/engine.dart` (shell), `lib/player/transport.dart` (sendable commands/events with `toMap`/`fromMap`, `PlayerTransport`, synchronous `InProcessTransport`
  with a `roundTrip` mode). `PlayerController` is the facade with the same API; it mirrors the snapshots and keeps the library, downloads, outbox, playlists.
  The whole suite also passes with `roundTrip: true` as the default (tried once). Documented in `docs/spec-player-state-machine.md`.

**P2b, gapless on just_audio (2026-10-30, built, only partly verified).**
- `AudioBackend.supportsGapless` (default false), `enqueueNext(path)`, `clearNext()`, and `ExternalControl.onAdvanced` (the backend switched by itself; `onExternalChange` is unchanged).
- `JustAudioBackend`: `AudioPlayer(useLazyPreparation: false)`; the playlist is `[current]` or `[current, next]` (`addAudioSource`/`removeAudioSourceRange`; the deprecated
  `ConcatenatingAudioSource` is not used). `currentIndexStream` 0 -> 1 is the switch: duration/ended bookkeeping is updated, the finished item is removed, the controller is told. A stream
  address is never queued; a failed enqueue is swallowed and the old path (load at the end) takes over, so no regression.
- Machine: `Core.queuedKey`, events `BackendAdvanced` and `PrefetchReady`, effects `EnqueueNext` and `ClearQueuedNext` (see `docs/spec-player-state-machine.md`). Files already on the device are queued
  at the first tick in the 45 s window; parts of the server after the prefetch.
- `FakeBackend(gapless: true)` implements the contract; `test/gapless_test.dart` (controller + fake: queued before the end, no `ended`/loading at any pump, progress once, seek back, skip/next/book change
  clear the queue) and the `gapless` group of `test/player_state_machine_test.dart`.
- `PipeWireBackend.supportsGapless` stays false; TODO in the code points to P3 (`psalm_engine_enqueue_next`).
- **Must be checked by hand (nothing of this is proven on a device):** a gapless album on the Android emulator and on an iPhone via TestFlight; MP3 encoder delay on Android (ExoPlayer trims with a LAME/Xing
  header, to be heard) and iOS (AVPlayer, iTunSMPB); Windows and macOS (just_audio_windows / darwin may not be gapless at all); that adding an item to a playing list does not hiccup; that the
  `positionDiscontinuity`/`currentIndex` events arrive in the order assumed.

### How to check gapless by hand

1. Use an album that is one continuous recording cut into tracks: a live concert, a DJ mix, classical movements with attacca, or a generated sine-wave album (10 files of one unbroken 440 Hz tone, cut at arbitrary points; MP3 and FLAC variants).
2. Open track 1, seek to about 60 s before its end (the next file is queued within 45 s of the end), listen on headphones at a decent volume.
3. At the join there must be no silence, no click or tick, no repeated or lost bit of sound. With the sine album a gap is a short dropout or a click; a pitch glitch means a lost sample. An audio editor on the recorded output (loopback) shows a flat waveform.
4. Check the log/UI: the track number moves on, the notification and lock screen change the title, the seek bar starts at 0, and no spinner flashes.
5. Repeat with: speed 1.5; the screen off; Bluetooth headphones; a pause just before the end (then play: still gapless); skip to the next track during the last seconds (a normal load is fine there); the last track of the album (ends cleanly).
6. Compare MP3 and FLAC/M4A of the same album: MP3 without a LAME/Xing header may keep a gap of some tens of ms that the player cannot remove.
7. Server albums with parts not downloaded yet: switch the network off after the prefetch; the join must still be gapless; with a slow network a gap is expected (the exception noted above).

Next: `IsolateTransport` (the engine builds backend and source inside the isolate: today they are passed in as objects, `BookData.live` is the only non-sendable
field and is dropped by the isolate transport); `EngineState` supervision and restart from the last snapshot; STAGE C machines (radio connection, podcast download lifecycle and list controllers).
Known gaps: `Core.book` holds the whole track list as `BookData` (cheap, but rebuilt on every `open`); the engine rebuilds a `Book` without author/cover when no live
book is given (enough for the source, not for the UI).
