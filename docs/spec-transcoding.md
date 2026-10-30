# Transcoding to MP3

Status: built (server and app), tested with generated files; not tried on a phone or on PDC.

## Why

The Linux engine of the app (symphonia) and Android (ExoPlayer) play Ogg Vorbis, FLAC and M4A (AAC, ALAC); the iPhone (AVPlayer) does **not** play Ogg Vorbis, Opus in Ogg, WMA, Monkey's Audio, Musepack, WavPack and Speex (`audioformat::ios_playable`). The album Caipiranha (Ogg Vorbis) is such a case (project owner, 2026-10-30: "take care of AAC, Vorbis and MP4"). The database knows the format of every file (migration `0051`), so the server can make an MP3 for the devices that need one. The original file is never changed.

## The interface

`GET /api/v1/parts/{id}/stream?as=mp3`

- The same rules as the plain stream: token, visibility, release and the weekly planner (a child who may not hear the part gets `404`, outside the schedule `403`). Nothing is made for somebody who may not hear it.
- A file that is MP3 already is served as it is (no copy).
- The answer is `200`/`206` with `Content-Type: audio/mpeg` and `Range` support, served from the cache file.
- `as=` with another value than `mp3`: `400 unsupported_as`. A file that cannot be decoded: `422 transcode_failed`.
- The length is the length of the original: the apps keep using `duration_ms` of the database.
- The parts of `GET /items/{id}` carry `transcode: "mp3"` when the format is one that some devices cannot play (the list of `audioformat::ios_playable`), else `null`. The app asks for `as=mp3` only when its platform cannot play the format (`app/lib/audio/file_formats.dart`).

## How it is made

`src/transcode.rs`: the file is decoded with symphonia (MP3, Ogg Vorbis, FLAC, WAV/PCM, AAC and ALAC in MP4) and encoded with LAME, the encoder of the CD import (`cdrip::encode::StreamEncoder`), CBR, joint stereo (mono stays mono), quality 3. The sample rate is kept; a rate that MP3 does not know (for instance 96 kHz) is resampled to 44.1 kHz by LAME. A file with more than two channels keeps the first two. A damaged frame is skipped like players do. Decoding and encoding run on a blocking thread, the file is written under `<name>.part` and renamed when whole.

## The cache

- Folder: `<data folder>/transcode/` (the folder of the database, nothing built in).
- Name: `t<track id>-<mtime ms>-<size>-<kbit/s>.mp3`. A changed file has another time or size, so it is made again, and the older versions of that track are removed at once.
- Size limit: the setting `transcode_cache_mb` (default 2048). Above it the least recently used files are removed (a file's time of use is set when it is served); the file just made or served always stays. A `.part` file older than an hour (a crash) is removed.
- A second request for the same track waits for the first and then finds the file: made once.

## Settings (settings > library)

| Setting | Default | Meaning |
|---|---|---|
| `transcode_kbps` | 128 | Bit rate of the MP3 (the nearest one LAME offers: 8 to 320). |
| `transcode_cache_mb` | 2048 | Size limit of the cache. |
| `transcode_parallel` | 2 | Files transcoded at the same time (1 to 8); read at the start of the server. |

Migration `0052` creates them (confirmed). The `.deb` needs nothing new: LAME is linked statically (cdrip), symphonia is Rust.

## The app

A track that the platform cannot play and that has `transcode: mp3` is fetched with `?as=mp3` and kept as `<part id>-mp3.audio` next to the original name `<part id>.audio`; downloads (offline) do the same, so the offline copy plays on the iPhone too. The first request can take seconds (a track of a few minutes). If the server answers `transcode_failed`, or is too old to offer `transcode` (the field is missing), the message "This file format cannot be played on iPhone yet" stays as before.

## Not done

- No transcoding to AAC or Opus (MP3 plays everywhere).
- The cache is not shown in the admin pages; `ls <data folder>/transcode` shows it.
- A changed `transcode_parallel` applies at the next start.
