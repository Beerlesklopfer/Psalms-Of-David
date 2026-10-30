<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Read-aloud books in the parents' voices (synthetic audiobooks)

Status 2026-10-30: **concept, nothing built.** Project owner: "I hope that on Hugging Face a model can learn the parents' voice, and then the child is
read to." Statements about models, licences and speeds are from the author's knowledge and **have to be checked** (model cards, a listening test) before
anything is built on them.

## Goal

A parent records a short sample of their voice once. From then on the server can turn a text (a fairy tale, a story the parents wrote) into an audio file
in that voice. The result is an ordinary audiobook in the library: the app plays it gaplessly, remembers the position, and applies the age limit and the
weekly schedule like for any other album. **No live synthesis on the phone, no streaming TTS**: the audio is generated ahead of time on the server side.

Not a goal: reading arbitrary copyrighted books for distribution, cloning voices of people who did not agree, or imitating public persons.

## Rules (hard)

1. **Consent.** A voice profile exists only for a person who agreed, in the system, with their own account (the person records or confirms the sample
   themselves). Parents may create profiles for themselves; a child's voice is never cloned.
2. **Reference recordings are private.** They live outside the library tree (configured path, never hardcoded), are readable by administrators only, are
   never served by the API, never appear in listings, backups of them are encrypted like other secrets.
3. **Deleting a profile deletes the sample and the model data**; generated audiobooks stay (they are the family's) unless the person asks for removal, then
   the audiobooks made with that voice are deleted as well (the album knows its voice, see below).
4. **Labelled as synthetic.** Every generated album carries the tag `Synthetic = yes` and the speaker name in the artist/narrator tag ("read by
   *Mama (synthetic voice)*"), so nobody mistakes it for a real recording. The app shows a small "synthetic voice" badge on the album.
5. **Only administrators** start a generation (parents). Children never see the function.
6. Texts: public-domain works or the family's own. For protected books the function is for private use in the family only; the UI says so and the
   text is never stored on the server longer than the job runs.

## Pipeline

```
text (paste / file)  →  job (queue)  →  split into sentences/paragraphs  →  TTS with voice profile  →  chapters (audio files)
                                                                           →  tags + cover  →  library scan  →  album visible
```

- **Job queue** in the server database (new migration, number follows 0056): `tts_job(id, owner, voice_id, title, status, progress, error, created, finished,
  album_id)`; status machine `queued → running → done | failed | cancelled` (explicit state machine, like the other long-running tasks; the GUI only shows
  state, it never drives the worker).
- **Worker**: a separate process or a thread pool outside the request path, so playback and the web are never slowed down. One job at a time by default
  (configurable). Resumable per chapter: a crash loses at most one chapter.
- **Chapters**: the text is cut at paragraph boundaries into pieces of a few minutes each; each piece becomes one file `NN - title.<ext>`. Sentence
  pauses are inserted by the engine; the pieces are joined with a configurable pause, never a hard cut. Gapless playback between chapters is the
  player's job as for every album.
- **Output format** follows the library's rules (`spec-transcoding.md`); tags per `spec-tags-id3.md` (Album, Artist = narrator, Track/Disc, Genre =
  audiobook, `Synthetic`, voice id in a private tag).
- **Backend adapter**: the TTS engine sits behind one Rust trait (`SpeechEngine`: `synthesize(text, voice, language) -> pcm`), with a local process
  adapter (command from the configuration: a Python script or a small HTTP service) and a null adapter for tests. Which model is used is configuration,
  not code. Paths, command lines, model names, language and pauses come from `/etc/david.toml` / the settings, never from constants.

## Voice profiles

- `voice(id, owner_user, display_name, language, sample_path, created, consent_at, consent_by)`.
- Creation in the admin web ("Voices"): upload or record 30 s – 5 min of clean speech (no music, no background, no echo), the page checks length,
  loudness and clipping and says what is wrong. The person confirms the consent text with their own login.
- Two quality levels: **zero-shot** (reference sample only, quick, usable) and **fine-tuned** (10–30 minutes of recordings, background training job,
  better and more stable; optional later).
- A profile is bound to one language; German first, English second (UI rule: EN first, DE second applies to the interface, the audio language follows
  the text).

## Models (candidates; to be verified)

| Candidate | German | Cloning | Licence (check) | Note |
|---|---|---|---|---|
| XTTS-v2 | yes | zero-shot, a few seconds | non-commercial | well known, usable for family use |
| F5-TTS (German fine-tunes) | via fine-tune | zero-shot | check per fine-tune | newer, natural prosody |
| Chatterbox (multilingual) | check | zero-shot | permissive (check) | good stability |
| fine-tuning of one of the above | yes | from 10–30 min | follows the base | best quality, GPU time |

Selection: listening test with the same paragraph and the same reference sample for each candidate; the owner picks. Decision criteria: naturalness,
stability over long texts (no drift, no repeated or skipped words), German pronunciation, speed, licence, hardware needs.

## Hardware

The PDC server probably has no suitable GPU. Options, in order of preference: the owner's workstation as a worker that fetches jobs from the server over
the API; CPU on the server (slower than real time, a book overnight); a rented GPU for fine-tuning only. The worker protocol is the same in all cases
(`GET /api/v1/tts/jobs/next`, `PUT .../progress`, upload of finished chapters), authenticated with a worker token, so the model can run anywhere.

## API and UI

- Admin web: section "Read aloud" (Voices, new job, jobs list with progress and cancel). New job: title, text (paste or file), voice, language, optional
  cover; estimate of duration and processing time before the start.
- App: no new function except the "synthetic voice" badge and the narrator in the album details.
- MCP tools (later): `list_voices`, `tts_job` (administrators only), consistent with the other tools.
- Endpoints (admin only): `GET/POST /api/v1/voices`, `DELETE /api/v1/voices/{id}`, `GET/POST /api/v1/tts/jobs`, `DELETE /api/v1/tts/jobs/{id}`; worker
  endpoints as above.

## Tests

- State machine of the job (queued → running → done / failed / cancelled, resume after a crash) with the null engine.
- Chapter splitting (paragraph boundaries, long sentences, empty text, umlauts and quotes).
- Permissions: only administrators; the sample is never reachable through any other endpoint.
- Consent: a profile cannot be created without the consent record; deleting a profile removes sample files and (on request) its albums.
- Tags: generated albums carry `Synthetic` and the narrator; the library scan picks them up.

## Phases

1. **Listening test** (no code in the product): the owner records 30 s of a parent; a script (outside the repo's product code) renders the same fairy-tale
   paragraph with two or three candidates; the owner decides.
2. Worker adapter + job queue + chapter splitting + tagging with the null engine and one real engine, CLI only.
3. Admin web: voices, jobs, consent flow.
4. Fine-tuning job (optional), MCP tools, app badge.

## Open questions

- Which candidate wins the listening test, and is zero-shot good enough for a child's bedtime story?
- Where does the worker run (workstation, server CPU, rented GPU)?
- Voice of one parent for one book, or several voices (dialogue parts) in one book? (Several voices need a markup in the text; deferred.)
- Do we keep the original text next to the album for later re-rendering with a better model? (Proposal: yes, for family-owned texts only, owner can delete.)
