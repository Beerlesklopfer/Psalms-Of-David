# Spec: data model and persistence

Status: implemented in `server/` (migrations `0001`, `0002` and `0003`), covered by tests.
Source of truth for the schema: the SQL files in `server/migrations/`, applied in order.

## Decisions

| Topic | Decision | Why |
|---|---|---|
| Database | SQLite, WAL mode | Family scale, mostly reads, no database server to run. A backup is one file. |
| Profiles | At most **150** | Raised from 25 by the project owner. Enforced by a trigger in the database (migration `0002` replaced the one from `0001`) and reported as `Error::ProfileLimitReached`. `MAX_PROFILES` in `error.rs` must match the trigger; a test checks it. |
| ORM | **SeaORM** 2.x (sqlx underneath) | Chosen by the project owner. Entities for simple CRUD, plain SQL for joins and the progress upsert. |
| DDL | Plain SQL files in `server/migrations/` | The ORM can be replaced without touching the schema. Applied by a small runner in `db.rs`, version kept in `PRAGMA user_version`. |
| Time | Unix milliseconds, `INTEGER` | No time zones, no text formats. |
| Paths | Relative to the library root | The library can move (Linux to Windows) without changing the database. |
| Tables | `STRICT` | Type errors are rejected by SQLite itself. |

## Tables

- `profile`: one per person. `max_age` limits what the profile may see (`NULL` = no limit). `pin_hash` is optional.
- `author`, `book`, `track`: the shared library. `book.min_age` is the age rating (`NULL` = unrated). `track` is unique per `(book, disc, number)` and `(book, file)`. `mtime` and `size_bytes` let a rescan skip unchanged files.
- `progress`: the **latest** position per `(profile, book)`. No history, on purpose (privacy, size).
- `favorite`: `(profile, book)`.
- `device`: one row per device. The WireGuard public key identifies the device, not the person. A device can be used by several profiles.
- `enrollment`: one-time codes with which an admin enrolls a new device.

### Visibility rule

Migration `0001` and the first server code use the age-based rule: a book is visible to a profile if `profile.max_age IS NULL OR book.min_age IS NULL OR book.min_age <= profile.max_age`.

**Extended (decision of 2026-10-27, built with migration `0003`):** the age rule stays, but it now applies only to *released* books. Unreleased books are hidden from everyone except admins, releasing a book requires an age rating, and exceptions per user or role can add visibility. See `spec-discovery-profiles-roles.md`, section 3b. The queries `visible_books` and `continue_listening` use the new rule through one shared SQL fragment, and `Store::access` decides per book with the same fragment.

### Known trade-off

`progress.track_id` references `track` with `ON DELETE CASCADE`. If a rescan finds that a file has vanished, the listening position pointing at that track is deleted as well. Renaming files therefore loses positions. A later migration can store the position per book plus a track identity that survives renames.

## File format of a track

Migration `0051` gives `track` the format of its file: `format` (the file kind: mp3, ogg, opus, flac, m4a, wav ...), `codec` (mpeg, vorbis, opus, flac, aac, pcm ...), `mime_type` (what the stream is served with: audio/mpeg, audio/ogg, audio/flac, audio/mp4 ...), `bitrate_kbps`, `sample_rate`, `channels`. The values come from the file itself (the extension is only the fallback); one function in `src/audioformat.rs` maps them. Old rows hold `''` and `0` until the next import has looked at the file: a row with an empty `format` is filled in at once, whether or not the file changed (`david reindex` does the same). Reason: the album Caipiranha (Ogg Vorbis) did not play in the Linux app and cannot play on an iPhone, and nothing in the system knew it. The MCP tool `list_formats` and the check `unsupported_format_on_ios` of `find_problems` read these columns.

## Disc and track numbers

The ripped audiobooks are named `<Author>_<Title>_CDnn_TRnn.mp3`. The scanner reads disc and track from that pattern first. If it is missing, it falls back to the audio tags (`disc`, `track`; a track tag of `101` means disc 1, track 1). If neither exists, the ordinal in the folder is used.

## Not in this version

- Authentication and sessions (needs the device and WireGuard work first).
- Series, several authors per book, narrator, cover art.
- Music (albums, artists). The audiobook tables only half fit.
- Configurable profile limit. It lives in the migration, so changing it needs a new migration (as `0002` did). That is intended.
- A profile picker that copes with 150 entries (search or groups). A plain grid is enough up to roughly 25.
