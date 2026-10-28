// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The C interface of the engine, for Flutter (`dart:ffi`) and other C callers.
//! The header is `include/psalm_audio.h`.
//!
//! Rules of this interface:
//! - An engine is an opaque pointer made by `psalm_engine_new` and released by
//!   `psalm_engine_free`. Every other function takes it as the first argument.
//! - Functions that can fail return 0 for success and -1 for failure, and
//!   `psalm_last_error` copies the reason of the last failure on the calling thread.
//! - No panic crosses the boundary: a panic becomes a failure with the reason "internal error".
//! - Strings are UTF-8, zero-terminated, owned by the caller; the engine copies them.
//! - All functions may be called from any thread, but not concurrently with
//!   `psalm_engine_free` on the same engine.

use std::cell::RefCell;
use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::time::Duration;

use crate::{Engine, EngineOptions, State};

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

fn set_error(message: impl Into<String>) {
    LAST_ERROR.with(|e| *e.borrow_mut() = message.into());
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

/// # Safety
/// `engine` is null or a pointer from `psalm_engine_new` that has not been freed.
unsafe fn engine<'a>(engine: *const Engine) -> Option<&'a Engine> {
    unsafe { engine.as_ref() }
}

/// Starts the engine and connects to PipeWire. `target` is the node name or serial
/// of a sink, or null for the default. Returns null on failure.
///
/// # Safety
/// `target` is null or a valid zero-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_new(target: *const c_char) -> *mut Engine {
    guard(std::ptr::null_mut(), || {
        let target = unsafe { text(target) }.map(str::to_owned);
        match Engine::start(EngineOptions { target, role: None }) {
            Ok(e) => Box::into_raw(Box::new(e)),
            Err(e) => {
                set_error(e.to_string());
                std::ptr::null_mut()
            }
        }
    })
}

/// Stops the engine and releases it. Null is allowed and does nothing.
///
/// # Safety
/// `e` is null or from `psalm_engine_new`, and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_free(e: *mut Engine) {
    guard((), || {
        if !e.is_null() {
            drop(unsafe { Box::from_raw(e) });
        }
    })
}

/// Opens a file at `start_ms`, playing (`play` non-zero) or paused.
///
/// # Safety
/// `e` is valid; `path` and `media_name` are valid zero-terminated strings (`media_name` may be null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_load(e: *mut Engine, path: *const c_char, start_ms: u64, play: i32, media_name: *const c_char) -> i32 {
    guard(-1, || {
        let (Some(engine), Some(path)) = (unsafe { engine(e) }, unsafe { text(path) }) else {
            set_error("engine or path missing");
            return -1;
        };
        let name = unsafe { text(media_name) }.unwrap_or("");
        match engine.load(Path::new(path), Duration::from_millis(start_ms), play != 0, name) {
            Ok(()) => 0,
            Err(err) => {
                set_error(err.to_string());
                -1
            }
        }
    })
}

macro_rules! simple {
    ($(#[$doc:meta])* $name:ident => |$engine:ident| $body:expr) => {
        $(#[$doc])*
        ///
        /// # Safety
        /// `e` is valid.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(e: *mut Engine) -> i32 {
            guard(-1, || match unsafe { engine(e) } {
                Some($engine) => {
                    $body;
                    0
                }
                None => {
                    set_error("engine missing");
                    -1
                }
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

/// Jumps to `ms`, keeping playing or paused.
///
/// # Safety
/// `e` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_seek(e: *mut Engine, ms: u64) -> i32 {
    guard(-1, || match unsafe { engine(e) } {
        Some(engine) => {
            engine.seek(Duration::from_millis(ms));
            0
        }
        None => {
            set_error("engine missing");
            -1
        }
    })
}

/// Playback speed, 0.5 to 3.0, pitch kept. Takes effect within about two seconds.
///
/// # Safety
/// `e` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_set_speed(e: *mut Engine, speed: f32) -> i32 {
    guard(-1, || match unsafe { engine(e) } {
        Some(engine) => {
            engine.set_speed(speed);
            0
        }
        None => {
            set_error("engine missing");
            -1
        }
    })
}

/// Volume of this stream, 0.0 to 1.0.
///
/// # Safety
/// `e` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_set_volume(e: *mut Engine, volume: f32) -> i32 {
    guard(-1, || match unsafe { engine(e) } {
        Some(engine) => {
            engine.set_volume(volume);
            0
        }
        None => {
            set_error("engine missing");
            -1
        }
    })
}

/// 0 idle, 1 playing, 2 paused, 3 ended, 4 failed; -1 if the engine is missing.
///
/// # Safety
/// `e` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_state(e: *const Engine) -> i32 {
    guard(-1, || {
        unsafe { engine(e) }.map_or(-1, |engine| match engine.state() {
            State::Idle => 0,
            State::Playing => 1,
            State::Paused => 2,
            State::Ended => 3,
            State::Failed => 4,
        })
    })
}

/// Position in milliseconds (audio handed to PipeWire, a little ahead of what is audible).
///
/// # Safety
/// `e` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_position_ms(e: *const Engine) -> i64 {
    guard(-1, || unsafe { engine(e) }.map_or(-1, |engine| engine.position().as_millis() as i64))
}

/// Length in milliseconds, or -1 if the file does not say (or the engine is missing).
///
/// # Safety
/// `e` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_duration_ms(e: *const Engine) -> i64 {
    guard(-1, || {
        unsafe { engine(e) }.and_then(|engine| engine.duration()).map_or(-1, |d| d.as_millis() as i64)
    })
}

/// The volume last set (1.0 at the start); -1.0 if the engine is missing.
///
/// # Safety
/// `e` is valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_engine_volume(e: *const Engine) -> f32 {
    guard(-1.0, || unsafe { engine(e) }.map_or(-1.0, |engine| engine.volume()))
}

/// Copies the reason of the last failure on this thread into `buffer` (at most
/// `len - 1` bytes, always zero-terminated) and returns the full length of the
/// reason in bytes. Returns 0 if there was none.
///
/// # Safety
/// `buffer` is null or points to `len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn psalm_last_error(buffer: *mut c_char, len: usize) -> usize {
    guard(0, || {
        LAST_ERROR.with(|e| {
            let message = e.borrow();
            if !buffer.is_null() && len > 0 {
                let n = message.len().min(len - 1);
                // SAFETY: `buffer` has `len` bytes and `n + 1 <= len`.
                unsafe {
                    std::ptr::copy_nonoverlapping(message.as_ptr(), buffer.cast::<u8>(), n);
                    *buffer.add(n) = 0;
                }
            }
            message.len()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::ptr::null_mut;

    fn last_error() -> String {
        let mut buf = vec![0 as c_char; 256];
        let n = unsafe { psalm_last_error(buf.as_mut_ptr(), buf.len()) };
        let bytes: Vec<u8> = buf.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        assert_eq!(bytes.len(), n.min(255));
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn a_missing_engine_is_a_failure_and_not_a_crash() {
        unsafe {
            assert_eq!(psalm_engine_play(null_mut()), -1);
            assert_eq!(psalm_engine_pause(null_mut()), -1);
            assert_eq!(psalm_engine_stop(null_mut()), -1);
            assert_eq!(psalm_engine_seek(null_mut(), 1000), -1);
            assert_eq!(psalm_engine_set_volume(null_mut(), 0.5), -1);
            assert_eq!(psalm_engine_state(std::ptr::null()), -1);
            assert_eq!(psalm_engine_position_ms(std::ptr::null()), -1);
            assert_eq!(psalm_engine_duration_ms(std::ptr::null()), -1);
            assert_eq!(psalm_engine_volume(std::ptr::null()), -1.0);
            psalm_engine_free(null_mut()); // allowed, does nothing
            let path = CString::new("/nonexistent.mp3").unwrap();
            assert_eq!(psalm_engine_load(null_mut(), path.as_ptr(), 0, 1, std::ptr::null()), -1);
        }
        assert_eq!(last_error(), "engine or path missing");
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
    fn errors_belong_to_the_thread_that_had_them() {
        set_error("mine");
        std::thread::spawn(|| assert_eq!(last_error(), "")).join().unwrap();
        assert_eq!(last_error(), "mine");
    }
}
