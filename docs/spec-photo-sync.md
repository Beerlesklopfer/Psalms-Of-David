<!--
SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
SPDX-License-Identifier: AGPL-3.0-or-later
-->
# Photo sync and automatic backup (design)

Status: **being built** (project owner, 2026-11-07). Stage 1 is done: the settings of the server and of the profile, with the page, the
profile page and the app API. Stage 2 is done: the storage, `have`, the upload in pieces that goes on after a lost connection, the SHA-256
check, bisync `changes` and the file of a picture (`src/photo_store.rs`, `src/api_photos.rs`; the exact protocol is at the top of
`src/api_photos.rs`). An upload that is not complete answers `202 {received}`, not `308` as sketched in section 4: a `308` is a redirect to
many HTTP clients. Stage 3, the sending half, is done in the Android app (`app/lib/photos/`): the screen "Photo backup" in the settings (this device, the choices of the profile with the text of the chosen option, what the server keeps), the gallery through `photo_manager` (with `ACCESS_MEDIA_LOCATION`, so the files keep their location), a WorkManager job every hour with the conditions of the profile (Wi-Fi → unmetered network, charging), and the loop of section 5 with the SHA-256 of the file as its upload id (a broken upload goes on where the server is; two devices with the same picture send it once). The device settings `sources` (`camera`/`all`) and `since` (the day it was switched on, "also the older ones" as a button) are built.
The receiving half of bisync is built too (decided 2026-11-07: either way, chosen on the device): `on_download` is called `receive` and has
`app` (previews in the app, the original loaded on a tap; the default), `gallery` (the originals, checked against their SHA-256, into the
gallery folder `Pictures/David`, marked as known so the backup does not send them back) and `off`. The previews are made by the device
that sends a picture (its gallery reads every format, also HEIC and videos; the server reads none) and kept by the server in
`<picture folder>/.david-thumbs/<id>.jpg` (`PUT`/`GET /photos/{id}/thumb`, `changes` says `thumb`). A device receives what was taken from
the day it was switched on, like it sends; "also the older ones" reads the changes again from the start. The trash is built too (2026-11-08,
`deletes` = `trash`, bisync only): a picture this device sent or received into its gallery and that is no longer in the gallery (checked
one by one with the gallery, and only with access to all pictures, so a picture outside the chosen ones or an access to some pictures
only never counts as deleted) goes into the trash of the server; more than 20 at once wait for the person ("Were they deleted?"). The
server moves the file into `<picture folder>/.david-trash/<year>/<month>/`, keeps the row (so no other device sends it again), tells
the other devices (`photo_event`, `GET /photos/removals`) and removes it for good after 30 days (the hourly job, with its preview).
Another device drops a preview of the app at once; a picture in its gallery waits for "delete here too" (Android asks once more) or
"keep them here". The trash screen of the app lists what is in it with the day it goes, and restores (back to its month; the devices
that still ask drop the question). A received picture can be put into the trash from the view of the app. Not built yet: `keep_local`
(received originals in the gallery stay), and a restored picture does not come back to a device that had dropped its preview. It builds on the **ownership** that
books have since schema 0040: now that something in the library belongs to a family member, the pictures on a device can be copied to the
server of the family and kept there, and that copy is the backup.

**Name.** The feature is the **photo backup** (*Bilder-Sicherung*; owner, 2026-11-07: first "Es geht um eine Handy-Sicherung", then the
name "Bilder-Sicherung"; English "Photo backup"). Its content is
photos and videos. Other things on a phone (contacts, documents, messenger folders) are not part of it unless the owner widens it.

**Decisions of 2026-11-07** (section 8):
- Server, profile and Android come in one step; iOS later.
- Both modes, unisync and bisync. The server allows bisync only when `photo_modes` says `both`.
- Parents look at a child's pictures only when the server allows it (`photo_parents_may_view`) **and** the child's profile does (`parent_view`),
  and the child sees that it is on. David has no parent relation between profiles: "parents" are the **administrators**, who manage the
  children's profiles already.
- Videos are included, with a limit per file (`photo_max_file_mb`, 2048 by default).
- The pictures of a person go into the **picture folder of that profile** (Users page, `profile_folder` of kind `photo`), not into a folder of
  the server. That replaces the setting `photo_folder` of section 2a. A profile without a picture folder cannot switch the backup on.
- Every setting is a **choice with an explanation of every option**; choosing an option swaps the text that explains it (owner, 2026-11-07).
  Without the script every explanation is shown under the name of its option (`templates/fragments/choice.html`, `static/ui.js`).
- Retention (section 8.3) is not decided: everything is kept, and the setting `photo_retention_days` is not built.

## 1. Goal

1. **Backup without thinking about it.** The pictures (and short videos) that a family member takes on a phone or tablet are copied to the family server by
   the app, in the background, as long as the device is on the home network or the family's own VPN. Nobody has to press anything.
2. **Every copy belongs to a person.** A picture is stored under the **owner** of the device's profile, the same notion as the owner of a book
   (`docs/spec-library-hiding.md`, schema 0040): it says whom it belongs to, not who may see it.
3. **Nothing leaves the house.** The server stays a private server of the family. The pictures are not offered to the internet, not shared and not
   listed in the library of the apps like a book. They are a backup that can be looked at and restored.

Not a goal: a gallery application, face recognition, cloud sharing, editing. A viewer for the pictures of the own profile in the app is a later step.

## 2. Rules

- **One-way.** The device copies to the server. Deleting a picture on the phone **does not** delete it on the server (that is the point of a backup).
  Removing something from the server is an administrator's decision.
- **Per profile, per device.** The sync belongs to the profile that is paired on the device. A second device of the same profile adds to the same
  owner's folder; two devices that hold the same picture upload it once (by content hash).
- **Opt-in and visible.** (The settings of the three levels are in section 2a.) A device syncs only after the person has switched it on in the app settings ("Back up my pictures") and given the operating
  system's permission. The app always says what it does: last backup, how many pictures are waiting, a pause button. A child's device is switched on
  by a parent (setting of the profile, see 4) or not at all.
- **Only on the home network by default.** Settings of the profile: only over Wi-Fi/home network (default), only while charging (default on),
  with mobile data (off). The app does not try the server from outside unless the profile has a way in (VPN).
- **Administrators are people, too.** A couple is often **two administrators**. The rule "an administrator sees everything" therefore does **not** reach into the pictures:
  an administrator's pictures are visible to that administrator only, as to anybody else (the visibility boxes of `docs/spec-admin-ui.md` work for administrators). What stays true is
  the honest limit of every home server: whoever can open the disk of the server can read the files (a folder per owner, readable by the service only).
- **Privacy.** A person's pictures are visible to nobody but that person and the administrators who run the server (they own the disk). They are
  **never** visible to other members, and the family administrators see them only in the administration pages' storage numbers (count, size), not as
  pictures, unless the person has allowed it. (To be decided with the family, section 8.)
- **Integrity.** Every file is checked against its SHA-256 after the upload; a picture is "backed up" only when the server has the same hash. Resumable
  uploads, so a long video or a lost connection does not start over.

## 2a. The settings in detail

Three levels, from the largest to the smallest. A lower level can only be **narrower** than the one above (a device cannot sync more than the profile
allows, a profile not more than the server allows).

### Server (administrators, in the settings of the pages; a key `photo_*` in the `setting` table, validated like the CD settings)

| Key | Values | Default | Meaning |
|---|---|---|---|
| `photo_sync` | `off` / `on` | `off` | The feature exists at all. Off: no route (404), no menu, the apps do not offer it (like the hidden features). |
| ~~`photo_folder`~~ | | | Not built: the picture folder of each profile takes its place (decisions above). |
| `photo_modes` | `uni` / `both` | `uni` | Which modes a profile may choose (below). `bi` must be allowed here before anybody can use it. |
| `photo_quota_gb` | number, `0` = none | `0` | The most one owner may keep. |
| `photo_reserve_gb` | number | the disk reserve of the server | An upload that would leave less free space is refused. |
| `photo_max_file_mb` | number | `2048` | The largest single file (a long video). |
| `photo_videos` | `on` / `off` | `on` | Videos are synced too. |
| `photo_children` | `off` / `parents` / `allowed` | `parents` | Whether a child's device can be switched on, and by whom (`parents`: only a parent turns it on; `allowed`: the child may). |
| `photo_retention_days` | number, `0` = for good | `0` | Pictures older than this are freed **on the server only after** they are confirmed on a second place (a backup job saw them); `0` keeps everything. |
| `photo_parents_may_view` | `off` / `on` | `off` | Parents may look at a child's pictures (decision of the family, section 8). |

### Profile (every member for themselves; a parent for a child, when `photo_children` says so)

| Key | Values | Default | Meaning |
|---|---|---|---|
| `photo_enabled` | `0` / `1` | `0` | The sync of this person is on. Off: nothing is read from the devices, nothing is offered. |
| `photo_mode` | `uni` / `bi` | `uni` | **`uni`** (unisync): one-way, the device copies to the server, the server never sends anything back; this is the backup. **`bi`** (bisync): two-way, see below. |
| `photo_wifi_only` | `0` / `1` | `1` | Only on the home network or the family VPN, never on mobile data. |
| `photo_charging_only` | `0` / `1` | `1` | Only while the device is charging (battery friendly). |
| `photo_mobile_data` | `0` / `1` | `0` | May use mobile data if `photo_wifi_only` is off. |
| `photo_include_videos` | `0` / `1` | `1` | Only when the server allows it (`photo_videos`). |
| `photo_parent_view` | `0` / `1` | `0` | This person lets the parents look at their pictures (only if the server allows it); the person sees that it is on. |

### Device (in the app, on that device)

| Key | Values | Default | Meaning |
|---|---|---|---|
| `enabled` | on/off | follows the profile, asked once | This device takes part. A device that was not asked syncs nothing. |
| `sources` | the albums/folders of the device (camera roll, screenshots, chosen folders) | camera roll | What is looked at; screenshots and downloads are off by default. |
| `since` | a date | the day it was switched on | Only pictures from then on (an old gallery is **not** uploaded by accident); "also the older ones" is a separate, explicit button. |
| `on_download` (bisync only) | `off` / `thumbnails` / `originals` | `thumbnails` | What a device receives of the pictures that other devices of the same profile uploaded. |
| `keep_local` (bisync only) | `all` / `last_n` / `free_space` | `all` | How much of the received pictures stays on the device (the rest is fetched again on demand). |

### The two modes

- **Unisync (`uni`, the default):** device → server only. A picture is read once, hashed (SHA-256), offered with `have`, uploaded if the server lacks it,
  and marked done. Deleting, editing or moving a picture on the device **never** changes the server. An edited picture is a new file with another
  hash and is stored next to the first (the original stays). The server's copy of a picture is never sent back.
- **Bisync (`bi`):** two-way, between the devices of **one profile** through the server (never between profiles). Everything of unisync, plus:
  - A picture that another device of the same profile uploaded is offered to this device (`on_download`).
  - **Deletions are not propagated** by default: deleting on one device removes it from that device's view only; the server keeps it. A setting
    `photo_propagate_delete` (`never` default / `trash`) can let a deletion move the picture to a trash folder on the server for 30 days and ask the
    other devices to remove it (a parent can turn it on; the app says so before it deletes anything).
  - **Conflicts:** the same name with another content never overwrites: both are kept (the later one gets a suffix). An edited picture is a second
    picture, not a conflict.
  - The state of each device (what it has, what the server has) is kept as hashes; a device that was off for a long time catches up in pieces.

### What the settings do not do

- No setting makes the server delete a picture on its own authority except the retention rule above, which acts only on pictures a backup job has confirmed.
- No setting shows a picture to another member. Only `photo_parent_view` / `photo_parents_may_view` together allow parents to look at a child's pictures,
  and the child sees that it is on.

## 3. Storage on the server

```
<photo folder>/<owner nickname>/<year>/<month>/<original file name>
```

- `<photo folder>` is a **setting** (not a built-in path; like the import folders), outside the library folders, so the scanner does not list pictures as
  books. A folder per owner, named by the nickname of the profile (a renamed nickname keeps the folder, the database says which folder is whose).
- The date comes from the picture (EXIF `DateTimeOriginal`), else from the file time. A name that is taken by another content gets a suffix.
- Metadata (original name, size, hash, taken-at, device, uploaded-at, width/height) lives in the database, so that restoring and counting does not read
  the disk. The files stay plain files that can be copied by any tool: a backup that does not need David to be read.

### Tables (sketch)

```
photo(id, owner_id -> profile, device_id -> device, sha256 UNIQUE per owner, file (relative path), original_name, size_bytes,
      taken_at, width, height, mime, uploaded_at, created_by, updated_by, ...)       -- audit fields as in schema 0028
photo_upload(id, device_id, client_id, sha256, size_bytes, received_bytes, state, started_at)   -- an upload in progress (resumable)
profile_photo_setting(profile_id, enabled, mode, wifi_only, charging_only, mobile_data, include_videos, parent_view, propagate_delete)   -- section 2a
device_photo_state(device_id, enabled, sources, since, on_download, keep_local, last_scan_at)
```

## 4. API (sketch; the app is the client)

| | |
|---|---|
| `GET /api/v1/photos/state` | `{ "enabled": bool, "known": N, "bytes": N, "last_at": ts, "settings": {...} }` for the profile of the token |
| `PUT /api/v1/photos/settings` | the settings of the profile (section 2a: `enabled`, `mode` `uni`/`bi`, the conditions, ...; a parent for a child profile); the server refuses a mode it does not allow (`photo_modes`) with `422 mode_not_allowed` |
| `PUT /api/v1/photos/device` | the settings of this device (section 2a: `sources`, `since`, `on_download`, `keep_local`) |
| `GET /api/v1/photos/changes?since=<cursor>` | **bisync only**: the pictures of the profile that other devices uploaded since the cursor (`{id, sha256, size, taken_at}` and a new cursor); the device fetches thumbnails or originals as `on_download` says |
| `POST /api/v1/photos/have` | body: a list of `{sha256, size}` the device holds; answer: those the server **lacks** (the device sends only these) |
| `PUT /api/v1/photos/upload/{client_id}` | the bytes (with `Content-Range` for resuming); `Digest`/`X-Sha256` header; answer `201` when complete and the hash fits, `308` for "go on", `409` for a clash |
| `GET /api/v1/photos?...` | later: the profile's own pictures, newest first, for a viewer and for "restore" |
| `GET /api/v1/photos/{id}/file` / `GET`/`PUT /api/v1/photos/{id}/thumb` | the file / the preview made by the sending device (owner only) |

All of it is **authenticated like the rest** (device token), on the same transport (TLS or the tunnel), with the same limit of devices. The server decides the
owner from the token, never from a request field.

## 5. The app

- **iOS** (2026-11-08): the same screen and loop through `photo_manager` (`PHPhotoLibrary`; "camera only" leaves out the screenshots,
  iOS has no folders), a `BGProcessingTask` `david-photo-backup` (Info.plist `BGTaskSchedulerPermittedIdentifiers`, `UIBackgroundModes`
  `processing`, registered in `AppDelegate.swift`) with "charging" as `requiresExternalPower`; iOS knows no "Wi-Fi only", so the job checks
  it itself before it sends. The task runs once per request and is asked for again after every run; iOS decides when (mostly at night
  while charging, never after the app was swiped away), the screen says so. Received originals go into the library (no folder).
- **Android** first (the pictures are in `MediaStore`; a `WorkManager` job with constraints: unmetered network, charging, battery not low), **iOS** later
  (`PHPhotoLibrary`, background tasks are less reliable: the sync also runs when the app is opened). The Linux desktop has no camera roll; a chosen folder
  can be backed up instead (setting).
- The loop: list new items since the last run → `have` with their hashes → upload what is missing, one file at a time, resumable → mark done. The
  device keeps a small local state (last scanned time, hashes already done) so that nothing is read twice.
- Settings screen: the switch, the three conditions, "back up now", the numbers, the list of the last errors. Texts in English first, German second.

## 6. Administration and MCP

- Pages: a storage overview per owner (count, size, last backup, devices), the settings (photo folder, limit per profile, retention), and a button to
  put a profile's switch off. **No picture is shown** in the administration pages.
- MCP tools (read for the overview, write for the settings): `photo_overview`, `set_photo_limit`, `list_photo_devices`; nothing that reads pictures.
- A quota per owner (setting, default none) and the existing disk reserve check (`diskspace.rs`): the server refuses an upload when the disk would fall below
  the reserve, and the app says why.

## 7. Backup of the backup

The server is the backup of the phones; the server itself needs one. The photo folder is **one folder** so that the existing backup of the server
(Bareos/Onesimus jobs) can include it with one line. The administration pages show when the folder was last seen by a backup job (optional, later).

## 8. Open questions (to decide with the family before building)

1. May the parents see the pictures of the children (a setting per child profile)? And is that shown to the child?
2. Pictures **and** videos? A video is large; a limit per file and a separate switch are likely.
3. Retention: keep everything for good (default), or free space automatically after a year for pictures that were uploaded and checked? (Deleting on the phone
   is always the person's own decision.)
4. A viewer in the app for the own pictures, and a "restore to a new device" in the pairing flow?
5. Shared albums of the family (a picture that several members want to see) as a later, explicit feature: a picture is shared by its owner by choice, never
   by default.
6. Limits: files per request, concurrency (one upload at a time per device), and how the sync behaves with 150 profiles (the profile limit).

## 9. How it will be checked

Tests with a fake device: an upload of a file, the same file again (not stored twice), an interrupted upload that resumes, a wrong hash that is refused, a
second device of the same profile, a picture of another owner that is `404`, the disk reserve, the owner chosen by the token and never by the request, and the
rule that a deleted picture on the device changes nothing on the server.

## 10. Slideshow (requested 2026-11-07, decided, not built)

The owner: "Die App soll auch eine Diashow mit gängigen Übergängen machen, um sie z.B. an einem Familienabend zu zeigen." The app shows the
backed-up photos and videos as a slideshow.

Decisions of 2026-11-07:
- **Transitions:** crossfade, slide, zoom and pan (in the manner of Ken Burns), and "mixed": a different one of the chosen ones for every picture.
- **Music:** freely chosen from the library of David (an album or a playlist); the pictures change at the time set for the show, not with the music.
- **Showing:** fullscreen on the phone or tablet (to the TV by cable or screen mirroring), **Chromecast** from the app, and the Linux desktop
  app of Psalm in fullscreen on a computer at the TV.
- **Whose pictures:** a show can hold the pictures of **several people** for the family evening. Since pictures otherwise never pass between
  people, a person **shares** pictures for it on purpose (a shared album of 8.5 that the owner of each picture fills by choice). Nobody's picture
  appears in another person's show without that choice; a child's only as section 2 allows.

Still to design: the shared album (who may see it, who may add, how a person takes a picture back out), the settings of a show (time per
picture, order: by date or shuffled, videos with their own sound or muted under the music), and how the Chromecast receiver gets the pictures
(through the server, with a short-lived link per show, never a public address).
