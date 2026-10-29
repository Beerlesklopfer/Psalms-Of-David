<!--
SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
SPDX-License-Identifier: AGPL-3.0-or-later
-->
# Photo sync and automatic backup (design)

Status: **designed, nothing built** (project owner, 2026-10-29). It builds on the **ownership** that books have since schema 0040: now that something in the
library belongs to a family member, the pictures on a device can be copied to the server of the family and kept there, and that copy is the backup.

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
| `photo_sync` | `0` / `1` | `0` | The feature exists at all. Off: no route (404), no menu, the apps do not offer it (like the hidden features). |
| `photo_folder` | a folder path | empty | Where the pictures are kept (section 3). Without it the feature is off. Never inside an import folder. |
| `photo_modes` | any of `uni`, `bi` | `uni` | Which modes a profile may choose (below). `bi` must be allowed here before anybody can use it. |
| `photo_quota_gb` | number, `0` = none | `0` | The most one owner may keep. |
| `photo_reserve_gb` | number | the disk reserve of the server | An upload that would leave less free space is refused. |
| `photo_max_file_mb` | number | `2048` | The largest single file (a long video). |
| `photo_videos` | `0` / `1` | `1` | Videos are synced too. |
| `photo_children` | `off` / `parents` / `allowed` | `parents` | Whether a child's device can be switched on, and by whom (`parents`: only a parent turns it on; `allowed`: the child may). |
| `photo_retention_days` | number, `0` = for good | `0` | Pictures older than this are freed **on the server only after** they are confirmed on a second place (a backup job saw them); `0` keeps everything. |
| `photo_parents_may_view` | `0` / `1` | `0` | Parents may look at a child's pictures (decision of the family, section 8). |

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
| `GET /api/v1/photos/{id}` / `/thumb` | later: the file / a small version (owner only) |

All of it is **authenticated like the rest** (device token), on the same transport (TLS or the tunnel), with the same limit of devices. The server decides the
owner from the token, never from a request field.

## 5. The app

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
