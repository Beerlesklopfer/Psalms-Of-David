/* Psalm audio engine: C interface (see src/ffi.rs for the rules).
 *
 * Build:  cargo build --release   ->  target/release/libpsalm_audio.so
 * All functions return 0 on success and -1 on failure unless said otherwise;
 * psalm_last_error() tells why. Strings are UTF-8 and zero-terminated.
 */
#ifndef PSALM_AUDIO_H
#define PSALM_AUDIO_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct PsalmEngine PsalmEngine;

/* Connects to PipeWire. `target`: node name or serial of a sink, NULL for the default.
 * Returns NULL on failure. */
PsalmEngine *psalm_engine_new(const char *target);
/* Stops and releases the engine. NULL is allowed. */
void psalm_engine_free(PsalmEngine *engine);

/* Opens `path` at `start_ms`; plays if `play` is non-zero, otherwise starts paused. */
int psalm_engine_load(PsalmEngine *engine, const char *path, uint64_t start_ms, int play, const char *media_name);
int psalm_engine_play(PsalmEngine *engine);
int psalm_engine_pause(PsalmEngine *engine);
int psalm_engine_stop(PsalmEngine *engine);
int psalm_engine_seek(PsalmEngine *engine, uint64_t ms);
int psalm_engine_set_speed(PsalmEngine *engine, float speed);   /* 0.5 to 3.0, pitch kept */
int psalm_engine_set_volume(PsalmEngine *engine, float volume); /* 0.0 to 1.0 */

/* 0 idle, 1 playing, 2 paused, 3 ended, 4 failed; -1 without an engine. */
int psalm_engine_state(const PsalmEngine *engine);
int64_t psalm_engine_position_ms(const PsalmEngine *engine);
int64_t psalm_engine_duration_ms(const PsalmEngine *engine); /* -1 if unknown */
float psalm_engine_volume(const PsalmEngine *engine);

/* Copies the reason of the last failure on this thread into `buffer` (zero-terminated,
 * cut to `len`) and returns its full length in bytes; 0 if there was none. */
size_t psalm_last_error(char *buffer, size_t len);

#ifdef __cplusplus
}
#endif

#endif
