<!--
SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
SPDX-License-Identifier: AGPL-3.0-or-later
-->
# Spec: opening an audio Bible at a passage

Status: **server side built and tested** (`server/src/bible.rs`, `GET /api/v1/bible/resolve`, migration `0005`). **Not built:** the import of anchors, speech input in the app, the player jump. Open items are marked **open**.

## Decision (project owner, 2026-10-27)

The interface grows so that passages of the Bible can be **opened by audio**: a spoken or typed reference ("Johannes 3,16") leads to the right place in a recorded Bible.

## How it works

1. **Speech to text happens in the app.** The app sends the text its recognizer heard. The server never receives audio. Recognizers write "Johannes 3 16", "Johannes Kapitel 3 Vers 16" or "john 3:16"; all of these are understood.
2. **The server parses the reference** (`bible::parse`): German (Luther and Einheitsübersetzung abbreviations) and English names and abbreviations of all 66 books, numbered books ("1. Korinther", "1 Cor", "Erster Korinther"), chapter alone, verse, verse range. One parser on the server means every app, present and future, understands the same spellings.
3. **Anchors say where a verse begins.** Table `bible_anchor`: for an item, a part (file), a position, and Bible book, chapter and verse. The start of a chapter is its verse 1. Several items can hold a Bible (translations, readers); each has its own anchors.
4. **Resolving** gives, for every item the user may see that has anchors for that chapter, the part and the position. A verse without an anchor leads to the nearest verse before it in the chapter, and the answer says so (`exact: false`, `matched_verse`). A chapter alone opens at its start.
5. The app then plays that part from that position, like any other item.

```
GET /api/v1/bible/resolve?q=Johannes 3,16[&item=<id>]
→ { "reference": {"book":43,"chapter":3,"verse":16,"verse_to":null},
    "matches": [ {"item_id","kind","title","part_id","position_ms","matched_verse","exact",
                  "access":{"state":"allowed"|"outside_schedule", "next_start"}} ] }
```

An unknown book or a missing chapter gives `422 invalid_reference`. No match is an empty list, not an error. More than one match means the app lets the user choose (translation or reader), or takes the one it played last.

The usual rules hold: an item the user may not see never appears, and `access` tells the app when the weekly planner forbids listening, so it can say why instead of failing.

## Where the anchors come from (open)

The table and `Store::set_bible_anchors` exist; nothing fills them yet. The proposal:

- **Chapter level** from the file layout: one file per chapter, named or tagged with book and chapter (`Johannes 03.mp3`). This needs no extra data and covers "open Psalm 23".
- **Verse level** from a sidecar file next to the item, `anchors.json`, a list of `{file, book, chapter, verse, position_ms}`, written by whoever times the recording (by hand, or by an alignment tool). The importer reads it when it finds an item.
- **Open:** a small admin page to time verses by listening, and the import rule that decides which items count as a Bible (a flag on the item set by an administrator, since the age release already needs an administrator).
- **Open:** licensing. Recorded Bibles are usually copyrighted; the family server only holds the family's own legal copies, as with all other media.

## Not in scope

- The **text** of the Bible (reading along, searching words). A text Bible would be a `document` item (see `spec-app-api.md`); the same parser can then map a reference to a page or anchor in it.
- Speech recognition on the server.
- Chapter ranges ("Johannes 3 bis 5"): only verse ranges inside a chapter.
