<!--
SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
SPDX-License-Identifier: AGPL-3.0-or-later
-->
# Spec: the interface of the apps (`/api/v1`)

Status: **built and tested** for audiobooks (`server/src/api.rs`, `server/tests/api.rs`). Other media kinds are **designed, not built** (below).

## Decisions

- JSON over HTTP(S). A device pairs once (`POST /pair` with the code from the QR code) and gets a bearer token, the session of the user with the device behind it. Revoking the device or logging out ends it. The token is sent as `Authorization: Bearer <token>`; no cookies, so no CSRF.
- **Media neutral from the start** (project owner, 2026-10-27: there will also be PDF, music and video). The interface speaks of **items** (`kind`, `title`, `creator`) and their **parts** (one file each: a track, an episode, the PDF), not of books and tracks. Today the only kind is `audiobook`; the database tables are still `book` and `track`.
- The rules are the same for every kind and live in the data layer: only released items are visible, the age rating hides items from children, the weekly planner limits listening unless the item is quota-exempt. An invisible item answers `404`; outside the planner the stream answers `403 outside_schedule`.
- Files are served with `Range` (seeking, resuming) through `tower-http::ServeFile`; the content type follows the file extension. A file is served only if its real path (symbolic links resolved) lies inside a library folder.
- The apps send the listening position as `PUT /progress` (`item_id`, `part_id`, `position_ms`, `finished`); the server writes it through the coalescing queue.

## Requests

| Request | Answer |
|---|---|
| `POST /api/v1/pair` `{code, device_name, public_key}` | `{token, device_id, shared, user:{id,name,role}}`. `public_key` is a WireGuard key; until the tunnel exists the app sends a random one. |
| `GET /api/v1/me`, `POST /api/v1/logout` | the user; `204` |
| `GET /api/v1/items` | `[{id, kind, title, creator, min_age, recommended_age, part_count, discs, duration_ms}]` |
| `GET /api/v1/items/{id}` | the same plus `parts:[{id,disc,number,title,duration_ms,labels}]` (`labels`: the labels of the title of its own; the ones of the album come with the item), `progress`, `access:{state, next_start}` |
| `GET /api/v1/labels` | `{labels:[{name, books}], can_manage}`: the labels with the number of the books (of those the asker sees) that carry each; a book counts once, whether the book or one of its titles carries the label |
| `PUT /api/v1/items/{id}/labels` `{labels:[..]}` | all labels of a book at once (the owner, a member, and an administrator) -> `{labels}`; `422 invalid_label` |
| `PUT /api/v1/items/{id}/parts/{part}/labels` `{labels:[..]}` | **labels of a single title** (schema 0046), all of its own at once (same permission as for the book: its owner and an administrator; `part` is the id of the part as the item lists it, `404` for a part of another item); at most 10 of its own, `422 invalid_label` otherwise -> `{labels}` |
| `GET /api/v1/continue` | `[{item_id, kind, title, creator, part_id, position_ms, updated_at}]` |
| `GET /api/v1/parts/{id}/stream` | the file (`200`, or `206` for a range) |
| `GET /api/v1/bible/resolve?q=` | passages of an audio Bible: part and position to start at; see `spec-bible.md` |
| `PUT /api/v1/progress` | `204`; `404` for an invisible item, `422` for a part of another item or a negative position |

Errors are `{"error": "<code>"}`: `unauthorized`, `not_found`, `outside_schedule`, `enrollment_invalid`, `invalid_public_key`, `invalid_device_name`, `device_limit_reached`, `invalid_progress`, `invalid_reference`, `internal`.

## Other media kinds (designed, not built)

| Kind | Parts | Needs |
|---|---|---|
| `audiobook` | tracks in order | built |
| `album` (music) | tracks in order | the scanner reads artist/album/track from tags; no resume position needed, but the same progress works |
| `video` | one file, or episodes | the scanner needs durations of video files; the engine needs a video player (a different backend from the audio engine); subtitles and audio tracks are open |
| `document` (PDF) | one file | no duration and no planner (reading is not listening time); progress is a page number; the viewer is the app's own |

Steps to get there:

1. Add a `kind` column to `book` (default `audiobook`) and rename the tables to `item` and `part` in one migration, when the second kind is built, not before.
2. The scanner reads the extension and the folder layout per kind (`Music/Artist/Album/*`, `Videos/...`, `Documents/...`); each kind has its own import root and rating rule, because "age rating by an admin" is the only rule that fits all of them.
3. **Open:** does the weekly planner apply to video and music, or only to audiobooks? (Learning content is already exempt per item.) Does `documents` skip it? The proposal: planner applies to `audiobook`, `album`, `video`; not to `document`.
4. **Open:** `progress.position_ms` for a document means a page number; a separate column is cleaner.

## The Dart client (`app/lib/server/`)

Status: **built and checked against a real server** (`test/server_e2e_test.dart` with `server/examples/serve_demo.rs`, and `test/server_real_test.dart` against the release `david` with the real library, both skipped unless their environment is set).

- `ApiClient`: the requests above. With a fingerprint it speaks HTTPS and accepts **only** the certificate with exactly that SHA-256 (no other, whoever signed it); a different certificate gives `ApiException(certificate)`.
- `PairingAddress`: reads `david://pair?...`. The app takes the text under the QR code (pasted); camera scanning is for the phones.
- `CredentialStore`: `server_id`, address, fingerprint and token in `$XDG_CONFIG_HOME/psalm/server.json`, mode 600.
- `discoverServers` / `findServer`: mDNS (`_david._tcp`). **One client per network interface, each told to send through its interface (`IP_MULTICAST_IF`).** Without that the query leaves through the default route, which on a machine with a VPN is the tunnel, and nothing answers (found on the development machine). A tunnel that refuses the send raises an error outside the search's own future; each search runs in a guarded zone so that it ends neither the app nor the other searches.
- At start the app signs in with the stored token. If the server does not answer at its stored address, it is looked for by its id; a new address is stored. `401` means the device was revoked: the pairing is deleted and the connect screen appears.
- `ServerSource`: downloads a part into the cache (`$XDG_CACHE_HOME/psalm/parts/<part id>.audio`, complete or not at all) and then plays the local file; the engine cannot play from a URL yet (**open**: range requests from the engine, which would start playing at once and need no cache). Reports the position every 10 seconds, on pause and on leaving a track.
- Search with a server shows "Open passage ..." for the typed text; the server parses it (`spec-bible.md`).

## Scanning the pairing code (project owner, 2026-10-28)

- **Computer (Linux first):** the QR button reads the code from a **picture** that the person chooses (a screenshot of the pairing page or a photo of it), through `file_selector` and `zxing2` (`app/lib/server/qr_reader.dart`). There is also a paste button, and a pairing address given on the command line (`psalm "david://pair?..."`) or on the clipboard is taken over by itself. No camera is used on the computer: there is no camera plugin for Linux.
- **Android and iOS: the camera.** There the same button opens the camera and scans the code live (planned with `mobile_scanner`; not built, because Android and iOS come after Linux). Until then the button on a phone also offers the choice of a picture. The result is the same string, so everything after the button (`PairingAddress`, pairing, token) stays as it is.

## Where the app keeps its files (2026-10-28)

The folder is asked from the system (`path_provider`, `getApplicationSupportDirectory`), not guessed from
`HOME`: Linux `~/.local/share/io.onesimus.psalm`, macOS `~/Library/Application Support/<bundle id>`, Windows
`%APPDATA%\<company>\<app>`, Android the app's internal files folder, iOS `Library/Application Support` of the app.
It holds the pairing (`server.json`) and the downloaded parts (`parts/`). It is deliberately not the cache folder:
a download is something the user asked for and must not be emptied by a cleaner. Version 1.0 on Linux moves its old
`~/.config/psalm` and `~/.cache/psalm/parts` there at the first start. `PSALM_DATA_DIR` (and `PSALM_CONFIG_DIR`,
`PSALM_CACHE_DIR` for single parts) override it for development and tests. Not done: on iOS the folder is part of the
iCloud backup unless excluded (needs a small native call), and nothing was run on macOS, Windows, Android or iOS.

## Labels of single titles (2026-10-30)

Project owner: "the tagging of the titles in the catalog is missing". A label (classic tagging, schema 0044) hangs on a book; since schema 0046 it can hang on
a **single title** (track) as well (`track_label`, dropped with the track or the label). The labels of a title are its own plus the ones of its album; only the
own ones are stored per title (at most 10), so a change of the album's labels reaches all its titles. `GET /api/v1/items` has `track_labels` for each item: the labels
its titles carry, without doubles. **The lists filter and search by `labels` and `track_labels` together** (the app: `Book.allLabels`), so an album with one
hymn about prayer is found under "Prayer"; the app shows the labels of a title under its name and, for the owner and an administrator, a button that opens the editor
(`LabelApi.setTrackLabels`). A label that only titles carry stays in the list; renaming merges the titles as well. The administrators' MCP tool `set_track_labels`
(dry run by default) sets them for the titles of an album; `get_album` is not changed, `list_labels` counts `books` and `tracks`.
