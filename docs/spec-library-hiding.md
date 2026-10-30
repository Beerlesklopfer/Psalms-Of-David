# Spec: the own library of a profile (hiding)

Status: **server (schema 0038) and the Linux app built; Android and iPhone follow with the next build of the app.** Decision of the project owner, 2026-10-29: in a family everyone has another taste. Every
member looks after their own library: items can be hidden and shown again (audiobooks or music **of an author/artist**, or a **single album**). A setting per user
says whether what was hidden is still listed ("show hidden").

## Rules

1. **Hiding is personal.** It changes only the lists of the profile that did it. It is **no access rule**: a book a profile may not see (age, release,
   plan) stays unseen whatever is hidden, and a hidden book can still be opened by its id (`GET /items/{id}`), played and found in playlists.
2. **What can be hidden:**
   - a **book** (an album of either kind), by id;
   - an **author or artist**, by name, for `audiobook`, for `music`, or for both (no `kind`). All books of that author of that kind are hidden, also those that
     arrive later.
3. **The setting `show_hidden`** (profile, default off): off, the lists leave the hidden items out; on, they are listed with `"hidden": true`, so that the
   app can show them dimmed and offer "show again".
4. **Where it applies:** `GET /items` (the library), `GET /new` ("new") and `GET /continue` ("continue listening"): a hidden item is not news and not offered for
   continuing, unless `show_hidden` is on. Search works on the list the app has, so it follows.
5. A profile that is deleted takes its choices with it; so does a book or an author that leaves the library.

## Interface (token of any device of the profile)

| | |
|---|---|
| `GET /api/v1/me/library` | `{ "show_hidden": bool, "books": [{id, title, author}], "authors": [{id, name, kind}] }` (`kind` empty = both) |
| `PUT /api/v1/me/library` | body `{ "show_hidden": bool }`; answers like the `GET` |
| `POST /api/v1/me/hidden` | body `{ "scope": "book", "id": 12, "hidden": true }` or `{ "scope": "author", "name": "Tolkien", "kind": "audiobook" \| "music" (optional), "hidden": true }`; `hidden: false` shows again; answers like the `GET`. `404 not_found` for an unknown book or author, `422 invalid_request` for a wrong body |
| `GET /api/v1/items` | each item has `hidden: bool`; hidden ones are absent unless `show_hidden` |
| `GET /api/v1/library/version` | has `hidden`: a short value that changes when this profile hides or shows something or changes the setting; the apps reload their lists when it differs |

## The apps

- A book: a menu at the top of the book page and a long press on a book in a list (a sheet) with "Hide this book" / "Show this book again".
- An author/artist: "Hide everything by …" for the audiobooks, the music or both.
- In the settings: "Show what I hid" (the setting), and a list "Hidden" with what was hidden (books and authors), each with "Show again".
- Hidden items, when shown, are dimmed and say "hidden".

## Owner (schema 0040, 2026-10-29)

A book has an **owner**: the family member it belongs to ("Music of Anni"). The owner is **not** the release: it says whom a book belongs to, not who may see
it (release, age, exceptions, the boxes of the matrix in the media list and `book_block` decide that).

- Chosen in the window of a book (a box "Owner", the nicknames of all profiles) and with the MCP tools `set_owner` (by nickname, `null` removes it) and
  `assign_owners` (fills the albums without an owner from their folders: the first folder of the place below the import folder is the **complete name**
  of a member, `First Last`, `Last First` or `Last, First`, or a nickname; other folders stay without owner; `dry_run` by default). Shown in the lists
  of the pages ("... of Anni"), in `list_albums` and in `GET /items` as `owner` (the nickname; **the nickname is what is shown, never the first name**).
- The app shows the music under **"Music of <Nickname>"** for every owner, the nicknames alphabetically, and what belongs to nobody under "Other music". The
  chosen genre narrows every group. Without any owner the music is shown by genre as before.
- A profile that is deleted leaves its books without owner. Owners are the ground for the photo sync (`docs/spec-photo-sync.md`).

## Offline (2026-10-30)

Hiding and showing a book or an author is kept on the device while the server is out of reach and sent later (`POST /me/hidden` sets the state,
so a repeat is harmless; the changes are replayed in order). The book leaves the lists at once. See `spec-app-api.md`, "Offline mode and
synchronisation".

