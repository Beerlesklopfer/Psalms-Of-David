/* Psalms audio engine: C interface (see src/ffi.rs for the rules).
 *
 * Build:  cargo build --release   ->  target/release/libpsalm_audio.so
 * An engine is an opaque 64-bit handle (slot and generation) in a registry inside the library; 0 is never a valid
 * handle. A handle that is 0, was never issued or has been freed is refused: the function returns its failure value
 * (-1) and sets a reason; it never crashes. Any thread may call any function, also while another thread frees.
 *
 * All functions return 0 on success and -1 on failure unless said otherwise. The reason of a failed call on a valid
 * handle is psalm_engine_last_error(handle); failures that belong to no engine (creation, an invalid handle) are in
 * psalm_last_error(). Strings are UTF-8 and zero-terminated.
 *
 * Version: check psalm_abi_version() >> 16 == PSALM_API_VERSION >> 16 before the first call, and refuse the library
 * otherwise. The major number changes when the interface breaks, the minor number when something is added.
 */
#ifndef PSALM_AUDIO_H
#define PSALM_AUDIO_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PSALM_API_VERSION ((2u << 16) | 0u)

typedef uint64_t PsalmHandle;

/* The version of the library: (major << 16) | minor. */
uint32_t psalm_abi_version(void);

/* Connects to PipeWire. `target`: node name or serial of a sink, NULL for the default.
 * Returns 0 on failure (psalm_last_error tells why). */
PsalmHandle psalm_engine_new(const char *target);
/* An engine with no output (no sound card, no PipeWire): the audio is decoded and dropped. For tests of callers.
 * speed: 0 as fast as the decoder delivers, 1 real time, n n times real time. Returns 0 on failure. */
PsalmHandle psalm_engine_new_null(uint32_t speed);
/* Stops and releases the engine; the handle is invalid afterwards. Returns -1 for a handle that is not valid (double
 * free). Does not wait for the audio thread. */
int psalm_engine_free(PsalmHandle engine);

/* Opens `path` at `start_ms`; plays if `play` is non-zero, otherwise starts paused. Returns at once with the id of the
 * request (1 or more) or -1 if the call itself failed (invalid handle, no path). A worker thread opens the file: the
 * state is 6 (loading) until it is done, then 1 or 2, or 4 (failed) with the reason in psalm_engine_error. A newer
 * load replaces one that is still being opened; position is `start_ms` and duration -1 meanwhile. play, pause and seek
 * that arrive while the file is being opened apply when it is open. */
int64_t psalm_engine_load(PsalmHandle engine, const char *path, uint64_t start_ms, int play, const char *media_name);
int psalm_engine_play(PsalmHandle engine);
int psalm_engine_pause(PsalmHandle engine);
int psalm_engine_stop(PsalmHandle engine);
int psalm_engine_seek(PsalmHandle engine, uint64_t ms);
int psalm_engine_set_speed(PsalmHandle engine, float speed);   /* 0.5 to 3.0, pitch kept */
int psalm_engine_set_buffer(PsalmHandle engine, uint32_t seconds); /* 1 to 30 (2 if never set); applies from the next load */
int psalm_engine_set_volume(PsalmHandle engine, float volume); /* 0.0 to 1.0 */

/* Gapless. enqueue_next queues the file that follows the loaded one. When the loaded one ends, the engine continues
 * with it at the exact sample (no silence, no repeated or lost sample; MP3 encoder delay and padding are trimmed) and
 * the state stays 1 (playing). It replaces an earlier queued file; a load or stop drops it. A file that cannot be
 * opened, or cannot follow without a change of sample rate or channel count, is not an error: the loaded one then
 * ends normally (state 3). track_generation counts the switches (over the life of the engine; a load, seek or stop
 * does not count): when it rises, position and duration belong to the new file. -1 for an invalid handle. */
int psalm_engine_enqueue_next(PsalmHandle engine, const char *path);
int psalm_engine_clear_next(PsalmHandle engine);
int64_t psalm_engine_track_generation(PsalmHandle engine);

/* 0 idle, 1 playing, 2 paused, 3 ended, 4 failed, 5 buffering (live stream), 6 loading (a file is being opened);
 * -1 for an invalid handle. */
int psalm_engine_state(PsalmHandle engine);
int64_t psalm_engine_position_ms(PsalmHandle engine);
int64_t psalm_engine_duration_ms(PsalmHandle engine); /* -1 if unknown */
float psalm_engine_volume(PsalmHandle engine);

/* Live MP3 stream (internet radio). The caller does the networking and pushes the
 * plain MP3 bytes. stream_open returns at once with a request id or -1 (state 5 until playback starts; a bad
 * stream ends in state 4, reason via psalm_engine_error). stream_push returns the
 * bytes taken (fewer than `len`: queue full, offer the rest later) or -1; it never
 * blocks. Seeking a live stream does nothing; pause keeps the queue (old data is
 * dropped) and play returns to the live edge. */
int64_t psalm_engine_stream_open(PsalmHandle engine, int play, const char *media_name);
int64_t psalm_engine_stream_push(PsalmHandle engine, const uint8_t *data, size_t len);
int psalm_engine_stream_end(PsalmHandle engine);
int psalm_engine_stream_fail(PsalmHandle engine, const char *message);
/* Why the state is 4: copies it into `buffer` (zero-terminated, cut to `len`) and returns its full length; 0 if none. */
size_t psalm_engine_error(PsalmHandle engine, char *buffer, size_t len);

/* The reason of the last failed call on this engine (same convention as psalm_engine_error). For an invalid handle it
 * returns the global reason. */
size_t psalm_engine_last_error(PsalmHandle engine, char *buffer, size_t len);
/* The reason of the last failure that belongs to no engine (creation failed, an invalid handle was used). Returns its
 * full length in bytes; 0 if there was none. */
size_t psalm_last_error(char *buffer, size_t len);

#ifdef __cplusplus
}
#endif

#endif
