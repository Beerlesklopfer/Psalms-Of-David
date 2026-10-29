# Spec: recording radio and a media library from DWGload (hidden features)

Status: **designed, nothing built.** Written after an evaluation of the services on 2026-10-28. Both functions are **hidden features**: off by default and switched on only in the configuration file (decision of the project owner, 2026-10-28). They stay off until the provider (DWG Radio e.V.) has been asked and has answered.

## 1. Goals

1. **Radio recording**: the server records a radio stream, on a button or by a weekly schedule, so that the family can listen to the programme later and offline.
2. **Own media library from DWGload**: an administrator collects talks from DWGload into the library of David, so that they are offered in the apps like any other audiobook.

Both are for the private, family use of the server. Nothing recorded or imported is passed on, and the server is not made reachable from the internet.

## 2. The services (facts, 2026-10-28)

The real addresses belong to **DWG Radio e.V.** (Schulstraße 19, 74251 Lehrensteinsfeld):

| What | Address |
|---|---|
| Radio | `https://radio.dwgradio.net/de/` (also `/cz/`, `/ru/`) |
| Media library "DWGload" | `https://load.dwgradio.net/de/` |

**The domains `dwg-radio.net` and `dwg-load.net` are not the provider.** They show advertising for unlicensed gambling. They must never be configured, and the whitelist of sources (section 4) is there to prevent that kind of mistake.

Radio: three Shoutcast streams of MP3 at 128 kbit/s, 44.1 kHz, no login, no redirect, no DRM; ICY titles come with the header `Icy-MetaData: 1`. The programme exists only as free text ("Programmschema") and as a PDF, there is no machine-readable schedule and no podcast feed with audio.

DWGload: server-side HTML only (no API, no feed, no sitemap). A talk has a stable numeric id (`/de/play/<id>`), a direct MP3 under `/de/data/mp3/<speaker>/<title>.mp3` (ID3v2.3 with title, artist, album, genre, year and cover) and an official download button (`/cgi-bin/download.pl?id=<id>&l=de`). `robots.txt` asks for a crawl delay of 10 seconds. The imprint says: "Alle Predigten stehen für private und pädagogische Zwecke zur freien Verfügung. Die Vervielfältigung und Weitergabe ist erlaubt." The radio pages say nothing about recording.

## 3. Switching on (configuration file only)

```toml
# /etc/david.toml
[features]
# radio_recording = true
# dwgload_library = true
```

- The keys are read like the other values of the file (see `src/system.rs`); the environment has `DAVID_FEATURE_RADIO_RECORDING=1` and `DAVID_FEATURE_DWGLOAD_LIBRARY=1`. The default is off.
- The values are **not** in the database and **not** in the settings of the pages. A change takes effect at the next start.
- Switched off, the function does not exist: its routes are not mounted (404), there is no menu entry, no page, no hint in the settings, no background task. The apps learn about a function only from an entry in `features` of an existing server-information answer, which is **absent** (not `false`) when the function is off.
- The comment of the example file carries sober notes: operation is at one's own responsibility; for private, family use only (§ 53 UrhG); no passing on, no public access to recorded or imported content (§ 19a UrhG), so the server must not be reachable from the internet; the rights stay with the providers and rights holders; the function must not circumvent technical protection; no legal advice, ask the provider in doubt.

## 4. Sources are a list in the configuration

Nothing is built in. A source is named in the file and has a kind:

```toml
[[radio_source]]
name = "DWG Radio"
playlist = "https://login.streamplus.de/app.php/streamserver/public/23644.m3u"   # resolved at each recording, the stream host name is a customer number
[[radio_source]]
name = "DWG Pur"
playlist = "https://login.streamplus.de/app.php/streamserver/public/25531.m3u"

[dwgload]
base = "https://load.dwgradio.net/de/"
```

All requests go through `src/fetch.rs` (public internet only, redirects checked), with an honest `User-Agent` of the form `david/<version> (<contact>)`. A host that is not in the list is refused.

## 5. Radio recording

1. **Recorder**: an HTTP read of the stream with `Icy-MetaData: 1`; the ICY blocks (a length byte × 16 after every `icy-metaint` bytes) are taken out, the MP3 data is written as it came (no re-encoding). `ffmpeg -c copy` is an alternative where ffmpeg is installed.
2. **Control**: start now, or a weekly schedule kept by the administrator (weekday, start, length, station, name of the programme; the free-text programme of the provider is copied by hand). Lead and trail times are settings. A recording can be cut at a change of the ICY title.
3. **Result**: one MP3 per recording with ID3v2.4 (title from the ICY title or the schedule, artist = station, album = programme, date, genre), filed as an audiobook ("Radio recordings") like any other import; the usual duplicate and ignore rules apply.
4. **Limits** (settings, with sober defaults): longest recording, largest file, disk reserve (`diskspace.rs`), how long recordings are kept; old ones are removed automatically.
5. **Preference**: a programme that DWGload offers as a file is imported from there and not recorded.

## 6. Import from DWGload

1. An administrator names single talks (`play/<id>` addresses) or a speaker; the server reads the page, takes title, speaker, date, category and Bible passage from the HTML, and loads the MP3 through the official download address, with at least **10 seconds** between requests.
2. The talk is filed as an audiobook: speaker as author, a series as the album with the part number; ID3v2 is checked and completed (the source is ID3v2.3).
3. Duplicates: the key is the DWGload id; size and hash are compared as well.
4. A **bulk mirror is not built** (about 20,000 files, hundreds of GB, not clearly covered by the licence text). A subscription to the page of new talks is possible only after the provider agreed.
5. Space: a hard limit and a warning, as for recordings.

## 7. Open until the provider has answered

1. May the family server record the three streams for private use? Is there a machine-readable schedule or a podcast feed?
2. May an administrator load many talks, or follow the page of new talks, and at what rate? Is there an API or a feed? Does the association hold the rights to all talks or only a right to distribute them?
3. Only the German version, or also the Czech and Russian ones?
4. May the apps offer the three streams as suggestions (with the logo), and may the provider point to the app?

## 8. Not decided

The schedule could later be filled from a feed of the provider; a station directory (radio-browser.info) could make the list of sources shorter. Both are optional.
