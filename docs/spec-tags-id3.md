# Spec: tags, ID3 and genres

Status: **built** (server 0.1.0, 2026-10-28). Decisions of the project owner are marked with their date. The code is in `server/src/tags.rs`, `scan.rs`, `placeholders.rs`, `genres.rs` and `relocate.rs`.

## 1. Decisions

1. **The tags of the files come first, the folders are only the filing** (2026-10-28). Author, title, narrator, genre and year of a book are read from the tags. The folder names are a fallback for what the tags do not say, and the place where a book lies on the disk.
2. **ID3v2 is binding for MP3 files** (2026-10-28). It is the one tag David reads first, writes, checks and, where it is missing, creates. ID3v1 is obsolete and is never the only tag David leaves behind.
3. **The genres the family looks after are a list of their own** (2026-10-28). The genre text in the files says what the tagger liked; the list says which names the apps show. Tag texts are mapped to the names of that list (section 5).

## 2. ID3v2 is binding

| Where | Rule |
|---|---|
| Reading | An MP3 is read from its ID3v2 tag. Only a file without ID3v2 is read from its ID3v1 tag, so that old files still show something. |
| Writing (`set_tags`, the tag dialog of the pages) | Always ID3v2 (version 2.4, what `lofty` writes). A file that has only ID3v1 gets an ID3v2 tag made from the ID3v1 one first, so that nothing it said is lost. |
| Report | `find_problems` marks an album with the problem `no_id3v2` if one of its MP3 files has only ID3v1 or no tag at all. |
| Repair | The MCP tool `upgrade_to_id3v2` gives such files their ID3v2 tag. Without `dry_run=false` it only lists what would change. The audio is not touched. |
| Ripping CDs | The files David (or the app) makes are written with ID3v2.4 (see `spec-cd-ripping.md`). |
| Other formats | FLAC, Ogg and Opus use Vorbis comments, MP4/M4A/M4B use `ilst`. They have their own tag and are not affected. |

Why ID3v2: ID3v1 holds 30 characters per field in Latin-1, has no album artist, no disc number, no cover and no composer. Everything this library needs (album artist, composer as the narrator, disc and track, cover, long titles, the date) is ID3v2.

## 3. Fields

The names are those of the shared view of `lofty` (`MCP field_names` lists all of them); the frame is what an MP3 stores.

| Meaning | Field | ID3v2 frame | Used for |
|---|---|---|---|
| Album | `AlbumTitle` | `TALB` | title of the book |
| Author or artist | `AlbumArtist`, else `TrackArtist` | `TPE2`, `TPE1` | author or artist |
| Narrator | `Composer` | `TCOM` | the narrator of an **audiobook** only (section 4) |
| Genre | `Genre` | `TCON` | the genre (section 5) |
| Year | `RecordingDate` | `TDRC` | the year; `Year` cannot be kept in an ID3v2.4 tag, so it is not written |
| Disc and track | `DiscNumber`, `TrackNumber` | `TPOS`, `TRCK` | order of the tracks |
| Cover | the front picture | `APIC` | the product picture; a `cover.jpg` in the folder is used if the file holds none |
| Age recommendation | `AGE_RECOMMENDATION` | `TXXX` (description `AGE_RECOMMENDATION`) | the publisher's recommendation, see `spec-discovery-profiles-roles.md` |

## 4. Author, title and narrator come from the tags

`scan.rs` reads the first file of a folder and takes:

- **Title**: `AlbumTitle`. A CD number in it (`Title (CD 2)`, `Title CD02`) is cut off, so that the discs of one work stay one book. Without a usable album tag the name of the folder.
- **Author**: `AlbumArtist`, then `TrackArtist`. Without a usable one the folder above the album, or `Unknown`.
- **Narrator**: `Composer`, **only for audiobooks**. On music the composer tag names the composer, who is nobody to listen to, so music has no narrator.
- **Placeholders** are no tags. A value that starts with one of the words of the setting `placeholder_tags` (any case) counts as missing, and the folder name speaks instead. The default words (migration 0025) are `unbekannt, unknown, kein künstler, kein titel, various artists, verschiedene`. The setting is on the settings page under *Library*, so it is changed without a rebuild. Example: `Unbekanntes Album (23.12.2020 20:59:13)` is not taken as a title.

The folder is still kept as `Author/Title` below the import folder, because that is where the import finds an album again. The MCP tool `rename_album` moves an album to the folder of an author and a title; it changes the place and nothing else. To change what an album is called, set its tags (`set_tags`: `AlbumArtist`, `AlbumTitle`) and file it with `rename_album`.

## 5. Genres

### The standard list

ID3 defines a numbered list of genres. `lofty` carries it as `lofty::id3::v1::GENRES`:

- **0 to 79**: the original ID3v1 list, for example `Blues` (0), `Classic rock` (1), `Country` (2), `Jazz` (8), `Pop` (13), `Rock` (17), `Soundtrack` (24), `Classical` (32), `Gospel` (38), `Soul` (42), `Show tunes` (69), `Musical` (77).
- **80 to 191**: the extensions that Winamp added and that all common taggers use, for example `Folk` (80), `Opera` (103), `Contemporary Christian` (140), `Christian rock` (141), `Baroque` (150), `World Music` (181), `Audiobook` (183), `Podcast` (186). In total 192 entries.
- In an ID3v2 `TCON` frame the genre may stand as a reference such as `(17)` or `17`. `lofty` translates it to the name when it reads (`Rock`), so the server only ever sees text.

Not in the list: `Children's`, `Kids`, `Religious`, `Hörbuch`, `Weihnachtsmusik`, `Country & Folk` and the other names that iTunes and similar programs write. Such text is free text and may be anything.

### The list of the family

Because the text in the files is free, the server keeps a list of its own (migration 0024):

- `genre` holds the names, their order and whether the apps show them (`visible`).
- `genre_alias` says which tag text belongs to which genre (not case sensitive). Many tag texts can lead to one genre (`Christian`, `Religious`, `Religiöse Musik` to one genre).
- `book.genre_tag` keeps the raw text of the file, `book.genre_id` the genre of the list, `book.genre_locked` is 1 if somebody chose the genre of this album by hand.
- The **effective genre** of an album is the genre of the alias of its tag text, unless the album was chosen by hand. A rescan never changes a genre that was chosen by hand. `set_genre` with `automatic=true` hands the album back to the tags.
- Tag texts without an alias are listed (`loose_tags`) on the page *Media > Genres* with the number of albums. There each can be given to a genre or made a genre of its own. The page filters the tag texts while typing.
- The apps show music in sections by genre, in the order of the list; albums without a genre are in *Other*. `GET /api/v1/genres` gives the visible names in order, `genre` is part of every item of `GET /api/v1/items`, and a change of the list changes the library version, so the apps reload.
- MCP tools: `list_genres` (genres, their tag texts, the loose tag texts), `edit_genres` (`add`, `rename`, `show`, `hide`, `move`, `delete`, `assign`, `unassign`) and `set_genre`.

The list is about the *view*. The genre text in the files is not rewritten by it. Whoever wants the files to agree can set the field with `set_tags`; use the names of the standard list where there is one, because other players and taggers then show the same.

### Suggested names

Where the standard list has the name, take it (`Rock`, `Pop`, `Classical`, `Jazz`, `Blues`, `Folk`, `Country`, `Musical`, `Gospel`, `Audiobook`); for the rest the family uses names of its own (for example children's music, Christmas, Christian music). The mapping is made on the genres page and is not built into the program.

## 6. Settings that belong here

| Setting | Meaning |
|---|---|
| `placeholder_tags` | words that mark a tag as a placeholder (section 4) |
| `audiobook_genres` | words in the genre tag that make an album an audiobook (the import of music that is really an audiobook, `move_to_audiobooks`) |
| `tree_audiobook`, `tree_music` | how the media pages group the albums (by folder, author, letter) |

## 7. Not built

- Writing the genres of the list back into the files in one step (only by `set_tags`).
- A check that the genre text of a file is a name of the standard list.
- ID3v2.3 as a selectable version for old players (the CD import has a setting for the tag version, see `spec-cd-import-api.md` when it exists).
