<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Speech to text: sermons become text (David as a client of Eliezer)

Status 2026-10-30: **concept, nothing built.** Project owner: "First we do speech to text: we connect a client to Eliezer to turn sermons into text."
Eliezer (`~/Dokumente/Projekte/eliezer`, a model server with an OpenAI-compatible interface) has **no speech recognition yet**: it serves text, image
and music models (`POST /v1/audio/speech` is music generation, `docs/wiki/API.md`). So the work has two halves: a speech model in Eliezer, and a client in David.

## Goal

A sermon (an audio file in the library, usually 20 to 60 minutes, German) is sent to Eliezer; the text comes back with time stamps, is stored with the
album, can be read in the administration and the app, and can be searched. Later: summary, verses mentioned, a transcript for the hard of hearing.

Not a goal: live transcription, speaker identification, translating, or sending audio anywhere outside the family's own server (the point of Eliezer is that
nothing leaves the house).

## Half 1: Eliezer (a task for its repository; this is the contract David relies on)

- **Model kind `stt`** (like `llm`, `image`, `music`): a speech recognition model, backend **whisper.cpp** (`whisper-server`) in the first step, the same way
  audio.cpp serves music (a process Eliezer starts, owns the GPU slot of, and stops). Candidates: Whisper large-v3 (multilingual, good German), a German
  fine-tune of it, or a smaller model for the CPU; the choice is a configuration (`models.toml`), not code. The model list reports `type: "stt"`.
- **`POST /v1/audio/transcriptions`**, OpenAI shape, multipart: `file` (the audio), `model` (can be left out when exactly one stt model exists), `language`
  (`de`), `response_format` (`json`, `text`, `verbose_json`; David uses `verbose_json` for the segments with start and end), `temperature`, `prompt`
  (a hint with names and terms: biblical names, the church's name), `timestamp_granularities` (`segment`).
- **Long files:** the server cuts the audio into pieces (for example 30 s with a little overlap, or by silence), so memory stays small, and joins the result;
  the request is long-running (minutes), so it also needs the **asynchronous form** that Eliezer offers for slow models, or David polls a job
  (`POST` returns a job id, `GET /v1/audio/transcriptions/{id}`); decide in Eliezer's spec, David supports both (see below).
- **Limits:** file size and length limits from the configuration (not constants); `413` over the limit; `400` for a file that is not audio; `404` unknown model.
- **Formats:** Eliezer decodes MP3, M4A, OGG, FLAC and WAV (ffmpeg or `symphonia`) to 16 kHz mono PCM before the model.
- The GPU is shared with image and music models (they wait for each other, as today).

## Half 2: the client in David

### Where it lives

`server/src/transcribe/` (name follows the code layout): a client of an OpenAI-compatible speech-to-text endpoint (`POST {base}/v1/audio/transcriptions`)
with the address, key, model and language from the **settings** (nothing hardcoded; the address can be Eliezer or any compatible server).

Settings (administration, group "Speech to text"): `stt_url`, `stt_key` (stored like other secrets, never shown again), `stt_model`, `stt_language`
(default `de`), `stt_prompt` (hint words), `stt_max_parallel` (default 1), `stt_chunk_minutes` (if David cuts the audio itself, default 10; 0 = send the file whole),
`stt_timeout_minutes`, `stt_auto_kinds` (which kinds of albums are transcribed by themselves; default none).

### Data

New migration (number follows the last one, 0056 today):

- `transcript(id, track_id, language, model, created_at, state, error, duration_ms)`: one per track and language; `state`: `queued`, `running`, `done`,
  `failed`, `cancelled` (explicit state machine, same style as the other long-running jobs; the GUI shows state only).
- `transcript_segment(transcript_id, start_ms, end_ms, text)`: the segments with time stamps, so the player can jump to a place and the text can be searched.
- A full-text index on the segment text (SQLite FTS5) for the search.

### Flow

```
choose album (or tracks)  →  job per track (queued)  →  worker: decode / cut  →  POST to Eliezer  →  segments stored  →  state done
```

- **Worker:** like the other background jobs (separate task, never in the request path), `stt_max_parallel` at a time, resumable per track, a failed
  track does not stop the others (`failed` with the error code, a retry button).
- **Cutting:** David cuts a long file into chunks of `stt_chunk_minutes` with a few seconds of overlap (decoding with the code that already reads the files),
  sends each chunk, and joins the segments with the chunk offset; duplicates from the overlap are dropped by time. If `stt_chunk_minutes = 0` the file goes whole.
- **Privacy rules of the library apply:** only administrators start a transcription; the text of an album is visible to those who may see the album (age
  limit, owner, hiding) and to nobody else; deleting an album deletes its transcripts.
- **Labels:** a transcript is marked as machine made (`model` and `created_at` shown), never as the author's text.

### API and UI

- Admin web: on the album dialog a button "Transcribe" (all tracks or selected ones), the state per track with progress, a "Text" tab with the segments (click to
  play from that place later), download as `.txt` and `.srt`/`.vtt`, delete.
- `GET /api/v1/books/{id}/transcript` (and per track) for the app: segments; the app shows the text under the player (later).
- `GET /api/v1/search?q=` gains the transcripts (only what the user may see).
- MCP tools (administrators only, `dry_run` first): `transcribe` (album id or track ids, language), `transcript_status`, `get_transcript`.
- Settings page has a "Test the connection" button (sends one second of silence) and shows the model list of the server.

### Tests

State machine of the job (queued → running → done | failed | cancelled, resume after a crash); a fake Eliezer (like `tests/m7/fake_audio_server.rs` there) that
answers `verbose_json`; chunk offsets and the overlap drop; permissions (only administrators start, the text follows the visibility of the album); the search sees
only what the user may see; secrets are never returned; a file that is not audio fails that track only.

## Phases

1. **Eliezer:** `stt` kind with whisper.cpp, `POST /v1/audio/transcriptions` (own spec and tests in its repository). Listening test with one real sermon: word error,
   names, speed (real-time factor on the GPU and on the CPU).
2. **David client:** settings, the client and the fake server, the job table and the state machine, MCP `transcribe` and `get_transcript` (CLI first).
3. **Admin web:** button, progress, text tab, download.
4. **Search and app:** FTS index, `/api/v1/search`, the text in the app, jump to the place.
5. Later: summary and verses with an LLM through Eliezer (separate spec), automatic transcription of new albums of a kind.

## Open questions

- Which sermons: all albums of a kind or genre (for example "Predigt"), or chosen by hand? Where are they today (own folder, an author, a label)?
- Whisper model and hardware: large-v3 on the GPU of Eliezer's machine, or smaller on the CPU? (The listening test decides.)
- Synchronous or job API in Eliezer for a 45 minute file? (David works with both; the job form is safer.)
- Do transcripts go into the tags (a `Lyrics` or `Comment` field of the files) or only into the database? Proposal: only the database, files untouched.
- Should the text be correctable by hand (an editor) and keep the correction when the sermon is transcribed again? Proposal: yes, later (a `corrected` flag per segment).
