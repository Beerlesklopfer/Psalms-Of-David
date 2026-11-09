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

## 6. The project: a slideshow as one file

Decided by the project owner on 2026-11-08: a slideshow has to lie on a device by itself, to be shown without the server and without a
network ("auf den Geräten muss die Datei autonom liegen"). The page `/pictures` saves the choice as a project ("Als Projekt speichern",
`GET /pictures/project`, for the apps `GET /api/v1/pictures/project`, with the query of the list and the choices `order`, `seconds`,
`transition`, `videos`, `info`, `music`). It is one ZIP file, written while it is sent (`server/src/show_project.rs`):

| Entry | What |
|---|---|
| `show.json` | `{format: "david-slideshow", version: 1, title, lang, made_at, settings: {order, seconds, transition, videos, info}, pictures: [{image, video, taken_at, place, transition?, seconds?}], music: [{file, title, artist}]}`; the apps read it. `transition` and `seconds` only for a picture of a project put together by hand (9) that has its own; a player that does not know them plays it with `settings` |
| `show.js` | the same as `window.DAVID_SHOW = …`, for a browser that opens `index.html` from the disk |
| `index.html`, `slideshow.js`, `slideshow.css` | the player: the slideshow of `/pictures` (the same script), reading the project |
| `pictures/0001.jpg` … | the pictures in the screen size; the frame of a video is its poster |
| `videos/0002.mp4` … | the videos as H.264/AAC (at most 1920 px), which every device plays; made by ffmpeg and kept in the cache |
| `music/01.mp3` … | the music of the chosen playlist as MP3 |

A picture the server cannot show is left out; `show.json` comes last and names only what is in the archive. On a computer the file is
unpacked and `index.html` opened; the apps keep it and play it (step 3). As a video for a television: see step 5.

## 7. The video for a television

Decided by the project owner on 2026-11-08: Full HD (1920×1080, 30 frames, H.264/AAC in MP4); asked for on `/pictures` ("Als Video
speichern", `POST /pictures/videos` with the query of a project), made by the server in the background, one after the other (the page
may be closed; a video that was being made when the server stopped is made again); listed with its progress, downloaded
(`GET /pictures/videos/{id}/file`), kept until the person deletes it (`POST /pictures/videos/{id}/delete`). Table `show_video`, files in
`<data dir>/show-videos/`, `server/src/show_video.rs`.

It looks like the slideshow: every picture its seconds, the transition of the choice (a cut is 0.04 s, a fade 1 s), the slow zoom, the
date and the place at the bottom on the left (DejaVu Sans: Debian `fonts-dejavu-core`, RPM `dejavu-sans-fonts`; without a font or
without `drawtext` in ffmpeg the video has no caption), the videos through with their sound, the music of the playlist again and again
behind it all, at 15 % while a video plays, fading out at the end. Shuffled, the order is drawn once, when it is made.

## 8. Steps

1. Server: the old folders, the previews (with HEIC and video), the places, the rules of access, the interface. *(done)*
2. Browser: the slideshow on the pages of the server (`/pictures`, menu of the person), for every person who signs in except a guest.
   It is the model of the slideshow in the apps (project owner, 2026-11-08): made for the phone first (tap shows the controls, swipe
   left/right goes on/back, swipe down ends it), the same choices in the same order. *(done)*
3. Apps: the projects kept on the device and played without the server (library, rubric "Pictures"; `app/lib/pictures/`): a new
   slideshow is chosen as on the page `/pictures` and loaded as a project (`GET /api/v1/pictures/project`), a project file saved in the
   browser can be opened, and the player is the slideshow of the browser (tap, swipe, a swipe down ends it, the keys; the music of the
   project, quieter while a video plays). Videos play on iPhone/iPad, Android and macOS; on Linux and Windows a video shows as its
   picture. *(done)*
4. The project as one file (6), with the player of the browser. *(done)*
5. The project as a video (MP4) for a television or a USB stick (7). *(done)*
6. Projects put together by hand in the browser (9): the preview to pick, the editor of the order and the transitions; slideshow,
   project file and video of a project; the apps play the transition and the seconds of each picture of a project file. *(done)*

## 9. Projects put together by hand

Decided by the project owner on 2026-11-08: in the browser a person picks pictures in a preview and puts them into a project (kept in
the database); a second editor sets their order by numbers (a field, the buttons up and down) and, below each picture, its transition
(a combobox); the properties of the project are what every picture starts with. Also decided: a project plays as a slideshow, is saved
as a project file and made into a video like the quick choice, which stays as it is; each picture has a transition and seconds of its
own; only the person who made a project sees it and changes it; the properties are the title, the music (an own playlist), the
choices of the slideshow and the order (by the numbers or shuffled).

- `/pictures` lists the projects of the person and makes a new one (a title), which opens the preview.
- The preview (`/pictures/shows/{id}/pick`): the sources and years as on `/pictures`, and whether videos show; the pictures as small
  pictures, month by month; a tap puts one into the project or takes it out, saved at once. New pictures go to the end in the order
  they were taken; "Add all shown" and "Take out all shown". At most 5000 pictures in a project.
- The editor (`/pictures/shows/{id}`): the properties (title, order, seconds, transition, videos, date and place, music), saved when a
  field changes; the pictures in their order, each with its number field, up and down, taking it out, and below it its transition
  ("as in the properties", cross-fade, cross-fade and zoom, cut) and its seconds ("as in the properties", 3 to 30; not for a video,
  which plays through). "Videos: show as a picture" plays a video of the project as its still picture. Start, save as a file, save as
  a video as on `/pictures`.
- Tables `slideshow` and `slideshow_picture` (schema 0067), `server/src/slideshow.rs`; every request that takes a choice takes
  `show=<id>` instead (`/pictures/list`, `/pictures/project`, `/api/v1/pictures/project`, `POST /pictures/videos`). A picture the person
  may no longer see stays in the project and is left out when it plays; the editor says how many.
- The apps do not make or change projects; a project file of one plays there with the transition and seconds of each picture.

Decided by the project owner on 2026-11-09: the projects reach the apps. The rubric "Pictures" of the app lists them under "Projects on
the server", and each one plays from the server or is loaded onto the device; when it changes on the server, the copy on the device says
"newer version on the server" and one tap brings it up to date (nothing is loaded by itself).

- `GET /api/v1/pictures/shows`: the projects of the person with `rev`, a mark of what a project plays now (title, choices, music list,
  the pictures in order with their transitions and seconds, and which of them the person may see); it changes with every change.
- Played from the server: `GET /api/v1/pictures/shows/{id}` gives the keys and the tracks; the app fetches each picture
  (`/pictures/{key}/screen`), video (`/pictures/{key}/video`, the MP4 of the project file) and track (`/parts/{id}/stream?as=mp3`) just
  before it is shown, the next one meanwhile, into a folder that goes when the show ends; the place comes from `/pictures/{key}/info`.
  When the server gives no picture at all, the show stops and says that the server cannot be reached.
- Loaded onto the device: the project file (`/api/v1/pictures/project?show=<id>`), and next to `show.json` a `server.json`
  (`{id, rev}`) that says which project it is a copy of and in which version. An update replaces the older copy. A copy plays offline.
- A change of the tracks inside the playlist of the music does not change `rev`.
