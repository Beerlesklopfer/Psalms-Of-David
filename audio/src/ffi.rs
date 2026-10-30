// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The C interface of the engine, for Flutter (`dart:ffi`) and other C callers.
//! The header is `include/psalm_audio.h`.
//!
//! Rules of this interface:
//! - An engine is an opaque `u64` handle (slot and generation) made by `psalm_engine_new` and released by
//!   `psalm_engine_free`. Every other function takes it as the first argument. The handles live in a global registry;
//!   a call clones the `Arc` of the engine under the lock and works on that. A handle that is 0, was never issued or has been
//!   freed is refused with the failure value of the function (and the reason "invalid engine handle"), never undefined behaviour.
//! - Functions that can fail return 0 for success and -1 for failure. The reason is kept per engine
//!   (`psalm_engine_last_error`); failures that belong to no engine (creation, an invalid handle) are in `psalm_last_error`.
//! - No panic crosses the boundary: a panic becomes a failure with the reason "internal error".
//! - Strings are UTF-8, zero-terminated, owned by the caller; the engine copies them.
//! - All functions may be called from any thread, also concurrently with `psalm_engine_free`. Freeing returns at once; the
//!   audio thread is stopped and joined by a thread of its own.
//! - `psalm_abi_version()` is `(major << 16) | minor`; a caller refuses a library whose major number differs from its header.

use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::output::null::{NullMode, NullOptions};
use crate::{Engine, EngineOptions, State};

/// The version of this interface, `(major << 16) | minor`. The major number changes when the interface breaks
/// (a caller built for another major must not use the library), the minor number when something is added.
#[allow(clippy::identity_op)]
pub const API_VERSION: u32 = (2 << 16) | 0;

// ---- the registry of handles ------------------------------------------------------------------------------------------------

/// An engine and the reason of its last failed call. The C side sees only a handle.
struct Entry {
    engine: Option<Engine>,
    last_error: Mutex<String>,
}

impl Entry {
    fn engine(&self) -> &Engine {
        self.engine.as_ref().expect("the engine lives as long as its entry")
    }

    fn fail(&self, message: impl Into<String>) {
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = message.into();
    }
}

impl Drop for Entry {
    /// Stopping an engine joins its thread, which can take a moment: the last owner may be a caller on the UI thread, so the
    /// engine goes to a thread of its own that does the joining.
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            let cell = Arc::new(Mutex::new(Some(engine)));
            let handover = cell.clone();
            if std::thread::Builder::new().name("psalm-reaper".into()).spawn(move || drop(handover.lock().ok().and_then(|mut e| e.take()))).is_err() {
                drop(cell.lock().ok().and_then(|mut e| e.take()));
            }
        }
    }
}

struct Slot {
    generation: u32,
    entry: Option<Arc<Entry>>,
}

#[derive(Default)]
struct Registry {
    slots: Vec<Slot>,
    free: Vec<usize>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry { slots: Vec::new(), free: Vec::new() });
/// The reason of the last failure that no engine owns: creation, and a handle that is not valid.
static GLOBAL_ERROR: Mutex<String> = Mutex::new(String::new());

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

fn set_error(message: impl Into<String>) {
    *GLOBAL_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = message.into();
}

/// A handle is `generation << 32 | (slot + 1)`: never 0, and a slot that is reused has another generation.
fn make_handle(slot: usize, generation: u32) -> u64 {
    (u64::from(generation) << 32) | (slot as u64 + 1)
}

fn insert(engine: Engine) -> u64 {
    let entry = Arc::new(Entry { engine: Some(engine), last_error: Mutex::new(String::new()) });
    let mut reg = registry();
    let index = match reg.free.pop() {
        Some(i) => i,
        None => {
            reg.slots.push(Slot { generation: 0, entry: None });
            reg.slots.len() - 1
        }
    };
    let slot = &mut reg.slots[index];
    slot.generation = slot.generation.wrapping_add(1).max(1);
    slot.entry = Some(entry);
    make_handle(index, slot.generation)
}

/// The engine behind `handle`, or `None` if it is 0, was never issued or has been freed.
fn lookup(handle: u64) -> Option<Arc<Entry>> {
    let index = (handle & 0xFFFF_FFFF).checked_sub(1)? as usize;
    let generation = (handle >> 32) as u32;
    let reg = registry();
    let slot = reg.slots.get(index)?;
    if slot.generation != generation {
        return None;
    }
    slot.entry.clone()
}

fn remove(handle: u64) -> Option<Arc<Entry>> {
    let index = (handle & 0xFFFF_FFFF).checked_sub(1)? as usize;
    let generation = (handle >> 32) as u32;
    let mut reg = registry();
    let slot = reg.slots.get_mut(index)?;
    if slot.generation != generation {
        return None;
    }
    let entry = slot.entry.take()?;
    reg.free.push(index);
    Some(entry)
}

/// Runs `f` on the engine behind `handle`; an invalid handle is `on_invalid` and an error, not undefined behaviour.
fn with<T>(handle: u64, on_invalid: T, f: impl FnOnce(&Entry) -> T) -> T {
    match lookup(handle) {
        Some(entry) => f(&entry),
        None => {
            set_error("invalid engine handle");
            on_invalid
        }
    }
}

/// `Ok` is 0, a failure is -1 with the reason kept on the engine.
fn status<E: std::fmt::Display>(entry: &Entry, result: Result<(), E>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => {
            entry.fail(e.to_string());
            -1
        }
    }
}

/// A request that got an id: the id (1 or more) on success, -1 on failure.
fn request<E: std::fmt::Display>(entry: &Entry, result: Result<u64, E>) -> i64 {
    match result {
        Ok(id) => id as i64,
        Err(e) => {
            entry.fail(e.to_string());
            -1
        }
    }
}

/// Runs `f`, turning a panic into `on_panic` and an error message.
fn guard<T>(on_panic: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        set_error("internal error");
        on_panic
    })
}

/// # Safety
/// `p` is null or points to a zero-terminated string that stays valid for the call.
unsafe fn text<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() { None } else { unsafe { CStr::from_ptr(p) }.to_str().ok() }
}

/// Copies `message` into `buffer` (cut to `len - 1` bytes, zero-terminated) and returns its full length.
///
/// # Safety
/// `buffer` is null or points to `len` writable bytes.
unsafe fn copy_out(message: &str, buffer: *mut c_char, len: usize) -> usize {
    if !buffer.is_null() && len > 0 {
        let n = message.len().min(len - 1);
        // SAFETY: `buffer` has `len` bytes and `n + 1 <= len`.
        unsafe {
            std::ptr::copy_nonoverlapping(message.as_ptr(), buffer.cast::<u8>(), n);
            *buffer.add(n) = 0;
        }
    }
    message.len()
}

/// The version of the interface: `(major << 16) | minor`. Compare the major number with the `PSALM_API_VERSION` of the header
/// you were built with, and refuse the library if they differ.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_abi_version() -> u32 {
    API_VERSION
}

/// Starts the engine and connects to PipeWire. `target` is the node name or serial
/// of a sink, or null for the default. Returns the handle of the engine, or 0 on failure
/// (the reason is in `psalm_last_error`).
///
/// # Safety
/// `target` is null or a valid zero-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_new(target: *const c_char) -> u64 {
    guard(0, || {
        let target = unsafe { text(target) }.map(str::to_owned);
        match Engine::start(EngineOptions { target, role: None }) {
            Ok(e) => insert(e),
            Err(e) => {
                set_error(e.to_string());
                0
            }
        }
    })
}

/// An engine with the null output (no sound card, no PipeWire; the audio is decoded and dropped), for tests of the callers.
/// `speed`: 0 plays as fast as the decoder delivers, 1 in real time, n at n times real time.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_new_null(speed: u32) -> u64 {
    guard(0, || {
        let mode = match speed {
            0 => NullMode::Accelerated,
            1 => NullMode::Realtime,
            n => NullMode::Speedup(n),
        };
        match Engine::start_null(NullOptions { mode, file: None }) {
            Ok(e) => insert(e),
            Err(e) => {
                set_error(e.to_string());
                0
            }
        }
    })
}

/// Stops the engine and releases it; the handle is invalid afterwards. Returns 0, or -1 if the handle was not valid (a second
/// free of the same handle, or a handle that never existed): that is an error and does nothing else. The call does not wait for
/// the audio thread to end.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_free(e: u64) -> i32 {
    guard(-1, || match remove(e) {
        Some(entry) => {
            drop(entry);
            0
        }
        None => {
            set_error("invalid engine handle");
            -1
        }
    })
}

/// Opens a file at `start_ms`, playing (`play` non-zero) or paused. Returns at once with the id of the request (1 or more), or
/// -1 if the call itself failed (invalid handle, no path). The file is opened by a worker thread: the state is 6 (loading) until
/// it is done, then 1 or 2, or 4 (failed) with the reason in `psalm_engine_error`. A newer load replaces one that is still open.
///
/// # Safety
/// `path` and `media_name` are valid zero-terminated strings (`media_name` may be null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_load(e: u64, path: *const c_char, start_ms: u64, play: i32, media_name: *const c_char) -> i64 {
    guard(-1, || {
        let Some(path) = (unsafe { text(path) }) else {
            set_error("path missing");
            return -1;
        };
        let name = unsafe { text(media_name) }.unwrap_or("");
        with(e, -1, |entry| request(entry, entry.engine().load(Path::new(path), Duration::from_millis(start_ms), play != 0, name)))
    })
}

/// Opens a live MP3 stream that the caller feeds with `psalm_engine_stream_push`.
/// Returns at once with the id of the request (1 or more), or -1; the state is 5 (buffering) until playback starts, and 4 (failed)
/// with `psalm_engine_error` if the data cannot be played. `media_name` may be null.
///
/// # Safety
/// `media_name` is null or a valid zero-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_stream_open(e: u64, play: i32, media_name: *const c_char) -> i64 {
    guard(-1, || {
        let name = unsafe { text(media_name) }.unwrap_or("");
        with(e, -1, |entry| request(entry, entry.engine().load_stream(play != 0, name)))
    })
}

/// Offers `len` bytes of the stream. Returns how many were taken (0 to `len`; fewer
/// means the queue is full, so wait and offer the rest again), or -1 on failure
/// (no stream open, or it was closed). Never blocks.
///
/// # Safety
/// `data` points to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_stream_push(e: u64, data: *const u8, len: usize) -> i64 {
    guard(-1, || {
        let bytes: &[u8] = if data.is_null() || len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(data, len) } };
        with(e, -1, |entry| match entry.engine().stream_push(bytes) {
            Ok(n) => n as i64,
            Err(err) => {
                entry.fail(err.to_string());
                -1
            }
        })
    })
}

/// The stream ended; what was pushed is still played, then the state is 3 (ended).
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_stream_end(e: u64) -> i32 {
    guard(-1, || with(e, -1, |entry| status(entry, entry.engine().stream_end())))
}

/// The stream failed on the sender's side; the state becomes 4 (failed) with `message`.
///
/// # Safety
/// `message` is a valid zero-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_stream_fail(e: u64, message: *const c_char) -> i32 {
    guard(-1, || {
        let message = unsafe { text(message) }.unwrap_or("stream failed");
        with(e, -1, |entry| status(entry, entry.engine().stream_fail(message)))
    })
}

/// Copies the engine's own failure text (why the state is 4) into `buffer`, like
/// `psalm_last_error`; returns its full length, 0 if there is none (or the handle is not valid).
///
/// # Safety
/// `buffer` is null or points to `len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_error(e: u64, buffer: *mut c_char, len: usize) -> usize {
    guard(0, || {
        let message = lookup(e).and_then(|entry| entry.engine().error()).unwrap_or_default();
        unsafe { copy_out(&message, buffer, len) }
    })
}

macro_rules! simple {
    ($(#[$doc:meta])* $name:ident => |$engine:ident| $body:expr) => {
        $(#[$doc])*
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(e: u64) -> i32 {
            guard(-1, || {
                with(e, -1, |entry| {
                    let $engine = entry.engine();
                    $body;
                    0
                })
            })
        }
    };
}

simple!(/// Resumes playback.
    psalm_engine_play => |e| e.play());
simple!(/// Pauses playback; the position stays.
    psalm_engine_pause => |e| e.pause());
simple!(/// Stops and unloads the file.
    psalm_engine_stop => |e| e.stop());

/// Queues the file that follows the loaded one. When the loaded one ends, the engine continues with it at the exact sample
/// (no silence, no repeated or lost sample; the encoder delay and padding of MP3 are trimmed) and counts it in
/// `psalm_engine_track_generation`; the state stays playing. Replaces an earlier queued file; a load or stop drops it. A file that
/// cannot be opened or cannot follow without a change of sample rate or channel count is not an error here: the loaded one
/// then ends normally (state 3). Returns at once.
///
/// # Safety
/// `path` is a valid zero-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_enqueue_next(e: u64, path: *const c_char) -> i32 {
    guard(-1, || {
        let Some(path) = (unsafe { text(path) }) else {
            set_error("path missing");
            return -1;
        };
        with(e, -1, |entry| status(entry, entry.engine().enqueue_next(Path::new(path))))
    })
}

simple!(/// Drops the file queued with `psalm_engine_enqueue_next`.
    psalm_engine_clear_next => |e| e.clear_next());

/// How many times the engine has moved on to the queued file by itself (gapless) since this engine was created, counted when the
/// first sample of the new file has been handed to the output; -1 for an invalid handle. A caller polls it: when it rises, the
/// position and length belong to the new file. A load, a seek or a stop do not count.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_track_generation(e: u64) -> i64 {
    guard(-1, || with(e, -1, |entry| entry.engine().track_generation() as i64))
}

/// Jumps to `ms`, keeping playing or paused.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_seek(e: u64, ms: u64) -> i32 {
    guard(-1, || {
        with(e, -1, |entry| {
            entry.engine().seek(Duration::from_millis(ms));
            0
        })
    })
}

/// Playback speed, 0.5 to 3.0, pitch kept. Takes effect within about two seconds.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_set_speed(e: u64, speed: f32) -> i32 {
    guard(-1, || {
        with(e, -1, |entry| {
            entry.engine().set_speed(speed);
            0
        })
    })
}

/// Seconds of audio kept ready between the decoder and the output, 1 to 30 (2 if never set). Applies from the next load.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_set_buffer(e: u64, seconds: u32) -> i32 {
    guard(-1, || {
        with(e, -1, |entry| {
            entry.engine().set_buffer_seconds(seconds);
            0
        })
    })
}

/// Volume of this stream, 0.0 to 1.0.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_set_volume(e: u64, volume: f32) -> i32 {
    guard(-1, || {
        with(e, -1, |entry| {
            entry.engine().set_volume(volume);
            0
        })
    })
}

/// 0 idle, 1 playing, 2 paused, 3 ended, 4 failed, 5 buffering (live stream), 6 loading (a file is being opened); -1 if the handle is not valid.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_state(e: u64) -> i32 {
    guard(-1, || {
        with(e, -1, |entry| match entry.engine().state() {
            State::Idle => 0,
            State::Playing => 1,
            State::Paused => 2,
            State::Ended => 3,
            State::Failed => 4,
            State::Buffering => 5,
            State::Loading => 6,
        })
    })
}

/// Position in milliseconds (audio handed to the output, a little ahead of what is audible); -1 if the handle is not valid.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_position_ms(e: u64) -> i64 {
    guard(-1, || with(e, -1, |entry| entry.engine().position().as_millis() as i64))
}

/// Length in milliseconds, or -1 if the file does not say (or the handle is not valid).
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_duration_ms(e: u64) -> i64 {
    guard(-1, || with(e, -1, |entry| entry.engine().duration().map_or(-1, |d| d.as_millis() as i64)))
}

/// The volume last set (1.0 at the start); -1.0 if the handle is not valid.
#[unsafe(no_mangle)]
pub extern "C" fn psalm_engine_volume(e: u64) -> f32 {
    guard(-1.0, || with(e, -1.0, |entry| entry.engine().volume()))
}

/// Copies the reason of the last failed call on this engine into `buffer` (at most `len - 1` bytes, always zero-terminated)
/// and returns its full length in bytes; 0 if there was none. For a handle that is not valid it gives the global reason
/// (`psalm_last_error`).
///
/// # Safety
/// `buffer` is null or points to `len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_last_error(e: u64, buffer: *mut c_char, len: usize) -> usize {
    guard(0, || {
        let message = match lookup(e) {
            Some(entry) => entry.last_error.lock().unwrap_or_else(|x| x.into_inner()).clone(),
            None => GLOBAL_ERROR.lock().unwrap_or_else(|x| x.into_inner()).clone(),
        };
        unsafe { copy_out(&message, buffer, len) }
    })
}

/// Copies the reason of the last failure that belongs to no engine (creation failed, or a handle was not valid) into `buffer`
/// (at most `len - 1` bytes, always zero-terminated) and returns the full length of the reason in bytes. Returns 0 if there was
/// none. The reason of a failed call on a valid handle is `psalm_engine_last_error`.
///
/// # Safety
/// `buffer` is null or points to `len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_last_error(buffer: *mut c_char, len: usize) -> usize {
    guard(0, || {
        let message = GLOBAL_ERROR.lock().unwrap_or_else(|x| x.into_inner()).clone();
        unsafe { copy_out(&message, buffer, len) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn text_of(f: impl FnOnce(*mut c_char, usize) -> usize) -> String {
        let mut buf = vec![0 as c_char; 256];
        let n = f(buf.as_mut_ptr(), buf.len());
        let bytes: Vec<u8> = buf.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        assert_eq!(bytes.len(), n.min(255));
        String::from_utf8(bytes).unwrap()
    }

    fn global_error() -> String {
        text_of(|b, l| unsafe { psalm_last_error(b, l) })
    }

    fn engine_error(h: u64) -> String {
        text_of(|b, l| unsafe { psalm_engine_last_error(h, b, l) })
    }

    /// Every call that takes a handle, with `h`; each must answer with its failure value.
    fn call_everything(h: u64) {
        unsafe {
            let path = CString::new("/nonexistent.mp3").unwrap();
            assert_eq!(psalm_engine_load(h, path.as_ptr(), 0, 1, std::ptr::null()), -1);
            assert_eq!(psalm_engine_enqueue_next(h, path.as_ptr()), -1);
            assert_eq!(psalm_engine_clear_next(h), -1);
            assert_eq!(psalm_engine_track_generation(h), -1);
            assert_eq!(psalm_engine_play(h), -1);
            assert_eq!(psalm_engine_pause(h), -1);
            assert_eq!(psalm_engine_stop(h), -1);
            assert_eq!(psalm_engine_seek(h, 1000), -1);
            assert_eq!(psalm_engine_set_speed(h, 1.5), -1);
            assert_eq!(psalm_engine_set_buffer(h, 5), -1);
            assert_eq!(psalm_engine_set_volume(h, 0.5), -1);
            assert_eq!(psalm_engine_state(h), -1);
            assert_eq!(psalm_engine_stream_open(h, 1, std::ptr::null()), -1);
            assert_eq!(psalm_engine_stream_push(h, b"x".as_ptr(), 1), -1);
            assert_eq!(psalm_engine_stream_end(h), -1);
            assert_eq!(psalm_engine_stream_fail(h, std::ptr::null()), -1);
            assert_eq!(psalm_engine_error(h, std::ptr::null_mut(), 0), 0);
            assert_eq!(psalm_engine_position_ms(h), -1);
            assert_eq!(psalm_engine_duration_ms(h), -1);
            assert_eq!(psalm_engine_volume(h), -1.0);
        }
    }

    #[test]
    fn the_version_has_the_major_number_two() {
        assert_eq!(psalm_abi_version() >> 16, 2);
    }

    #[test]
    fn a_zero_handle_is_an_error_and_not_a_crash() {
        call_everything(0);
        assert_eq!(global_error(), "invalid engine handle");
        assert_eq!(psalm_engine_free(0), -1);
    }

    #[test]
    fn a_handle_that_was_never_issued_is_an_error() {
        for h in [1u64 << 32 | 77, u64::MAX, 0xdead_beef_0000_0001, 12345] {
            call_everything(h);
        }
    }

    #[test]
    fn a_freed_handle_is_an_error_and_a_double_free_too() {
        let h = psalm_engine_new_null(0);
        assert_ne!(h, 0);
        assert_eq!(psalm_engine_state(h), 0);
        assert_eq!(psalm_engine_free(h), 0);
        call_everything(h); // use after free
        assert_eq!(psalm_engine_free(h), -1, "double free");
        assert_eq!(psalm_engine_free(h), -1);
        assert_eq!(global_error(), "invalid engine handle");
    }

    #[test]
    fn a_reused_slot_does_not_revive_the_old_handle() {
        let old = psalm_engine_new_null(0);
        assert_eq!(psalm_engine_free(old), 0);
        let new = psalm_engine_new_null(0);
        assert_ne!(old, new, "another generation");
        assert_eq!(psalm_engine_state(old), -1, "the stale handle is refused even if its slot is in use again");
        assert_eq!(psalm_engine_state(new), 0);
        assert_eq!(psalm_engine_free(old), -1, "and cannot free the new engine");
        assert_eq!(psalm_engine_state(new), 0);
        assert_eq!(psalm_engine_free(new), 0);
    }

    #[test]
    fn errors_belong_to_the_engine_that_had_them() {
        let (a, b) = (psalm_engine_new_null(0), psalm_engine_new_null(0));
        // Pushing into a stream that is not open fails at once; opening a missing file only later (state 4).
        assert_eq!(unsafe { psalm_engine_stream_push(a, b"x".as_ptr(), 1) }, -1);
        assert!(engine_error(a).contains("no stream is open"), "{}", engine_error(a));
        assert_eq!(engine_error(b), "", "the other engine has no failure");
        assert_eq!(psalm_engine_free(a) + psalm_engine_free(b), 0);
    }

    #[test]
    fn the_error_text_is_cut_to_the_buffer_and_always_terminated() {
        let message = "a rather long reason for a failure";
        set_error(message);
        let mut small = [0x7f as c_char; 8];
        let full = unsafe { psalm_last_error(small.as_mut_ptr(), small.len()) };
        assert_eq!(full, message.len(), "the full length is reported");
        assert_eq!(small[7], 0, "terminated");
        let text: Vec<u8> = small.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        assert_eq!(text, b"a rathe");
        // Asking only for the length works with no buffer.
        assert_eq!(unsafe { psalm_last_error(std::ptr::null_mut(), 0) }, message.len());
    }

    #[test]
    fn calls_from_other_threads_and_a_free_in_between_are_safe() {
        let h = psalm_engine_new_null(0);
        let workers: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(move || {
                    let (mut ok, mut refused) = (0u32, 0u32);
                    for _ in 0..2000 {
                        match psalm_engine_state(h) {
                            -1 => refused += 1,
                            _ => ok += 1,
                        }
                        let _ = psalm_engine_set_volume(h, 0.5);
                        let _ = psalm_engine_position_ms(h);
                    }
                    (ok, refused)
                })
            })
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(psalm_engine_free(h), 0);
        for w in workers {
            let (ok, refused) = w.join().unwrap();
            assert_eq!(ok + refused, 2000, "every call answered");
        }
        assert_eq!(psalm_engine_state(h), -1);
    }

    #[test]
    fn freeing_does_not_wait_for_the_audio_thread() {
        let handles: Vec<u64> = (0..20).map(|_| psalm_engine_new_null(1)).collect();
        let t = std::time::Instant::now();
        for h in handles {
            assert_eq!(psalm_engine_free(h), 0);
        }
        assert!(t.elapsed() < std::time::Duration::from_millis(100), "20 frees took {:?}", t.elapsed());
    }

    #[test]
    fn many_engines_at_once_are_independent() {
        let hs: Vec<u64> = (0..8).map(|_| psalm_engine_new_null(0)).collect();
        let mut sorted = hs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 8);
        for (i, &h) in hs.iter().enumerate() {
            psalm_engine_set_volume(h, i as f32 / 10.0);
        }
        for (i, &h) in hs.iter().enumerate() {
            assert!((psalm_engine_volume(h) - i as f32 / 10.0).abs() < 1e-6);
            assert_eq!(psalm_engine_free(h), 0);
        }
    }
}
