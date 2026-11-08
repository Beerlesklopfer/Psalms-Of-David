<!--
SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
SPDX-License-Identifier: AGPL-3.0-or-later
-->
# Pictures to look at: the slideshow

Decided by the project owner on 2026-11-08. The photo backup (docs/spec-photo-sync.md) brings the pictures to the server. This part shows them
again: as a slideshow in the apps (iPhone/iPad, Android) and in the browser.

## 1. What a person sees

| Source | Key of the source | Who sees it |
|---|---|---|
| Own backup | `own` | Everybody but a guest: the pictures of their own devices (not those in the trash). |
| A child's backup | `p<profile id>` | The parents (the administrators, as in the photo backup), when the server allows it (`photo_parents_may_view`) **and** the child's profile does (`parent_view`). |
| An old folder | `f:<folder>` | The adults (administrators and members); not children, not guests. |

The old folders are the folders in the **folder of the family's pictures** (setting `photo_library`, e.g. `/srv/Bilder`) that are not the
picture folder of a profile: `Allgäu2017`, `Familie`, `Hochzeit`, ... Pictures right in that folder are a source under its own name. The
server only reads there; it reads the folders again every hour, at the start and when the setting changes. Hidden folders (`.…`, `@…`,
`#recycle`) are left out, symbolic links are not followed, and a file is only served when it lies below the folder.

A picture is named by its key: `b<id>` (backup), `l<id>` (old folder). Every request checks again that the person may see it; a picture
of somebody else is "not found".

## 2. The slideshow

- Choose: one or more sources, a time range (from, to), with or without videos.
- Order: by the date taken, or shuffled (chosen before the start).
- Goes on by itself after 3, 5, 8 (default), 15 or 30 seconds; swiping goes forward and back.
- Transition: cross-fade, cross-fade with a slow zoom, cut, or random (each change a different one).
- Shows the date and the place taken (the place only when the server names places, see 4).
- Videos play through, then the show goes on; meanwhile the music is quieter.
- Music: a playlist of the server, chosen before the start, plays behind it.

## 3. The pictures the server makes

The server makes a preview (at most 480 px) and a screen size (at most 2048 px) of every picture, as a JPEG, upright, kept in
`<data dir>/picture-cache/` and made again when the file is newer. The devices load these, not the originals (an original is only loaded
for a video, or when asked for).

- JPEG, PNG, WebP, GIF, BMP, TIFF: in the server itself.
- HEIC/HEIF/AVIF (the iPhones): libheif, in the Linux packages (Debian: `libheif1`, with `libheif-plugin-libde265`). The servers for Windows
  and macOS show such a picture only by the preview its device sent.
- Videos: one frame through `ffmpeg`, which the package recommends; without it a video has the preview its device sent, or a symbol.

For the pictures of the backup the preview the device sent (`.david-thumbs`) comes first.

## 4. The places

With `photo_places` = `osm` (the default) the server asks OpenStreetMap (Nominatim, `reverse`) for the name of the place a picture was
taken: the coordinates from the EXIF data, rounded to three places (about 100 m), at most one request per second, every answer kept in
`photo_place`. The coordinates leave the house; with `off` nothing does and no place is shown.

## 5. The interface of the apps

`/api/v1/pictures`, see `server/src/api_pictures.rs`: `sources`, the list, `thumb`, `screen`, `file`, `info`.

## 6. Steps

1. Server: the old folders, the previews (with HEIC and video), the places, the rules of access, the interface. *(done)*
2. Browser: the slideshow on the pages of the server, for every person who signs in.
3. Apps: the slideshow with the music.
