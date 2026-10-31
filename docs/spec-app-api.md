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
| `GET /api/v1/barcode/{code}` | is this EAN/UPC in the library: `{code, gtin, found, items:[{item_id,kind,title,creator,has_cover}]}`; `422 invalid_barcode` |
| `GET /api/v1/items/{id}` | the same plus `barcode` (digits or `null`), `parts:[{id,disc,number,title,duration_ms,labels}]` (`labels`: the labels of the title of its own; the ones of the album come with the item), `progress`, `access:{state, next_start}` |
| `GET /api/v1/labels` | `{labels:[{name, books, podcasts, stations, count}], can_manage}`: the labels with the number of the books (of those the asker sees), podcasts (that they follow) and radio stations (their favourites) that carry each; an item counts once, a book whether the book or one of its titles carries the label; `count` adds the kinds up |
| `PUT /api/v1/items/{id}/labels` `{labels:[..]}` | all labels of a book at once (the owner, a member, and an administrator) -> `{labels}`; `422 invalid_label` |
| `PUT /api/v1/podcasts/{id}/labels` `{labels:[..]}` and `PUT /api/v1/radio/{id}/labels` | **labels of a podcast / a radio station** (schema 0047), all at once -> `{labels}`; an administrator always, a member for what they follow / have as a favourite themselves, a child or a guest never (`403`); a podcast or station the member does not have is `404`; at most 30, `422 invalid_label`. The lists `GET /api/v1/podcasts` and `GET /api/v1/radio` carry `labels` and `can_label` for each entry |
| `PUT /api/v1/items/{id}/parts/{part}/labels` `{labels:[..]}` | **labels of a single title** (schema 0046), all of its own at once (same permission as for the book: its owner and an administrator; `part` is the id of the part as the item lists it, `404` for a part of another item); at most 10 of its own, `422 invalid_label` otherwise -> `{labels}` |
| `GET /api/v1/continue` | `[{item_id, kind, title, creator, part_id, position_ms, updated_at}]` |
| `GET /api/v1/parts/{id}/stream` | the file (`200`, or `206` for a range) |
| `GET /api/v1/bible/resolve?q=` | passages of an audio Bible: part and position to start at; see `spec-bible.md` |
| `GET /api/v1/family/listening` | what the other members of the family listened to lately, for the rows "recently heard by" of the start page: `[{user_id, name, has_avatar, items: [{item_id, kind, title, creator, part_id, position_ms, updated_at}]}]`, per member the latest ten (finished or not) that the caller may see. A member shows when `share_listening` is on or an administrator forbade hiding (`may_hide_listening` off). Guests see and show nothing |
| `GET /api/v1/family/media-time?days=14` | the media time of the family for the line charts of the start page (`days`: 7 to 90): `{from_day, days, members: [{user_id, name, has_avatar, me, kinds: {audiobook: [ms…], music: [ms…], podcast: [ms…]}, total: [ms…]}]}`; every array has `days` entries, the first is the calendar day `from_day` (the date counted in days since 1970-01-01) and the last is today; the days are those of the clock of the server (its setting `time_zone`, daylight saving time included), not UTC. The caller is always there; another member shows when `share_listening` is on or an administrator forbade hiding (`may_hide_listening` off) and when they listened in the period. Guests see only themselves. Time is counted from the position reports (the smaller of the way the position moved and the time between two reports, at most two minutes), so there is no history from before the server had this (schema 0055) |
| `PUT /api/v1/me` `{share_listening}` | the own choice whether the family sees the listening; `403 listening_not_hideable` for `false` when `may_hide_listening` is off. `GET /me` carries both flags (`share_listening` is `true` whenever hiding is forbidden) |
| `PUT /api/v1/progress` | body as above plus the optional `at` (Unix ms of the listening; later than now counts as now): the newer state wins, an older report never overwrites it. `204`; `404` for an invisible item, `422` for a part of another item or a negative position |

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
- **Android, iOS and macOS: the camera** (2026-10-30, built after the first test on an iPhone: the button only offered a picture). The QR button opens `QrScanScreen` (`app/lib/screens/qr_scan_screen.dart`, plugin `mobile_scanner` 7: Apple Vision on iOS and macOS, ML Kit bundled on Android): full-screen preview with a frame, a torch button where the device has one, "Cancel" and "Choose a picture instead". It takes the first QR code whose text starts with `david://pair?`, stops the camera at once, puts the address into the field of the connect screen and focuses "Connect" (nothing connects by itself). Any other QR code is ignored with one short hint, the scanning goes on. If the camera is not allowed, the screen says so and (on iOS) opens the settings of the app (`app-settings:`); with no camera at all (simulator, a Mac without one) the picture chooser opens by itself. The result is the same string as before, so everything after the button (`PairingAddress`, pairing, token) stays as it is. Needs: `NSCameraUsageDescription` (iOS, macOS Info.plist, present), `com.apple.security.device.camera` (macOS entitlements), `CAMERA` and `uses-feature camera required=false` (Android manifest; the plugin of the profile picture asks for the permission at runtime when it is declared). Linux and Windows keep the picture chooser. Tested with a fake camera (`app/test/qr_scan_test.dart`); the real camera needs a phone.

## Where the app keeps its files (2026-10-28)

The folder is asked from the system (`path_provider`, `getApplicationSupportDirectory`), not guessed from
`HOME`: Linux `~/.local/share/com.the-abraham-box.psalm`, macOS `~/Library/Application Support/<bundle id>`, Windows
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

### Labels on all media (podcasts and radio)

Project owner, 2026-10-30: "the tagging of the app covers ALL media". Besides books and titles the labels hang on **podcasts** (`feed`, table
`podcast_label`) and **radio stations** (`station`, table `station_label`), schema 0047. Both belong to the family, so their labels are shared: everybody who
has the podcast or the station sees them. Who sets them: an administrator always; a member for the podcasts they follow and the stations among their
favourites; a child or a guest never. The label list (`GET /api/v1/labels`) counts every kind (`books`, `podcasts`, `stations`, `count`), renaming and
deleting reach every kind, a label that nothing carries any more goes, `library_version` contains them. In the app the label filter of the library and the
search also find podcasts and stations (`labels`), and their screens show the labels with an editor for those who may. The administrators' MCP tool
`set_feed_labels` (`kind` podcast or station, `names`, add/remove/set, dry run by default) sets them; `list_podcasts` and `list_stations` show them and
`list_labels` counts `podcasts` and `stations`. **Not covered:** photos and e-books (only import folders so far, no rows to hang a label on) and playlists
(a personal collection of a profile, not a medium of the library); the web administration has no page for podcasts and radio, so there is no chips editor there.

## Offline mode and synchronisation (project owner, 2026-10-30)

"The app needs an offline mode: only the downloaded titles are shown. What a user changes meanwhile (new tags, ignored albums) has to be
synchronised, and the listening progress too."

**The view.** Offline is the person's switch (settings, device-local, `offline.json`) **or** the fact that the server does not answer (the look
at the library every 30 seconds fails). The lists, the search and the start page then show the **downloaded** titles only: an album of which some
titles are downloaded shows those, one without any is left out, radio stations are left out. The page Radio says that it needs the server;
the page Podcasts shows the podcasts that have downloaded episodes (see "Podcast downloads"). A bar at the top says "Offline mode" or "Server not reachable", how many changes wait, and (unreachable only) has "Try again". What is
downloaded plays without the server. With the switch on the app does not ask the server at all (no look at the library, no sending); switching
it off sends what waits and loads the library again.

**The cache.** Every library that is loaded is written to `library/library.json` (+ `cover-<id>` files) in the data folder of the app
(`AppPaths.base`). A start without the server (or with the switch on) opens it, so the offline view works after a restart. The list of what
the profile hid (the page "Hidden") is not cached; the hidden flag of the items is. Forgetting the pairing deletes the cache and the outbox.

**The outbox** (`outbox.json`, `lib/offline/outbox.dart`). Changes made while the server is out of reach, or whose sending fails for want of a
connection, are written down and survive a restart. They are sent in the order in which they were made, as soon as the look at the library
succeeds (or the switch goes off, or the person taps "Send changes now"); then the library is loaded again, **the server decides**.

| change | what is kept | merged with an earlier change of the same thing | sent as |
|---|---|---|---|
| labels of a book, a title, a podcast, a station | a delta `add`/`remove` against what the app showed | for each word the last thing done to it counts | `PATCH /items/{id}/labels`, `/items/{id}/parts/{part}/labels`, `/podcasts/{id}/labels`, `/radio/{id}/labels` with `{"add":[],"remove":[]}` |
| a book hidden or shown | the state | the last state wins | `POST /me/hidden` (sets the state) |
| an author hidden or shown | the state | the last state wins | `POST /me/hidden` |
| where the listening to a book is | item, part, position, finished, time | only the newest of a book | `PUT /progress` with `at` |
| where the listening to a podcast episode is | episode, position, finished, time | only the newest of an episode | `PUT /episodes/{id}/state` with `at` |

- A label **delta** (not the whole list) is sent, so two devices that change the labels of the same book at the same time do not overwrite each
  other: the words added and taken off are applied to what the server has. Same permission as the `PUT` (administrator, or the owner of the
  book; podcasts and stations: what the member follows). Sending a delta twice is harmless; unusable words are `422 invalid_label`.
- Hiding sets a state, so a repeat is harmless; the changes are replayed in order, so the last one made on the device is the last one applied.
  Offline a hidden book (or the books of a hidden author) leaves the lists at once (unless hidden books are shown); showing it again offline
  takes effect when the library is loaded again.
- The listening position is stamped with the time of the listening (`at`). The server keeps the position with the newest time per user and
  book, so an old offline report cannot overwrite what another device wrote later. Stations have no position. The state of an episode
  (`PUT /episodes/{id}/state {position_ms, finished, at?}`) works the same way: the newer state wins, a time in the future counts as now,
  sending it twice changes nothing; the position of an episode is reported like the one of a book (before 2026-10-30 the app never reported
  episodes: they have no part, and the report waited for one).
- Answers: `0`/`5xx`/`429`/`401` stop the round and keep the rest; `403`, `404`, `409`, `422` drop the change and tell the person ("Not accepted
  by the server": labels of / hiding of "<title>" with the reason not allowed, no longer there, refused), in a bar and in the settings until
  dismissed. Refused listening positions are dropped silently.
- Labels of podcasts and stations can only be set with the server (their pages are offline-only placeholders); a change that fails for want of a
  connection while the page is open waits like the others, but is not shown until it is sent.


## Podcast downloads (project owner, 2026-10-30)

"For podcasts the download function for listening offline." Code: `app/lib/podcast_downloads.dart`, `app/lib/screens/podcast_downloads_ui.dart`.

- **Where the audio comes from.** The server does not carry the episodes: `audio_url` is the address at the publisher. The app fetches it with a
  plain HTTPS client (no token, the publisher's own certificate rules, redirects are followed), **only `http` and `https` addresses** (any other
  scheme is refused with `scheme` before a connection is made). The file is written through a `.part` file and renamed when complete; a `.part`
  file of an earlier run is deleted at the start. A download that is cancelled or fails leaves no file.
- **Where it lives.** In the folder of the other downloads (`AppPaths.parts`), named `ep-<sha1 of the address>.audio`. The player fetches an
  episode the first time it plays it into the same file, so *played* and *downloaded* are the same thing for the files; "storage used" and
  "remove all downloads" of the About page count and delete them too. A small file `podcast-downloads.json` (`AppPaths.base`) says which
  episodes the files are (episode, podcast, title, address, size, date, position, finished) and holds the two settings. The files are the truth:
  an entry whose file is gone is dropped; an episode that was only played gets its entry when the episodes of its podcast are listed.
- **The button** (every episode in the list): not downloaded, downloading (bytes of the total, tap to stop), on this device (check mark; tap
  asks, then removes). The size of a downloaded episode is shown in the list. The app bar of the episodes has "download the newest 5 unheard
  episodes" (one after the other, the first failure stops it with its message). Messages: not a web address, the publisher refused (HTTP
  error), larger than the limit, no connection / certificate, the file could not be saved.
- **Settings** (Settings > Podcast downloads): what the episodes take and "delete all"; "Delete episodes after listening" (**off by default**:
  nothing is deleted unless the person does it; on: an episode heard to the end goes, and so does one that the podcast no longer lists, meaning
  it is newer than the oldest listed episode but missing from the list); the largest single download (100 MB to 2 GB, default 500 MB).
  Forgetting the pairing forgets the list of episodes but keeps the files (like the downloaded parts of books).
- **The gate in front of "download the whole collection"** (project owner, 2026-10-30: a collection can take several gigabytes). The app bar of the episodes has a second
  button, "Download the whole collection", for every episode that is not on the device (heard or not, the newest first). It **never starts by itself**: a question
  says how many episodes and about how large (the size is estimated from the duration at 128 kbit/s, 40 MB for an episode without a duration), and "Download" starts
  them one after the other, with a "Stop" in the message; the first failure stops it. There is a **limit per collection download, 1 GB until the person changes it,
  250 MB to 20 GB in Settings > Podcast downloads** (`bulk_megabytes` in `podcast-downloads.json`): when more is missing than the limit allows, the question says so and
  loads only the newest episodes that fit; when not even the newest fits there is no "Download", only the hint to raise the limit. The pure part is
  `app/lib/podcasts/download_gate.dart` (`planBulk`), the question is `askBulkDownload`. Not done: a check of the free space of the device and a "Wi-Fi only" rule
  (neither is available in the app yet).
- **Offline.** An episode on the device plays without the server. In the offline view the page Podcasts (and the category in the library) lists
  the podcasts with downloaded episodes and, below, only those episodes, with the position of this device. If the server does not answer while
  the list of a podcast is open the downloaded episodes are shown instead of the error. Where the person is in an episode is reported to the
  server like the position of a book and waits in the outbox offline (table above).

## The system label "Blacklist" (administrators only)

The server marks media that hit the blacklist words (see `spec-admin-ui.md`, "The blacklist"). The mark is the reserved label `Blacklist`: it appears in the
`labels` of items, podcasts and stations and as a row of `GET /api/v1/labels` **only for an administrator**; every other role never receives it, so the
app needs no filter of its own. The app does not let a user type the name (`cleanLabel` refuses it) or suggest it, and the settings list offers no rename or delete
for it (the server answers 404 / 422). An administrator who removes the chip from a medium (PUT without it, or PATCH `remove: ["Blacklist"]`, also from the
outbox) approves the medium; adding it does nothing.

## Personal profile (`/api/v1/me`)

Project owner, 2026-10-30: "the app needs the personal profile settings; the picture can be taken with the camera". What a person may change about
themself, nothing else (role, age limit, owner and the limits of the devices stay with the administrator); every role may, a guest too.
Implemented in `server/src/api_me.rs`.

| Request | Answer |
|---|---|
| `GET /me` | `{id, name, nickname, first_name, last_name, role, max_age, language, has_avatar, avatar_version, device_count, device_limit, cd_rights}` (`cd_rights` is `{may_insert, no_approval, may_approve}`, all `false` for a child, a guest and a session on a shared device of one; it decides which CD symbols and buttons the app shows), plus `downloads: true` **only** when the hidden feature `download` is on (see below); the field is absent, never `false`, when it is off |
| `PUT /me` `{nickname?, first_name?, last_name?, language?}` | changes what is given and answers like `GET /me`. `language`: `en`, `de` or `""` (follow the device). `409 exists` (nickname taken, no regard to case), `422 invalid_name`, `422 invalid_language`. Other fields (`role`, `max_age` ...) are ignored. |
| `PUT /me/pin` `{current, new}` | `204`; `403 wrong_pin` (counts like a wrong sign-in), `429 locked` (still waiting; the PIN was not checked), `422 invalid_pin`. With `wrong_pin` and `locked` the answer carries `retry_after_s` (seconds until the next try, a × 2ⁿ, see `spec-admin-ui.md`) and the header `Retry-After`; the app counts it down. The sessions of the browser end, the paired devices keep theirs. |
| `GET /me/devices` | `[{id, name, shared, last_seen, this}]` (not revoked) |
| `GET /me/avatar` | the picture (`ETag` = `avatar_version`, `304` for a repeated request), `404` without one |
| `PUT /me/avatar` | the picture as the body; type from the first bytes (JPEG, PNG, WebP; SVG and GIF are refused), at most 256 KiB: `415 avatar_type`, `413 avatar_size`, `422 avatar_none`. Answers like `GET /me`. |
| `DELETE /me/avatar` | removes it; answers like `GET /me` |

**The app** crops the picture square from the middle, makes it at most 512 pixels and encodes it as JPEG (under about 200 KiB) *before* it sends it; this
also drops the metadata of the camera (place, time) and applies the orientation. The server only checks and stores it (the image decoders are not a
dependency of the server). The camera and the photos come from `image_picker` on Android and iOS (the camera of the system, no `CAMERA` permission of
its own on Android; `NSCameraUsageDescription` and `NSPhotoLibraryUsageDescription` on iOS); on a computer there is no camera button and a file
chooser takes its place. A camera or photos that may not be used are said in a message and break nothing.

The profile and the picture are kept on the device (`<app data>/profile`), so that the offline view shows them. A change needs the server: nothing
waits in the outbox for it, and without the server the page says so and its buttons are off. Disconnecting the app deletes the copy. The profile page
is reached from the first entry of the settings and from the picture on the home page.

## Background playback and the car

The app keeps playing with the screen locked and shows the controls on the lock screen and in the notification (Android: foreground service, iOS: remote command centre); a car (CarPlay, Android Auto) is the next step. What is built, the wiring of the extension point for the car and what each platform needs: `docs/spec-car.md`.

## File format of a part

Every part of an item (`GET /items/{id}`, `parts[]`) carries `format` (mp3, ogg, flac, m4a ...), `codec` and `mime` (additive fields; an app ignores what it does not know, a part not looked at yet has an empty `format`, `mime` is then guessed from the file name). The stream is served with that `Content-Type`. An app that cannot play a format says so instead of failing silently: on iOS the player (AVPlayer) cannot play Ogg Vorbis, Opus in Ogg, WMA or APE; a tap on such a track shows "This file format cannot be played on iPhone yet" (`app/lib/audio/file_formats.dart`). The Linux engine plays MP3, Ogg Vorbis, FLAC, WAV and AAC/ALAC in MP4; Android (ExoPlayer) plays all of them.

`GET /parts/{id}/stream?as=mp3` serves an MP3 that the server makes of the file (cached; see `docs/spec-transcoding.md`): `400 unsupported_as` for another target, `422 transcode_failed` for a file that cannot be decoded, the original for a file that is MP3 already. A part whose format some devices cannot play carries `transcode: "mp3"`; an app that cannot play the format itself asks for `as=mp3` (and keeps the file under another name than the original) and shows the "cannot be played on iPhone yet" message only if the server answers `transcode_failed` or does not offer `transcode`.

## Barcodes (app side, project owner, 2026-10-30)

The person holds a CD and wants to know whether the album is already in the family library. The icon button "Check a barcode" sits in the app
bar of the Library and the Search screen and opens `BarcodeScanScreen`. On Android, iOS and macOS it shows a live camera view (EAN-13, EAN-8,
UPC-A, UPC-E; codes with a wrong check digit are ignored; it stops at the first valid code). On every platform, and as the fallback on the
phones, a text field "Barcode" takes the digits (spaces and hyphens are removed). The answer comes in a bottom sheet: "In the library:" with
cover, title, creator, kind and an Open button, or "Not in the library: <code>".

Order: the **local** library is asked first (`Book.barcode`, kept in the offline cache; codes are compared as GTIN-14, so a UPC-A and the EAN-13
with a leading 0 are the same). Only when nothing is found locally and the app is online, `GET /api/v1/barcode/{code}` asks the server, which
also knows albums that this device has not loaded yet (they are listed but cannot be opened until the library is reloaded). Offline, a code that
is not found locally gets the answer that the library of this device does not have it and that the server was not asked. A server `422
invalid_barcode` is shown as an invalid code.

### Barcodes (server side)

Albums carry their EAN in the library (schema `0056`: `book.barcode` = the digits of the tag `Barcode`, `book.gtin` = the same as 14 digits, indexed). The scanner reads the first valid `Barcode` tag among the files of an album (digits only, 8/12/13/14 digits, correct GTIN check digit; anything else counts as none); a tag edit in the administration (`Barcode=...` line of the tags window) or by MCP `set_tags` updates the library at once and refuses a wrong code. A CD import writes the barcode of the chosen MusicBrainz release into the files.

`GET /api/v1/barcode/{code}`, any signed-in device. `code`: digits, spaces and hyphens ignored, 8, 12, 13 or 14 digits with a valid check digit, else `422 {"error":"invalid_barcode"}`. Answer `200 {"code":"<cleaned digits>","gtin":"<14 digits>","found":bool,"items":[{"item_id","kind","title","creator","has_cover"}]}`. The match is by GTIN-14, so a UPC-A and the EAN-13 with a leading 0 are one product and an EAN-8 is padded. Only albums the caller may see are listed (the rule of the lists: age limit, release, blocks); what the caller hid in their own library still counts as present. `GET /api/v1/items/{id}` carries `"barcode"` (digits or `null`) so that an app can keep a copy and check offline. Albums get their code at the next rescan or at a tag edit.

## Capability `downloads` (hidden feature, 2026-10-31)

`GET /api/v1/me` carries `"downloads": true` when the administrator of the machine switched the hidden feature `download` on (`[features] download = true` in
`/etc/david.toml`, or `DAVID_FEATURE_DOWNLOAD=1`; see `spec-admin-ui.md`, "Downloading files"). Without the switch the field is **absent** (not `false`), so a
server that has not got the feature says nothing about it, and an app must treat a missing field as "no". The field is the same for every role: it says what the
server offers, not who may use it (today only administrators, in the web interface; there is no download route for the apps). No app uses it yet.


## CDs: jobs, approval and a new try (for the app of phase 4; server built 2026-10-31)

The full description of the routes, the states and the errors is `docs/spec-cd-import-api.md` ("States of a job"); this is what the app is built against. All requests under `/api/v1/cd`, bearer token, JSON. The app learns what the person may do from `GET /me` -> `cd_rights`:

| Right | The app shows |
|---|---|
| `may_insert` | the import and eject symbols, `POST /cd/rip` (with `"eject": true`), the entry of the own CD while it waits ("waits for approval", no buttons) |
| `may_approve` | the tasks of CDs that wait, with the buttons Approve / Reject; Try again / Give up for CDs of others that could not be read |
| neither | nothing of the CD import (every `/cd` route answers `403 forbidden`; `GET /cd/jobs` and the event channel stay open to somebody with `may_approve` only) |

**A job** (`GET /cd/jobs` -> `{"jobs":[...],"busy","busy_elsewhere","device"}`, `GET /cd/jobs/{id}` -> `{"job":{...}}`, the events `job`, `progress`, `done`, `failed` and the `snapshot` of the event channel): besides the members already described there are `cd_state`, `reason`, `decided_by`, `decided_ms`, `user_id`, `starter`, `held_ms`. The app shows the job by `cd_state`:

| `cd_state` | The app shows | Buttons |
|---|---|---|
| `ripping` | progress (`fraction`, `track` of `tracks_total`, `track_states`) | stop (`POST /cd/rip/{id}/cancel`, starter or approver) |
| `retry_wait` | "track could not be read, the CD is still in the drive", `error` (`unreadable_track`, `read`, `verify`, `interrupted`) | Try again (`POST /cd/jobs/{id}/retry`), Give up (`POST /cd/jobs/{id}/give_up`) for the starter and `may_approve`; the eject button also gives up |
| `ripped` | short; "being filed" | none |
| `awaiting_approval` | author, `album`, `tracks_total`, `held_ms` ("since"), `starter`; the files are no album and no track yet | Approve / Reject (`POST /cd/jobs/{id}/approve`, `.../reject`) only with `may_approve`; the starter without it sees no buttons |
| `imported` | "n tracks imported"; `import_ran`; the album is in the library once the import has run | none |
| `rejected` | `reason` `declined` or `expired`; `decided_by` | none |
| `present` | "already in the library", `message` names the book | none |
| `cancelled`, `failed` | `error`, `message` | none |

**Requests and answers** (all errors `{"error":"<code>"}`):

| Request | Success | Errors |
|---|---|---|
| `POST /cd/rip` `{author, title, disc, discs_total?, tracks?, only?, mbid?, kind?, eject?, ...}` | `201 {"id","job","resume"}`; the job is `ripping` (a CD of a profile that needs approval is read at once and then waits) | `403 forbidden` (no `may_insert`), `409 busy` / `no_disc` / `tray_open` / `no_drive` / `disc_exists` (+`taken`), `503 drive_denied`, `422 ...`; `409 disc_present` with `present`, `missing`, `damaged` and **`ejected`** (true when the request said `"eject": true` and the CD is whole in the library: nothing was read, the tray is open) |
| `POST /cd/jobs/{id}/approve` | `200 {"job"}` | `403 forbidden` (no `may_approve`), `409 invalid_move` (+`state`), `409 decided` (+`by`, `resolution` = `approved`/`rejected`/`expired`; show "Papa approved the CD" instead of the buttons), `404 not_found`, `500 internal` (the files could not be placed; the CD waits again) |
| `POST /cd/jobs/{id}/reject` | `200 {"job"}` | as approve |
| `POST /cd/jobs/{id}/retry` | `202 {"job","resume"}` | `403 forbidden` (neither starter nor `may_approve`), `409 invalid_move`, `409 busy` / `no_disc` / `tray_open` / `no_drive`, `503 drive_denied`; the job stays `retry_wait` after a refusal |
| `POST /cd/jobs/{id}/give_up` | `200 {"job"}` | `403 forbidden`, `409 invalid_move` |

An upload (`POST /cd/import`) of somebody who needs approval answers `201` with `"import":"held"`, `"placed":null` and `job.cd_state` `awaiting_approval`; the app may eject (`eject`), the CD is read. The app tells the cases apart by `cd_state` and the codes, not by the text; the texts are the app's own (en first, de second). The inbox follows below; the spoken messages (phase 5) are the app's own business and use only the marks `speak` and `device` described there.

## The inbox of the CDs (for the app of phase 4; server built 2026-10-31)

Messages about CDs are kept on the server (`docs/spec-cd-inbox.md`). The app reads the list when it opens and after every reconnect; the WebSocket only adds. Bearer token, JSON, all under `/api/v1`. The inbox exists for a profile with `cd_rights.may_insert` or `cd_rights.may_approve`; for everybody else (child, guest, a member with no CD right) every answer is the empty inbox below, and `POST .../read` answers `404 not_found`.

| Request | Answer |
|---|---|
| `GET /inbox?after=<id>&limit=<n>` | `200 {"items":[Item...],"unread":n,"tasks_open":n,"next":id or null}`, newest first. `limit` 1 to 200 (default 50); `after` is the `next` of the page before (the items with a smaller id follow); `next` is `null` on the last page. `unread` and `tasks_open` count the whole inbox, not the page. `422 invalid_paging` for `limit` or `after` below 1. |
| `POST /inbox/{id}/read` | `204`; reading twice is fine; `404 not_found` if the item is not in the inbox of this profile |
| `POST /inbox/read_all` | `204` |
| `GET /me` | carries `inbox_unread` and `inbox_tasks_open` (0 without an inbox), for the badge |
| `POST /cd/jobs/{job_id}/approve`, `POST /cd/jobs/{job_id}/reject` | **the only way to decide a task** (the buttons of the notification and of the card): `job_id` is `item.job_id`; `200 {"job"}`; `403 forbidden` without `may_approve`; `409 decided` with `{"by","resolution"}` if somebody was faster (`resolution` is `approved`, `rejected` or `expired`; show "<by> approved the CD" instead of the buttons); `409 invalid_move`; `404 not_found` |
| `POST /cd/jobs/{job_id}/retry` | "Try again" of a `cd_read_error` notice, only while `item.job_state` is `retry_wait` (see the CD section above) |

**Item:**

```json
{"id":17,"kind":"task","code":"cd_awaiting_approval","job_id":42,
 "args":{"job_id":42,"author":"Ken Follett","title":"Sturz der Titanen","disc":1,"discs_total":12,"tracks":14,"by":"Anna"},
 "created_ms":1793000000000,"read":false,"resolved":false,"can_decide":true,"job_state":"awaiting_approval"}
```

| Member | Meaning |
|---|---|
| `id` | the paging key; grows with time |
| `kind` | `notice` (own read mark per person) or `task` (one decision for everybody) |
| `code`, `args` | all the server stores; **the app makes the text** (en first, de second) from them. Never prose. |
| `job_id` | the CD job, or `null` (`cd_no_match`, `cd_ambiguous` and the other messages of a disc that was not started have none) |
| `created_ms` | Unix ms |
| `read` | this profile has read it. A task that is decided counts as read. |
| `resolved` | `true` for a task that is decided. Then also `resolved_ms`, `resolution` (`approved`, `rejected`, `expired`) and `by` (nickname, empty for `expired`) |
| `can_decide` | `true` only for an open task and a profile that may decide it now: show the buttons Approve / Reject only then. The starter of a held CD sees the same task with `can_decide: false`: the entry "waits for approval", without buttons. |
| `job_state` | the `cd_state` of the job now, or `null` (`retry_wait`: "try again" still works) |
| `speak`, `device` | only on `cd_imported`, `cd_awaiting_approval`, `cd_read_error`, `cd_present`: see below |

A decided task stays in the list of the people who could decide it (as "done", `resolved: true`); for the starter who could not decide it, it is removed from the list when it is decided (the entry of the job is gone; the decision comes as a notice).

**Codes and `args`** (every number and name is data; `author`, `title` are as in the library):

| `code` | kind | `args` | Who gets it |
|---|---|---|---|
| `cd_awaiting_approval` | task | `job_id, author, title, disc, discs_total, tracks, by` (`by` is the nickname of the starter, `auto` for a CD put in at the server) | all with `may_approve` (`can_decide: true`); the starter without that right (`can_decide: false`) |
| `cd_decided` | notice | `job_id, author, title, disc, approved (bool), by` | the other approvers and the starter, not the one who decided |
| `cd_expired` | notice | `job_id, author, title, disc` | the approvers and the starter |
| `cd_imported` | notice | `job_id, author, title, disc, discs_total, tracks, album_id` | the starter; the approvers too if the CD was put in at the server or its starter needs approval |
| `cd_present` | notice | `job_id, author, title, disc, album_id` (`album_id` is `null` if it could not be found by name) | as `cd_imported` |
| `cd_read_error` | notice | `job_id, author, title, disc, tracks (the numbers that failed), reason` | as `cd_imported` |
| `cd_no_match` | notice | `disc_id` (+ `author, title, disc` if known) | approvers (a CD put in at the server) |
| `cd_ambiguous` | notice | `disc_id, candidates (number of releases)` (+ `author, title, disc`) | approvers |
| `cd_heal_needed`, `cd_no_space`, `cd_no_target` | notice | `disc_id, author, title, disc` | approvers |

**Spoken messages.** `speak` is `"starter"` when an app started the rip: `device` is then the id of that device, the `device` of the `hello` message of the event channel; only the app whose own id it is speaks. `speak` is `"home"` when no device is known (the rip was started at the web panel, by MCP or at the drive; `device` is `""`): the devices of approvers that are playing and on the home network speak. The server cannot know the starter's device for a rip started from the web panel, so such a message is always `home`. Items of other codes carry neither member.

**WebSocket `/cd/events`** (open to Admin and Member; the new types are sent only to the profiles the item went to, to all their devices; a member without any CD right gets none and an empty inbox):

```json
{"v":1,"type":"inbox","item":{...Item as for this profile...},"unread":3,"tasks_open":1}
{"v":1,"type":"inbox_resolved","id":17,"job_id":42,"resolution":"approved","by":"Bea","resolved_ms":1793000100000,"unread":2,"tasks_open":0}
```

`inbox` is a new item (a notice, or a task; the item is complete: show it, notify, and make a card). `inbox_resolved` says that the task with this `id` is decided for everybody: remove its buttons and its notification; the notice "<by> approved the CD" comes as an `inbox` message of its own (not to the one who decided). `unread` and `tasks_open` are the counts after the change. The `snapshot` after `hello` carries `unread` and `tasks_open` too. **Nothing is replayed after a reconnect:** read `GET /inbox` (the state is in the list, not in missed events); the same after a `snapshot` that follows a lost message.
