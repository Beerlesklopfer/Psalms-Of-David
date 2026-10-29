<!--
SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
SPDX-License-Identifier: AGPL-3.0-or-later
-->
# Playlists (server contract for the apps)

Status: implemented in the server (schema 0035, `server/src/playlists.rs`, `api_playlists.rs`, MCP tools in `mcp.rs`; tests `tests/playlists.rs`, `tests/mcp.rs`). This file is the contract for the app.

## Rules

- A playlist lives as rows in the database. The server never writes playlist files; the app builds the `.m3u8` itself when it downloads.
- A playlist belongs to exactly one profile and is the same on every device of that profile. A profile only ever sees its own lists; a list of somebody else answers `404 not_found` (no "forbidden", so nothing can be probed). Administrators and the MCP tools can look at all lists, always naming the profile (never an id).
- An entry is a real part (`track`) of an item. Duplicates of the same part in one list are allowed.
- Order: entries are numbered `position` 1..n without gaps. `position` in answers is always 1..n (also after a track was deleted from the library). Added parts keep the order in which they were sent; a whole item (`book_id`) is appended with all its parts in item order (disc, number).
- Visibility: an entry whose part the profile may not see now (child limit, no release, item removed from the profile's view) is NOT deleted. When the list is read it comes back with `available: false` and `part`/`book` set to `null` (nothing about the hidden item is revealed); it is not counted in `items`/`seconds`. The app skips unavailable entries when playing. If the part or its item is deleted from the library, the entry is deleted with it (`ON DELETE CASCADE`). Deleting a profile deletes its lists.
- Progress is reported exactly as before with the real `item_id` and `part_id` (`PUT /api/v1/progress`); nothing changes there.
- Playback uses the existing `GET /api/v1/parts/{id}/stream` (planner, `outside_schedule`, 404 unchanged).

## Data model

```
playlist(id, profile_id -> profile ON DELETE CASCADE, name, created_at, created_by, updated_at, updated_by)
    name: trimmed, 1..100 characters, unique per profile ignoring case (unique index)
playlist_item(id, playlist_id -> playlist ON DELETE CASCADE, position >= 1, track_id -> track ON DELETE CASCADE, created_at, created_by, updated_at, updated_by)
```

Audit fields and triggers follow schema 0028 (`created_by`/`updated_by` is `profile:<id>` for the app, `mcp:<token name>` for MCP). Limits (constants in `playlists.rs`): name 100 characters, 100 lists per profile, 5000 entries per list.

## Version / sync decision

`GET /api/v1/library/version` is one global fingerprint of the library (books, tracks, release exceptions). Playlists are per profile, so they are NOT mixed into it (that would make every device of every profile reload for one profile's edit). Instead:

- `GET /api/v1/library/version` gets an additional field `playlists`: a short fingerprint of the calling profile's lists (names, entries, order). The old `version` is unchanged.
- `GET /api/v1/playlists` carries the same fingerprint as `ETag` header (the body stays a plain array); `If-None-Match` with it is answered `304`.

The app polls `/library/version` as today. It reloads the library when `version` changed, and reloads the playlists when `playlists` changed (a change of the library can also change which entries are `available`, so reload the playlists after a library reload as well). Only devices of the profile that was edited see a new `playlists` value.

## API (all under `/api/v1`, `Authorization: Bearer <token>`)

Errors are `{"error": "<code>"}`.

### `GET /playlists`

```json
[ { "id": 3, "name": "Car trip", "items": 12, "seconds": 3420, "updated_at": 1793280000000 } ]
```

`items` counts available entries, `seconds` sums their duration. Header `ETag`.

### `POST /playlists` `{"name": "Car trip"}`

`201 {"id": 3, "name": "Car trip"}`. `409 name_taken`; `422 invalid_name` (empty after trimming, or longer than 100); `422 too_many_playlists`.

### `GET /playlists/{id}`

```json
{ "id": 3, "name": "Car trip",
  "items": [
    { "item_id": 41, "position": 1, "available": true,
      "part": { "id": 880, "disc": 1, "number": 2, "title": "Chapter 2", "duration_ms": 240000, "plays": 1, "last_played_at": 1793000000000 },
      "book": { "id": 17, "title": "Winter", "author": "Follett" } },
    { "item_id": 42, "position": 2, "available": false, "part": null, "book": null }
  ] }
```

`part` has the same fields as a part in `GET /items/{id}`. `item_id` is the id of the playlist entry (use it for delete/order); `part.id` is the id for streaming and progress. `404 not_found` for an unknown or foreign list.

### `PATCH /playlists/{id}` `{"name": "..."}`

`200 {"id", "name"}`; `409 name_taken`; `422 invalid_name`; `404`.

### `DELETE /playlists/{id}`

`204`; `404`.

### `POST /playlists/{id}/items`

Exactly one of `{"part_ids": [880, 881]}` or `{"book_id": 17}`; optional `"at": 3` (1-based position of the first new entry; default: end; 1..n+1). Answers `200` with the list as in `GET /playlists/{id}`.
`422 invalid_request` (not exactly one of the two, or empty `part_ids`), `422 unknown_part` (a part does not exist or the profile may not see it), `422 unknown_book`, `422 invalid_position`, `422 playlist_full`. Nothing is written when any id is refused.

### `DELETE /playlists/{id}/items/{item_id}`

`204`; `404 not_found` (list or entry). The remaining entries are renumbered.

### `PUT /playlists/{id}/order` `{"item_ids": [42, 41]}`

Must contain every `item_id` of the list exactly once (a permutation), otherwise `422 invalid_order`. Answers `200` with the list.

## MCP tools (`--mcp`; always by names, never ids as the main information)

- `list_playlists` `{profile?}`: name, profile name, number of available entries, duration, for one profile or all.
- `get_playlist` `{profile, name}`: entries with position, album title and author, track title, availability.
- `edit_playlists` `{action, profile, name, ...}` (needs a write token; changes run as actor `mcp:<token name>`): `create`, `rename` (`new_name`), `delete`, `clear`, `add` (`album` title, optional `author`, optional `track` title or `track_number`; without `track` the whole album is appended; optional `at`), `remove` (`position` 1-based or `track` title), `move` (`position` or `track`, and `to` 1-based). An ambiguous album or track title is refused with the candidates (title and author); nothing is guessed. `dry_run` is not offered (like `edit_stations`/`edit_genres`); every call is logged in the change log.
