<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Video: family films and series

Status 2026-10-31: **draft, nothing built.** Project owner: "We are taking video on board now; we take what we learned from audio with us." Decided in the
conversation: the library holds **family films and series**; **PDC cannot transcode** (it has no power for it); **age ratings (FSK) apply**; discs are
read by **MakeMKV** (an outside program, nothing of it is rebuilt). Statements about outside programs and device formats have to be checked before
anything is built on them (marked *check*).

## Goal

1. **Host** the family's films and series on David like the audiobooks: one library, the same roles, age limits, releases, hiding, owner and listening
   (here: watching) progress per profile.
2. **Play** them in Psalms (Linux first, then Android, iPhone, iPad, macOS) with a position that is kept on the server and a next episode that follows.
3. **Take in** files from a folder or an upload, and discs through MakeMKV (a hidden feature, off by default).

Not a goal: transcoding on PDC, a shop, streaming services, and anything that works around copy protection by itself (MakeMKV is the owner's own
tool; David only calls it, see "Discs").

## What carries over from audio

| Learned from audio | Here |
|---|---|
| The files are the truth; nothing is copied into the database that the file says (the year of an album is read from the tags) | Title, year, duration, streams come from the file (ffprobe) or a sidecar file; the database keeps what is needed to list and filter |
| Every device must be able to play it (`ios_playable`, `docs/spec-transcoding.md`) | Per file we know container and codecs and say which devices play it; PDC does not convert (see "Formats") |
| Rights, approval and inbox of the CDs (`docs/spec-cd-inbox.md`) | The same pattern for put-in video, later; first step: only administrators take in video |
| Legally unclear things are off and switched on in `/etc/david.toml` under `[features]` | Disc reading is a hidden feature `video_discs` |
| The GUI shows state only; explicit state machines | The video player and the disc job are state machines (`docs/spec-state-machines.md`) |
| Listening progress per profile on the server | Watching progress per profile, per film or episode, same table |
| Nothing hardcoded (paths, folders, word lists) | Roots, naming rules and limits are settings |
| UI is checked in the Linux app only | Same for video; nothing else is built for it unasked |
| Name things, do not number them in reports | Films and episodes are named by title in reports and messages |

## Words

- **Film**: one video file as one item. **Series**: a show with seasons; **episode**: one video file of a season.
- In the database a film is a `book` row with `kind = 'video'` and one `track`; a **season** is a `book` row (title "Show, Season 2") with one `track` per
  episode. This keeps the whole library (visibility, releases, owner, labels, progress, planner) unchanged. The split into `item` and `part` stays for
  later, as in `spec-app-api.md` ("when the second kind is built, not before"). *Decision needed at the start of phase 1: this, or an own table like
  `ebook`; the recommendation is this.*

## Data

Migration after the current last one. On `track`, for video only (NULL for audio): `width`, `height`, `video_codec`, `fps`, `streams` (JSON: audio
tracks with language and codec, subtitle tracks with language and kind). The existing `format` (container), `codec`, `mime_type`, `duration_ms`,
`size_bytes` are reused. On `book` for a season or a film: `year` (read from the file, like the year of an album), `min_age` and `recommended_age` as today.

Folder layout below the video roots of the settings (nothing built in), after the naming that the common media programs use so existing collections work
(*check* the exact rules before the scanner):

```
Films/<Title> (<Year>)/<Title> (<Year>).mp4        poster.jpg
Series/<Show>/Season 01/<Show> S01E02 <Title>.mp4  poster.jpg
Family/<Year> <Event>/<name>.mp4                   (family films: a folder is one item, the files are its parts)
```

Poster and metadata come from files next to the video (`poster.jpg`, an optional `.nfo`); **no lookup on the internet by default** (privacy, and the
terms of such services have to be read first, *check*).

## Age rating (FSK)

The steps of the library are 0, 6, 12, 16, 18 (`spec-discovery-profiles-roles.md`); they are the FSK steps, so `min_age` is the FSK rating as it is.
Releasing a film or season means assigning one (the database already refuses a release without a rating). `recommended_age` can come from the file or
the `.nfo` (information only). Family films default to "all ages" when the owner releases them. The weekly planner applies to video
(`spec-app-api.md` proposes `audiobook`, `album`, `video`).

## Formats (PDC cannot transcode)

- The server **streams the file as it is** (`GET /api/v1/parts/{id}/stream`, `Range`), nothing is re-encoded. There is no `as=` for video.
- **Remuxing is not transcoding:** copying streams from one container into another costs little CPU. PDC may do it, for example MKV to MP4 when the
  streams are compatible (H.264 or H.265 video; AAC or AC-3 audio; subtitles as text). *Check* what PDC manages and what iOS accepts in MP4.
- Files whose streams no device plays (for example DTS audio or VC-1 or MPEG-2 video for the iPhone) are **marked**, not converted: the parts of
  `GET /items/{id}` carry `playable_on` (the devices that play it, from container and codecs); the app shows "This film cannot be played on this
  device" with the reason, and the administration lists the files to be converted elsewhere.
- Conversion, when it is needed, happens **outside PDC** (the workstation); the converted file is put back into the folder. A drop folder or a worker
  on the workstation is a later step and its own spec.
- Players: Android (ExoPlayer) and Linux (libmpv, *check* packaging) play MKV; the iPhone (AVPlayer) plays MP4 and MOV with H.264 or H.265.

## Discs (MakeMKV, hidden feature)

- David **calls `makemkvcon`** as a child process (the path is a setting, like the programs of Eliezer); the program is **not** in David's package and
  the person who uses it installs it and holds the licence key. Its terms have to be read before the README says anything (*check*).
- Off by default: `[features] video_discs = true` in `/etc/david.toml` (or `DAVID_FEATURE_VIDEO_DISCS=1`); without it there is no button, no tool and no
  hint in the app.
- A **disc job** with explicit states like the CD job: no disc, disc read, titles listed, a choice (which titles are the film, which are extras),
  reading with progress (robot output `-r --progress=-same`), done, failed, cancelled; one at a time, never in the request path.
- The result is an `.mkv` in a staging folder from the settings; it takes the same way as any file (scan, remux if needed, rating, release).
- Rights: only administrators (first step); later the pattern of the CD inbox.

## API

`GET /items` and `GET /items/{id}` as today with `kind: "video"`; a part carries `width`, `height`, `video_codec`, `container`, `streams`, `playable_on`.
`GET /parts/{id}/stream` as today. Progress and resume as today (`finished` of an episode offers the next one). The poster is the cover of the item.

## The app

- The category "Videos" (today "coming soon") lists films and series, a season opens its episodes; the poster is the cover.
- A **video player** is a different backend from the audio engine (`spec-audio-pipewire.md` stays for audio): `video_player` on Android and iOS,
  libmpv on Linux (*check*); it sits behind an interface like `AudioBackend` and has an explicit state machine (idle, loading, playing, paused,
  buffering, ended, error) that the GUI only shows (`spec-player-state-machine.md`).
- Position is saved as for audio; the next episode follows after a short count-down that can be stopped.
- Offline: downloads are large; only on Wi-Fi and with room (the rule of the podcasts), per episode, never automatic.
- Later: subtitles, audio track choice, chapters, skip intro.

## Phases

1. **Scanner and listing** (server): video roots, ffprobe, `kind = 'video'`, year, poster, `min_age`, the API fields; the category in the app lists them.
2. **Playing** in the Linux app: player interface, libmpv backend, state machine, progress, next episode.
3. **Remux** and `playable_on` (the marks in the app and the administration).
4. **Discs**: `video_discs`, the disc job, the staging folder.
5. Android and iOS players; offline; subtitles and audio tracks.
6. Later: an outside worker for conversion; the rights and inbox pattern for put-in video.

## Tests

Scanner on generated small files (naming, a film, a season with episodes, a family folder, a file without a year); ffprobe answered by a fake program;
the rating rule (no release without a rating, a child with a lower limit does not see it, the planner applies); `playable_on` from container and codecs;
the disc job as a state machine against a fake `makemkvcon` (robot output, failure, cancel, a second job refused); the feature switch (off: no route,
no tool, no text); a stream with `Range`.

## Open questions

- A season as a `book` row or an own table (see "Words")? Recommendation: `book` row.
- Do family films belong to the library of one owner or to the whole family by default?
- Remux on PDC: how much CPU and time does it take there, and is the result kept next to the original or does it replace it?
- Which device formats matter first (the iPhone is the strictest)?
- Is `ffprobe` installed on PDC, or does the scanner read the containers itself?
