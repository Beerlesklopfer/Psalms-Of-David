# Spec: ripping audio CDs inside David

Status: **design, being built in the server** (see "Where it is ripped" and `docs/spec-cd-import-api.md`; the app part is built in `app/lib/cd/`). Written after reading the sources of `cd-da-reader` 1.0.0 (the crate archive from crates.io, unpacked and read file by file), its GitHub issues and commits, the MusicBrainz and Cover Art Archive documentation, and the David sources named below. Statements about the crate carry a source in square brackets, `[crate: path]` meaning a file inside the crate archive `cd-da-reader-1.0.0`. Anything that could not be shown from a source is marked **unverified** or listed under "Open".

Scope: Linux server (David runs as a systemd service). Windows and macOS are not goals of this spec, although the crate supports them.

## Requirements (project owner, 2026-10-28)

1. Read audiobook CDs directly in David instead of by hand with `abcde`.
2. Same result as today: MP3, 64 kbit/s, 24 kHz joint stereo, tags and cover, one folder per book, the CD ejected at the end.
3. Many-disc books ("CD 1 of 12") arrive as **one book**.
4. Nothing hard-coded: folders, bitrate, naming scheme, genre text and service addresses are settings (`/etc/david.toml` or the browser settings).
5. Copy-protected discs are recognised and refused, not circumvented.

## Where it is ripped: in the server (the drive is on the server machine)

The CD drive is attached to the server (PDC), so the server reads, encodes, tags and files the disc, and detects drive and disc itself (kernel uevents when the setting `cd_udev` is on, else a look every few seconds; the code is the crate `server/cdrip/`). The apps (all platforms, not only Linux) show what the server reports and trigger the work: two symbols in the library, import and eject, shown by the server's drive state (shut tray; an audio CD for import, any disc for eject; nothing for an open tray, because nothing ever closes a tray), the import dialog (`POST /api/v1/cd/rip`), the progress from the job events, and `POST /api/v1/cd/eject`. The interface is `docs/spec-cd-import-api.md`; the app code is `app/lib/cd/`. How the rip reads (a pipeline of a reading thread and an encoding thread with a queue of 8 seconds, the verification as a second pass over the disc in the direction of the disc, a steady read speed set by `SET CD-ROM SPEED` and given back, no query of the drive while it rips) is described in `docs/spec-cd-import-api.md`, section "The drive of the server". An interrupted rip is resumed from the folder `disc-<Disc ID>` and its manifest; a CD that is in the library already is checked and repaired, and an automatic import can be switched on (see the same document). A first version had the app rip locally through an FFI library; that was removed when the drive moved to the server.

## 1. What `cd-da-reader` is

| Fact | Value | Source |
|---|---|---|
| Version | 1.0.0, published 2026-08-26; eight releases since 2025-08-31 (0.1.0, 0.2.0, 0.2.1, 0.3.0, 0.3.1, 0.4.0, 0.4.1, 1.0.0) | https://crates.io/api/v1/crates/cd-da-reader |
| Licence | MIT | `[crate: Cargo.toml]`, `[crate: LICENSE]` |
| Repository | https://github.com/Bloomca/rust-cd-da-reader, one main author (Bloomca), contributions from two others in the last commits; 5 stars, 1 fork, 5 open issues, last push 2026-09-12, not archived | GitHub API, `repos/Bloomca/rust-cd-da-reader` and `.../commits` |
| Downloads | 2,220 in total, 1,283 "recent" | crates.io API |
| Dependencies | none on Linux except `libc` (`windows-sys` on Windows, `libc` plus a C shim built with `cc` on macOS); no features | `[crate: Cargo.toml]` |
| Edition | 2024 | `[crate: Cargo.toml]` |

**Maintenance judgement:** young, small, one active maintainer, but active (the 1.0 release is five weeks old at the time of writing, with changes in each of the last weeks of August 2026). Small user base: a bug found by David will probably be David's to report or fix. This is an assessment, not a measured fact.

### What it can do

- **Drive listing.** `CdReader::list_drives()` reads `/sys/class/block` for names starting with `sr` and falls back to `/dev/cdrom`; for each it opens the drive and reads the TOC to set `has_audio_cd`. `open_default()` opens the first drive with an audio disc. `[crate: src/platform/linux/device.rs, src/discovery.rs]`
- **Opening.** `CdReader::open_path("/dev/sr0")` opens with `O_RDWR | O_NONBLOCK` (read-write is needed for the SG_IO ioctl). `[crate: src/platform/linux/device.rs, src/lib.rs]`
- **Open by descriptor.** `CdReader::from_file(File)` (Unix only) takes a descriptor that the caller opened itself, documented for privilege-separated helpers; on Linux it must be `O_RDWR`. `[crate: src/lib.rs, doc of `from_file`]`
- **Table of contents.** `read_toc()` sends SCSI `READ TOC/PMA/ATIP` (0x43, format 0) and returns `Toc { first_track, last_track, tracks: Vec<Track>, leadout_lba }`; each `Track` has `number`, `start_lba`, `start_msf`, `is_audio` (from the control nibble). `[crate: src/platform/linux/toc.rs, src/parse_toc.rs]`
- **Reading audio.** SCSI `READ CD` (0xBE), expected sector type CD-DA, main channel only (CDB byte 9 = 0x10), at most 64 KiB per command (27 sectors); PCM is signed 16-bit little-endian stereo at 44.1 kHz, 2,352 bytes per sector. `[crate: src/data_reader/mod.rs `build_read_cd_cdb`, src/data_reader/sector_read_format.rs]`
- **Whole track or stream.** `read_track` buffers a whole track (the docs warn of hundreds of megabytes); `open_track_stream(&toc, number)` returns a `TrackStream` whose `next_chunk()` gives sector-aligned PCM. The stream knows `total_sectors()`, `current_sector()`, `current_seconds()`, `total_seconds()`, `seek_to_sector()`, `seek_to_seconds()`, `with_sectors_per_chunk()`. That is the progress information. `[crate: src/stream.rs, examples/stream_with_progress.rs]`
- **Errors and retries.** Each chunk is retried with capped exponential backoff (default 4 attempts, 20 ms to 300 ms), and after a failure the chunk shrinks (27 sectors, then 8, then 1). A short read counts as a failure. After the last attempt the whole read returns an error: the crate never fills a bad sector with silence and never skips it. `RetryConfig` is configurable. `[crate: src/retry.rs, src/read_loop.rs]`
- **Error detail.** `CdReaderError::{Io, Scsi, Parse, TrackFormatMismatch, CannotDetectTrackFormat, NoUsableDrive, Backend}`; `ScsiError` carries the operation, LBA, sector count, SCSI status, sense key, ASC and ASCQ. Useful to tell "no disc" or "drive went away" from "unreadable sector". `[crate: src/errors.rs, src/platform/linux/sg_io.rs]`
- **Read speed.** `ReadSpeed::{Unchanged, Optimal, CustomMultiplier(n)}`; on Linux this is the `CDROM_SELECT_SPEED` ioctl (0x5322); the crate does not restore the previous speed. `[crate: src/data_reader/read_speed.rs, src/platform/linux/speed.rs, README "Read options"]`
- **Timeouts.** 30 s per `READ CD` command, 10 s for the TOC. `[crate: src/platform/linux/read_cd.rs, toc.rs]`
- **A seam for tests.** The trait `AudioSectorReader` (`read_audio_sectors(&self, start_lba, count) -> Result<Vec<u8>, Error>`) lets `read_track` and the stream run over a file, an image or memory, given a hand-made `Toc`. `CdReader` itself implements it. Two examples show an in-memory disc and a BIN/CUE image. `[crate: README "Reading from a file image", src/backend.rs, examples/file_backend.rs, examples/bin_cue_backend.rs]`
- **Other.** `create_wav(pcm)` (44-byte header), `lba_to_msf`, data-track reading (not needed here), `TrackBounds` (including the CD-Extra session-gap rule: 11,400 sectors are subtracted from the last audio track before a data session). `[crate: README]`
- **Platforms.** Linux (SG_IO), Windows (SPTI), macOS (IOKit shim); README says "tested on Windows, macOS, and Linux", and CI builds all three. `[crate: README, src/platform/*, .github/workflows/pull-request-workflow.yaml]` How well each is tested on hardware is **unverified**.
- **Rights.** The crate says it "operates entirely in user space" `[crate: src/lib.rs]`. In practice it needs a descriptor that the kernel lets the process open `O_RDWR` and use for SG_IO. On Linux that is access to the device node (`/dev/sr*`), normally by group membership. The crate does not mention a group name; the group on the development machine is `cdrom` with an ACL (`ls -l /dev/sr1`: `brw-rw----+ root cdrom`), which is a property of that distribution, not of the crate. Whether SG_IO needs `CAP_SYS_RAWIO` for READ CD on a block device opened read-write: **unverified**; the kernel rules for SG_IO command filtering for unprivileged users were not read for this spec. **Spike 1 must show it** (open question 1).

### What it cannot do

| Missing | Evidence | Consequence for David |
|---|---|---|
| **Encoding** (MP3, FLAC, Opus) | no encoder anywhere; output is PCM or WAV | needs a separate encoder (section 3) |
| **Tags, covers** | not present | David already has `lofty` (tags.rs) |
| **MusicBrainz, Disc ID, CDDB** | README "What about metadata?": the TOC "exposes the track addresses and lead-out required by the MusicBrainz disc ID algorithm"; the crate does not calculate or look up anything | David computes the Disc ID and queries MusicBrainz itself (section 4) |
| **CD-Text** | README: "CD-TEXT exists, but it is unreliable and is not provided by this crate"; no READ TOC format 5 in the source | not in version 1 of the feature; see Open |
| **Eject** | no eject code; issue #66 "Add CD Drive eject command" is open (created 2026-08-26) | David ejects itself (ioctl or a program, section 5) |
| **Live drive state** | issue #67 "Change DriveInfo to Drive" is open: `DriveInfo` is a snapshot, "drive state can change after eject" | David polls the drive itself |
| **C2 error flags, subchannel data** | `Raw` formats "do not include subchannel data or C2 error information"; the audio format asks for the main channel only (byte 9 = 0x10); issue #53 "Add subchannel info option" is open | a sector the drive reads without a SCSI error is accepted, even if it is wrong |
| **Verification, jitter correction, drive cache handling** | the retry loop reacts only to an error return; no re-read comparison, no overlap search (what `cdparanoia` calls paranoia) | see "Reliability" below |
| **AccurateRip, copy-protection detection** | no such code | see section 9 |
| **Async** | README: streaming "is still synchronous"; a GUI "should run the loop on a worker thread" | David runs it on a blocking thread or in a child process |
| **Identity of a drive** | `DriveInfo` has only `path` and `has_audio_cd` | David reads vendor, model and serial from the system itself (section 5) |
| **Cancelling a running command** | the SG_IO call blocks until the drive answers or the 30 s timeout ends `[crate: src/platform/linux/sg_io.rs]`; no cancel API | cancel between chunks; for a hung drive, a child process that can be killed (section 5) |

**Reliability, stated plainly.** For a clean disc `READ CD` returns the right samples and this is enough. For a scratched disc a drive may return interpolated data without reporting an error; `cdparanoia` was written to catch that. `cd-da-reader` does not. Consequence: David should do its own cheap check (read each track twice and compare a SHA-256 of the PCM; section 5) or hand a bad disc to `cdparanoia` as a child process (section 3 alternative). Whether the second read is really independent of the drive's read cache on a given USB drive is **unverified** and has to be tried with the owner's drive.

Whether `CdReader` is `Send` follows from its only field (a `File` inside `Drive` `[crate: src/platform/linux/device.rs]`), so it should be; it was not compiled against that bound here (**unverified**).

## 2. What is on the machine today (for comparison)

`cdparanoia` III 10.2 (Debian package 3.10.2), `lame` 3.100, `abcde` 2.8.2 and `ffmpeg` are installed on the development machine (`cdparanoia -V`, `lame --version`, `/etc/abcde.conf`). `cdparanoia -h` lists drive analysis (`-A`), speed (`-S`), overlap search (`-o`), device (`-d`) and progress to stderr for wrappers (`-e`). Those are the facilities `cd-da-reader` lacks. The tool dates from 2008 but is stable and widely packaged; it is a package dependency, not a crate.

## 3. Encoding

The wanted output is the current one: MP3, 64 kbit/s CBR, 24 kHz, joint stereo. A CD gives 44.1 kHz stereo, so the encoder must resample to 24 kHz (LAME does this itself when the output rate differs, as the owner's `lame -b 64 -m j` already does; the exact command line in `abcde.conf` was not read, **unverified**).

| Option | For | Against |
|---|---|---|
| **A. `lame` as a child process** (reads WAV/PCM on stdin, writes a file) | the exact encoder and options used today, same output; no build dependency; PCM is piped, so memory stays small; an encoder crash cannot take David down | needs the `lame` package at run time; a pipe to manage; LGPL program only, no linking issue |
| **B. `mp3lame-encoder` crate** (0.2.5, 2026-08-20, LGPL-3.0, "high level bindings to mp3lame"; builder with channels, sample rate, bitrate, quality, VBR, ID3 tag) | in process, no pipe, one binary; the crate is popular (1.18 million downloads) | LGPL-3.0 library inside an AGPL-3.0-or-later program: allowed, but static linking has extra obligations (re-link possibility); the docs do not say whether LAME is compiled from source or linked from the system (**unverified**; it depends on `mp3lame-sys`); the docs do not mention joint stereo (**unverified**); needs a C compiler or `libmp3lame` at build time; a LAME crash or leak sits in David's process |
| **C. `ffmpeg` as a child process** (`-c:a libmp3lame -b:a 64k -ar 24000 -ac 2`) | also does FLAC/Opus/AAC, can write tags and cover in one call | large dependency; the options map to LAME's but the result is not byte-identical to today's; more to parse for progress |
| **D. Opus (`opus` crate, or ffmpeg)** | better quality per bit for speech; at 32 to 48 kbit/s a book is smaller than at 64 kbit/s MP3 | **not the current format**: the player (symphonia 0.6, see spec-audio-pipewire.md) and the apps must handle `.opus` containers; changes the whole library, not just ripping; a separate decision |
| **E. FLAC (`flac` program, or `flacenc`)** | lossless, allows re-encoding later | roughly 10 times the size of 64 kbit/s MP3 for a book of 12 discs (about 7 to 8 GB at 600 MB of PCM per disc, compressed to roughly half: an estimate, not measured); the library and the apps expect MP3 today |

**Recommendation (decision of the project owner, 2026-10-28): version 1 uses option B, the `mp3lame-encoder` crate**, producing the same files as today (MP3, 64 kbit/s CBR, 24 kHz, joint stereo). Option A (`lame` as a child process) stays as the fallback behind the same trait, to be used if a check below fails or if the LGPL point is judged to be a problem. Opus (D) comes later, only as an option for players that can play it. The `lame` crate and `audiopus` are not used. The encoder sits behind a trait (`Encoder`), so changing it touches one module. All values (bitrate, sample rate, mode, program name for option A) are settings (`rip.*`), nothing is built in.

### Evaluation of the crates (2026-10-28, from crates.io and the repositories)

| Crate | State | Licence | Native part | Verdict |
|---|---|---|---|---|
| **`mp3lame-encoder`** 0.2.5 (2026-08-20) | 12 versions, 1.18 million downloads (297,000 recent); repository `DoumanAsh/mp3lame-encoder` | LGPL-3.0 | wraps `mp3lame-sys` 0.1.11 (2025-12-04, LGPL-3.0, 1.1 million downloads), which **bundles LAME 3.100 as C source and builds it statically** ("static only build with bundled version 3.100"); no system library needed; a C compiler is expected (not stated, **unverified**) | **used for version 1**, after the checks below |
| **`lame`** 0.1.3 (2017-10-29) | 4 versions, 32,000 downloads, nothing since 2017 | MIT | links a system `libmp3lame` | **rejected**: unmaintained for nine years; `mp3lame-encoder` replaces it |
| **`opus`** 0.4.0 (2026-08-23) | 7 versions, 3.4 million downloads; repository `SpaceManiac/opus-rs` | MIT or Apache-2.0 | `opusic-sys` 0.7.6 (2026-10-26, BSD-3-Clause); the README says "by default, you need `cmake` and a C compiler", so libopus is most likely built from source (**unverified**) | **later**, as an option; it yields packets only: an Ogg container (another crate, not evaluated) and Vorbis comments (`lofty` can write them) are needed |

Points that count for the choice:

* **Licence.** LAME is LGPL; David is AGPL-3.0-or-later and its source is public, so a user can re-link. Statically linking an LGPL library is allowed on that condition; the package carries the licence notices and says where the source is. The `decoder` feature of `mp3lame-sys` (it enables a GPL decoder) **stays off**; it is not needed to encode.
* **Opus saves space but is not playable everywhere.** For speech about 24 to 32 kbit/s is usually enough where MP3 needs 64 (the usual experience, **not measured here**). But `symphonia` 0.6.1, which the Linux sound engine (`audio/`) uses with only its `mp3` feature, has no Opus decoder in its feature list, so Linux would need `opus` for decoding and an Ogg reader. On iOS `just_audio` uses AVPlayer, which as far as is known does not play Ogg Opus (**unverified**: test on a device before any decision). Android's ExoPlayer plays it. The whole library is MP3 today; a second format would double work in the scan, the merge of discs, covers, tags and the apps.

### Checks before version 1 may rely on option B (spike 3)

1. Encode one ripped track with `mp3lame-encoder` and compare with the same track encoded by `lame -b 64 -m j` (the owner's file from `abcde`): bitrate 64 kbit/s, sample rate 24 kHz, joint stereo, duration within one frame, size within a few percent, no audible difference.
2. The crate lets the sample rate, the bitrate, CBR and **joint stereo** be set (the README does not say; **unverified**).
3. It builds on the owner's build machine and in the GitHub workflow (C compiler present) and for the packages (`cargo deb`, `cargo generate-rpm`) without a system `libmp3lame`.
4. A crash or leak of LAME cannot corrupt the staging files (the worker process of section 5 is where the encoder runs, so a crash is a failed track, not a failed server).

If one of 1 to 3 fails, option A is used and nothing else in this spec changes.

Encoding runs **after** a track is read completely and verified, not during the read: the drive is kept running at its own pace and a slow encoder cannot make it stall. PCM of one track goes to a temporary file in the staging folder (a track is some tens of MB), then `lame` reads that file. This also gives resumption (section 5).

Tags are written with `lofty` after encoding (David already depends on it, `server/Cargo.toml`): title, artist, album artist, album, disc number, track number, genre (setting), embedded cover. Today's convention (track = disc * 100 + track number) is a **naming rule setting**, `rip.track_number_scheme`, with the values `plain` and `disc_times_100`; the default is a decision for the owner (open question 4).

## 4. Flow from "CD inserted" to "book in the library"

States of one **rip job** (stored in the database, section 8): `waiting_for_drive` → `identifying` → `awaiting_confirmation` → `ripping` → `encoding` → `placing` → `done`; side exits `failed` and `cancelled`. A **batch** is the list of discs that belong to one book (one confirmation, many discs).

1. **Find the drive.** Every second or two while the page is open (and every ten seconds otherwise; both are settings), David lists optical drives: the names from the crate's `list_drives` (paths `sr*`) combined with identity from sysfs (vendor, model, serial: `/sys/class/block/srN/device/…`; serial may be missing on a USB bridge; **unverified** on the owner's drive). A setting `rip.drive_match` (list of "vendor model" or by-id patterns, empty = any optical drive) says which one is *the* drive. The path is never stored as the identity; the job remembers the identity and re-resolves the path at each step. This is how a drive that moves from `/dev/sr0` to `/dev/sr1` is followed.
2. **Recognise the disc.** Open the device, `read_toc()`. Outcomes: no disc / tray open / not ready (SCSI sense key "not ready", shown as "no disc in the drive"); TOC with at least one audio track: go on; TOC with only data tracks: "this is a data disc, not an audio CD". A CD-Extra disc (audio tracks, then a data session) is read as audio tracks only.
3. **Calculate the MusicBrainz Disc ID** from the TOC: the algorithm is documented at https://musicbrainz.org/doc/Disc_ID_Calculation (first and last track, lead-out and up to 99 offsets as `%02X`/`%08X` hex text, offset = LBA + 150, SHA-1, base64 with `.`, `_`, `-` for `+`, `/`, `=`; data tracks excluded; for a disc with a data session the lead-out is the end of the last audio track). That needs a SHA-1 implementation (David has `sha2` only; a `sha1` crate is a small addition, or the `Sha1` of a crate already in `Cargo.lock`; to be checked in spike 2).
4. **Fetch metadata**, in this order, first hit wins but all candidates are shown:
   1. *Already known in David:* a job or a book with the same Disc ID (a disc that was ripped before; it avoids a second ripping by mistake and offers "rip again").
   2. *MusicBrainz* `GET {musicbrainz_url}/ws/2/discid/{id}?inc=…&fmt=json`, and if the exact id is unknown the fuzzy form with `toc=1+<tracks>+<leadout>+<offsets…>` (documented at https://musicbrainz.org/doc/MusicBrainz_API; "toc" lookups return CD-format mediums by default and CD stubs are returned when there is no release; `cdstubs=no` turns that off). The owner's earlier `…/discid/-?toc=…` call is this form. The exact `inc=` values (artist credits, media, recordings) are **to be fixed in spike 2** from the live response. The reply gives release title, artist credit, the list of media with `position` and the number of media of the release. That is "CD n of N".
   3. *Cover:* `GET {coverart_url}/release/{mbid}/front-500` (also `-250`, `-1200`); the answer is a 307 redirect to archive.org and a 404 if the community has chosen no front image (https://musicbrainz.org/doc/Cover_Art_Archive/API). The existing downloader `fetch::download_image` (public addresses only, size limit, checks that it is a picture) is reused.
   4. *CD-Text:* not in version 1 (the crate does not read it); see Open.
   5. *By hand:* an empty form. For title text like `Foo (CD 2)`, the existing `relocate::strip_disc` splits title and disc number.
   MusicBrainz rules to follow (https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting): about **one request per second per IP**, excess requests get HTTP 503; every request needs a **User-Agent** with application name, version and a contact address, such as `Name/1.2.0 ( contact )`. So: one global rate limiter (queue, one request at a time, at least one second apart), User-Agent from a setting (`rip.user_agent_contact`) that is mandatory before lookups are enabled, and a short cache by Disc ID in the database so a retry does not ask again. No request is sent without the setting.
5. **Confirm.** The page shows the guess (author, title, "CD n of N", cover, track list with durations, total size) as an editable form. Nothing is read from the disc beyond the TOC until the user presses *Save* (section 7).
6. **Check space and permission**, then **rip** (section 5), **encode**, **tag**, **place** (section 6).
7. **Eject** (section 5) once the files are safely placed, and the page says "Insert CD n+1 of N". The next disc of the same batch is recognised by its TOC (different Disc ID) and pre-filled from the same release: author, title and N stay; n comes from the matching medium of the MusicBrainz release, or "previous n + 1" if there is no match; the user confirms with one click.
8. **Import.** After the last track of a disc is placed, call `Store::rescan_soon()` (existing, waits 5 s for a burst to end, one import at a time; `server/src/importer.rs`). If the import is locked (`ImportLocked`, spec-discovery-profiles-roles.md 3c) the files are still placed and the page says that the import is locked; new books stay unreleased until the owner releases them, as for every other import.

## 5. Where it runs, and how it survives a bad drive

### In the service or on its own

The service starts as root and then drops to the user `david`, taking the groups of that account plus the list `groups` in `/etc/david.toml` (`server/src/system.rs`: `group_list` and `setgroups`/`setgid`/`setuid`). So drive access is a **group**: add the group that owns the optical device to `groups` (or to the account with the system's tools). No rights on other people's files change, and no rights are given to the service beyond what a desktop user in the same group has. The unit file today has no `PrivateDevices=` and no `DeviceAllow=` (`server/packaging/david.service`), so `/dev/sr*` is visible; if the unit is hardened later it needs `DeviceAllow=block-sr rw` (**unverified**). A USB drive that appears after the start is visible because `/dev` is the host's.

Options:

| | In-process (blocking thread) | Same binary as a child process, `david rip-worker …` |
|---|---|---|
| Simplicity | least code | one more subcommand and a small line protocol |
| Hung drive | a thread stuck in an ioctl cannot be cancelled; it ends when the 30 s command timeout returns, and if the kernel never returns, only a restart of David helps | the parent kills the worker; the kernel releases the descriptor |
| Memory and leaks | share David's heap | freed with the process |
| Rights | same user and groups | same; and later it could be sandboxed more tightly than the server |
| Progress | direct | JSON lines on stdout |

**Recommendation: the child process (same binary, subcommand `rip-worker`), JSON lines for progress and results.** The drive is the least reliable part of the system; isolating it costs a few dozen lines. Everything above the drive trait (job state, naming, tags, placing) stays in the server, so the worker is small: "open this device, read these tracks into these files, report". In-process on a blocking thread is acceptable for spike 1 and 2.

### The trait in front of the crate

```text
trait DiscDrive {            // what David needs from a drive
    fn identity(&self) -> DriveIdentity;
    fn read_toc(&self) -> Result<Toc, RipError>;
    fn read_track(&self, number, progress, cancel) -> Result<PcmFile, RipError>;
    fn eject(&self) -> Result<(), RipError>;
}
```

Implementations: `CdDaReaderDrive` (the crate, with `from_file`), `ProgramDrive` (optional, `cdparanoia` as a child process, see section 11), `FakeDrive` (tests, section 10). The crate's own `AudioSectorReader` is the lower seam and is used by `FakeDrive`.

### Reading a track safely

1. Check free space for the whole disc **before** starting: PCM is 176,400 bytes per second; the disc's total PCM plus encoded output plus a reserve is compared with `diskspace::fits` (existing, `server/src/diskspace.rs`) on the staging folder and on the library folder (`diskspace::same_disk` tells whether one check is enough). The reserve is the existing setting used by `diskspace::reserve`.
2. Per track: stream with `open_track_stream`, `next_chunk()` in a loop, append to `track-NN.pcm.part` in the staging folder, SHA-256 over the bytes, progress = `current_sector()/total_sectors()`; check the cancel flag between chunks. Read speed from a setting (`rip.read_speed`: `unchanged`, `optimal`, or a number; slower is more reliable on a USB drive, and the crate's issue #59 reports a disc that only read right at a limited speed).
3. **Verification pass** (setting `rip.verify`: `off`, `second_read`; recommended default `second_read`): read the track again and compare the SHA-256. If different, read a third time and take the two that agree; if no two agree after N reads (setting, default 3), the track is **failed** and marked "read errors on this disc" rather than being kept. This is weaker than `cdparanoia`'s paranoia mode (no overlap search) and the drive cache may return the same wrong data twice (**unverified**; spike 1 must test with a scratched disc, ideally a worthless one).
4. When the PCM is good: rename `.part` to the final `.pcm`, write the hash to the job row, then encode (section 3) to `track-NN.mp3.part`, tag, `fsync`, rename.
5. **No half results.** Nothing enters the library folder until every wanted track of the disc is complete. Staging is under the data folder (setting `rip.staging`, default below `data_dir`). At the end the disc folder is **moved** into the library (`rename` when on the same file system, otherwise copy then rename within the destination, then delete the source). If the destination folder or file exists, the job stops with a clear message instead of overwriting (section 6).

### Drive disappears, name changes, disc removed

- A read error with `Io` `ENODEV`/`EIO`/`ENXIO` or SCSI "not ready" / "medium not present" is a **drive-state error**, not a data error (the exact errno and sense values are **unverified** and come from spike 1 by pulling the cable). The worker exits with code "drive gone"; the server sets the job to `waiting_for_drive`.
- While waiting, the server polls the drive list. When a drive that matches `rip.drive_match` appears (new path or same), it starts a new worker, reads the TOC and **compares it with the job's TOC**. Same TOC: continue with the first track that has no verified PCM hash. Different TOC or no disc: the page says "Insert CD n of N again" and keeps the finished tracks in staging until the job is cancelled, expires (setting, days) or the right disc comes back.
- Tracks that were fully read and hashed before the break are not read again. A track in progress starts over (resuming mid-track is possible with `seek_to_sector` `[crate: src/stream.rs]` but needs the partial PCM kept; not worth it for tracks of minutes).
- A server restart finds jobs in `ripping`/`encoding`, deletes `.part` files, and sets them to `waiting_for_drive`.
- **Eject** is not in the crate (issue #66). Two options: the `CDROMEJECT` ioctl (`linux/cdrom.h`; the constant was not checked here, **unverified**) on the same descriptor, or the `eject` program (util-linux). Recommendation: the ioctl in the worker, with the program as a setting-driven fallback (`rip.eject_command`, empty = ioctl). Eject only happens after "placing" succeeded, or on the user's *Eject* button; never on failure with unplaced data unless asked.

## 6. Several discs, one book

- The user confirms **once per batch**: author, title, N, and per disc the number n. MusicBrainz gives n as the position of the medium that carries the scanned Disc ID, and N as the number of media of the release (documented structure of a release; the fields' names to be verified in spike 2).
- **Layout written by David** (all parts are settings, defaults shown as examples): `<audiobook folder>/<Author>/<Title>/CD <n>/<Author>_<Title>_CD<nn>_TR<nn>.mp3`. The scanner already reads exactly this: disc folders `CD n` inside a book folder that has no audio files of its own become discs of that one book, and the file name pattern `…_CD02_TR05` gives disc and track number, with the tags as fallback (`server/src/scan.rs`, header comment and `collect`). The owner's current flat layout (`<Author>/<Album>/…_CDnn_TRnn.mp3`) is the other setting value (`rip.layout = disc_folders | flat`). With `flat`, the existing merge (`relocate::plan`/`execute`, which understands `Title (CD 1)`, `Title CD2`, folder names `CD n`) is the way to put discs together later; David only has to write a title that `strip_disc` understands (`Title (CD 1)`).
- Because David wrote the discs of a batch, it knows they belong together, so with `disc_folders` there is nothing to merge. If the first disc was already imported as a book, the scanner's next import finds the new `CD 2` folder under the same book folder (**unverified**; to be tested in step 8, because the book id and the listening progress must stay).
- A disc that is already in the library (same book, same n) is refused with "CD n of this book is already in the library; replace it?" and replaced only after the new files are complete (rename of the old folder to the quarantine folder, existing setting).
- File and folder names are cleaned with the existing `upload::clean_name`; slashes and control characters in titles from MusicBrainz must not create folders.
- Genre text (today "Audiobook") is `rip.genre`; it must contain one of the words of `audiobook_genres` (`Store::audiobook_genres`), otherwise `relocate` would not treat the book as an audiobook. The settings page warns when it does not.

## 7. Browser interface (administration)

Same rules as spec-admin-ui.md: server-side pages (MiniJinja), htmx fragments, one form with one Cancel/Save pair, every text through the message catalogue (`server/src/admin/i18n.rs`, `messages.rs`), English first and German as second language, plain words. New menu entry under the media menu: **Read CD**.

The page **Read CD** (`/admin/rip`) has four blocks:

1. **Drive.** One line: drive name and one of "No drive found", "Drive found: no disc", "Disc found: audio CD, 14 tracks, 63 minutes", "Data disc, not an audio CD", "Copy-protected disc, not read" (section 9), "Drive not responding". A refresh every two seconds through htmx while the page is open (a polling fragment, no JavaScript beyond htmx).
2. **Confirm** (shown when a disc is recognised and no job exists for it): the form with author, title, disc n of N, genre, cover (picture, "choose another" through URL, as the cover form of the audiobook page already does), the track list with titles editable and a checkbox per track, size and free space. The foot has **Cancel** and **Save**; Save starts the job. The candidate list (several MusicBrainz releases) is a select field at the top that refills the form.
3. **Progress** (job running): per disc "track 3 of 14, 41 %", a bar, the state words (reading, checking, encoding, placing), elapsed time, and a **Stop** button. Errors appear here in words: "The drive stopped answering. Plug it in again; reading continues with track 4 when the same disc is back." / "Track 6 could not be read reliably after 3 tries. The disc may be scratched." / "Not enough space: this CD needs about 150 MB, 90 MB are free." Technical detail (SCSI sense values) goes into a fold-out "Details" and into the log, not into the headline.
4. **Queue and history.** The batch: discs done, the one in progress, "waiting for CD n+1". Past jobs with result and a link to the book.

All state changing calls require the admin session the other pages require; the page works without JavaScript apart from the live refresh (a link "Refresh" is the fallback).

### MCP tools (`server/src/mcp.rs`)

Read-only by default, in the style of the existing tools (`list_albums`, `get_tags`):

- `rip_status`: drive state, disc state, current job, queue (what block 1, 3 and 4 show).
- `rip_start`: parameters author, title, disc, discs_total, genre (optional), tracks (optional), `dry_run` (default true: returns what would be done and the space needed). With `dry_run=false` it starts the job, but only for a disc that is in the drive and whose metadata came from `rip_status` candidates or are given in full.
- `rip_cancel`: stops the running job; keeps or deletes staging files (`keep_files`).
- `rip_eject`: opens the tray when no job is using the drive.

Every call is logged in the change log like other tools (`change_log`).

## 8. Data and settings

### Tables (one migration, number to be taken when built; the last is `0023`)

- `rip_job`: id, batch_id, state, drive identity (text), disc_id (MusicBrainz), toc (text, the offsets), author, title, disc, discs_total, genre, mbid (nullable), cover (path or null), created, updated, error (text, user words), error_detail, book_id (nullable, set after import), bytes_needed.
- `rip_track`: job_id, number, title, include (bool), sectors, pcm_sha256 (nullable until verified), state (`pending`, `read`, `encoded`, `failed`), file (final name), attempts.
- `rip_lookup_cache`: disc_id, fetched_at, body (JSON of the answer), so a retry or the second page view does not ask MusicBrainz again.

Staging files are not in the database; their names derive from the job id.

### Settings

Order of precedence as everywhere (`server/src/system.rs`): command line, environment, `/etc/david.toml`, default; values the administrator edits in the browser live in the `setting` table, shown on the Settings page in a new tab **CD**. Machine-bound values go to `/etc/david.toml`, library-bound ones to the table.

| Key | Where | Meaning (example value) |
|---|---|---|
| `rip.enabled` | table | master switch (false until the owner turns it on) |
| `rip.staging` | toml | folder for work files (below the data folder) |
| `rip.library` | table | target audiobook folder (one of the import folders) |
| `rip.layout` | table | `disc_folders` or `flat` |
| `rip.name_pattern` | table | e.g. `{author}_{title}_CD{disc:02}_TR{track:02}` |
| `rip.track_number_scheme` | table | `plain` or `disc_times_100` |
| `rip.genre` | table | text of the genre tag (must match `audiobook_genres`) |
| `rip.encoder` | toml | program and argument list (`lame -b 64 -m j …`) |
| `rip.bitrate_kbps`, `rip.sample_rate_hz`, `rip.stereo_mode` | table | fill the argument template, so the owner changes numbers, not command lines |
| `rip.read_speed` | table | `unchanged`, `optimal`, or a number |
| `rip.verify`, `rip.max_reads` | table | `off`/`second_read`, 3 |
| `rip.drive_match` | toml | list of drive identities; empty means any |
| `rip.poll_seconds` | table | drive polling interval |
| `rip.eject_command` | toml | empty = ioctl |
| `rip.musicbrainz_url`, `rip.coverart_url` | toml | base addresses of the lookup services (defaults are the public ones, but they are values, so a mirror can be used) |
| `rip.user_agent_contact` | table | contact for the User-Agent; lookups stay off while it is empty |
| `rip.lookup_enabled` | table | lets the owner switch the internet lookups off and type metadata by hand |
| `rip.job_expiry_days` | table | when abandoned staging files are deleted |

The service takes the group for the drive from the existing `groups` key in `/etc/david.toml`, so no new key is needed for that.

## 9. Legal note (not legal advice)

Copying a CD you own for private use is allowed in Germany under § 53 UrhG (Vervielfältigungen zum privaten Gebrauch), as long as the copy is not made from a manifestly unlawful source and is not passed on or published. Circumventing an effective technical protection measure is not allowed under § 95a UrhG, and § 95b UrhG does not give a right to circumvent in this case. David therefore **detects and refuses** discs that look copy-protected, and does not offer any way around that. What David can see: the crate reads only plain CD-DA sectors and the TOC. Typical signs of copy-protected "audio" CDs are a TOC with a second session or unusual track layout, and read errors at fixed places; **a reliable detection is not available from the crate** (nothing in the source) and has not been designed here. The honest rule: David reads what a normal drive reads without special tricks; if a disc cannot be read like that, it reports "this disc cannot be read; it may be copy-protected or damaged" and does nothing else. A rule "refuse when the TOC has a data session before the audio session" is a proposal (**unverified**, to be tried with the discs the owner has). Spoken-word CDs of the family library are normally not protected. The rights of the other family members to hear the copy are those of the household; the owner decides whether copies may be kept after the disc is sold, which is a legal question and not decided here.

## 10. How it is checked

Without hardware:

- **Fake drive.** `FakeDrive` implements `DiscDrive` over the crate's `AudioSectorReader` with a hand-made `Toc` and generated PCM (a sine wave per track, different frequency per track, 3 to 10 seconds). Cases: normal disc; disc with a data session (CD-Extra, `TrackBounds` rule); no disc; a read error at a chosen sector that succeeds on the 2nd try; an error that never succeeds; the drive vanishing in track 3; a different disc after the break; two reads that disagree once.
- **Job engine tests** run on the fake: state sequence, no half results (a failure leaves the library folder untouched), resume after a break from track 4, cancel between chunks, restart recovery, space refusal (`diskspace` reserve very large), duplicate disc refusal.
- **Encoder.** A test with the real `lame` if installed (skipped otherwise): output is MP3, 24 kHz, 64 kbit/s, duration within one frame of the input (checked with `lofty`, which gives sample rate, bitrate and duration), tags and cover readable.
- **Disc ID.** The unit test uses the published example of the MusicBrainz page; further vectors come from real discs in spike 2 (the owner's "Sturz der Titanen", CD 1 of 12, can be the first). The calculation is checked against `cd-discid` or `discid` output if available on the machine (not checked here).
- **Metadata.** Lookups use a small HTTP stub (a local test server in the same style as the cover test in `fetch.rs`), recorded answers for: exact hit, fuzzy hit, no hit, 503, slow, 404 cover. A test proves **one request per second**, in a row, and the User-Agent header.
- **Naming and layout.** Table test: author/title with slashes, umlauts, very long names, empty fields; the output is scanned by the real `scan` into one book with N discs and the right track numbers.
- **Pages.** The usual page tests (spec-admin-ui.md): each state of the drive block renders; the form refuses empty author/title; Save and Cancel; messages exist in both languages.

Only with hardware (owner's USB drive, a few discs including a scratched one that may be thrown away):

- Whether the process can open and use the device as user `david` with the group alone (spike 1).
- Real read speed and the effect of `rip.read_speed` on reliability.
- The errno and SCSI sense values when the cable is pulled or the tray is opened, and whether a stuck command returns after its 30 s timeout.
- Whether the second read really differs from the first on a damaged disc (the drive cache question).
- Eject by ioctl on this drive.
- The sysfs identity of this drive when it appears as `sr0` and `sr1`.
- CD-Extra and protected discs.

## 11. Steps, each small and checkable

1. **Spike 1: the crate on the owner's drive.** A throw-away example in a scratch folder (no David change): list drives, read TOC, read one track, write a WAV. Run once as the owner, once as `david` with the group only. Check: WAV plays; the result for the rights question; the pulled-cable errors. Half a day.
2. **Spike 2: metadata.** Disc ID from the TOC and a lookup with `curl` and `ureq` (rate limit, User-Agent), cover download, field names of the answer. Check: "Sturz der Titanen" CD 1 is found and says disc 1 of 12. Half a day.
3. **Spike 3: encoder.** Read the owner's `abcde` configuration for the exact `lame` flags; encode the WAV from step 1 with `mp3lame-encoder` and with `lame` through a pipe; compare both with a file ripped by `abcde` (sample rate, bitrate, joint stereo, size, duration, listening). Check: the four checks at the end of section 3. Half a day.
4. **`DiscDrive` trait, `FakeDrive`, `CdDaReaderDrive` and job engine without UI** (module `rip`, one table migration). Check: the fake-drive tests of section 10. About 2 days.
5. **Worker subcommand** `david rip-worker` with JSON lines, kill and restart behaviour. Check: tests that kill the worker mid-track; manual cable test. 1 day.
6. **Encoder, tags, cover, placing and import trigger.** Check: encoder test, scan test with N discs; one real disc end to end from the command line. 1 to 2 days.
7. **Settings tab "CD" and the space check.** Check: page tests, refusal on too little space. 1 day.
8. **Page "Read CD"**: drive block, confirm form, progress, queue; messages in two languages. Check: page tests, then by hand with a real disc. 2 to 3 days.
9. **Several discs per book**: batch, n of N from MusicBrainz or by hand, the next disc pre-filled; test that a second disc added to an imported book keeps the book id and the progress. 1 to 2 days.
10. **MCP tools** `rip_status`, `rip_start`, `rip_cancel`, `rip_eject`. Check: tests like the existing MCP tests, plus a dry run. 1 day.
11. **Hard cases**: scratched disc, CD-Extra, protected disc, drive vanishing, restart in the middle. By hand with the owner. 1 day.
12. **Optional: `ProgramDrive`** (`cdparanoia` as child process) as a selectable reader for damaged discs. Half a day to one day, only if step 11 shows the crate's reading is not enough.

Total about two weeks of work, the first three steps decide whether the rest is worth doing.

## 12. Recommendation

**Yes, with conditions.** `cd-da-reader` is worth using for the reading itself because it removes the need for `cdparanoia` and shell glue in the server, it has a sensible streaming API with progress, a retry policy that does not hide errors, a useful test seam, a Linux-only code path of about 300 lines that can be audited, no dependencies besides `libc`, and an MIT licence. The conditions:

1. Spike 1 shows that user `david` with a group can read a disc (otherwise the design needs a helper with more rights, and the case for the crate weakens).
2. The owner accepts that the crate has **no paranoia mode**: David adds a second-read comparison, and `cdparanoia` stays available as a fallback reader (step 12) for scratched discs.
3. Encoding uses the `mp3lame-encoder` crate if spike 3 passes (else `lame` as a child process); Eject, Disc ID, metadata, tags and cover are David's own code in any case.

### Alternatives

| Alternative | Gains | Costs |
|---|---|---|
| **`cdparanoia` (and `lame`) as child processes** | proven error correction and drive cache modelling; `-e` gives progress for wrappers; no new crate | parsing text output; a 2008 C program that is not on every system; output is one file per track or a stream; no way to test without a drive other than with a fake script; the cdparanoia drive autosense differs from the kernel's names (`-d` can be given a device) |
| **`abcde` as a child process** | exactly today's workflow, including CDDB and tagging hooks | a large shell script with its own configuration, hard to give progress, state and clean errors in a web page; CDDB, not MusicBrainz, by default; hard to guarantee "no half results" |
| **`cd-da-reader` (this spec)** | one binary, typed errors, progress, tests with a fake disc, no shell | no C2, no paranoia, no eject, no CD-Text; a young crate with one maintainer; extra code for what the programs already do |
| **Hybrid (recommended path)** | the crate by default, `cdparanoia` as a selectable fallback behind the same trait | two readers to test; justified only if step 11 shows damaged discs matter |
| **Do not build it; keep ripping by hand** | nothing to build or maintain | the owner keeps a manual, error-prone chain for every disc of a 12-disc book |

## Open

Questions to the project owner:

1. **Rights on the machine.** May `david` join the optical-drive group? (Spike 1 shows whether it is enough.)
2. **Which layout?** `Author/Title/CD n/` (what David writes directly) or the old flat `Author/Album/…_CDnn_TRnn.mp3` with merge afterwards?
3. **Metadata policy.** Is it acceptable that David sends the TOC (not the audio) to MusicBrainz, with a contact address in the User-Agent? Which contact address?
4. **Track numbers.** Keep "disc * 100 + track" in the tag, or plain track numbers plus a disc number tag (David reads the disc from the file name anyway)?
5. **Bad discs.** Is a second-read check enough, or should `cdparanoia` be available as a fallback from the start?

Technical points not settled by sources:

- Whether SG_IO `READ CD` works for an unprivileged group member on this kernel (spike 1).
- The errno and sense values for a vanished drive, and whether the drive cache makes the second read useless (hardware tests).
- CD-Text: whether it is worth a separate reader (own SG_IO call or a program such as libcdio's `cd-info`; not checked). The owner says it is often missing.
- Whether `mp3lame-encoder` lets joint stereo and CBR 64 kbit/s be set, and whether it builds in the packages without a system `libmp3lame` (it bundles LAME 3.100 and builds it statically, per the README of `mp3lame-sys`; the points that remain are checks 2 and 3 of spike 3 in section 3).
- Whether the next import adds a new `CD n` folder to the existing book without losing id and progress.
- The `CDROMEJECT` ioctl value, and whether `DeviceAllow` is needed under a hardened unit.
- Copy-protection detection: nothing in the crate; the rule in section 9 is a proposal.
- `SHA-1` source for the Disc ID (a small crate, to be chosen).
- How well the crate is tested on real hardware by others: only the project's own statement, 5 stars and 2,220 downloads, no independent report was found.
