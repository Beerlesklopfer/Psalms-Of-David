# Spec: the CD import interface for the apps

Status: **built** in `server/src/api_cd.rs` (routes), `server/src/cdimport.rs` (jobs, naming, placing), `server/src/cd_settings.rs` (settings), `server/src/musicbrainz.rs` (Disc ID, parser, lookup), `server/src/ws.rs` (WebSocket). Tests: `server/tests/cd_import.rs`, `server/tests/web.rs`. Background: `docs/spec-cd-ripping.md`.

**The CD drive is at the server** (decision of the project owner, 2026-10-28), so the server reads, encodes, tags and files the disc (section "The drive of the server"). The upload path below stays for files that are ready (another source, an app that rips by itself): the app encodes, names and tags the files **as the settings say** and hands them over; the server checks them again, renames and re-tags them by the same settings (the server is the authority), puts them into the import folder and starts the import.

All paths are under `/api/v1/cd`, with `Authorization: Bearer <device token>`. **Only the roles Admin and Member**; Child and Guest get `403 {"error":"forbidden"}`, no token `401`. Errors are always `{"error":"<code>"}`.

## Settings (`GET /cd/settings`)

Flat JSON, the values of the **tab "CD import" / "CD-Import"** of the web settings page (`/admin/settings?tab=cdimport`; migration 0026). The tab also shows at the top what keeps the import from working (no contact address, no import folder or not writable, import locked) and below the form the last CD imports (book, CD n / m, state, time, error as text with the code in brackets). Refused values are shown in the tab, next to the field and at the top, in the language of the administrator, and nothing is saved then. `cd_log_keep` (50, 10..1000) says how many log entries are kept; it is not sent to the apps.

| Key | Meaning (default) | Setting |
|---|---|---|
| `format` | file format (`mp3`; the list is open for `opus`) | `cd_format` |
| `extension` | ending of the format (`mp3`) | derived |
| `bitrate_kbps` | constant bit rate, 8..320 (64) | `cd_bitrate_kbps` |
| `sample_rate_hz` | one of 8000 11025 12000 16000 22050 24000 32000 44100 48000 (24000) | `cd_sample_rate_hz` |
| `channel_mode` | `joint_stereo`, `stereo`, `mono` (joint_stereo) | `cd_channel_mode` |
| `quality` | encoder quality 0 best .. 9 fastest (3) | `cd_quality` |
| `tag_version` | `2.4` or `2.3`; ID3v2 is binding, ID3v1 alone is never written (2.4) | `cd_id3_version` |
| `file_scheme` | file name without ending (`{author}_{title}_CD{disc:02}_TR{track:02}`) | `cd_file_pattern` |
| `folder_scheme` | folders below the import folder, `/` separated (`{author}/{title}`) | `cd_folder_pattern` |
| `disc_folder_scheme` | folder of one disc of a work with several discs (`CD {disc}`; empty = flat) | `cd_disc_folder_pattern` |
| `title_scheme` | title tag: `file_name` (name without ending) or `track_title` (file_name) | `cd_title_pattern` |
| `track_number_scheme` | `disc_times_100` (disc 2 track 5 = 205) or `plain` | `cd_track_scheme` |
| `genre` | genre tag of an audiobook (Audiobook) | `cd_genre` |
| `cover_size` | edge length of the cover in pixels, 100..2000 (500) | `cd_cover_px` |
| `eject` | eject the disc after a successful import (true) | `cd_eject_after` |
| `kind` | `audiobook` or `music`, used when the app does not say (audiobook) | `cd_target_kind` |
| `udev` | the Linux desktop app detects drives and discs by udev events instead of polling (false); the app has no switch of its own | `cd_udev` |
| `lookup_ready` | a contact address for MusicBrainz is set | `musicbrainz_contact` |
| `device` | the name of this device in jobs (see below) | derived from the token |

Placeholders of the schemes: `{author}`, `{title}` (title of the book or album; `{album}` is the same), `{narrator}`, `{year}`, `{disc}`, `{track}`, `{track_title}`; `{disc:02}` pads a number with zeros. File scheme needs `{disc}` and `{track}`, folder scheme needs `{title}`; folder scheme is the only one that may contain `/`. A value is refused when it has an unknown placeholder, `..`, a backslash, characters no file system allows, or control characters (the settings page answers `422`). The server cleans each value inserted into a name (`/ \ : * ? " < > |` become `_`).

The service addresses (`cd_musicbrainz_url`, `cd_coverart_url`; empty = the public MusicBrainz and Cover Art Archive) and the MusicBrainz contact address (`musicbrainz_contact`) are settings of the same group; the apps do not need them.

## Lookup (`POST /cd/lookup`)

Request: `{"toc": <toc>, "disc_id": "<optional, ignored>"}`. `<toc>` is either the string of the MusicBrainz parameter that the Linux helper has, `"1+14+334409+150+16910+..."` (first track, last track, lead-out, one offset per track, all with the lead-in of 150 sectors), or an object `{"first_track":1,"last_track":14,"leadout_lba":334259,"starts_lba":[0,16760,...]}` (sector numbers as `cd-da-reader` gives them; the server adds 150). Audio tracks only. The server computes the Disc ID itself.

Answer `200`: `{"disc_id":"AzCi4YfdjUrdmGcBX7URtQixDmE-","candidates":[...]}`, `candidates` empty when MusicBrainz does not know the disc (it is asked by Disc ID, then by the table of contents; both answers, also "nothing", are cached in the database for 30 days / one day). A candidate:

```json
{"release_id":"<mbid>","title":"Sturz der Titanen","author":"Ken Follett","narrators":["Johannes Steck"],
 "credits":[{"name":"Ken Follett","join":" gelesen von "},{"name":"Johannes Steck","join":""}],
 "date":"2010-09-28","year":"2010","language":"deu","disc":2,"discs_total":12,"disc_id_matched":true,
 "tracks":["Teil 014","..."],"track_details":[{"number":1,"title":"Teil 014","length_ms":223000}],
 "cover":"<mbid>","cover_available":true}
```

`author` is the first name of the credit, `narrators` the others (a person decides when that is wrong; `credits` has the joining texts). `disc` is `null` when the disc itself is not in the release. `cover` is `null` without a cover, else the reference for `GET /cd/cover`.

Errors: `422 invalid_toc`, `409 no_contact` (no `musicbrainz_contact`: nothing is ever sent without it), `502 lookup_failed` (the service does not answer or answers nonsense). The server keeps one request per second to MusicBrainz and sends `david-server/<version> ( <contact> )` as User-Agent.

## Cover (`GET /cd/cover?ref=<mbid>`)

`ref` is the `cover` of a candidate (`release` is accepted as the same). Answer: the picture (`image/jpeg` or `image/png`), from the Cover Art Archive at the size nearest `cover_size` (250, 500 or 1200), cached. Errors: `404 not_found` (also cached), `422 invalid_release`, `409 no_contact`, `502 lookup_failed`. The server does not resize.

## Import (`POST /cd/import`, multipart)

Two ways to say what the CD is. **Way 1 (the Linux app)**: text fields first, then the files:

```
kind=audiobook|music   author=...   title=...   disc=2   discs_total=12      (kind, discs_total optional; narrator, year, mbid, genre optional)
path=<name of the file>  file=<the file, filename percent-encoded>            (repeated for every file)
path=cover.jpg           file=<the cover>                                     (optional; a jpg/jpeg/png part is the cover)
```

The files are already named and tagged; the server takes the track number from the tag (minus `disc x 100` for `disc_times_100`), else from `_TRnn` in the name; with `title_scheme=track_title` it also takes the track title from the title tag. Parts that are neither audio of the format nor a picture are read and dropped. **Way 2**: a JSON part `meta` first: `{"author","title" (or "album"),"narrator?","year?","disc","discs_total?","kind?","mbid?","genre?","job_id?","tracks":[{"number":1,"title":"...","file":"t1.mp3"}]}`, then one `file` part per track whose filename is `tracks[].file`, optional `cover` part. With `mbid` and no cover part the server fetches the cover itself (best effort).

What the server does, in this order, and refuses with a code when it cannot (nothing stays behind; the staging folder `<data dir>/cd-import/<uuid>` is removed in every case and old leftovers after a day):

1. numbers: tracks `1..n` without gap or double (`422 track_numbers`), texts and numbers sane (`422 invalid_meta`);
2. each file readable and as the settings say: format, sample rate, bit rate (+-1 kbit/s), channels (`422 unreadable_file`, `422 format_mismatch`; `415 bad_extension` for audio of another format; `422 unknown_file` / `422 missing_file` in way 2; `422 bad_cover`);
3. names and tags are made by the settings: file name from `file_scheme`, title tag from `title_scheme`, track tag from `track_number_scheme`, disc, album, artist and album artist, narrator (composer), year, genre (`genre` for an audiobook, the app's `genre` for music), the cover embedded; tag version `tag_version` (`422 bad_tags`);
4. placed in `<import folder of the kind>/<folder_scheme>/`; a work with several discs (`discs_total > 1` or `disc > 1`) gets `<disc_folder_scheme>` (`CD 2`) below the book folder, unless the book folder already holds audio files directly (a book that is kept flat stays flat); the cover goes next to it as `cover.jpg` if the book has none (`503 no_import_folder`, `507 no_space`, `409 disc_exists` when a target file exists: never overwritten);
5. a job is closed `done`; the import of the library is started (`import:"queued"`), or not when the import is locked (`import:"locked"`, the files are placed anyway).

Answer `201`: `{"id":12,"job":{...},"placed":{"folder":"Ken Follett/Sturz der Titanen","files":[...]},"import":"queued"|"locked","eject":true}`. The app ejects only on success and only when `eject` is true. Other errors: `413 too_large`, `400 bad_request`.

## Jobs

A job is one CD on its way in. It is made by `POST /cd/import` (and, optionally earlier, by the app with `POST /cd/jobs`, so that other devices see the ripping). The JSON of a job:

```json
{"id":12,"device":"a1b2c3d4e5f6","phase":"importing","state":"working","active":true,"fraction":0.5,
 "track":7,"tracks_total":14,"author":"Ken Follett","album":"Sturz der Titanen","title":"Sturz der Titanen",
 "disc":2,"discs_total":12,"error":"","message":"...","import_ran":false,"created_ms":0,"updated_ms":0}
```

`phase`: `ripping`, `encoding`, `uploading` (set by the app), `placing`, `importing` (set by the server), `done`, `failed`. `state` is the same in the words `working`, `done`, `failed` (`queued` is not used). `fraction` is `track / tracks_total` or `null`. `error` is a short code when failed (`disc_exists`, `format_mismatch`, `import_failed`, `interrupted` after a restart, `stale` after 15 minutes without news, `client` ...). `device` is a piece of the hash of the token, the same value as `device` in `/cd/settings`; the token is never shown. Jobs belong to the account (user), not to a device; another account gets `404`.

| Request | Answer |
|---|---|
| `POST /cd/jobs` `{tracks_total?,author?,album?,disc?,discs_total?,phase?}` | `201 {"job":{...},"busy_elsewhere":bool}`; announces that this device starts ripping. `phase` is `ripping` (default), `encoding` or `uploading`. `422 invalid_job` |
| `PUT /cd/jobs/{id}/progress` `{phase?,track?,tracks_total?,author?,album?,disc?,discs_total?,error?,message?}` | `200 {"job":{...}}`; `phase` one of `ripping encoding uploading failed` (`failed` needs/gets an `error` code); `409 job_closed` for a finished job or one the server is working on; `404` |
| `GET /cd/jobs` | `{"jobs":[the open ones and the last finished, newest first],"busy":bool,"busy_elsewhere":bool,"device":"..."}`; `busy_elsewhere` = a job under way belongs to another device of the account |
| `GET /cd/jobs/{id}` and `GET /cd/import/{id}` | `{"job":{...}}` (the fallback when the event channel is down); `404 not_found` |
| `GET /cd/import` | `{"jobs":[only the open ones]}` |

## Event channel (`GET /cd/events`, WebSocket, `wss` behind the server's TLS)

Authentication as everywhere: `Authorization: Bearer <token>` in the upgrade request. Not an upgrade request: `400 bad_upgrade`; role: `403`. Implemented in `ws.rs` on top of hyper's upgrade (RFC 6455: text messages, ping/pong, close; no compression, no fragments from the client). Every message is JSON text with `"v":1` and a `"type"`:

| `type` | when | members |
|---|---|---|
| `hello` | first message | `device` (this device's name), `heartbeat_s` |
| `snapshot` | second message, and after a lost-messages gap | `jobs` (as `GET /cd/jobs`), `busy`, `busy_elsewhere` |
| `job` | a job was created | `job` |
| `progress` | phase, track or counts changed | `job` |
| `done` | a job finished well | `job` (with `message`, `import_ran`) |
| `failed` | a job failed or was given up | `job` (with `error`, `message`) |

Only the jobs of the account are sent, to all its devices (also to the device that made the change). A device tells "another device is ripping" from `job.device != hello.device` and `job.active`. The server sends a WebSocket ping every `heartbeat_s` (25 s); the client answers with a pong (any frame counts as news). It checks the session at every heartbeat: **a revoked or signed-out token ends the connection with close code 4401**; no frame from the client for three heartbeats: close 4408; an unmasked frame: 1002; a frame over 64 KiB: 1009; a client close is answered with 1000. After a break the client connects again and reads the `snapshot`; missed events are never replayed.

## The drive of the server

Code: `server/src/cdrive.rs` (state, monitor, rip, eject), the crate `server/cdrip/` (drive, TOC, tray state, uevents, encoder, ID3, ripper). A job made by a rip has `device: "server"`.

**State** (`GET /cd/drive`, and the event `drive`): `{"v":1,"type":"drive","mode":"udev"|"poll","drives":[{"device":"/dev/sr0","tray":"open"|"closed"|"unknown","medium":"none"|"audio"|"other"|"unknown","tracks":14,"track_list":[{"number":1,"seconds":447}],"disc_id":"...","toc":"1+14+...","seconds":3600,"denied":false,"ripping":false}]}`; `GET` adds `busy`. The device path is whatever the system reports (nothing is built in); the setting `cd_device` can name one drive, empty = every drive found. `tracks` is the number of audio tracks; `track_list` lists them with their numbers on the disc and their lengths in seconds (from the table of contents), for a choice of tracks. `tray: unknown` means the drive is not ready (moving, spinning up) and is looked at again after a second. `denied` = the service user may not open the drive (group missing; the tab warns and `FAQ.md` says how). The event is sent to **every** device of every account whenever the state changes (and once after the snapshot when a device connects); apps show their import and eject symbols from it: import only for `tray: closed` with `medium: audio`, eject for a shut tray with `audio` or `other`, nothing for an open tray (nothing ever closes a tray).

How the server learns of changes: with `cd_udev` = 1 from kernel uevents (`NETLINK_KOBJECT_UEVENT`: `DISK_EJECT_REQUEST` of the eject button and `DISK_MEDIA_CHANGE` are the cause to look; the real state is then read with the ioctl `CDROM_DRIVE_STATUS`: open, no disc, not ready, disc ok, and only then the table of contents), plus a look every minute as a safety net; with `cd_udev` = 0 by a look every `cd_poll_seconds` (2..300, default 5). Both settings are read all the time, so a change applies without a restart (`mode` says which is active; if the kernel events cannot be opened it stays `poll`). While a rip runs the drive is not probed. The table of contents is read again only for a new disc (the kernel's media-change flag, or an event).

| Request | Answer |
|---|---|
| `GET /cd/drive` | the state above |
| `POST /cd/rip` `{author,title,disc,discs_total?,kind?,narrator?,year?,mbid? (or release_id),tracks?:[titles],only?:[numbers],eject?,device?}` | `201 {"id","job"}` at once; the work runs in the background. One rip per drive. Reads the CD (every track twice and compared when `cd_verify_read`), encodes by the settings of the tab, tags, takes the cover from the Cover Art Archive if `mbid` is given, then goes through the same checking, naming, placing and import as an upload (`finalize`). Files are made in `<data dir>/cd-import/rip-<uuid>` and removed whatever happens; nothing half done reaches the library. `eject` (default: setting `cd_eject_after`) opens the tray after success, only if the tray is shut. Refusals: `409 busy`, `409 no_disc`, `409 tray_open`, `409 no_drive`, `503 drive_denied`, `422 invalid_meta`, `503 no_import_folder`, `409 no_contact` is **not** a refusal (the cover is skipped) |
| `POST /cd/rip/{id}/cancel` | `202 {"cancelling":true}`; the flag is looked at before every command to the drive (a command in flight ends at the drive's own timeout of 30 s). The job fails with `cancelled`. `404 not_found` if no rip with this id runs |
| `POST /cd/eject` `{device?}` | `200 {"result":"ejected"\|"nothing_to_eject","ejected":bool}`. The real state is asked first: a tray that is open, or a drive without a disc, gets no command (the kernel's eject toggles an open tray shut). `409 busy` while a rip runs; `404 no_drive`; `503 drive_denied` |
| `POST /cd/lookup` | `toc` may be left out: the server then uses the disc in its own drive (`409 no_disc`) |

**Choice of tracks.** `only` lists the numbers (on the disc, as in `track_list`) of the tracks to read; absent = all. The files keep their numbers on the disc: the name has `TRnn` and the tag the track `disc x 100 + nn` of the disc, so a choice has **gaps** (the rule "numbers 1..n without a gap" holds only for files that are handed over). `tracks` is always the list of titles for the **whole disc in disc order** (the `tracks` of a candidate), whatever `only` says; the server takes the titles of the chosen numbers from it. The progress counts the chosen tracks only (`track` / `tracks_total`, "3 of 5"). `422 empty_selection` for `only: []`, `422 invalid_selection` for a number that is not on the disc or named twice. A disc may be completed later: files that exist are never overwritten, and a start whose choice includes a track that is in the library already is refused **before the disc is read** with `409 {"error":"disc_exists","taken":[3,5]}` (the numbers that are there; the job log gets an entry with the same numbers); a choice with free numbers only is taken over.

**Candidates** (`POST /cd/lookup`) carry the titles already: `tracks` (strings, in disc order) and `track_details` (`number`, `title`, `length_ms`), and `audiobook` (true if MusicBrainz marks the release group as an audiobook / audio drama / spoken word, or the credit names a speaker besides the author); the web tab and the apps start "Import as" at `audiobook` for such a release, else at the setting `cd_target_kind`.

Progress of a rip comes as the usual job events (`job`, `progress` with `track` = tracks done and `fraction` inside the track, `done`, `failed`). Codes a rip adds to jobs: `read` (not readable), `verify` (two reads never agreed), `drive_gone`, `encode`, `no_space`, `io`, `cancelled`, `unsupported`; refusals add `busy`, `no_drive`, `no_disc`, `tray_open`, `drive_denied`, `eject_failed`, `invalid_selection`, `empty_selection`.

**How a rip reads (so that the drive runs evenly).** (`server/cdrip/src/rip.rs`, `server/cdrip/src/ring.rs`) The drive is pumped, not fed piece by piece: this thread reads the tracks in the direction of the disc and writes the PCM, chunk by chunk (27 sectors, the most one `READ CD` takes), into a **ring buffer** in memory (made once: a byte ring with a write position and a read position, `Mutex` and two condition variables). It goes on over the border of a track, because audio is continuous, and waits only when the ring is full. A second thread takes the PCM of one track after the other out of the ring (the ends of the tracks are marked by offsets), encodes it with LAME and tags it into memory and hands the result on at once to a third thread that writes the files (`.part`, sync, rename later); the encoder therefore never waits for the disk and empties the ring also at the borders of the tracks. Cancel, an error of any thread and a stopped encoder wake both sides of the ring, so nothing waits for ever. Before, reading, encoding and writing were one after the other and the drive stood still in between.

The size of the ring is a setting (migration 0033): `cd_buffer_seconds` is the seconds of audio it holds, **0 = automatic** (the default): the longest chosen track by the table of contents plus 10 s, so that a whole track fits; otherwise 2 to 900. `cd_buffer_max_mb` (default 256, 16 to 4096) is the most memory it may take, also for the automatic size; a track that does not fit under the limit is read through as before (the reader waits when the ring is full). 1 s of audio is 176,400 bytes; a typical audiobook track of 20 minutes needs 1,210 s = about 213 MB, a track of 5 minutes about 55 MB. A setting applies to the next rip. Two jobs at once do not exist (one rip per drive), so the memory is that of one ring.

The second read of a track (setting `cd_verify_read`) is shifted against the first (setting `cd_verify_shift`, migration 0036; 0 to 26 sectors, default 13, 0 = no shift): its first command to the drive is that many sectors short, so every later command lies off the grid of the first read, and nothing outside the track is read. Reads that follow because the two disagree use other shifts (multiples of the setting modulo 27). The reason is the one cd-paranoia gives for its "jiggle": a fault tied to the border of a command (a drive that reads it wrong the same way every time) would otherwise be read the same way twice and so pass the comparison. A rip with the shift has one command more per track than one without.

The encoder thread runs at the priority of the setting `cd_encoder_nice` (migration 0034; -20 = highest, 19 = lowest; default -20), so that the PCM leaves the ring as fast as the drive fills it. A plain user may not raise the priority; the service unit sets `LimitNICE=40`, which allows -20 to the user `david`. If the system refuses the value, the nearest one it allows is used, and the journal line of the rip ("rip measured") says which value the encoder got or that the system refused all of them.

What the ripper measures is shown while it runs and written down at the end: the job JSON and `drives[].rip` of `GET /cd/drive` have `buffer_seconds` (audio waiting in the ring now), `buffer_max` (the ring in seconds), `buffer_percent`, `reader_waits` (how often the reader found the ring full and had to wait) and `reader_wait_max_s` (the longest wait). A full ring means the encoder or the writing brakes, an empty one that the drive is the brake. After the rip the message of the job says in one line: `drive read 1500 s, reader waited 14.2 s in 3 pauses (longest 6.0 s), buffer peak 41 s of 230 s` (also in the journal).

With `cd_verify_read` the second reading is a **second pass over the disc** in the same direction (one jump back to the start, not one per track) that only compares SHA-1 hashes; a track whose two reads differ is read once more (three reads at most) and the two that agree win (the MP3 is made again from the work file if the first read was the wrong one). A rip without verification reads each sector once; the setting stays, default on. No PCM is written to disk on the normal path (no fsync that would hold the reader up); the MP3 of a track is written in one piece. Files get their name only when whole and, with verification, only after every track was read alike twice. `cd_read_speed` (0 to 52, default 8; 1x = 176 kB/s, 0 = the drive's own) is set before the rip with `SET CD-ROM SPEED` (SCSI 0xBB through `SG_IO`) and given back (0xFFFF, the drive's fastest) after every end, success, error or cancel; a drive that refuses is logged and keeps its speed. The progress `fraction` of a rip is that of the whole job (both readings), not of one track.

**The service leaves the drive alone while a rip runs.** On the device that rips, nothing asks the drive: not the interval look, not the kernel events (the look skips a busy drive and keeps the state it had), not the safety look every minute, not the web panel (it draws from the state the service holds and the table of contents it read before) and not the API (`GET /cd/drive`, `POST /cd/lookup` without `toc` use the same held state; a second rip and an eject are turned away with `busy` before they touch the drive). A test counts the probes of the fake drive over a running rip.

Settings added (migration 0029): `cd_poll_seconds`, `cd_device`, `cd_verify_read`; `cd_read_speed` follows in migration 0030 (0029 was in a package already); `cd_udev` (0026) now acts in the server, and `GET /cd/settings` still returns it as `udev`. Web: the menu item "CD import" (page `/admin/cd`, next to "Import") shows the drive state, the MusicBrainz releases for the disc in it (choose one to fill the form), one form with "Import" / "Cancel", the progress with a stop button, and "Eject"; it refreshes every 3 seconds through htmx and keeps the form while nothing changes (`GET /admin/rpc/cd.drive?sig=` answers 204 when the shown state is the same). Above the fields is the **list of tracks** (box, number, title, length; all ticked at first so that "Import" reads the whole CD; the titles come from the chosen release, or from the only release MusicBrainz knows; before that "Track 01"), the buttons All / None / Invert (a small function in `static/ui.js`; without it all boxes stay ticked), a line "n of m tracks, mm:ss" that follows the ticks, and an "Import" button that is disabled with 0 tracks (the server refuses `empty_selection` as well). When the panel is drawn again because the state changed, the page sends back the ticks and the typed text with the request (`sel=1`, `disc_id`, `only`, the fields) and they are kept as long as the same disc is in. Under every template field of the tab (file name, folder, folder of a CD) is a line with the placeholders that field knows, in plain words and as buttons that insert the placeholder at the cursor (visible while the field has the focus, always visible as text without JavaScript), and an example that follows the typing: `GET /admin/rpc/cd.preview?field=<setting>&pattern=<text>` (the admin session; the value may also come as the parameter named like the field) runs the real naming with sample values from the catalog and answers `Example: Ken Follett_Sturz der Titanen_CD03_TR05.mp3`, or the reason why the template is refused, with the placeholders in plain words (`Unknown placeholder {x}; allowed are: Author {author}, ...`).

### Resuming an interrupted rip

A rip works in `<data dir>/cd-import/disc-<Disc ID>/` and writes `manifest.json` there (in one piece: `.part`, then rename; a cut-off manifest counts as none): the Disc ID, the settings that decide the audio (format, bit rate, sample rate, channel mode, quality), the choice of tracks, and for every track its number, file, size, the SHA-1 of the PCM that was read, the length by the table of contents and the state `encoded` (written, to be confirmed by the second reading) or `done`. A track counts only when its file and its entry are written. Names and tags are given when the files are placed, so the title and author typed in the form do not matter for a resume.

When the service stops (a restart, a package update) the open jobs are closed `interrupted` (as before) and the folder stays for `cd_resume_keep_hours` (default 48, setting of the tab; migration 0031). A stop by SIGTERM lets the rip finish the track it is on if that takes no more than 10 s, writes the manifest and ends (the unit has `TimeoutStopSec=60`). The folder also stays after `read`, `verify`, `drive_gone` and `cancelled`; after other failures and after success it is removed. Folders without a good manifest are removed after a day.

The same disc again: `POST /cd/rip` takes over what checks out (the file is there, has the size of the manifest, can be read as audio and is as long as the table of contents says, ±2 s) and reads only the rest; a track that does not check out is read again. With `cd_verify_read` the new tracks are read twice, the taken-over ones count by the manifest. The progress counts them as done at once. A different choice of tracks still resumes: what was read and is chosen again is taken over, a track that was read and is not chosen now is dropped (its file goes), and a track that is chosen now and was not read is read. Only a change of the settings that decide the audio means no resume: the old folder is removed when the new rip starts and the answer says `"resume":"mismatch"` (hint code `resume_mismatch`). A repair (`mode: "heal"`) takes over an interrupted rip only with the same choice. A track that was read once and waits for the second reading is kept as `NN.mp3.part` with the state `encoded` and is taken over like a finished one (its second reading is still to come). `"resume": false` in the request throws the old folder away (`"resume":"fresh"`); the default is `true`.

`resume` in every drive of `GET /cd/drive` and of the `drive` message (`null` if there is nothing): `{"disc_id","tracks_done":[1,2,...],"tracks_total":n,"only":[...],"interrupted_at":ms,"keep_until":ms,"settings_match":bool}`. `POST /cd/rip` answers `{"id","job","resume":"resumed"|"fresh"|"mismatch"}`. For the app: show "This CD was read to 8 of 11 tracks already (interrupted <time>). Continue / Read again" when `resume` is not null and `settings_match` is true and `only` equals the choice; the buttons send `resume: true` / `resume: false` with the same details; if `settings_match` is false say that the CD will be read again.

### A CD that is in the library already, checking and repairing

A CD is **present** when an earlier import stored its Disc ID (tables `cd_disc` and `cd_disc_track`, filled by every import and every repair; migration 0032) or when the library has a book of the same author and title with tracks of this CD number (how the CDs were ripped by hand; the author, title and CD number come from what the caller typed or from the one release MusicBrainz names). The table of contents is compared with the files: a number without a file is **missing**; a file that is empty, cannot be read as audio, or differs from the length of the table by more than `cd_heal_tolerance_s` (default 3, 1 to 60) is **damaged**. The SHA-1 of the PCM is stored for information only: the server has no decoder.

`present` in every drive of `GET /cd/drive` and of the `drive` message (`null` if the CD is not in the library or no look was made): `{"book","author","disc","via":"disc_id"|"book","tracks_in_library":n,"tracks_on_disc":m,"missing":[4,9],"damaged":[{"number":7,"reason":"empty"|"unreadable"|"length","have_s":192,"want_s":245}],"complete":bool}`.

* `POST /cd/rip` (mode `import`, the default) for a CD that is present is turned down with `409 {"error":"disc_present","book","author","disc","missing","damaged","present":{...}}` and starts nothing; `"force": true` imports it as a new copy anyway (overwriting stays forbidden: `disc_exists` with `taken`).
* `POST /cd/rip` with `"mode":"heal"` reads **only the missing and damaged tracks** (the details come from the library; nothing needs to be typed), puts a damaged file that is there into the quarantine first (`quarantine_folder`, below the import folder in the same place, the name with `.<unix time>` appended), then places the new file with the same name pattern as the other files of this CD (`Follett_..._CD02_TR09.mp3` stays so, not the pattern of the settings), tags it like the others and starts the import. If placing fails the damaged file is put back. Errors: `409 not_present`, `409 nothing_to_heal`. A track that cannot be read after the second reading is told at the end (job `done` with `error:"unreadable_track"` and the numbers in `message`); the others are repaired anyway.
* For the check before the repair nothing is read from the CD: the person looks at `present` (the web tab shows it as "This CD is in the library already", with Missing / Damaged and the buttons Repair / Cancel; nothing happens without the click) and the app shows the same. "The CD is whole in the library (11 of 11 tracks)" when `complete`.

### Progress with a list of tracks

The job JSON of a rip that the server does (`device:"server"`) has `track_states` (also in `GET /cd/import/{id}`, `GET /cd/import` and the events), and every drive of `GET /cd/drive` has `rip: {"job_id","finished":bool,"error":"","fraction":0.4,"track_states":[...]}` for the rip that runs, or ran last until it is closed. `track_states` lists the **chosen** tracks only: `[{"number":3,"title":"Teil 031","seconds":245,"state":"reading","fraction":0.4,"error":""}]`; `state` is `waiting`, `reading` (first reading; `fraction` is that of the track), `verifying` (second reading), `encoding`, `done` or `failed` (then `error` is the code). Tracks taken over from an earlier run are `done` from the start. `fraction` of the job is that of the whole job (both readings). The list stays with the result after the end until a new rip starts or `POST /cd/rip/dismiss` (`{device?}`, `204`, `404` if there is none) closes it.

### Automatic import (`cd_auto_import`)

Off by default (migration 0032; needs `cd_udev` on). A disc that is put in (a new disc with the tray shut; putting it in again counts anew) starts an import by itself only if MusicBrainz names exactly one release for it, the CD is not in the library, there is no interrupted rip of it, the target is writable and there is room: the type is the release's (audiobook by the type of the release group or a speaker), all tracks, eject by `cd_eject_after`. The job belongs to `system` (audit field) and has `device:"auto"`, shown as "automatic" in the log. **Every other case writes one entry** into the log of the tab and a journal line (`info`: `device`, `disc_id`, `reason`, `detail`, author, book, disc) and is no error: phase `skipped` (state `skipped`; these entries are not sent as events and not in `GET /cd/jobs`), `error` = the reason, `message` = the detail:

| Reason | Meaning (detail) |
|---|---|
| `disc_present` | the CD is whole in the library (book, CD number) |
| `heal_needed` | it is there but tracks are missing or damaged (book, CD number, "n of m"); repair it in the panel |
| `resume_pending` | an interrupted rip of this CD waits ("n of m"); not continued by itself |
| `ambiguous_match` | several releases (their number); choose one in the panel |
| `no_match` | MusicBrainz does not know the CD |
| `no_contact` | no `musicbrainz_contact` |
| `lookup_failed` | MusicBrainz or the network did not answer |
| `busy` | a rip runs on the drive |
| `not_audio` | the disc is not an audio CD |
| `drive_denied` | the service may not use the drive |
| `no_target` | no import folder the server may write to |
| `no_space` | not enough free space |
| `udev_off` | `cd_auto_import` is on but `cd_udev` is off (once at the start and at a change of the setting) |
| `start_failed` | the start was refused (detail: the code) |

## MCP tools for the CD drive (2026-10-30)

For an assistant on `/mcp` (`david run --mcp`, see `docs/releasing.md`) that reads a CD from photos of its cover and back: it asks the drive, fills in author, title and track titles and starts the rip. Code: `server/src/mcp.rs` (the tools), `server/src/cdrive.rs` (`rip_inner`, shared with the web form and `POST /cd/rip`: there is one code path, `preview_rip` stops it before anything is created). Tests: `server/tests/mcp_cd.rs`.

**Who.** All three tools need a token that an **administrator made** (`mcp_token.created_by`; the role of that profile is read at every call, so a demoted or deleted administrator loses the tools at once). Others get an error that says so. `cd_rip` with `dry_run=false` needs a token that may write too.

| Tool | Does |
|---|---|
| `cd_drive` | Read only. `drives` (as `GET /cd/drive`, with `present` and `resume`), `rip_running`, `disc` (`device`, `disc_id`, `tracks` with `number` and `seconds`, `total_minutes`) and `lookup`: `status` (`ok`, `no_disc`, `no_contact`, `no_match`, `lookup_failed`), `contact_set` and `candidates` (as in `POST /cd/lookup`: title, author, tracks, `release_id`). |
| `cd_rip` | Parameters `author`, `title`, `tracks` (one title per **selected** track, in disc order), `disc` (1), `discs_total` (1), `narrator`, `kind` (`audiobook`/`music`, default the setting `cd_target_kind`), `mbid`, `eject` (default the setting), `only_tracks` (numbers on the disc), `dry_run` (**default true**). A wrong number of titles is refused with both counts ("2 track titles were given but 3 tracks are selected"), before anything else. With `dry_run` the answer is the plan: `would_start`, `refusal` (`disc_present`, `disc_exists`: reported, not an error), `folder`, `files` (`number`, `title`, `file`, `seconds`, `tag_title`, `tag_track`, `exists`), `tags`, `format`, `eject`, `present`, `warnings` (an album of that name or this disc is in the library already, files that exist, a folder that exists, an interrupted rip waits, no `mbid` so no cover, no contact address for the cover); nothing is created: no job, no staging folder, no log entry. With `dry_run=false`: `{"started":true,"job_id","resume","job"}` and an entry in the change log (`CdRip`, token, job, author, title, CD n of m). Refused as the API refuses: `busy` (also for the plan), `no_disc`, `tray_open`, `no_drive`, `drive_denied`, `invalid_meta`, and `disc_present` / `disc_exists` for the start. A hand-filled rip needs no MusicBrainz contact address; it matters for lookups and the cover only. |
| `cd_progress` | Read only. `state`: `none`, `running`, `done`, `done_with_warnings` (the job ended with `unreadable_track` or `unverified_track`), `failed`, `cancelled`; `percent`, `error`, `tracks` (`number`, `title`, `seconds`, `state`, `percent`, `error`), `track_errors`, `job`. `job` selects a job, default the newest rip of the server. |

Not built: `set_cover_from_image` (a cover as base64). The server has no image decoder in its production build (see "Not done" above), so the data could not be checked by decoding; the existing `set_cover_from_url` and `set_cover_from_folder` stay the ways to set a cover.

## Rules the server keeps

* Nothing is built in: formats, ranges, names, genre, addresses and the contact address are settings; the program only has the named defaults of the two public services and the fallbacks for a missing or damaged value (the values of migration 0026).
* No lookup without `musicbrainz_contact`. Lookups reach only public addresses (`fetch::get_bytes`).
* Tests need no internet: the lookup is a trait (`musicbrainz::Lookup`) with a fake in the tests.

## Not done / open

* The server does not resize covers (no image library in the server); the app delivers `cover_size`.
* "Already ripped before" detection by Disc ID is not built (spec-cd-ripping.md 4.4.1).
* `opus` is not accepted yet; adding it means an entry in `cd_settings::FORMATS` and the check in `cdimport::check_file`.

## Error codes

The codes are English and stay the same; they are in `{"error":"<code>"}` of the API and in `error` of a job. The page "CD import" shows each as `cd.err.<code>` in EN and DE (`server/src/admin/messages.rs`); a test checks that every code has both texts. The app writes its own texts by the same codes. A code the server does not know is shown as `other`.

| Code | Meaning | HTTP |
|---|---|---|
| `forbidden` | the role is not Admin or Member | 403 |
| `unauthorized` | no or an invalid token | 401 |
| `no_contact` | no `musicbrainz_contact` in the settings, so no lookup | 409 |
| `invalid_toc` | the table of contents is not valid | 422 |
| `invalid_release` | `ref` is not a MusicBrainz identifier | 422 |
| `lookup_failed` | MusicBrainz or the Cover Art Archive is unreachable or answered nonsense | 502 |
| `not_found` | no such job or no cover | 404 |
| `invalid_job` | the details of a job are not valid | 422 |
| `job_closed` | the job is finished or the server works on it | 409 |
| `invalid_meta` | author, title, numbers or tracks are missing or not valid | 422 |
| `track_numbers` | gap or double in the track numbers, or a file without a number | 422 |
| `unreadable_file` | a file is damaged, empty or not audio of the format | 422 |
| `format_mismatch` | sample rate, bit rate or channels differ from the settings | 422 |
| `bad_extension` | the ending is of another audio format | 415 |
| `unknown_file` | a file that `meta` does not list | 422 |
| `missing_file` | a file that `meta` lists did not come | 422 |
| `bad_cover` | the cover is not JPEG or PNG | 422 |
| `bad_tags` | tags could not be written | 422 |
| `disc_exists` | a target file exists; nothing is overwritten | 409 |
| `no_space` | not enough free disk space | 507 |
| `no_import_folder` | no import folder for the kind | 503 |
| `too_large` | the upload or a file is too large | 413 |
| `bad_request` | the upload could not be read | 400 |
| `bad_upgrade` | not a valid WebSocket request | 400 |
| `internal` | internal error, see the server log | 500 |
| `import_failed` | files placed, the import of the library failed (job) | - |
| `interrupted` | the server restarted while the job ran (job) | - |
| `stale` | no news of the job for 15 minutes (job) | - |
| `busy` | the drive already rips a CD (or a rip/eject was refused for it) | 409 |
| `no_drive` | the server has no CD drive (or `cd_device` names one that is not there) | 409 / 404 |
| `no_disc` | no audio CD in the drive, or a lookup without a disc | 409 |
| `tray_open` | the tray is open | 409 |
| `drive_denied` | the service user may not use the drive (group missing) | 503 |
| `eject_failed` | the drive did not open its tray | 500 |
| `invalid_selection` | `only` names a track that is not on the disc, or one twice | 422 |
| `empty_selection` | `only` is empty | 422 |
| `disc_present` | the CD is in the library already; check and repair it (409, with `present`) | 409 |
| `not_present` / `nothing_to_heal` | heal for a CD that is not in the library / that is whole | 409 |
| `unreadable_track` | a track could not be read or confirmed; it was left out and the others were filed (job `done`, also for an ordinary import) | - |
| `unverified_track` | a track was not read the same way twice and was filed with its first reading (setting `cd_accept_unverified`); job `done` | - |
| `resume_mismatch` | hint: settings or choice changed, the CD is read again | - |
| `heal_needed`, `resume_pending`, `ambiguous_match`, `no_match`, `no_target`, `udev_off`, `not_audio`, `start_failed` | reasons the automatic import turned a disc down (see above) | - |
| `verify` | a track was not read the same way twice | - |
| `locked` | files placed, the import is locked (state of a finished job, not an error) | - |
| `client`, `cancelled` | the app stopped the job (set by the app) | - |
| `read`, `drive_gone`, `encode`, `io`, `unsupported` | reasons an app may report with `phase: "failed"` | - |

A refused import that never became a job (for example `409 disc_exists`, `422 invalid_meta`) is written to the log as a failed entry with its code, so it shows in the tab; the same entry is sent as a `failed` event to the devices of the account. The log keeps the last `cd_log_keep` entries.

Reasons why a value of the tab was refused (`cd.verr.<code>`, only in the web page): `out_of_range`, `not_allowed`, `invalid_text`, `invalid_address`, `template_empty`, `template_syntax`, `template_unsafe`, `template_unknown_placeholder`, `template_missing_placeholder`.

## Releases: the disc, near ones, and a search by hand (2026-10-29)

`POST /cd/lookup` gives the releases of the disc: those whose Disc ID is exactly the one of the disc come first (with the tracks of that disc), then those that MusicBrainz finds by the table of contents (another pressing, a wrong or incomplete entry), without doubles. Two more fields serve a person who finds the right release missing:

- `query` (words, at least two letters, at most 200): the releases that a search of MusicBrainz finds for the words (`release:(…) OR artist:(…)`, 15 at most) follow the others. They come **without tracks** (`tracks: []`). Error `invalid_query` for a query that is too short or too long.
- `release_id` (an MBID): the answer is that one release with the tracks of its medium that has as many tracks as the disc (else the first medium), `disc` is that medium's position. Error `invalid_query` for something that is no MBID.

The web page (`/admin/cd`) has a search field above the form and the app has one in the dialog; both let the person choose from all matches, and a chosen release from a search is completed with `release_id` at once. The words stay in the panel while it is drawn again (hidden field `mbq`).

## A track that cannot be confirmed

**Cache (2026-10-29).** A drive keeps what it read last in a cache and may serve a second reading of the same place from it, which confirms every fault of the first. Before a repeated reading (the third and fourth) of a track, the helper reads about 4 MB (1800 sectors) from the start of the track of the disc that lies farthest from it, so that the cache holds other data; what that read returns is thrown away and a failure of it does not matter. The first two readings need no flush: the second one comes after all other tracks were read. A disc with one track has no far place and is not flushed.

**A scratch (measured 2026-10-29).** Raw readings of a track with a scratch (a BD drive, CD 4 track 7) showed the damage as a run of sectors, 2 at a time every 17.5 sectors (one turn of the disc), where the drive could not correct the data and **guessed** the samples, differently at each reading: about 30 of the 588 frames of a sector differ, by 60 to 90 typically (the music has about 2565 rms), by 4000 at most; the rest of the track is identical at every reading. No frame is lost or repeated, so the comparison with shifts cannot find the readings equal, and `cd-paranoia` waits for readings that never agree. Two readings count as the same music with **concealed errors** when the differing sectors are at most 5 % of the track (at least 16), each differs in at most 420 frames, and no sample differs by more than 8192 (`align::concealed_agree`). The track is then filed with the later reading, the rip reports `Concealed { track, sectors }`, and the job message names the track and the number of sectors (`scratched (the drive guessed sectors): 7 (172 sectors)`). More garbage than that is no agreement.

**Readings that slip (cd-paranoia style, 2026-10-29).** Before a track counts as "cannot be confirmed", the repeated readings are compared the way `cd-paranoia` does: not byte by byte but with a tolerance for a few frames lost or repeated. The rip reads the track a third and, if no hash matches, a fourth time into work files; two readings agree when they are equal but for shifts of at most two sectors (1176 frames), each shift followed by at least 128 frames that agree again (at most 400 shifts per track). A changed sample, a burst of garbage or a shift larger than two sectors is a real disagreement. The MP3 is then made from the reading that was confirmed. This costs two extra readings, and only of a track whose first two readings differ; there is no setting for it (`JobSpec.verify_tolerant`, true, exists for tests).

A drive that does not read an audio CD bit-exact (a Blu-ray or DVD drive for one) gives two readings of the same place that can differ a little. When no two of the readings of a track agree (the first, the second and the repeated one), the rip does **not** stop (project owner, 2026-10-29):

- `cd_accept_unverified` = `0` (default): the track is left out, its state is `failed` with `unreadable_track`, the other tracks are imported, the job ends `done` with `error: "unreadable_track"` and the message names the tracks. "Check and repair" (`mode: "heal"`) reads the missing ones again.
- `cd_accept_unverified` = `1`: the track is filed with its **first** reading, its state is `unverified` (and stays so when it is filed), and the job ends `done` with `error: "unverified_track"` (unless a track was left out as well, which shows instead) and the message "not confirmed (taken with the first reading): n". The file may hold a small flaw; the log says where to look.

A repeated reading is shown while it runs: the state `rereading` of the track with its progress (`pass` 3 and more of `TrackProgress`). The job and `drives[].rip` carry `speed_x`, the speed the drive reads at as a multiple of 1x, measured from the sectors it delivered and the time it took (absent until 2 s of reading).
