// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Access for a remote LLM over MCP (Model Context Protocol, JSON-RPC over HTTP), to look at the
//! library and correct its ID tags. Off unless the server is started with `--mcp`.
//!
//! One endpoint, `POST /mcp`, with `Authorization: Bearer <token>`. The tokens are made in the
//! settings (tab "LLM access"); a token either may only read or may write tags too. Every changed
//! tag value is logged with the old and the new value (`tag_change`, and the server log).

use std::path::{Path, PathBuf};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::{Value, json};

use crate::store::{McpToken, Store};

/// The newest protocol version this server knows; a client that asks for another gets this one.
const PROTOCOL: &str = "2025-06-18";
/// Most albums one `find_problems` or `list_albums` answer holds.
const PAGE_MAX: usize = 200;

#[derive(Clone)]
pub struct McpState {
    pub store: Store,
    pub library: Option<PathBuf>,
}

pub fn router(state: McpState) -> Router {
    Router::new().route("/mcp", post(rpc)).layer(axum::middleware::from_fn(crate::actor::scope)).with_state(state)
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer realm=\"david\"")], "unauthorized").into_response()
}

async fn rpc(State(s): State<McpState>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let Some(secret) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")) else {
        return unauthorized();
    };
    let token = match s.store.mcp_token_for(secret.trim()).await {
        Ok(Some(t)) => t,
        Ok(None) => return unauthorized(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    crate::actor::set(crate::actor::mcp(&token.name));
    let Ok(request) = serde_json::from_slice::<Value>(&body) else {
        return reply(Value::Null, Err((-32700, "parse error".to_owned())));
    };
    // A batch is answered as a batch.
    if let Some(items) = request.as_array() {
        let mut answers = Vec::new();
        for item in items {
            if let Some(a) = handle(&s, &token, item).await {
                answers.push(a);
            }
        }
        return if answers.is_empty() { StatusCode::ACCEPTED.into_response() } else { axum::Json(Value::Array(answers)).into_response() };
    }
    match handle(&s, &token, &request).await {
        Some(answer) => axum::Json(answer).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn reply(id: Value, result: Result<Value, (i32, String)>) -> Response {
    axum::Json(answer(id, result)).into_response()
}

fn answer(id: Value, result: Result<Value, (i32, String)>) -> Value {
    match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    }
}

/// One JSON-RPC message; `None` for a notification, which gets no answer.
async fn handle(s: &McpState, token: &McpToken, request: &Value) -> Option<Value> {
    let id = request.get("id").cloned()?;
    let method = request.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "david", "version": crate::buildinfo::describe() },
            "instructions": prompt_of(s).await
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_list() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            tracing::info!(token = %token.name, tool = %name, "MCP call");
            Ok(match call(s, token, &name, &args).await {
                Ok(v) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] }),
                Err(message) => {
                    tracing::warn!(token = %token.name, tool = %name, %message, "MCP call failed");
                    json!({ "isError": true, "content": [{ "type": "text", "text": message }] })
                }
            })
        }
        _ => Err((-32601, format!("unknown method {method}"))),
    };
    Some(answer(id, result))
}

fn tool_list() -> Value {
    let id = json!({ "type": "integer", "description": "Album id from list_albums" });
    json!([
        {
            "name": "list_albums",
            "description": "Lists audiobooks and albums: id, kind (audiobook or music), title, author, narrator, tracks, recommended_age and whether it has a blurb (has_description). With with_description=true each entry carries its blurb text, to compare many at once.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string", "description": "Only albums whose title or author matches: words (case ignored), * and ? as wildcards, or a regular expression between slashes (/^der .* welt$/)" },
                "kind": { "type": "string", "enum": ["audiobook", "music"] },
                "with_description": { "type": "boolean", "description": "Put the blurb text (Klappentext) of each album into the answer" },
                "has_description": { "type": "boolean", "description": "true: only albums that have a blurb; false: only those that have none" },
                "format": { "type": "string", "description": "Only albums that have a track of this file format (mp3, ogg, opus, flac, m4a, wav, ...; see list_formats); a leading ! means: only albums without any track of it, e.g. !mp3" },
                "offset": { "type": "integer" }, "limit": { "type": "integer", "description": format!("At most {PAGE_MAX}") }
            } }
        },
        {
            "name": "get_album",
            "description": "One album with its tracks (id, disc, number, title, file, format, codec, mime, sample_rate, channels) and the folder picture, if any.",
            "inputSchema": { "type": "object", "properties": { "id": id }, "required": ["id"] }
        },
        {
            "name": "get_tags",
            "description": "All tag fields of the tracks of an album (or of one track). Field names are the same for every file format.",
            "inputSchema": { "type": "object", "properties": { "id": id, "track": { "type": "integer", "description": "Only this track id" } }, "required": ["id"] }
        },
        {
            "name": "field_names",
            "description": "The names of all tag fields that set_tags accepts.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "set_tags",
            "description": "Sets tag fields in every track of an album, or in one track. `fields` maps a field name to a string, a list of strings (several values) or null (remove the field). Use dry_run first to see what would change. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": {
                "id": id,
                "track": { "type": "integer", "description": "Only this track id; without it every track of the album" },
                "fields": { "type": "object", "description": "e.g. {\"AlbumArtist\": \"Ken Follett\", \"Genre\": [\"Audiobook\"], \"Comment\": null}" },
                "dry_run": { "type": "boolean" }
            }, "required": ["id", "fields"] }
        },
        {
            "name": "set_cover_from_folder",
            "description": "Embeds the picture file of the album folder (cover.jpg, Folder.jpg, cdcover.jpg, ...) into every track. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "id": id }, "required": ["id"] }
        },
        {
            "name": "set_cover_from_url",
            "description": "Fetches a picture from an https:// address (only public hosts, at most 8 MiB, JPEG or PNG), makes it the cover of the album folder (cover.jpg or cover.png, replacing other cover files) and embeds it in every track. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "id": id, "url": { "type": "string", "description": "https:// address of the picture" } }, "required": ["id", "url"] }
        },
        {
            "name": "list_formats",
            "description": "The file formats of the library: for every format (mp3, ogg, flac, m4a, ...) its codec, mime type, the number of tracks and albums, whether an iPhone can play it (ios_playable) and the sample rates. With format=ogg the albums that have such files. Tracks that an import has not looked at yet show as format \"\" (unknown): run an import (david reindex) once.",
            "inputSchema": { "type": "object", "properties": { "format": { "type": "string", "description": "List the albums that have a track of this format" } } }
        },
        {
            "name": "find_problems",
            "description": "Looks for albums with faulty tags: missing artist, album, year, genre (music), narrator (audiobook) or cover; tracks that disagree on album or artist; stray spaces; gaps in track numbers; album tag unlike the title; titles with a CD number, underscores or a number prefix; an author that looks like a file name; files that an iPhone cannot play (unsupported_format_on_ios: Ogg Vorbis, Opus, WMA, APE; a hint, not an error). Returns only albums with problems; continue with next_offset.",
            "inputSchema": { "type": "object", "properties": { "offset": { "type": "integer" }, "limit": { "type": "integer", "description": "Albums to look at, at most 200 (default 100)" } } }
        },
        {
            "name": "move_to_audiobooks",
            "description": "Finds albums in the music folders that are audiobooks (genre Hörbuch, Hörspiel, Lesung, Audiobook or Spoken) and moves them to <audiobook folder>/<author>/<title>, named by their tags. CDs of one work (Title (CD 1), Title CD2, ...) are put together into one book with a folder CD n per disc; the book of the first CD keeps its id and the progress of the others moves to it. Without dry_run=false it only reports what would move (the default). Moving needs a token that may write. Pass ids to look at certain albums only.",
            "inputSchema": { "type": "object", "properties": {
                "ids": { "type": "array", "items": { "type": "integer" }, "description": "Only these albums" },
                "dry_run": { "type": "boolean", "description": "true by default; false moves the folders" }
            } }
        },
        {
            "name": "find_duplicates",
            "description": "Finds books that look like the same work twice: the same title (without CD numbers, case or brackets), one title inside another, or the same recording (the same number of tracks, each of the same length within a second) under another title. Each group says whether the author, the running time, the title and the tracks agree (same_author, same_length, same_title, same_tracks) and so whether it is certain or possible. Same tracks can be wanted on purpose: then use ignore. Nothing is removed; the owner decides which copy stays.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "quarantine_duplicates",
            "description": "Puts the given copies of duplicate books into quarantine: the folder moves to the hidden folder .quarantine of its import folder (one per import folder, same place below it), and the book leaves the library. Nothing is deleted; to get a book back, move its folder back and import again. Only books from find_duplicates (not parts of one work) can go, and one copy of each work must stay. Without dry_run=false it only reports (the default). Needs a token that may write to move.",
            "inputSchema": { "type": "object", "properties": {
                "ids": { "type": "array", "items": { "type": "integer" }, "description": "The copies to put aside (not the ones that stay)" },
                "dry_run": { "type": "boolean", "description": "true by default; false moves the folders" }
            }, "required": ["ids"] }
        },
        {
            "name": "set_recommended_age",
            "description": "Sets the age recommendation of a book (the publisher's \"ab N Jahren\", 0 to 21; null removes it). It is information for the family; it does not release the book. Stored in the library, not in the files. The old value is kept in the change log. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "id": id, "age": { "type": ["integer", "null"], "minimum": 0, "maximum": 21 } }, "required": ["id", "age"] }
        },
        {
            "name": "merge_albums",
            "description": "Puts albums together as the CDs of one work. The CD number of every album is what its tag `DiscNumber` says (`TPOS`, `2/4`); `discs` overrides it with numbers you give (one for every album of `ids`), and an album without a number counts by its place in `ids`. The number is then written into the tag of every file, so it stays. `discs_total` (else `DiscTotal` of the tags) says how many CDs the work has, also written into the files: the list then shows that a CD is missing. The first album remains (its id, listening progress, owner); the others go into it; the folders become `<folder above the first album>/<Title>/CD n`. Title and author default to the first album's, the title without the number at its end. Without dry_run=false it only lists what would happen (the default). Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "ids": { "type": "array", "items": { "type": "integer" }, "description": "The albums, first CD first" }, "discs": { "type": "array", "items": { "type": "integer" }, "description": "The CD number of each album of ids" }, "discs_total": { "type": "integer", "description": "How many CDs the work has" }, "title": { "type": "string" }, "author": { "type": "string" }, "dry_run": { "type": "boolean", "description": "true by default; false moves the folders and writes the tags" } }, "required": ["ids"] }
        },
        {
            "name": "set_discs_total",
            "description": "Says how many CDs a work has (1 to 99), or null for not said. The number is written into the tag `DiscTotal` of every file of the album and kept in the library; the list shows a red mark when the library holds fewer CDs than that. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "id": id, "total": { "type": ["integer", "null"], "description": "How many CDs, or null" } }, "required": ["id", "total"] }
        },
        {
            "name": "list_labels",
            "description": "The labels (classic tagging: Gebet, Schule, Gemeinsame Urlaube ...) with the number of albums (`books`), of single titles (`tracks`, see set_track_labels), of podcasts (`podcasts`) and of radio stations (`stations`, see set_feed_labels) that carry each. With ids it lists the labels of those albums instead. A label hangs on the album and everybody who sees the album sees it; the owner of an album sets its labels, an administrator may override. Labels are not the ID3 tags of the files (see get_tags).",
            "inputSchema": { "type": "object", "properties": { "ids": { "type": "array", "items": { "type": "integer" }, "description": "Only the labels of these albums" } } }
        },
        {
            "name": "set_labels",
            "description": "Gives albums labels (classic tagging: Gebet, Schule, Gemeinsame Urlaube ...), as an administrator overriding the owner. `add` puts labels on every album of `ids`, `remove` takes them off, `set` replaces all labels of each album (an empty list takes them all off); give `set` or `add`/`remove`, not both. Names: 1 to 40 characters, no comma, semicolon or bar; case does not matter; a label that no album carries any more goes. Labels are kept in the library, the files are not touched. Without dry_run=false it only reports what would change (the default). Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "ids": { "type": "array", "items": { "type": "integer" } }, "add": { "type": "array", "items": { "type": "string" } }, "remove": { "type": "array", "items": { "type": "string" } }, "set": { "type": "array", "items": { "type": "string" } }, "dry_run": { "type": "boolean", "description": "true by default; false writes the labels" } }, "required": ["ids"] }
        },
        {
            "name": "set_track_labels",
            "description": "Gives single titles (tracks) of an album labels of their own (classic tagging, like set_labels, but for the title: a hymn of an album about prayer). The labels of the album stay and count for its titles as well. `tracks` are track ids of the album `id` (see get_album); without them all titles of the album. `add` puts labels on every title, `remove` takes them off, `set` replaces the own labels of each title (an empty list takes them all off); give `set` or `add`/`remove`, not both. Names as for set_labels; at most 10 own labels per title. Without dry_run=false it only reports what would change (the default). Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "id": id, "tracks": { "type": "array", "items": { "type": "integer" } }, "add": { "type": "array", "items": { "type": "string" } }, "remove": { "type": "array", "items": { "type": "string" } }, "set": { "type": "array", "items": { "type": "string" } }, "dry_run": { "type": "boolean", "description": "true by default; false writes the labels" } }, "required": ["id"] }
        },
        {
            "name": "set_feed_labels",
            "description": "Gives podcasts or radio stations labels (classic tagging, like set_labels, so that the labels cover all media of the apps). `kind` is `podcast` or `station`; `names` are the titles or feed addresses of podcasts (see list_podcasts), or the names or stream addresses of stations (see list_stations). `add` puts labels on every one, `remove` takes them off, `set` replaces all labels of each (an empty list takes them all off); give `set` or `add`/`remove`, not both. Names as for set_labels; at most 30 labels per podcast or station. Everybody who has the podcast or the station sees its labels. Without dry_run=false it only reports what would change (the default). Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "kind": { "type": "string", "enum": ["podcast", "station"] }, "names": { "type": "array", "items": { "type": "string" } }, "add": { "type": "array", "items": { "type": "string" } }, "remove": { "type": "array", "items": { "type": "string" } }, "set": { "type": "array", "items": { "type": "string" } }, "dry_run": { "type": "boolean", "description": "true by default; false writes the labels" } }, "required": ["kind", "names"] }
        },
        {
            "name": "set_owner",
            "description": "Says whom an album belongs to: a family member by nickname (\"Music of Anna\" in the apps); null removes the owner. The owner is not the release: who may see an album is set by release_albums, the age and the visibility boxes of the pages. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "id": id, "owner": { "type": ["string", "null"], "description": "The nickname of a profile (see check_access or the pages), or null" } }, "required": ["id", "owner"] }
        },
        {
            "name": "hide_albums",
            "description": "Hides albums from the own lists of a profile (like \"Hide\" in the apps), or shows them again. It is no access rule: the profile can show them again under \"Hidden\" in the apps, and nothing else changes. Name the albums by `ids`, or by `label` (every album that carries the label). `profile` is a nickname; `hide` is true by default, false shows them again. Without dry_run=false it only reports what would change (the default). Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": {
                "profile": { "type": "string", "description": "The nickname of the profile" },
                "ids": { "type": "array", "items": { "type": "integer" }, "description": "Album ids" },
                "label": { "type": "string", "description": "Every album with this label (case does not matter)" },
                "hide": { "type": "boolean", "description": "true by default; false shows the albums again" },
                "dry_run": { "type": "boolean", "description": "true by default; false writes" }
            }, "required": ["profile"] }
        },
        {
            "name": "assign_owners",
            "description": "Gives the albums that have no owner the owner their folder names: the first folder of the place below the import folder is the complete name of a family member (First Last, Last First or Last, First) or a nickname. Other folders (a collection, a genre) stay without owner. Without dry_run=false it only reports (the default). Pass kind (audiobook or music) or ids to look at some albums only. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": {
                "ids": { "type": "array", "items": { "type": "integer" }, "description": "Only these albums" },
                "kind": { "type": "string", "enum": ["audiobook", "music"] },
                "dry_run": { "type": "boolean", "description": "true by default; false sets the owners" }
            } }
        },
        {
            "name": "release_albums",
            "description": "Releases albums to the children by the age recommendation they carry: every album that is not released and has a recommended age (set_recommended_age) is released from that age on (the same as the Release button of the pages with that age). Albums without a recommended age are not touched and are listed (no_age), so that someone sets an age first. Albums that are released already stay as they are. Without dry_run=false it only reports what would be released (the default). Pass ids or kind (audiobook or music) to look at some albums only. Each release is written to the change log. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": {
                "ids": { "type": "array", "items": { "type": "integer" }, "description": "Only these albums" },
                "kind": { "type": "string", "enum": ["audiobook", "music"], "description": "Only albums of this kind" },
                "dry_run": { "type": "boolean", "description": "true by default; false releases" }
            } }
        },
        {
            "name": "list_genres",
            "description": "The genres the family looks after (the list the apps show music by): name, position, visible, how many albums, and the tag texts that belong to each. Also the loose tag texts that have no genre yet, with the number of albums, so that they can be given one with edit_genres.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "edit_genres",
            "description": "Maintains the list of genres. action: add (name), rename (genre, name), show or hide (genre; hidden genres are not offered to the apps), move (genre, by: -1 up or 1 down), delete (genre; its albums become without genre), assign (tag, genre: the tag text belongs to that genre from now on, and every album with the text takes it unless somebody chose its genre by hand), unassign (tag). genre is the name of a genre, not an id. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["add", "rename", "show", "hide", "move", "delete", "assign", "unassign"] },
                "genre": { "type": "string", "description": "Name of the genre" },
                "name": { "type": "string" }, "tag": { "type": "string" }, "by": { "type": "integer", "enum": [-1, 1] }
            }, "required": ["action"] }
        },
        {
            "name": "list_playlists",
            "description": "The playlists of the family: name, profile (by name), number of tracks the profile may hear, and duration. Without profile: the playlists of every profile. Each playlist belongs to one profile and is the same on all its devices.",
            "inputSchema": { "type": "object", "properties": { "profile": { "type": "string", "description": "Name of a profile" } } }
        },
        {
            "name": "get_playlist",
            "description": "The entries of one playlist in order: position, album title and author, track title, duration, and whether the profile may hear it now (an entry it may not hear stays in the list but is skipped when playing). The playlist is named, together with the name of its profile.",
            "inputSchema": { "type": "object", "properties": { "profile": { "type": "string" }, "name": { "type": "string", "description": "Name of the playlist" } }, "required": ["profile", "name"] }
        },
        {
            "name": "edit_playlists",
            "description": "Maintains playlists. action: create, rename (new_name), delete, clear (removes all entries), add (album: its title, optional author to tell albums of the same title apart; optional track: a track title, or track_number with optional disc; without track the whole album is appended in album order; optional at: 1-based position to insert at), remove (position, 1-based, or track: a track title in the playlist), move (position or track, and to: the new 1-based position). The playlist is named by name and the profile by its name, never by number. An album or track that several match is refused with the candidates, nothing is guessed. Needs a token that may write. See list_playlists and get_playlist.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["create", "rename", "delete", "clear", "add", "remove", "move"] },
                "profile": { "type": "string", "description": "Name of the profile that owns the playlist" },
                "name": { "type": "string", "description": "Name of the playlist" }, "new_name": { "type": "string" },
                "album": { "type": "string" }, "author": { "type": "string" },
                "track": { "type": "string" }, "track_number": { "type": "integer" }, "disc": { "type": "integer" },
                "at": { "type": "integer" }, "position": { "type": "integer" }, "to": { "type": "integer" }
            }, "required": ["action", "profile", "name"] }
        },
        {
            "name": "set_genre",
            "description": "Chooses the genre of one album by hand (genre: the name of a genre from list_genres; null: without genre). The tags of the files then no longer change it. automatic=true hands the album back to the tags. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "id": id, "genre": { "type": ["string", "null"] }, "automatic": { "type": "boolean" } }, "required": ["id"] }
        },
        {
            "name": "list_podcasts",
            "description": "The podcasts of the library: title, feed address, the profiles that follow each (a request of a child that still waits for a release is marked (waiting)), and the last error of reading the feed. profile: only the podcasts that profile follows.",
            "inputSchema": { "type": "object", "properties": { "profile": { "type": "string" } } }
        },
        {
            "name": "edit_podcasts",
            "description": "Maintains the podcasts. action: follow (url: the address of the feed or of a page that points to it; for: profile names, or [\"*\"] for every administrator and member; the feed is read, saved and followed at once, released), unfollow (podcast: the title or the feed address; for: profile names, or [\"*\"] for everybody who follows it), refresh (podcast: read the feed again now). Profiles are named, never numbered. Needs a token that may write. See list_podcasts.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["follow", "unfollow", "refresh"] },
                "url": { "type": "string" }, "podcast": { "type": "string" },
                "for": { "type": "array", "items": { "type": "string" } }
            }, "required": ["action"] }
        },
        {
            "name": "list_stations",
            "description": "The radio stations of the library: name, stream address, home page, logo, and the profiles that have each among their favourites (a request of a child that still waits for a release is marked (waiting)).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "edit_blacklist",
            "description": "The blacklist: words (one word or phrase each; `*` and `?` as in the search, case and accents ignored, a word without them matches anywhere in the text, so `katze` also hits `Katzenjammer`). A medium whose title, author, narrator, blurb, track titles or labels (podcasts: title, description, labels; stations: name, labels) contain one of them gets the system label \"Blacklist\", which only administrators see; nothing is blocked. action: list (default; the words and the marked media with the word that hit, and whether an administrator approved them: set_labels with remove [\"Blacklist\"] takes the mark off), add or remove (words; dry_run is true by default and then only shows what would be marked, so false positives can be judged first). No word comes with the program. Reading needs a token; add and remove need a token that may write.",
            "inputSchema": { "type": "object", "properties": { "action": { "type": "string", "enum": ["list", "add", "remove"] }, "words": { "type": "array", "items": { "type": "string" } }, "dry_run": { "type": "boolean", "description": "true by default for add and remove; false saves the words and marks the media" } } }
        },
        {
            "name": "edit_stations",
            "description": "Maintains the radio stations. action: add (name, stream_url, optional homepage, logo_url, and for: profile names that get it as a favourite at once, or [\"*\"] for every profile that is not a child or guest), update (station, then any of name, stream_url, homepage, logo_url; an empty homepage or logo removes it), assign or unassign (station, for: profile names), delete (station: gone from every list). station is the name or the stream address of a station, profiles are named, never numbered. Needs a token that may write. See list_stations.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["add", "update", "assign", "unassign", "delete"] },
                "station": { "type": "string" }, "name": { "type": "string" }, "stream_url": { "type": "string" },
                "homepage": { "type": "string" }, "logo_url": { "type": "string" },
                "for": { "type": "array", "items": { "type": "string" } }
            }, "required": ["action"] }
        },
        {
            "name": "move_albums",
            "description": "Moves a whole branch of the filing: every album whose place is `from` or lies below it goes to the same place below `to`, for example from=JÖRG/MP3-I to=JÖRG puts JÖRG/MP3-I/Reinhard Mey/Ikarus at JÖRG/Reinhard Mey/Ikarus. Places are written as list_albums and get_album show the folder: below the import folder, with /; an empty `to` is the import folder itself. The ids stay, so listening progress, releases and owners stay; the folders left empty are removed. Without dry_run=false it only lists the moves (the default). An album whose new place exists, or that the server may not write, stays and says why. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "from": { "type": "string", "description": "The place to move, e.g. JÖRG/MP3-I" }, "to": { "type": "string", "description": "Where it goes, e.g. JÖRG; empty: the import folder" }, "dry_run": { "type": "boolean", "description": "true by default; false moves the folders" } }, "required": ["from"] }
        },
        {
            "name": "rename_album",
            "description": "Moves an album to the folder that belongs to an author and a title: the folders are only the filing (Author/Title below the import folder), the ID tags decide what the album is called, so this changes the place and nothing else. Set the tags with set_tags (AlbumArtist, AlbumTitle) and then file the album with this. The id stays, so listening progress and releases stay. Without dry_run=false it only shows from and to (the default). It refuses if the new place exists, lies in the old one, or the server may not write there. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "id": id, "author": { "type": "string" }, "title": { "type": "string" }, "dry_run": { "type": "boolean", "description": "true by default; false moves the folder" } }, "required": ["id"] }
        },
        {
            "name": "upgrade_to_id3v2",
            "description": "ID3v2 is the binding tag of MP3 files. This gives every MP3 that has only the obsolete ID3v1 tag (or none) an ID3v2 tag, made from the ID3v1 one, so nothing it said is lost; the audio is not touched. Without dry_run=false it only lists what would change (the default). ids limits it to some albums; without ids it looks at all, and find_problems shows them as no_id3v2. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "ids": { "type": "array", "items": { "type": "integer" } }, "dry_run": { "type": "boolean", "description": "true by default; false writes the tags" } } }
        },
        {
            "name": "remove_empty_folders",
            "description": "Finds folders in the import folders that hold no file at all (only empty folders inside), what moved albums leave behind, and removes them. Hidden folders such as .quarantine and the import folders themselves are never touched, and a folder with any file in it is not empty. Without dry_run=false it only lists (the default). Needs a token that may write to remove.",
            "inputSchema": { "type": "object", "properties": { "dry_run": { "type": "boolean", "description": "true by default; false removes the folders" } } }
        },
        {
            "name": "list_quarantine",
            "description": "What lies in the quarantine folders (.quarantine in each import folder): every album with where it came from, how many audio files and how big, and whether the server put it there (and who and when) or somebody did it by hand on the disk.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "set_description",
            "description": "Sets the blurb (Klappentext, the text of the back cover) of a book, shown in the app and the pages. Plain text, at most 4000 characters; an empty text removes it. The old text is kept in the change log. Needs a token that may write. Read the current one with get_album.",
            "inputSchema": { "type": "object", "properties": { "id": id, "text": { "type": "string" } }, "required": ["id", "text"] }
        },
        {
            "name": "ignore",
            "description": "Hides something about a book from the reports, or says that a duplicate is wanted on purpose. scope: album (this book only), title (every book with this title, whatever the CD number) or work (the book with all its CDs and copies by the same author). what: duplicate (default; no more duplicate warning), a problem code of find_problems (e.g. missing_year), or * for everything. Nothing in the library changes. Needs a token that may write. Undo with unignore; see list_ignores.",
            "inputSchema": { "type": "object", "properties": {
                "id": id,
                "scope": { "type": "string", "enum": ["album", "title", "work"] },
                "what": { "type": "string", "description": "duplicate (default), a problem code, or *" },
                "note": { "type": "string", "description": "Why, for whoever reads the list later" }
            }, "required": ["id", "scope"] }
        },
        {
            "name": "list_ignores",
            "description": "The rules made with ignore: id, scope, what, the key they match, who made them and the note.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "unignore",
            "description": "Removes a rule (its id is in list_ignores). What it hid shows up in the reports again. Needs a token that may write.",
            "inputSchema": { "type": "object", "properties": { "rule": { "type": "integer" } }, "required": ["rule"] }
        },
        {
            "name": "version",
            "description": "Which David is running: version, git revision (with -dirty if it was built from uncommitted changes), build time (UTC), when the server started, and the SHA-256 of the binary to compare with a package. Check it before saying that something is or is not installed.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "check_access",
            "description": "Checks, with the rights of the server's user, which folders of the library cannot be read or written (ACLs). Returns the folders with the problem and the command that fixes each root. Looks at the disk again, which can take a while.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "list",
            "description": "Lists all tools of this server with a one-line summary and whether they change anything. Use help_for for the details of one tool.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "help_for",
            "description": "The general help text of this server, or with a tool name the full description, the parameters and an example call of that tool.",
            "inputSchema": { "type": "object", "properties": { "tool": { "type": "string", "description": "Name of the tool, see list; without it the general help text" } } }
        },
        {
            "name": "change_log",
            "description": "The newest tag changes made through MCP: who, when, file, field, old and new value.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "integer", "description": "Only this album" }, "limit": { "type": "integer" } } }
        }
    ])
}

/// What a client is told about the server: the text the project owner set, else the built-in one.
pub const DEFAULT_PROMPT: &str = "David is a family audiobook and music server. Use find_problems to see which albums have faulty or missing tags, get_tags to read them, set_tags (dry_run first) to correct them. Every change is logged. Call list for the tools and help_for for the details of one.";

async fn prompt_of(s: &McpState) -> String {
    s.store.mcp_prompt().await.ok().flatten().unwrap_or_else(|| DEFAULT_PROMPT.to_owned())
}

/// The tools that change files; a token that may only read cannot use them.
const WRITING_TOOLS: [&str; 28] = ["hide_albums", "edit_blacklist", "merge_albums", "set_discs_total", "set_labels", "set_track_labels", "set_feed_labels", "move_albums", "set_owner", "assign_owners", "release_albums", "edit_podcasts", "edit_stations", "edit_playlists", "upgrade_to_id3v2", "rename_album", "edit_genres", "set_genre", "remove_empty_folders", "set_recommended_age", "set_tags", "set_cover_from_folder", "set_cover_from_url", "move_to_audiobooks", "quarantine_duplicates", "set_description", "ignore", "unignore"];

/// A sample call of a tool, for `help_for`.
fn example_of(tool: &str) -> Value {
    match tool {
        "list_albums" => json!({ "kind": "audiobook", "with_description": true, "has_description": true }),
        "get_album" | "get_tags" | "set_cover_from_folder" => json!({ "id": 12 }),
        "set_tags" => json!({ "id": 12, "dry_run": true, "fields": { "AlbumTitle": "Back to the Street" } }),
        "set_cover_from_url" => json!({ "id": 12, "url": "https://example.org/cover.jpg" }),
        "find_problems" => json!({ "offset": 0, "limit": 100 }),
        "list_formats" => json!({ "format": "ogg" }),
        "change_log" => json!({ "id": 12, "limit": 20 }),
        "help_for" => json!({ "tool": "set_tags" }),
        "check_access" => json!({}),
        "version" => json!({}),
        "list_quarantine" => json!({}),
        "remove_empty_folders" => json!({ "dry_run": true }),
        "set_recommended_age" => json!({ "id": 12, "age": 8 }),
        "release_albums" => json!({ "kind": "audiobook", "dry_run": true }),
        "set_owner" => json!({ "id": 12, "owner": "Anna" }),
        "hide_albums" => json!({ "profile": "Anna", "label": "Schule", "dry_run": true }),
        "list_labels" => json!({}),
        "merge_albums" => json!({ "ids": [143, 144, 145], "discs_total": 4, "dry_run": true }),
        "set_discs_total" => json!({ "id": 143, "total": 4 }),
        "set_labels" => json!({ "ids": [12, 13], "add": ["Schule"], "dry_run": true }),
        "set_track_labels" => json!({ "id": 12, "tracks": [301, 302], "add": ["Gebet"], "dry_run": true }),
        "set_feed_labels" => json!({ "kind": "station", "names": ["DWG Lyra"], "add": ["Gebet"], "dry_run": true }),
        "assign_owners" => json!({ "kind": "music", "dry_run": true }),
        "ignore" => json!({ "id": 152, "scope": "work", "what": "duplicate", "note": "both copies are wanted: the school set and the family set" }),
        "unignore" => json!({ "rule": 3 }),
        "list_ignores" => json!({}),
        "list_genres" => json!({}),
        "upgrade_to_id3v2" => json!({ "dry_run": true }),
        "rename_album" => json!({ "id": 43, "author": "Edith Piaf", "title": "Edith Piaf", "dry_run": true }),
        "move_albums" => json!({ "from": "JÖRG/MP3-I", "to": "JÖRG", "dry_run": true }),
        "edit_genres" => json!({ "action": "assign", "tag": "Prog Rock", "genre": "Rock" }),
        "list_podcasts" => json!({ "profile": "Anna" }),
        "edit_podcasts" => json!({ "action": "follow", "url": "https://example.org/podcast/feed.xml", "for": ["Anna"] }),
        "list_stations" => json!({}),
        "edit_blacklist" => json!({ "action": "add", "words": ["sex * katze"], "dry_run": true }),
        "edit_stations" => json!({ "action": "add", "name": "DWG Lyra", "stream_url": "https://server34599.streamplus.de/stream.mp3", "for": ["*"] }),
        "list_playlists" => json!({ "profile": "Anna" }),
        "get_playlist" => json!({ "profile": "Anna", "name": "Car trip" }),
        "edit_playlists" => json!({ "action": "add", "profile": "Anna", "name": "Car trip", "album": "Winter", "author": "Follett", "track": "Chapter 2" }),
        "set_genre" => json!({ "id": 12, "genre": "Jazz" }),
        "set_description" => json!({ "id": 12, "text": "Ein Junge findet ein Geheimnis ..." }),
        "quarantine_duplicates" => json!({ "ids": [152], "dry_run": true }),
        "find_duplicates" => json!({}),
        "move_to_audiobooks" => json!({ "dry_run": true }),
        _ => json!({}),
    }
}

/// A profile by its name (any case); the id and the name as it is written.
fn profile_named(people: &[crate::store::UserRow], name: &str) -> Result<(i64, String), String> {
    people.iter().find(|u| u.name.eq_ignore_ascii_case(name.trim())).map(|u| (u.id, u.name.clone())).ok_or_else(|| format!("no profile called {name}"))
}

/// The profiles a tool is for: named, and `*` is every adult (administrators and members). Sorted, without doubles.
fn profiles_named(people: &[crate::store::UserRow], wanted: &[String]) -> Result<Vec<(i64, String)>, String> {
    let mut profiles: Vec<(i64, String)> = Vec::new();
    for w in wanted {
        if w == "*" {
            profiles.extend(people.iter().filter(|u| u.role == "admin" || u.role == "member").map(|u| (u.id, u.name.clone())));
        } else {
            profiles.push(profile_named(people, w)?);
        }
    }
    profiles.sort();
    profiles.dedup();
    Ok(profiles)
}

fn plist(e: crate::playlists::PlaylistError) -> String {
    e.to_string()
}

/// Seconds as `h:mm:ss`.
fn clock(seconds: i64) -> String {
    format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
}

/// The playlist of a profile by its name (any case).
async fn playlist_named(s: &McpState, profile: i64, name: &str) -> Result<i64, String> {
    let lists = s.store.playlist_summaries(Some(profile)).await.map_err(plist)?;
    lists.iter().find(|p| p.name.eq_ignore_ascii_case(name.trim())).map(|p| p.id).ok_or_else(|| format!("no playlist called {name} for this profile"))
}

/// How an album is told to a person: "Title (Author)".
fn album_label(title: &str, author: &str) -> String {
    format!("{title} ({author})")
}

/// An album by its title (exact, any case), told apart by the author if needed. Several matches are refused with the
/// candidates; nothing is guessed.
async fn album_named(s: &McpState, title: &str, author: Option<&str>) -> Result<(i64, String), String> {
    let rows = s.store.admin_book_rows().await.map_err(|e| format!("database error: {e}"))?;
    let (title, author) = (title.trim().to_lowercase(), author.map(|a| a.trim().to_lowercase()));
    let by_author = |b: &&crate::store::AdminBookRow| author.as_deref().is_none_or(|a| b.author.to_lowercase() == a);
    let exact: Vec<_> = rows.iter().filter(|b| b.title.to_lowercase() == title).filter(by_author).collect();
    let list = |found: &[&crate::store::AdminBookRow]| found.iter().take(10).map(|b| album_label(&b.title, &b.author)).collect::<Vec<_>>().join("; ");
    match exact.as_slice() {
        [one] => Ok((one.id, album_label(&one.title, &one.author))),
        [] => {
            let similar: Vec<_> = rows.iter().filter(|b| b.title.to_lowercase().contains(&title)).filter(by_author).collect();
            if similar.is_empty() { Err(format!("no album called {title}")) } else { Err(format!("no album called {title}; similar: {}", list(&similar))) }
        }
        many => Err(format!("several albums are called {title}; add the author: {}", list(many))),
    }
}

/// A track of an album by its title (exact, any case) or its number (with the disc if the album has several).
async fn track_of(s: &McpState, book: i64, title: Option<&str>, number: Option<i64>, disc: Option<i64>) -> Result<i64, String> {
    let tracks = s.store.tracks_of(book).await.map_err(|e| format!("database error: {e}"))?;
    let found: Vec<_> = tracks
        .iter()
        .filter(|t| title.is_none_or(|n| t.title.to_lowercase() == n.trim().to_lowercase()))
        .filter(|t| number.is_none_or(|n| t.number == n))
        .filter(|t| disc.is_none_or(|d| t.disc == d))
        .collect();
    match found.as_slice() {
        [one] => Ok(one.id),
        [] => Err("no such track in this album".into()),
        many => Err(format!("several tracks match; give the disc and number: {}", many.iter().take(10).map(|t| format!("disc {} track {} {}", t.disc, t.number, t.title)).collect::<Vec<_>>().join("; "))),
    }
}

/// The entry of a playlist that `position` (1-based) or a track title names, as an index. Several matches are refused.
fn entry_at(view: &crate::playlists::PlaylistView, position: Option<i64>, track: Option<&str>) -> Result<usize, String> {
    if let Some(p) = position {
        return usize::try_from(p - 1).ok().filter(|i| *i < view.entries.len()).ok_or_else(|| format!("position must be between 1 and {}", view.entries.len()));
    }
    let title = track.ok_or("position or track is required")?.trim().to_lowercase();
    let hits: Vec<usize> = view.entries.iter().enumerate().filter(|(_, e)| e.title.to_lowercase() == title).map(|(i, _)| i).collect();
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("no entry called {title} in this playlist")),
        many => Err(format!(
            "several entries match; use the position: {}",
            many.iter().take(10).map(|i| format!("{} = {} ({})", i + 1, view.entries[*i].title, album_label(&view.entries[*i].book_title, &view.entries[*i].author))).collect::<Vec<_>>().join("; ")
        )),
    }
}

/// A playlist as the tools tell it: names, never numbers.
async fn playlist_json(s: &McpState, id: i64, profile: i64) -> Result<Value, String> {
    let view = s.store.playlist_view(id, profile).await.map_err(plist)?.ok_or("no such playlist")?;
    let heard: i64 = view.entries.iter().filter(|e| e.available).map(|e| e.duration_ms).sum::<i64>() / 1000;
    let entries: Vec<Value> = view
        .entries
        .iter()
        .map(|e| json!({ "position": e.position, "album": e.book_title, "author": e.author, "disc": e.disc, "track_number": e.number, "track": e.title, "duration": clock(e.duration_ms / 1000), "available": e.available }))
        .collect();
    Ok(json!({ "profile": view.profile_name, "name": view.name, "tracks": view.entries.iter().filter(|e| e.available).count(), "duration": clock(heard), "entries": entries }))
}

async fn call(s: &McpState, token: &McpToken, name: &str, args: &Value) -> Result<Value, String> {
    let int = |key: &str| args.get(key).and_then(Value::as_i64);
    let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_owned);
    let db = |e: crate::Error| format!("database error: {e}");
    match name {
        "list_albums" => {
            let matcher = crate::search::compile(&text("query").unwrap_or_default()).map_err(|e| format!("query: {e}"))?;
            let kind = text("kind");
            let with_description = args.get("with_description").and_then(Value::as_bool).unwrap_or(false);
            let offset = int("offset").unwrap_or(0).max(0) as usize;
            let limit = (int("limit").unwrap_or(100).max(1) as usize).min(PAGE_MAX);
            let rows = s.store.admin_book_rows().await.map_err(db)?;
            // The formats of the albums (book id -> formats of its tracks), only if the filter asks for it.
            let wanted_format = text("format").map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty());
            let formats = if wanted_format.is_some() { s.store.formats_by_book().await.map_err(db)? } else { Default::default() };
            let matching: Vec<_> = rows
                .into_iter()
                .filter(|b| match &wanted_format {
                    None => true,
                    Some(f) => match f.strip_prefix('!') {
                        Some(not) => !formats.get(&b.id).is_some_and(|set| set.contains(not.trim())),
                        None => formats.get(&b.id).is_some_and(|set| set.contains(f.as_str())),
                    },
                })
                .filter(|b| kind.as_deref().is_none_or(|k| b.kind == k))
                .filter(|b| args.get("has_description").and_then(Value::as_bool).is_none_or(|want| want == b.description.as_deref().is_some_and(|d| !d.trim().is_empty())))
                .filter(|b| matcher.is_match(&b.title) || matcher.is_match(&b.author))
                .collect();
            let total = matching.len();
            let albums: Vec<Value> = matching
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|b| {
                    let mut entry = json!({
                        "id": b.id, "kind": b.kind, "title": b.title, "author": b.author, "narrator": b.narrator, "tracks": b.tracks,
                        "recommended_age": b.recommended_age, "has_description": b.description.as_deref().is_some_and(|d| !d.trim().is_empty()),
                        "owner": b.owner, "released": b.released
                    });
                    if with_description {
                        entry["description"] = json!(b.description);
                    }
                    entry
                })
                .collect();
            Ok(json!({ "total": total, "albums": albums }))
        }
        "list_formats" => {
            let only = text("format").map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty());
            let rows = s.store.format_summary().await.map_err(db)?;
            let formats: Vec<Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "format": r.format, "codec": r.codec, "mime": crate::audioformat::mime_or_guess(&r.mime_type, &format!("x.{}", r.format)),
                        "tracks": r.tracks, "albums": r.albums, "sample_rates": r.sample_rates,
                        "ios_playable": crate::audioformat::ios_playable(&r.format),
                    })
                })
                .collect();
            match only {
                None => Ok(json!({ "formats": formats })),
                Some(f) => {
                    let books = s.store.format_albums(&f).await.map_err(db)?;
                    let albums: Vec<Value> = books.into_iter().map(|(id, title, author, n)| json!({ "id": id, "title": title, "author": author, "tracks_in_format": n })).collect();
                    Ok(json!({ "format": f, "ios_playable": crate::audioformat::ios_playable(&f), "total": albums.len(), "albums": albums }))
                }
            }
        }
        "get_album" => {
            let id = int("id").ok_or("id is required")?;
            let album = Album::load(s, id).await?;
            let own = s.store.track_labels().await.map_err(db)?;
            let tracks: Vec<Value> = album.tracks.iter().map(|(t, _)| json!({ "track": t.id, "disc": t.disc, "number": t.number, "title": t.title, "file": t.file, "labels": own.get(&t.id).cloned().unwrap_or_default(), "format": t.format, "codec": t.codec, "mime": crate::audioformat::mime_or_guess(&t.mime_type, &t.file), "sample_rate": t.sample_rate, "channels": t.channels })).collect();
            let picture = album.dir.as_deref().and_then(folder_picture);
            Ok(json!({
                "id": id, "kind": album.row.kind, "title": album.row.title, "author": album.row.author,
                "narrator": album.row.narrator, "owner": album.row.owner, "description": album.row.description, "folder": album.folder, "folder_picture": picture, "tracks": tracks
            }))
        }
        "get_tags" => {
            let id = int("id").ok_or("id is required")?;
            let only = int("track");
            let album = Album::load(s, id).await?;
            let files: Vec<(i64, String, PathBuf)> =
                album.tracks.iter().filter(|(t, _)| only.is_none_or(|o| o == t.id)).map(|(t, p)| (t.id, t.file.clone(), p.clone())).collect();
            if files.is_empty() {
                return Err("no such track in this album".into());
            }
            let read = tokio::task::spawn_blocking(move || {
                files
                    .into_iter()
                    .map(|(track, file, path)| match crate::tags::read_fields(&path) {
                        Ok(fields) => json!({ "track": track, "file": file, "fields": fields_json(fields) }),
                        Err(e) => json!({ "track": track, "file": file, "error": e.to_string() }),
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({ "tracks": read }))
        }
        "field_names" => Ok(json!(crate::tags::field_names())),
        "set_tags" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let only = int("track");
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
            let changes = parse_changes(args.get("fields"))?;
            let album = Album::load(s, id).await?;
            let files: Vec<(String, PathBuf)> =
                album.tracks.iter().filter(|(t, _)| only.is_none_or(|o| o == t.id)).map(|(t, p)| (t.file.clone(), p.clone())).collect();
            if files.is_empty() {
                return Err("no such track in this album".into());
            }
            let wanted = changes.clone();
            let outcome = tokio::task::spawn_blocking(move || -> Result<Vec<(String, String, String, String)>, String> {
                let mut log = Vec::new();
                for (file, path) in &files {
                    let before = crate::tags::read_fields(path).map_err(|e| e.to_string())?;
                    let mut effective = Vec::new();
                    for (field, new) in &wanted {
                        let old = before.iter().find(|(n, _)| n.eq_ignore_ascii_case(field)).map(|(_, v)| v.clone()).unwrap_or_default();
                        if old != *new {
                            log.push((file.clone(), field.clone(), old.join(" | "), new.join(" | ")));
                            effective.push((field.clone(), new.clone()));
                        }
                    }
                    if !dry && !effective.is_empty() {
                        crate::tags::write_fields(path, &effective).map_err(|e| format!("{file}: {e}"))?;
                    }
                }
                Ok(log)
            })
            .await
            .map_err(|e| e.to_string())??;
            if !dry {
                s.store.rescan_soon();
                for (file, field, old, new) in &outcome {
                    tracing::info!(token = %token.name, album = id, %file, %field, %old, %new, "tag changed");
                    s.store.log_tag_change(&token.name, id, file, field, old, new).await.map_err(db)?;
                }
            }
            // Grouped, so that an album of 200 files answers in a few lines: what changed from what to what,
            // in how many files, and one file as an example.
            let mut groups: Vec<(String, String, String, usize, String)> = Vec::new();
            for (file, field, old, new) in &outcome {
                match groups.iter_mut().find(|g| g.0 == *field && g.1 == *old && g.2 == *new) {
                    Some(g) => g.3 += 1,
                    None => groups.push((field.clone(), old.clone(), new.clone(), 1, file.clone())),
                }
            }
            let files = outcome.iter().map(|c| &c.0).collect::<std::collections::BTreeSet<_>>().len();
            let summary: Vec<Value> = groups.iter().take(50).map(|(field, old, new, n, example)| json!({ "field": field, "old": old, "new": new, "files": n, "example_file": example })).collect();
            Ok(json!({ "dry_run": dry, "changes": outcome.len(), "files_changed": files, "summary": summary, "more_groups": groups.len().saturating_sub(50) }))
        }
        "set_cover_from_folder" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let album = Album::load(s, id).await?;
            let dir = album.dir.clone().ok_or("the folder of the album is not known")?;
            let (mime, bytes) = tokio::task::spawn_blocking(move || crate::api::cover_in(&dir)).await.map_err(|e| e.to_string())?.ok_or("the folder has no picture file")?;
            let files: Vec<PathBuf> = album.tracks.iter().map(|(_, p)| p.clone()).collect();
            let need = (bytes.len() as u64).saturating_mul(files.len() as u64 + 1);
            if !crate::diskspace::fits(&files[0], need, crate::diskspace::reserve(&s.store).await) {
                return Err("not enough free space on the disk".into());
            }
            let count = files.len();
            tokio::task::spawn_blocking(move || -> Result<(), String> {
                for path in &files {
                    crate::tags::write_cover(path, &bytes, mime).map_err(|e| e.to_string())?;
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())??;
            tracing::info!(token = %token.name, album = id, files = count, "cover embedded from the folder picture");
            s.store.log_tag_change(&token.name, id, "(all files)", "Picture", "", "folder picture").await.map_err(db)?;
            s.store.rescan_soon();
            Ok(json!({ "embedded_in": count }))
        }
        "set_cover_from_url" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let url = text("url").ok_or("url is required")?;
            let album = Album::load(s, id).await?;
            let dir = album.dir.clone().ok_or("the folder of the album is not known")?;
            let fetched = url.clone();
            let (mime, bytes) = tokio::task::spawn_blocking(move || crate::fetch::download_image(&fetched, crate::tags::MAX_COVER_BYTES)).await.map_err(|e| e.to_string())??;
            if mime == "image/webp" {
                return Err("WebP is not accepted as a cover; use a JPEG or PNG address".into());
            }
            let files: Vec<PathBuf> = album.tracks.iter().map(|(_, p)| p.clone()).collect();
            let need = (bytes.len() as u64).saturating_mul(files.len() as u64 + 1);
            if !crate::diskspace::fits(&files[0], need, crate::diskspace::reserve(&s.store).await) {
                return Err("not enough free space on the disk".into());
            }
            let count = files.len();
            let size = bytes.len();
            let (name, embedded, first_error) = tokio::task::spawn_blocking(move || -> Result<(&'static str, usize, Option<String>), String> {
                // The picture file of the folder wins over the embedded ones, so there must be only one.
                for old in crate::api::COVER_FILES {
                    let _ = std::fs::remove_file(dir.join(old));
                }
                let name = if mime == "image/png" { "cover.png" } else { "cover.jpg" };
                std::fs::write(dir.join(name), &bytes).map_err(|e| format!("{name}: {e}"))?;
                // The folder picture is what the apps show; a file that cannot take the embedded copy
                // (read-only for the service, an odd tag) does not undo it.
                let (mut embedded, mut first_error) = (0, None);
                for path in &files {
                    match crate::tags::write_cover(path, &bytes, mime) {
                        Ok(()) => embedded += 1,
                        Err(e) => {
                            first_error.get_or_insert_with(|| e.to_string());
                        }
                    }
                }
                Ok((name, embedded, first_error))
            })
            .await
            .map_err(|e| e.to_string())??;
            tracing::info!(token = %token.name, album = id, %url, bytes = size, files = count, embedded, "cover fetched from an address");
            s.store.log_tag_change(&token.name, id, name, "Picture", "", &url).await.map_err(db)?;
            s.store.rescan_soon();
            Ok(json!({ "folder_file": name, "bytes": size, "type": mime, "embedded_in": embedded, "of_files": count, "first_embed_error": first_error }))
        }
        "find_problems" => {
            let offset = int("offset").unwrap_or(0).max(0) as usize;
            let limit = (int("limit").unwrap_or(100).max(1) as usize).min(PAGE_MAX);
            let rows = s.store.admin_book_rows().await.map_err(db)?;
            let rules = s.store.ignore_rules().await.map_err(db)?;
            let total = rows.len();
            let mut found = Vec::new();
            for row in rows.into_iter().skip(offset).take(limit) {
                let (id, title, author) = (row.id, row.title.clone(), row.author.clone());
                let Ok(album) = Album::load(s, id).await else {
                    if !crate::ignore::ignored(&rules, &row, "files_missing") {
                        found.push(json!({ "id": id, "title": title, "author": author, "problems": ["files_missing"] }));
                    }
                    continue;
                };
                let mut problems = tokio::task::spawn_blocking(move || problems_of(&album)).await.map_err(|e| e.to_string())?;
                problems.retain(|p| !crate::ignore::ignored(&rules, &row, p));
                if !problems.is_empty() {
                    found.push(json!({ "id": id, "title": title, "author": author, "problems": problems }));
                }
            }
            let next = (offset + limit < total).then_some(offset + limit);
            Ok(json!({ "looked_at": limit.min(total.saturating_sub(offset)), "total": total, "next_offset": next, "albums_with_problems": found }))
        }
        "merge_albums" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let ids: Vec<i64> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).ok_or("ids is required")?;
            let given: Option<Vec<i64>> = args.get("discs").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect());
            // What the tags say: the number of each album's first file, and how many CDs the set has.
            let mut from_tags: Vec<Option<i64>> = Vec::new();
            let mut total_tag: Option<i64> = None;
            for id in &ids {
                let album = Album::load(s, *id).await?;
                let first = album.tracks.first().map(|(_, p)| p.clone());
                let fields = match first {
                    Some(p) => tokio::task::spawn_blocking(move || crate::tags::read_fields(&p).unwrap_or_default()).await.map_err(|e| e.to_string())?,
                    None => Vec::new(),
                };
                let number = |name: &str, part: usize| fields.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).and_then(|(_, v)| v.first()).and_then(|v| v.split('/').nth(part)).and_then(|n| n.trim().parse::<i64>().ok());
                from_tags.push(number("DiscNumber", 0));
                total_tag = total_tag.max(number("DiscTotal", 0).or_else(|| number("DiscNumber", 1)));
            }
            // The numbers: the ones given, else the ones of the tags when every album has one, each once, else the order of ids.
            let tag_numbers: Option<Vec<i64>> = from_tags.iter().copied().collect::<Option<Vec<i64>>>().filter(|n| { let mut u = n.clone(); u.sort_unstable(); u.dedup(); u.len() == n.len() });
            let discs = given.clone().or(tag_numbers);
            let total = args.get("discs_total").and_then(Value::as_i64).or(total_tag).filter(|t| (1..=99).contains(t));
            let plans = crate::relocate::plan_merge(&s.store, s.library.as_deref(), &ids, discs.as_deref(), text("author").as_deref(), text("title").as_deref()).await?;
            let blocked = plans.iter().any(|p| p.problem.is_some());
            let mut merged = false;
            if !dry && !blocked {
                // The numbers go into the files first (they stay there), then the folders move and the library follows.
                for p in &plans {
                    let album = Album::load(s, p.id).await?;
                    let files: Vec<PathBuf> = album.tracks.iter().map(|(_, f)| f.clone()).collect();
                    let disc = p.disc.unwrap_or(1);
                    // Title and author go in too: the next import reads them from the files, and the work is called by its own title, not by the one of a CD.
                    let mut changes: Vec<(String, Vec<String>)> = vec![("DiscNumber".to_owned(), vec![disc.to_string()]), ("AlbumTitle".to_owned(), vec![p.title.clone()]), ("AlbumArtist".to_owned(), vec![p.author.clone()])];
                    if let Some(t) = total {
                        changes.push(("DiscTotal".to_owned(), vec![t.to_string()]));
                    }
                    tokio::task::spawn_blocking(move || -> Result<(), String> {
                        for f in &files {
                            crate::tags::write_fields(f, &changes).map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    })
                    .await
                    .map_err(|e| e.to_string())??;
                }
                let members: Vec<&crate::relocate::Plan> = plans.iter().collect();
                crate::relocate::execute(&s.store, &members).await?;
                if let Some(t) = total {
                    s.store.set_book_discs_total(ids[0], Some(t)).await.map_err(db)?;
                }
                s.store.log_tag_change(&token.name, ids[0], "", "Merged CDs", &ids.iter().map(i64::to_string).collect::<Vec<_>>().join(", "), &plans[0].new_folder).await.map_err(db)?;
                tracing::info!(token = %token.name, album = ids[0], albums = ?ids, "CDs put together");
                s.store.rescan_soon();
                merged = true;
            }
            let albums: Vec<Value> = plans.iter().map(|p| json!({ "id": p.id, "disc": p.disc, "from": p.from, "to": p.to, "problem": p.problem })).collect();
            Ok(json!({ "dry_run": dry, "merged": merged, "keep": ids[0], "author": plans[0].author, "title": plans[0].title, "folder": plans[0].new_folder, "discs_total": total, "from_tags": from_tags, "albums": albums }))
        }
        "set_discs_total" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let total = match args.get("total") {
                Some(Value::Null) => None,
                Some(v) => Some(v.as_i64().filter(|t| (1..=99).contains(t)).ok_or("total is a whole number from 1 to 99, or null")?),
                None => return Err("total is required (a number, or null)".into()),
            };
            let album = Album::load(s, id).await?;
            let files: Vec<PathBuf> = album.tracks.iter().map(|(_, f)| f.clone()).collect();
            if let Some(t) = total {
                let changes = vec![("DiscTotal".to_owned(), vec![t.to_string()])];
                tokio::task::spawn_blocking(move || -> Result<(), String> {
                    for f in &files {
                        crate::tags::write_fields(f, &changes).map_err(|e| e.to_string())?;
                    }
                    Ok(())
                })
                .await
                .map_err(|e| e.to_string())??;
            }
            if !s.store.set_book_discs_total(id, total).await.map_err(db)? {
                return Err("no such album".into());
            }
            Ok(json!({ "id": id, "discs_total": total }))
        }
        "list_labels" => {
            let ids: Option<Vec<i64>> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect());
            match ids {
                Some(ids) => {
                    let all = s.store.book_labels_with_mark().await.map_err(db)?;
                    let albums: Vec<Value> = ids.iter().map(|id| json!({ "id": id, "labels": all.get(id).cloned().unwrap_or_default() })).collect();
                    Ok(json!({ "albums": albums }))
                }
                None => {
                    let labels = s.store.labels_with_mark().await.map_err(db)?;
                    Ok(json!({ "labels": labels }))
                }
            }
        }
        "set_labels" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let ids: Vec<i64> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).ok_or("ids is required")?;
            let names = |key: &str| -> Result<Option<Vec<String>>, String> {
                match args.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_owned).ok_or_else(|| format!("{key} holds names, as text"))).collect::<Result<Vec<_>, _>>().map(Some),
                    _ => Err(format!("{key} is a list of names")),
                }
            };
            let (add, remove, set) = (names("add")?, names("remove")?, names("set")?);
            // The system mark "Blacklist" is no ordinary label: it is not set by hand (the words decide, see edit_blacklist); taking it off approves the medium.
            if add.iter().chain(set.iter()).flatten().any(|n| crate::blacklist::is_mark(n)) {
                return Err("\"Blacklist\" is the system mark of the blacklist words (see edit_blacklist); it cannot be set by hand, only taken off with `remove`".into());
            }
            let (remove, approve_mark) = match remove {
                Some(r) => {
                    let (rest, had) = crate::blacklist::split_mark(&r);
                    (Some(rest), had)
                }
                None => (None, false),
            };
            if set.is_some() && (add.is_some() || remove.is_some()) {
                return Err("give set, or add and remove, not both".into());
            }
            if set.is_none() && add.is_none() && remove.is_none() {
                return Err("nothing to do: give add, remove or set".into());
            }
            for n in add.iter().chain(remove.iter()).chain(set.iter()).flatten() {
                if crate::labels::clean_name(n).is_none() {
                    return Err(format!("{n:?} is not a usable label (1 to 40 characters, no comma, semicolon or bar)"));
                }
            }
            let rows = s.store.admin_book_rows().await.map_err(db)?;
            let current = s.store.book_labels().await.map_err(db)?;
            let (mut albums, mut changed) = (Vec::new(), 0usize);
            for id in &ids {
                let Some(row) = rows.iter().find(|r| r.id == *id) else {
                    albums.push(json!({ "id": id, "problem": "no such album" }));
                    continue;
                };
                let before = current.get(id).cloned().unwrap_or_default();
                // What the album would have: `set` replaces, else add and remove (case ignored).
                let after: Vec<String> = match &set {
                    Some(set) => set.clone(),
                    None => {
                        let gone: Vec<String> = remove.iter().flatten().map(|r| r.trim().to_lowercase()).collect();
                        before.iter().filter(|b| !gone.contains(&b.to_lowercase())).cloned().chain(add.iter().flatten().cloned()).collect()
                    }
                };
                let result = if dry {
                    let mut seen: Vec<String> = Vec::new();
                    after.iter().filter_map(|n| crate::labels::clean_name(n)).filter(|n| { let low = n.to_lowercase(); if seen.contains(&low) { false } else { seen.push(low); true } }).collect::<Vec<_>>()
                } else {
                    s.store.set_book_labels(*id, &after).await.map_err(db)?.unwrap_or_default()
                };
                let differs = {
                    let (mut a, mut b): (Vec<String>, Vec<String>) = (before.iter().map(|x| x.to_lowercase()).collect(), result.iter().map(|x| x.to_lowercase()).collect());
                    a.sort();
                    b.sort();
                    a != b
                };
                if differs {
                    changed += 1;
                    if !dry {
                        tracing::info!(token = %token.name, album = id, labels = ?result, "labels of an album changed");
                        s.store.log_tag_change(&token.name, *id, "", "Labels", &before.join(", "), &result.join(", ")).await.map_err(db)?;
                    }
                }
                let mut approved = false;
                if approve_mark && !dry {
                    approved = s.store.blacklist_approve(crate::blacklist::Kind::Book, *id).await.map_err(db)?;
                }
                albums.push(json!({ "id": id, "title": row.title, "author": row.author, "before": before, "after": result, "changed": differs, "mark_approved": approved }));
            }
            Ok(json!({ "dry_run": dry, "changed": changed, "albums": albums }))
        }
        "set_track_labels" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let names = |key: &str| -> Result<Option<Vec<String>>, String> {
                match args.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_owned).ok_or_else(|| format!("{key} holds names, as text"))).collect::<Result<Vec<_>, _>>().map(Some),
                    _ => Err(format!("{key} is a list of names")),
                }
            };
            let (add, remove, set) = (names("add")?, names("remove")?, names("set")?);
            // The system mark "Blacklist" is no ordinary label: it is not set by hand (the words decide, see edit_blacklist); taking it off approves the medium.
            if add.iter().chain(set.iter()).flatten().any(|n| crate::blacklist::is_mark(n)) {
                return Err("\"Blacklist\" is the system mark of the blacklist words (see edit_blacklist); it cannot be set by hand, only taken off with `remove`".into());
            }
            let (remove, _approve_mark) = match remove {
                Some(r) => {
                    let (rest, had) = crate::blacklist::split_mark(&r);
                    (Some(rest), had)
                }
                None => (None, false),
            };
            if set.is_some() && (add.is_some() || remove.is_some()) {
                return Err("give set, or add and remove, not both".into());
            }
            if set.is_none() && add.is_none() && remove.is_none() {
                return Err("nothing to do: give add, remove or set".into());
            }
            for n in add.iter().chain(remove.iter()).chain(set.iter()).flatten() {
                if crate::labels::clean_name(n).is_none() {
                    return Err(format!("{n:?} is not a usable label (1 to 40 characters, no comma, semicolon or bar)"));
                }
            }
            // The labels live in the library; the files of the album are not needed.
            s.store.admin_book_row(id).await.map_err(db)?.ok_or("no such album")?;
            let all_tracks = s.store.tracks_of(id).await.map_err(db)?;
            let wanted: Option<Vec<i64>> = args.get("tracks").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect());
            let current = s.store.track_labels().await.map_err(db)?;
            let (mut titles, mut changed) = (Vec::new(), 0usize);
            for track in wanted.clone().unwrap_or_else(|| all_tracks.iter().map(|t| t.id).collect()) {
                let Some(t) = all_tracks.iter().find(|t| t.id == track) else {
                    titles.push(json!({ "track": track, "problem": "no such track in this album" }));
                    continue;
                };
                let before = current.get(&track).cloned().unwrap_or_default();
                let after: Vec<String> = match &set {
                    Some(set) => set.clone(),
                    None => {
                        let gone: Vec<String> = remove.iter().flatten().map(|r| r.trim().to_lowercase()).collect();
                        before.iter().filter(|b| !gone.contains(&b.to_lowercase())).cloned().chain(add.iter().flatten().cloned()).collect()
                    }
                };
                let result = if dry {
                    let mut seen: Vec<String> = Vec::new();
                    after.iter().filter_map(|n| crate::labels::clean_name(n)).filter(|n| { let low = n.to_lowercase(); if seen.contains(&low) { false } else { seen.push(low); true } }).collect::<Vec<_>>()
                } else {
                    s.store.set_track_labels(track, &after).await.map_err(|e| match e { crate::error::Error::InvalidName => "too many labels on one title (at most 10) or an unusable name".to_owned(), e => db(e) })?.unwrap_or_default()
                };
                let differs = {
                    let (mut a, mut b): (Vec<String>, Vec<String>) = (before.iter().map(|x| x.to_lowercase()).collect(), result.iter().map(|x| x.to_lowercase()).collect());
                    a.sort();
                    b.sort();
                    a != b
                };
                if differs {
                    changed += 1;
                    if !dry {
                        tracing::info!(token = %token.name, album = id, track, labels = ?result, "labels of a title changed");
                        s.store.log_tag_change(&token.name, id, &t.file, "Labels", &before.join(", "), &result.join(", ")).await.map_err(db)?;
                    }
                }
                titles.push(json!({ "track": track, "title": t.title, "before": before, "after": result, "changed": differs }));
            }
            Ok(json!({ "dry_run": dry, "changed": changed, "tracks": titles }))
        }
        "set_feed_labels" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let kind = text("kind").ok_or("kind is required: podcast or station")?;
            if kind != "podcast" && kind != "station" {
                return Err("kind is podcast or station".into());
            }
            let wanted: Vec<String> = args.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).ok_or("names is required")?;
            let names = |key: &str| -> Result<Option<Vec<String>>, String> {
                match args.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_owned).ok_or_else(|| format!("{key} holds names, as text"))).collect::<Result<Vec<_>, _>>().map(Some),
                    _ => Err(format!("{key} is a list of names")),
                }
            };
            let (add, remove, set) = (names("add")?, names("remove")?, names("set")?);
            // The system mark "Blacklist" is no ordinary label: it is not set by hand (the words decide, see edit_blacklist); taking it off approves the medium.
            if add.iter().chain(set.iter()).flatten().any(|n| crate::blacklist::is_mark(n)) {
                return Err("\"Blacklist\" is the system mark of the blacklist words (see edit_blacklist); it cannot be set by hand, only taken off with `remove`".into());
            }
            let (remove, approve_mark) = match remove {
                Some(r) => {
                    let (rest, had) = crate::blacklist::split_mark(&r);
                    (Some(rest), had)
                }
                None => (None, false),
            };
            if set.is_some() && (add.is_some() || remove.is_some()) {
                return Err("give set, or add and remove, not both".into());
            }
            if set.is_none() && add.is_none() && remove.is_none() {
                return Err("nothing to do: give add, remove or set".into());
            }
            for n in add.iter().chain(remove.iter()).chain(set.iter()).flatten() {
                if crate::labels::clean_name(n).is_none() {
                    return Err(format!("{n:?} is not a usable label (1 to 40 characters, no comma, semicolon or bar)"));
                }
            }
            // The items by title or address: (id, shown name, address).
            let items: Vec<(i64, String, String)> = if kind == "podcast" {
                s.store.feeds_with_followers().await.map_err(db)?.into_iter().map(|(f, _)| (f.id, f.title, f.url)).collect()
            } else {
                s.store.stations_with_owners().await.map_err(db)?.into_iter().map(|(st, _)| (st.id, st.name, st.stream_url)).collect()
            };
            let current = if kind == "podcast" { s.store.podcast_labels().await.map_err(db)? } else { s.store.station_labels().await.map_err(db)? };
            let (mut rows, mut changed) = (Vec::new(), 0usize);
            for key in &wanted {
                let hits: Vec<&(i64, String, String)> = items.iter().filter(|(_, name, address)| address == key.trim() || name.eq_ignore_ascii_case(key.trim())).collect();
                let [(id, name, _)] = hits.as_slice() else {
                    rows.push(json!({ "name": key, "problem": if hits.is_empty() { format!("no {kind} called {key}") } else { format!("several are called {key}; use the address") } }));
                    continue;
                };
                let before = current.get(id).cloned().unwrap_or_default();
                let after: Vec<String> = match &set {
                    Some(set) => set.clone(),
                    None => {
                        let gone: Vec<String> = remove.iter().flatten().map(|r| r.trim().to_lowercase()).collect();
                        before.iter().filter(|b| !gone.contains(&b.to_lowercase())).cloned().chain(add.iter().flatten().cloned()).collect()
                    }
                };
                let result = if dry {
                    let mut seen: Vec<String> = Vec::new();
                    after.iter().filter_map(|n| crate::labels::clean_name(n)).filter(|n| { let low = n.to_lowercase(); if seen.contains(&low) { false } else { seen.push(low); true } }).collect::<Vec<_>>()
                } else {
                    let saved = if kind == "podcast" { s.store.set_podcast_labels(*id, &after).await } else { s.store.set_station_labels(*id, &after).await };
                    saved.map_err(|e| match e { crate::error::Error::InvalidName => "too many labels (at most 30) or an unusable name".to_owned(), e => db(e) })?.unwrap_or_default()
                };
                let differs = {
                    let (mut a, mut b): (Vec<String>, Vec<String>) = (before.iter().map(|x| x.to_lowercase()).collect(), result.iter().map(|x| x.to_lowercase()).collect());
                    a.sort();
                    b.sort();
                    a != b
                };
                if differs {
                    changed += 1;
                    if !dry {
                        tracing::info!(token = %token.name, %kind, item = %name, labels = ?result, "labels of a podcast or station changed");
                        s.store.log_tag_change(&token.name, 0, "", "Labels", &format!("{name}: {}", before.join(", ")), &result.join(", ")).await.map_err(db)?;
                    }
                }
                let mut approved = false;
                if approve_mark && !dry {
                    approved = s.store.blacklist_approve(if kind == "podcast" { crate::blacklist::Kind::Podcast } else { crate::blacklist::Kind::Station }, *id).await.map_err(db)?;
                }
                rows.push(json!({ "name": name, "before": before, "after": result, "changed": differs, "mark_approved": approved }));
            }
            Ok(json!({ "dry_run": dry, "kind": kind, "changed": changed, "items": rows }))
        }
        "set_owner" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let row = s.store.admin_book_row(id).await.map_err(db)?.ok_or("no such album")?;
            let owner = match args.get("owner") {
                Some(Value::Null) => None,
                Some(Value::String(name)) => {
                    let name = name.trim().to_lowercase();
                    let users = s.store.user_rows().await.map_err(db)?;
                    Some(users.iter().find(|u| u.name.to_lowercase() == name).map(|u| u.id).ok_or("no profile with this nickname")?)
                }
                _ => return Err("owner is a nickname, or null to remove it".into()),
            };
            s.store.set_book_owner(id, owner).await.map_err(db)?;
            let now = match owner {
                Some(o) => s.store.user_rows().await.map_err(db)?.into_iter().find(|u| u.id == o).map(|u| u.name),
                None => None,
            };
            s.store.log_tag_change(&token.name, id, "", "Owner", row.owner.as_deref().unwrap_or(""), now.as_deref().unwrap_or("")).await.map_err(db)?;
            tracing::info!(token = %token.name, album = id, old = ?row.owner, new = ?now, "owner set");
            Ok(json!({ "id": id, "owner": now, "was": row.owner }))
        }
        "hide_albums" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            let hide = args.get("hide").and_then(Value::as_bool).unwrap_or(true);
            let people = s.store.user_rows().await.map_err(db)?;
            let (profile, nick) = profile_named(&people, args.get("profile").and_then(Value::as_str).ok_or("profile is required")?)?;
            let mut ids: Vec<i64> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
            if let Some(label) = args.get("label").and_then(Value::as_str).map(str::trim).filter(|l| !l.is_empty()) {
                let wanted = label.to_lowercase();
                for (book, labels) in s.store.book_labels().await.map_err(db)? {
                    if labels.iter().any(|l| l.to_lowercase() == wanted) && !ids.contains(&book) {
                        ids.push(book);
                    }
                }
            }
            if ids.is_empty() {
                return Err("give ids, or a label that some album carries".into());
            }
            ids.sort_unstable();
            let mut done = Vec::new();
            let mut missing = Vec::new();
            for id in ids {
                let Some(row) = s.store.admin_book_row(id).await.map_err(db)? else {
                    missing.push(id);
                    continue;
                };
                if !dry {
                    s.store.set_book_hidden(profile, id, hide).await.map_err(db)?;
                }
                done.push(json!({ "id": id, "title": row.title, "author": row.author }));
            }
            if !dry {
                tracing::info!(token = %token.name, profile = %nick, albums = done.len(), hide, "albums hidden or shown for a profile");
            }
            Ok(json!({ "dry_run": dry, "profile": nick, "hide": hide, "albums": done.len(), "list": done, "no_such_album": missing }))
        }
        "assign_owners" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            let only: Option<Vec<i64>> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect());
            let kind = args.get("kind").and_then(Value::as_str);
            if kind.is_some_and(|k| k != "audiobook" && k != "music") {
                return Err("kind is audiobook or music".into());
            }
            // The names a folder can have: the complete name in both orders, and the nickname.
            let mut names: std::collections::HashMap<String, (i64, String)> = std::collections::HashMap::new();
            for u in s.store.user_rows().await.map_err(db)? {
                let mut keys = vec![u.name.to_lowercase()];
                if let (Some(f), Some(l)) = (u.first_name.as_deref().map(str::trim).filter(|x| !x.is_empty()), u.last_name.as_deref().map(str::trim).filter(|x| !x.is_empty())) {
                    let (f, l) = (f.to_lowercase(), l.to_lowercase());
                    keys.extend([format!("{f} {l}"), format!("{l} {f}"), format!("{l}, {f}")]);
                }
                for k in keys {
                    names.entry(k).or_insert((u.id, u.name.clone()));
                }
            }
            let mut assigned = Vec::new();
            let mut without = 0;
            for row in s.store.admin_book_rows().await.map_err(db)? {
                if row.owner_id.is_some() || only.as_ref().is_some_and(|ids| !ids.contains(&row.id)) || kind.is_some_and(|k| row.kind != k) {
                    continue;
                }
                let top = row.folder.split('/').next().unwrap_or("").trim().to_lowercase();
                let Some((profile, nick)) = names.get(&top) else {
                    without += 1;
                    continue;
                };
                if !dry {
                    s.store.set_book_owner(row.id, Some(*profile)).await.map_err(db)?;
                    s.store.log_tag_change(&token.name, row.id, "", "Owner", "", nick).await.map_err(db)?;
                }
                assigned.push(json!({ "id": row.id, "title": row.title, "owner": nick }));
            }
            if !dry {
                tracing::info!(token = %token.name, count = assigned.len(), "owners assigned from the folders");
            }
            Ok(json!({ "dry_run": dry, "assigned": assigned, "without_match": without }))
        }
        "release_albums" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            let only: Option<Vec<i64>> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect());
            let kind = args.get("kind").and_then(Value::as_str);
            if kind.is_some_and(|k| k != "audiobook" && k != "music") {
                return Err("kind is audiobook or music".into());
            }
            let mut released = Vec::new();
            let mut no_age = Vec::new();
            let mut already = 0;
            for row in s.store.admin_book_rows().await.map_err(db)? {
                if only.as_ref().is_some_and(|ids| !ids.contains(&row.id)) || kind.is_some_and(|k| row.kind != k) {
                    continue;
                }
                if row.released {
                    already += 1;
                    continue;
                }
                let Some(age) = row.recommended_age else {
                    no_age.push(json!({ "id": row.id, "title": row.title, "author": row.author }));
                    continue;
                };
                if !dry {
                    s.store.release_book(row.id, age).await.map_err(db)?;
                    s.store.log_tag_change(&token.name, row.id, "", "Released", "", &age.to_string()).await.map_err(db)?;
                    tracing::info!(token = %token.name, album = row.id, age, "album released by age recommendation");
                }
                released.push(json!({ "id": row.id, "title": row.title, "author": row.author, "from_age": age }));
            }
            Ok(json!({ "dry_run": dry, "released": released, "no_age": no_age, "already_released": already }))
        }
        "set_recommended_age" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let age = match args.get("age") {
                Some(Value::Null) => None,
                Some(v) => Some(v.as_i64().filter(|a| (0..=21).contains(a)).ok_or("age is a whole number from 0 to 21, or null")?),
                None => return Err("age is required (a number, or null to remove it)".into()),
            };
            let row = s.store.admin_book_row(id).await.map_err(db)?.ok_or("no such album")?;
            s.store.set_book_recommended_age(id, age).await.map_err(db)?;
            let show = |a: Option<i64>| a.map_or_else(String::new, |a| a.to_string());
            s.store.log_tag_change(&token.name, id, "", "RecommendedAge", &show(row.recommended_age), &show(age)).await.map_err(db)?;
            tracing::info!(token = %token.name, album = id, old = ?row.recommended_age, new = ?age, "recommended age set");
            Ok(json!({ "id": id, "recommended_age": age, "was": row.recommended_age }))
        }
        "remove_empty_folders" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let folders = crate::relocate::empty_folders(&s.store).await?;
            let mut removed = Vec::new();
            let mut failed = Vec::new();
            if !dry {
                for f in &folders {
                    let path = f.clone();
                    match tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&path)).await {
                        Ok(Ok(())) => {
                            tracing::info!(token = %token.name, folder = %f.display(), "empty folder removed");
                            s.store.log_tag_change(&token.name, 0, "", "EmptyFolderRemoved", &f.display().to_string(), "").await.map_err(db)?;
                            removed.push(f.clone());
                        }
                        Ok(Err(e)) => failed.push(json!({ "folder": f, "error": e.to_string() })),
                        Err(e) => failed.push(json!({ "folder": f, "error": e.to_string() })),
                    }
                }
            }
            Ok(json!({ "dry_run": dry, "empty_folders": folders, "removed": removed, "failed": failed }))
        }
        "list_quarantine" => {
            let entries = crate::quarantine::list(&s.store).await?;
            let by_hand = entries.iter().filter(|e| e.put_there_by == "by hand").count();
            Ok(json!({ "albums": entries.len(), "by_hand": by_hand, "entries": entries }))
        }
        "version" => {
            let sha = tokio::task::spawn_blocking(crate::buildinfo::binary_sha256).await.ok().flatten();
            Ok(json!({ "version": crate::buildinfo::VERSION, "git": crate::buildinfo::GIT, "built_at": crate::buildinfo::BUILT, "started_at_ms": crate::buildinfo::started_at(), "binary_sha256": sha, "summary": crate::buildinfo::describe() }))
        }
        "check_access" => Ok(json!(crate::access::refresh(&s.store).await)),
        "move_albums" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let from = text("from").ok_or("from is required")?;
            let to = text("to").unwrap_or_default();
            let plans = crate::relocate::plan_move_branch(&s.store, s.library.as_deref(), &from, &to).await?;
            let (mut moved, mut failed) = (0usize, 0usize);
            let mut albums: Vec<Value> = Vec::new();
            for plan in &plans {
                let mut problem = plan.problem.clone();
                if !dry && problem.is_none() {
                    match crate::relocate::execute_rename(&s.store, plan).await {
                        Ok(()) => {
                            moved += 1;
                            tracing::info!(token = %token.name, album = plan.id, from = %plan.from.display(), to = %plan.to.display(), "album moved");
                            s.store.log_tag_change(&token.name, plan.id, "", "Folder", &plan.from.display().to_string(), &plan.to.display().to_string()).await.map_err(db)?;
                        }
                        Err(e) => {
                            failed += 1;
                            problem = Some(e);
                        }
                    }
                }
                albums.push(json!({ "id": plan.id, "author": plan.author, "title": plan.title, "to": plan.new_folder, "from": plan.from, "problem": problem }));
            }
            if moved > 0 {
                s.store.rescan_soon();
            }
            let movable = plans.iter().filter(|p| p.problem.is_none()).count();
            Ok(json!({ "dry_run": dry, "albums": albums, "movable": movable, "blocked": plans.len() - movable, "moved": moved, "failed": failed }))
        }
        "rename_album" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let plan = crate::relocate::plan_rename(&s.store, s.library.as_deref(), id, text("author").as_deref(), text("title").as_deref()).await?;
            let mut renamed = false;
            if !dry && plan.problem.is_none() {
                crate::relocate::execute_rename(&s.store, &plan).await?;
                renamed = true;
                tracing::info!(token = %token.name, album = id, from = %plan.from.display(), to = %plan.to.display(), "album renamed");
                s.store.log_tag_change(&token.name, id, "", "Folder", &plan.from.display().to_string(), &plan.to.display().to_string()).await.map_err(db)?;
                s.store.rescan_soon();
            }
            Ok(json!({ "dry_run": dry, "renamed": renamed, "plan": plan }))
        }
        "upgrade_to_id3v2" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let ids: Option<Vec<i64>> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect());
            let rows = s.store.admin_book_rows().await.map_err(db)?;
            let (mut found, mut changed, mut failed) = (Vec::new(), 0usize, Vec::new());
            for row in rows.into_iter().filter(|r| ids.as_ref().is_none_or(|ids| ids.contains(&r.id))) {
                let album = Album::load(s, row.id).await?;
                let files: Vec<PathBuf> = album.tracks.iter().map(|(_, p)| p.clone()).collect();
                let todo = tokio::task::spawn_blocking(move || files.into_iter().filter(|p| crate::tags::lacks_id3v2(p)).collect::<Vec<_>>()).await.map_err(|e| e.to_string())?;
                if todo.is_empty() {
                    continue;
                }
                found.push(json!({ "id": row.id, "title": row.title, "author": row.author, "files": todo.len() }));
                if dry {
                    continue;
                }
                for path in todo {
                    let shown = path.display().to_string();
                    match tokio::task::spawn_blocking(move || crate::tags::upgrade_to_id3v2(&path)).await.map_err(|e| e.to_string())? {
                        Ok(true) => changed += 1,
                        Ok(false) => {}
                        Err(e) => failed.push(json!({ "file": shown, "error": e.to_string() })),
                    }
                }
                s.store.log_tag_change(&token.name, row.id, "", "Id3v2", "", "upgraded").await.map_err(db)?;
            }
            if !dry && changed > 0 {
                s.store.rescan_soon();
            }
            tracing::info!(token = %token.name, dry, albums = found.len(), changed, "ID3v2 upgrade");
            Ok(json!({ "dry_run": dry, "albums": found, "files_changed": changed, "failed": failed }))
        }
        "list_genres" => Ok(json!({
            "genres": s.store.genres().await.map_err(db)?,
            "loose_tags": s.store.loose_genre_tags().await.map_err(db)?,
            "albums_without_genre": s.store.albums_without_genre().await.map_err(db)?,
        })),
        "list_podcasts" => {
            let people = s.store.user_rows().await.map_err(db)?;
            let only = match text("profile") {
                Some(n) => Some(profile_named(&people, &n)?.1),
                None => None,
            };
            let feeds = s.store.feeds_with_followers().await.map_err(db)?;
            let podcast_labels = s.store.podcast_labels_with_mark().await.map_err(db)?;
            let list: Vec<Value> = feeds
                .into_iter()
                .filter(|(_, followers)| only.as_ref().is_none_or(|n| followers.iter().any(|(f, _)| f.eq_ignore_ascii_case(n))))
                .map(|(feed, followers)| {
                    let names: Vec<String> = followers.iter().map(|(n, ok)| if *ok { n.clone() } else { format!("{n} (waiting)") }).collect();
                    json!({ "title": feed.title, "feed_url": feed.url, "followed_by": names, "last_error": feed.last_error, "labels": podcast_labels.get(&feed.id).cloned().unwrap_or_default() })
                })
                .collect();
            Ok(json!({ "podcasts": list }))
        }
        "edit_podcasts" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let action = text("action").ok_or("action is required")?;
            let people = s.store.user_rows().await.map_err(db)?;
            let wanted: Vec<String> = args.get("for").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            let profiles = profiles_named(&people, &wanted)?;
            let feed_id = |key: Option<String>| async {
                let key = key.ok_or("podcast (the title or the feed address) is required")?;
                let all = s.store.feeds_with_followers().await.map_err(db)?;
                let hits: Vec<(i64, String)> = all.iter().filter(|(f, _)| f.url == key.trim() || f.title.eq_ignore_ascii_case(key.trim())).map(|(f, _)| (f.id, f.title.clone())).collect();
                match hits.as_slice() {
                    [one] => Ok(one.clone()),
                    [] => Err(format!("no podcast called {key}")),
                    _ => Err(format!("several podcasts are called {key}; use the feed address")),
                }
            };
            let mut done = json!({});
            match action.as_str() {
                "follow" => {
                    let url = text("url").ok_or("url is required")?;
                    if profiles.is_empty() {
                        return Err("for: the profiles that follow it is required".into());
                    }
                    let (address, feed) = crate::api_feeds::read_feed(&s.store, url.trim()).await.map_err(|_| format!("{url} is not a podcast feed, or it cannot be read"))?;
                    let id = s.store.save_feed(&address, &feed).await.map_err(db)?;
                    for (profile, _) in &profiles {
                        s.store.subscribe(*profile, id, true).await.map_err(db)?;
                    }
                    done = json!({ "followed": feed.title, "feed_url": address, "for": profiles.iter().map(|p| p.1.clone()).collect::<Vec<_>>() });
                }
                "unfollow" => {
                    let (id, title) = feed_id(text("podcast")).await?;
                    let followers = s.store.feeds_with_followers().await.map_err(db)?;
                    let mut gone = Vec::new();
                    if wanted.iter().any(|w| w == "*") {
                        // Everybody who follows it, also a child.
                        for (n, _) in followers.iter().filter(|(f, _)| f.id == id).flat_map(|(_, fs)| fs.iter()) {
                            if let Some(p) = people.iter().find(|u| u.name == *n) {
                                s.store.unsubscribe(p.id, id).await.map_err(db)?;
                                gone.push(n.clone());
                            }
                        }
                    } else {
                        if profiles.is_empty() {
                            return Err("for: the profiles that stop following it is required".into());
                        }
                        for (profile, name) in &profiles {
                            s.store.unsubscribe(*profile, id).await.map_err(db)?;
                            gone.push(name.clone());
                        }
                    }
                    done = json!({ "unfollowed": title, "for": gone });
                }
                "refresh" => {
                    let (id, title) = feed_id(text("podcast")).await?;
                    let url = s.store.feed_url(id).await.map_err(db)?.ok_or("the feed is gone")?;
                    crate::feeds::refresh_feed(&s.store, id, &url).await.map_err(|e| format!("{title} could not be read: {e}"))?;
                    done = json!({ "refreshed": title });
                }
                other => return Err(format!("unknown action {other}")),
            }
            tracing::info!(token = %token.name, %action, "podcasts edited");
            s.store.log_tag_change(&token.name, 0, "", "Podcasts", &action, &text("podcast").or_else(|| text("url")).unwrap_or_default()).await.map_err(db)?;
            Ok(done)
        }
        "list_stations" => {
            let stations = s.store.stations_with_owners().await.map_err(db)?;
            let station_labels = s.store.station_labels_with_mark().await.map_err(db)?;
            let list: Vec<Value> = stations.into_iter().map(|(st, owners)| json!({ "labels": station_labels.get(&st.id).cloned().unwrap_or_default(), "name": st.name, "stream_url": st.stream_url, "homepage": st.homepage, "logo_url": st.logo_url, "favourite_of": owners })).collect();
            Ok(json!({ "stations": list }))
        }
        "edit_blacklist" => {
            let action = text("action").unwrap_or_else(|| "list".to_owned());
            let words: Vec<String> = args.get("words").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            let current = s.store.blacklist_words().await.map_err(db)?;
            if action == "list" {
                let rows = s.store.blacklist_rows().await.map_err(db)?;
                let books = s.store.admin_book_rows().await.map_err(db)?;
                let feeds = s.store.feeds_with_followers().await.map_err(db)?;
                let stations = s.store.stations_with_owners().await.map_err(db)?;
                let marked: Vec<Value> = rows
                    .iter()
                    .map(|r| {
                        let title = match r.kind.as_str() {
                            "book" => books.iter().find(|b| b.id == r.id).map(|b| album_label(&b.title, &b.author)),
                            "podcast" => feeds.iter().find(|(f, _)| f.id == r.id).map(|(f, _)| f.title.clone()),
                            _ => stations.iter().find(|(st, _)| st.id == r.id).map(|(st, _)| st.name.clone()),
                        };
                        json!({ "kind": r.kind, "id": r.id, "title": title, "word": r.word, "field": r.field, "approved": r.approved })
                    })
                    .collect();
                return Ok(json!({ "words": current, "marked": marked }));
            }
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            if words.iter().all(|w| w.trim().is_empty()) {
                return Err("words is required: the words to add or to remove".into());
            }
            let wanted: Vec<String> = match action.as_str() {
                "add" => {
                    let mut all = current.clone();
                    for w in words.iter().map(|w| w.trim()).filter(|w| !w.is_empty()) {
                        if !all.iter().any(|c| c.eq_ignore_ascii_case(w)) {
                            all.push(w.to_owned());
                        }
                    }
                    all
                }
                "remove" => {
                    let gone: Vec<String> = words.iter().map(|w| w.trim().to_lowercase()).collect();
                    if let Some(missing) = gone.iter().find(|g| !current.iter().any(|c| c.to_lowercase() == **g)) {
                        return Err(format!("{missing:?} is not in the list"));
                    }
                    current.iter().filter(|c| !gone.contains(&c.to_lowercase())).cloned().collect()
                }
                _ => return Err("action is list, add or remove".into()),
            };
            crate::blacklist::Blacklist::parse(&wanted.join("\n")).map_err(|line| format!("not usable (a broken regular expression, a control character, a line over {} characters or more than {} lines): {line}", crate::blacklist::MAX_LINE_CHARS, crate::blacklist::MAX_LINES))?;
            if dry {
                let would = s.store.blacklist_preview(&wanted).await.map_err(db)?;
                return Ok(json!({ "dry_run": true, "words": wanted, "would_mark": would.len(), "media": would }));
            }
            let report = s.store.save_blacklist_words(&wanted).await.map_err(db)?;
            tracing::info!(token = %token.name, words = ?wanted, marked = report.marked, "the blacklist changed");
            Ok(json!({ "dry_run": false, "words": wanted, "marked": report.marked, "added": report.added, "removed": report.removed }))
        }
        "edit_stations" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let action = text("action").ok_or("action is required")?;
            let people = s.store.user_rows().await.map_err(db)?;
            let wanted: Vec<String> = args.get("for").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            // Profiles are named; "*" is every adult (administrators and members).
            let mut profiles: Vec<(i64, String)> = Vec::new();
            for w in &wanted {
                if w == "*" {
                    profiles.extend(people.iter().filter(|u| u.role == "admin" || u.role == "member").map(|u| (u.id, u.name.clone())));
                } else {
                    let u = people.iter().find(|u| u.name.eq_ignore_ascii_case(w.trim())).ok_or_else(|| format!("no profile called {w}"))?;
                    profiles.push((u.id, u.name.clone()));
                }
            }
            profiles.sort();
            profiles.dedup();
            let station_id = |key: Option<String>| async {
                let key = key.ok_or("station (the name or the stream address) is required")?;
                let all = s.store.stations_with_owners().await.map_err(db)?;
                let hits: Vec<i64> = all.iter().filter(|(st, _)| st.stream_url == key.trim() || st.name.eq_ignore_ascii_case(key.trim())).map(|(st, _)| st.id).collect();
                match hits.as_slice() {
                    [one] => Ok(*one),
                    [] => Err(format!("no station called {key}")),
                    _ => Err(format!("several stations are called {key}; use the stream address")),
                }
            };
            let mut done = json!({});
            match action.as_str() {
                "add" => {
                    let (name, url) = (text("name").ok_or("name is required")?, text("stream_url").ok_or("stream_url is required")?);
                    let id = s.store.save_station(&url, &name, text("homepage").as_deref(), text("logo_url").as_deref()).await.map_err(db)?;
                    for (profile, _) in &profiles {
                        s.store.favorite_station(*profile, id, true).await.map_err(db)?;
                    }
                    done = json!({ "added": name, "for": profiles.iter().map(|p| p.1.clone()).collect::<Vec<_>>() });
                }
                "update" => {
                    let id = station_id(text("station")).await?;
                    s.store.update_station(id, text("name").as_deref(), text("stream_url").as_deref(), text("homepage").as_deref(), text("logo_url").as_deref()).await.map_err(db)?;
                }
                "assign" => {
                    let id = station_id(text("station")).await?;
                    if profiles.is_empty() {
                        return Err("for: the profiles that get it is required".into());
                    }
                    for (profile, _) in &profiles {
                        s.store.favorite_station(*profile, id, true).await.map_err(db)?;
                        s.store.decide_release("station", *profile, id, true).await.map_err(db)?;
                    }
                }
                "unassign" => {
                    let id = station_id(text("station")).await?;
                    for (profile, _) in &profiles {
                        s.store.unfavorite_station(*profile, id).await.map_err(db)?;
                    }
                }
                "delete" => {
                    let id = station_id(text("station")).await?;
                    s.store.delete_station(id).await.map_err(db)?;
                }
                other => return Err(format!("unknown action {other}")),
            }
            tracing::info!(token = %token.name, %action, "stations edited");
            s.store.log_tag_change(&token.name, 0, "", "Stations", &action, &text("station").or_else(|| text("name")).unwrap_or_default()).await.map_err(db)?;
            let stations = s.store.stations_with_owners().await.map_err(db)?;
            let list: Vec<Value> = stations.into_iter().map(|(st, owners)| json!({ "name": st.name, "stream_url": st.stream_url, "favourite_of": owners })).collect();
            if let Some(map) = done.as_object_mut() {
                map.insert("stations".into(), Value::Array(list));
            }
            Ok(done)
        }
        "list_playlists" => {
            let people = s.store.user_rows().await.map_err(db)?;
            let only = match text("profile") {
                Some(n) => Some(profile_named(&people, &n)?.0),
                None => None,
            };
            let lists = s.store.playlist_summaries(only).await.map_err(plist)?;
            let lists: Vec<Value> = lists.iter().map(|p| json!({ "profile": p.profile_name, "name": p.name, "tracks": p.items, "duration": clock(p.seconds), "seconds": p.seconds })).collect();
            Ok(json!({ "playlists": lists }))
        }
        "get_playlist" => {
            let people = s.store.user_rows().await.map_err(db)?;
            let (profile, _) = profile_named(&people, &text("profile").ok_or("profile (the name of a profile) is required")?)?;
            let id = playlist_named(s, profile, &text("name").ok_or("name (of the playlist) is required")?).await?;
            playlist_json(s, id, profile).await
        }
        "edit_playlists" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let action = text("action").ok_or("action is required")?;
            let people = s.store.user_rows().await.map_err(db)?;
            let (profile, profile_name) = profile_named(&people, &text("profile").ok_or("profile (the name of a profile) is required")?)?;
            let name = text("name").ok_or("name (of the playlist) is required")?;
            if action == "create" {
                s.store.create_playlist(profile, &name).await.map_err(plist)?;
            }
            let id = playlist_named(s, profile, &name).await?;
            match action.as_str() {
                "create" => {}
                "rename" => s.store.rename_playlist(id, profile, &text("new_name").ok_or("new_name is required")?).await.map_err(plist)?,
                "delete" => s.store.delete_playlist(id, profile).await.map_err(plist)?,
                "clear" => s.store.clear_playlist(id, profile).await.map_err(plist)?,
                "add" => {
                    let title = text("album").ok_or("album (its title) is required")?;
                    let (book, _) = album_named(s, &title, text("author").as_deref()).await?;
                    let at = int("at");
                    if text("track").is_some() || int("track_number").is_some() {
                        let track = track_of(s, book, text("track").as_deref(), int("track_number"), int("disc")).await?;
                        s.store.add_playlist_parts(id, profile, &[track], at).await.map_err(plist)?;
                    } else {
                        s.store.add_playlist_book(id, profile, book, at).await.map_err(plist)?;
                    }
                }
                "remove" | "move" => {
                    let view = s.store.playlist_view(id, profile).await.map_err(plist)?.ok_or("no such playlist")?;
                    let from = entry_at(&view, int("position"), text("track").as_deref())?;
                    if action == "remove" {
                        s.store.remove_playlist_item(id, profile, view.entries[from].item_id).await.map_err(plist)?;
                    } else {
                        let to = int("to").ok_or("to (the new 1-based position) is required")?;
                        if to < 1 || to as usize > view.entries.len() {
                            return Err(format!("to must be between 1 and {}", view.entries.len()));
                        }
                        let mut order: Vec<i64> = view.entries.iter().map(|e| e.item_id).collect();
                        let moved = order.remove(from);
                        order.insert(to as usize - 1, moved);
                        s.store.reorder_playlist(id, profile, &order).await.map_err(plist)?;
                    }
                }
                other => return Err(format!("unknown action {other}")),
            }
            tracing::info!(token = %token.name, %action, profile = %profile_name, playlist = %name, "playlists edited");
            s.store.log_tag_change(&token.name, 0, "", "Playlists", &action, &format!("{profile_name}: {name}")).await.map_err(db)?;
            if action == "delete" {
                return Ok(json!({ "deleted": name, "profile": profile_name }));
            }
            // After a rename the list is found under its new name.
            let id = if action == "rename" { id } else { playlist_named(s, profile, &name).await? };
            playlist_json(s, id, profile).await
        }
        "edit_genres" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let action = text("action").ok_or("action is required")?;
            let list = s.store.genres().await.map_err(db)?;
            let genre_id = |key: &str| -> Result<i64, String> {
                let n = text(key).ok_or_else(|| format!("{key} (the name of a genre) is required"))?;
                list.iter().find(|g| g.name.eq_ignore_ascii_case(n.trim())).map(|g| g.id).ok_or_else(|| format!("no genre called {n}"))
            };
            match action.as_str() {
                "add" => {
                    s.store.add_genre(&text("name").ok_or("name is required")?).await.map_err(db)?;
                }
                "rename" => s.store.rename_genre(genre_id("genre")?, &text("name").ok_or("name is required")?).await.map_err(db)?,
                "show" | "hide" => s.store.set_genre_visible(genre_id("genre")?, action == "show").await.map_err(db)?,
                "move" => s.store.move_genre(genre_id("genre")?, int("by").ok_or("by is required (-1 or 1)")?).await.map_err(db)?,
                "delete" => s.store.delete_genre(genre_id("genre")?).await.map_err(db)?,
                "assign" => s.store.assign_genre_tag(&text("tag").ok_or("tag is required")?, genre_id("genre")?).await.map_err(db)?,
                "unassign" => s.store.remove_genre_tag(&text("tag").ok_or("tag is required")?).await.map_err(db)?,
                other => return Err(format!("unknown action {other}")),
            }
            tracing::info!(token = %token.name, %action, "genres edited");
            s.store.log_tag_change(&token.name, 0, "", "Genres", &action, &text("genre").or_else(|| text("name")).unwrap_or_default()).await.map_err(db)?;
            Ok(json!({ "genres": s.store.genres().await.map_err(db)?, "loose_tags": s.store.loose_genre_tags().await.map_err(db)? }))
        }
        "set_genre" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let row = s.store.admin_book_row(id).await.map_err(db)?.ok_or("no such album")?;
            let automatic = args.get("automatic").and_then(Value::as_bool).unwrap_or(false);
            let genre = match args.get("genre") {
                Some(Value::String(n)) => {
                    let list = s.store.genres().await.map_err(db)?;
                    Some(list.iter().find(|g| g.name.eq_ignore_ascii_case(n.trim())).map(|g| g.id).ok_or_else(|| format!("no genre called {n}"))?)
                }
                _ => None,
            };
            s.store.set_book_genre(id, genre, automatic).await.map_err(db)?;
            let now = s.store.admin_book_row(id).await.map_err(db)?.and_then(|r| r.genre);
            s.store.log_tag_change(&token.name, id, "", "Genre", row.genre.as_deref().unwrap_or(""), now.as_deref().unwrap_or("")).await.map_err(db)?;
            Ok(json!({ "id": id, "genre": now, "was": row.genre, "by_hand": !automatic }))
        }
        "list_ignores" => Ok(json!({ "rules": s.store.ignore_rules().await.map_err(db)? })),
        "ignore" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let scope = text("scope").ok_or("scope is required (album, title or work)")?;
            let what = text("what").unwrap_or_else(|| crate::ignore::DUPLICATE.to_owned());
            let rule = s.store.add_ignore(id, &scope, &what, text("note").as_deref(), &format!("mcp:{}", token.name)).await.map_err(db)?;
            tracing::info!(token = %token.name, album = id, %scope, %what, rule, "ignore rule made");
            Ok(json!({ "rule": rule, "scope": scope, "what": what }))
        }
        "unignore" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let rule = int("rule").ok_or("rule is required")?;
            let removed = s.store.remove_ignore(rule).await.map_err(db)?;
            tracing::info!(token = %token.name, rule, removed, "ignore rule removed");
            Ok(json!({ "rule": rule, "removed": removed }))
        }
        "set_description" => {
            if !token.can_write {
                return Err("this token may only read".into());
            }
            let id = int("id").ok_or("id is required")?;
            let new = text("text").ok_or("text is required")?;
            let row = s.store.admin_book_row(id).await.map_err(db)?.ok_or("no such album")?;
            let old = row.description.clone().unwrap_or_default();
            s.store.set_book_description(id, &new).await.map_err(db)?;
            let short = |t: &str| t.chars().take(300).collect::<String>();
            s.store.log_tag_change(&token.name, id, "", "Description", &short(&old), &short(new.trim())).await.map_err(db)?;
            tracing::info!(token = %token.name, album = id, old_chars = old.chars().count(), new_chars = new.trim().chars().count(), "description set");
            Ok(json!({ "id": id, "chars": new.trim().chars().count(), "replaced_chars": old.chars().count() }))
        }
        "quarantine_duplicates" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let ids: Vec<i64> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect()).ok_or("ids is required")?;
            let plans = crate::quarantine::plan(&s.store, s.library.as_deref(), &ids).await?;
            let mut albums = Vec::new();
            let mut moved = 0;
            for p in &plans {
                let mut entry = json!({ "id": p.id, "title": p.title, "author": p.author, "from": p.from, "to": p.to, "stays": p.kept, "problem": p.problem });
                if !dry && p.problem.is_none() {
                    match crate::quarantine::execute(&s.store, p).await {
                        Ok(()) => {
                            moved += 1;
                            entry["moved"] = json!(true);
                            tracing::info!(token = %token.name, album = p.id, from = %p.from.display(), to = %p.to.display(), "album put into quarantine");
                            s.store.log_tag_change(&token.name, p.id, "", "Quarantine", &p.from.display().to_string(), &p.to.display().to_string()).await.map_err(db)?;
                        }
                        Err(e) => {
                            tracing::warn!(token = %token.name, album = p.id, error = %e, "album could not be put into quarantine");
                            entry["moved"] = json!(false);
                            entry["problem"] = json!(e);
                        }
                    }
                }
                albums.push(entry);
            }
            let ok = plans.iter().filter(|p| p.problem.is_none()).count();
            Ok(json!({ "dry_run": dry, "movable": ok, "blocked": plans.len() - ok, "moved": moved, "albums": albums }))
        }
        "find_duplicates" => {
            let rules = s.store.ignore_rules().await.map_err(db)?;
            let groups = s.store.duplicate_groups().await.map_err(db)?;
            Ok(json!({ "groups": groups.len(), "duplicates": groups, "ignored_by_rules": rules.iter().filter(|r| r.what == crate::ignore::DUPLICATE || r.what == crate::ignore::ALL).count() }))
        }
        "move_to_audiobooks" => {
            let dry = args.get("dry_run").and_then(Value::as_bool).unwrap_or(true);
            if !dry && !token.can_write {
                return Err("this token may only read".into());
            }
            let ids: Option<Vec<i64>> = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_i64).collect());
            let plans = crate::relocate::plan(&s.store, s.library.as_deref(), ids.as_deref()).await?;
            let mut albums: Vec<Value> = plans
                .iter()
                .map(|p| json!({ "id": p.id, "book": p.keep, "genre": p.genre, "author": p.author, "title": p.title, "disc": p.disc, "from": p.from, "to": p.to, "problem": p.problem }))
                .collect();
            let mut moved = 0;
            if !dry {
                // One book at a time: a single album, or all the CDs of a work together.
                let mut books: Vec<i64> = plans.iter().filter(|p| p.problem.is_none()).map(|p| p.keep).collect();
                books.dedup();
                books.sort_unstable();
                books.dedup();
                for book in books {
                    let members: Vec<&crate::relocate::Plan> = plans.iter().filter(|p| p.keep == book).collect();
                    let result = crate::relocate::execute(&s.store, &members).await;
                    for p in &members {
                        let entry = albums.iter_mut().find(|a| a["id"] == p.id).expect("planned");
                        match &result {
                            Ok(()) => {
                                moved += 1;
                                entry["moved"] = json!(true);
                                tracing::info!(token = %token.name, album = p.id, book, from = %p.from.display(), to = %p.to.display(), "album moved to the audiobooks");
                                s.store.log_tag_change(&token.name, p.id, "", "Folder", &p.from.display().to_string(), &p.to.display().to_string()).await.map_err(db)?;
                            }
                            Err(e) => {
                                tracing::warn!(token = %token.name, album = p.id, error = %e, "album could not be moved");
                                entry["moved"] = json!(false);
                                entry["problem"] = json!(e);
                            }
                        }
                    }
                }
            }
            let movable = plans.iter().filter(|p| p.problem.is_none()).count();
            Ok(json!({ "dry_run": dry, "movable": movable, "blocked": plans.len() - movable, "moved": moved, "albums": albums }))
        }
        "list" => {
            let tools = tool_list();
            let tools: Vec<Value> = tools
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| {
                    let name = t["name"].as_str().unwrap_or_default();
                    let description = t["description"].as_str().unwrap_or_default();
                    let summary = description.split_once(". ").map_or(description, |(first, _)| first);
                    json!({ "name": name, "changes_files": WRITING_TOOLS.contains(&name), "summary": summary })
                })
                .collect();
            Ok(json!({ "tools": tools, "your_token_may_write": token.can_write }))
        }
        "help_for" => {
            let tools = tool_list();
            let Some(wanted) = text("tool") else {
                // Without a tool name: the general help text.
                return Ok(json!({ "help": prompt_of(s).await, "tools": tools.as_array().into_iter().flatten().filter_map(|t| t["name"].as_str()).collect::<Vec<_>>() }));
            };
            let tool = tools.as_array().into_iter().flatten().find(|t| t["name"] == wanted.as_str());
            let Some(tool) = tool else {
                let names: Vec<&str> = tools.as_array().into_iter().flatten().filter_map(|t| t["name"].as_str()).collect();
                return Err(format!("unknown tool {wanted}; the tools are: {}", names.join(", ")));
            };
            Ok(json!({
                "name": wanted,
                "description": tool["description"],
                "parameters": tool["inputSchema"],
                "changes_files": WRITING_TOOLS.contains(&wanted.as_str()),
                "needs_write_token": WRITING_TOOLS.contains(&wanted.as_str()),
                "example": example_of(&wanted),
            }))
        }
        "change_log" => {
            let limit = (int("limit").unwrap_or(50).clamp(1, 500)) as u64;
            let rows = s.store.tag_changes(int("id").unwrap_or(0), limit).await.map_err(db)?;
            Ok(json!(rows))
        }
        other => Err(format!("unknown tool {other}")),
    }
}

/// An album with the real files of its tracks.
struct Album {
    row: crate::AdminBookRow,
    folder: String,
    /// The folder of the album on the disk.
    dir: Option<PathBuf>,
    tracks: Vec<(crate::entities::track::Model, PathBuf)>,
}

impl Album {
    async fn load(s: &McpState, id: i64) -> Result<Album, String> {
        let db = |e: crate::Error| format!("database error: {e}");
        let row = s.store.admin_book_row(id).await.map_err(db)?.ok_or("no such album")?;
        let folder = s.store.book_folder(id).await.map_err(db)?.ok_or("no such album")?;
        let mut tracks = Vec::new();
        for t in s.store.tracks_of(id).await.map_err(db)? {
            let path = crate::api::resolve(&s.store, s.library.as_deref(), &folder, &t.file).await.ok_or("the files of the album are missing")?;
            tracks.push((t, path));
        }
        if tracks.is_empty() {
            return Err("the album has no tracks".into());
        }
        // The track may lie in a disc folder below the album folder (`CD 1/01.mp3`).
        let depth = tracks[0].0.file.split('/').count();
        let dir = tracks[0].1.ancestors().nth(depth).map(Path::to_path_buf);
        Ok(Album { row, folder, dir, tracks })
    }
}

/// The name of the picture file of a folder, if there is a usable one.
fn folder_picture(dir: &Path) -> Option<String> {
    let mut names: Vec<(u8, String)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            Some((crate::api::cover_rank(&name)?, name))
        })
        .collect();
    names.sort();
    names.into_iter().next().map(|(_, n)| n)
}

fn fields_json(fields: Vec<(String, Vec<String>)>) -> Value {
    let mut map = serde_json::Map::new();
    for (name, mut values) in fields {
        map.insert(name, if values.len() == 1 { Value::String(values.remove(0)) } else { json!(values) });
    }
    Value::Object(map)
}

/// `{"Genre": "Jazz", "Comment": null, "TrackArtist": ["A", "B"]}` becomes field/values pairs.
fn parse_changes(fields: Option<&Value>) -> Result<Vec<(String, Vec<String>)>, String> {
    let map = fields.and_then(Value::as_object).ok_or("fields must be an object")?;
    if map.is_empty() {
        return Err("fields is empty".into());
    }
    let names = crate::tags::field_names();
    let mut out = Vec::new();
    for (name, value) in map {
        let canonical = names.iter().find(|n| n.eq_ignore_ascii_case(name)).ok_or_else(|| format!("unknown field {name} (see field_names)"))?;
        let values: Vec<String> = match value {
            Value::Null => Vec::new(),
            Value::String(s) => vec![s.clone()],
            Value::Number(n) => vec![n.to_string()],
            Value::Array(items) => items
                .iter()
                .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| format!("{name}: only strings in a list")))
                .collect::<Result<_, _>>()?,
            _ => return Err(format!("{name}: a string, a list of strings or null")),
        };
        if values.iter().any(|v| v.chars().count() > 2000 || v.contains('\0')) {
            return Err(format!("{name}: value too long or invalid"));
        }
        out.push(((*canonical).to_owned(), values));
    }
    Ok(out)
}

/// What is wrong with the tags of an album (reads every file).
fn problems_of(album: &Album) -> Vec<String> {
    let mut problems = Vec::new();
    let mut per_file: Vec<Vec<(String, Vec<String>)>> = Vec::new();
    for (_, path) in &album.tracks {
        match crate::tags::read_fields(path) {
            Ok(f) => per_file.push(f),
            Err(_) => {
                problems.push("unreadable_file".to_owned());
                per_file.push(Vec::new());
            }
        }
    }
    let first = |f: &[(String, Vec<String>)], name: &str| f.iter().find(|(n, _)| n == name).and_then(|(_, v)| v.first().cloned()).filter(|v| !v.trim().is_empty());
    let any = |name: &str| per_file.iter().any(|f| first(f, name).is_some());
    let music = album.row.kind == "music";
    if !any("AlbumArtist") && !any("TrackArtist") {
        problems.push("missing_artist".into());
    }
    if !any("AlbumTitle") {
        problems.push("missing_album".into());
    }
    if !any("RecordingDate") && !any("Year") && !any("OriginalReleaseDate") {
        problems.push("missing_year".into());
    }
    if music && !any("Genre") {
        problems.push("missing_genre".into());
    }
    if !music && !any("Composer") {
        problems.push("missing_narrator".into());
    }
    // ID3v2 is the binding tag of an MP3 (project owner, 2026-10-28); a file with only ID3v1 can be upgraded with upgrade_to_id3v2.
    if album.tracks.iter().any(|(_, p)| crate::tags::lacks_id3v2(p)) {
        problems.push("no_id3v2".into());
    }
    let has_picture = album.dir.as_deref().is_some_and(|d| folder_picture(d).is_some()) || album.tracks.iter().any(|(_, p)| crate::tags::embedded_cover(p).is_some());
    if !has_picture {
        problems.push("missing_cover".into());
    }
    for (field, label) in [("AlbumTitle", "tracks_disagree_on_album"), ("AlbumArtist", "tracks_disagree_on_album_artist")] {
        let mut values: Vec<String> = per_file.iter().filter_map(|f| first(f, field)).collect();
        values.sort();
        values.dedup();
        if values.len() > 1 {
            problems.push(label.into());
        }
    }
    if let Some(tag) = per_file.iter().find_map(|f| first(f, "AlbumTitle"))
        && !tag.trim().eq_ignore_ascii_case(album.row.title.trim())
    {
        problems.push("album_tag_differs_from_title".into());
    }
    // Titles and authors that still look like file names or a ripper's leftovers.
    let (title, author) = (album.row.title.trim(), album.row.author.trim());
    let disc_suffix = title.rsplit_once(' ').is_some_and(|(_, last)| {
        let digits = last.to_ascii_lowercase();
        digits.strip_prefix("cd").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    });
    let number_prefix = title.split_once('_').is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if disc_suffix {
        problems.push("title_ends_with_cd_number".into());
    }
    if number_prefix || title.contains('_') {
        problems.push("title_has_underscores_or_number_prefix".into());
    }
    if author.contains(" - ") || author.contains('(') || author.contains('_') {
        problems.push("author_looks_like_a_file_name".into());
    }
    let stray = per_file.iter().flatten().flat_map(|(_, v)| v).any(|v| v != v.trim() || v.contains("  "));
    if stray {
        problems.push("stray_spaces".into());
    }
    // A file an iPhone cannot play (the format as the database knows it, else the extension says): a hint for the owner.
    if album.tracks.iter().any(|(t, _)| {
        let format = if t.format.is_empty() { crate::audioformat::from_extension(t.file.rsplit_once('.').map_or("", |(_, e)| e)).format } else { t.format.clone() };
        !crate::audioformat::ios_playable(&format)
    }) {
        problems.push("unsupported_format_on_ios".into());
    }
    // Gaps in the track numbers of every disc.
    let mut discs: std::collections::BTreeMap<i64, Vec<i64>> = std::collections::BTreeMap::new();
    for (t, _) in &album.tracks {
        discs.entry(t.disc).or_default().push(t.number);
    }
    if discs.values().any(|numbers| {
        let mut n = numbers.clone();
        n.sort();
        n.dedup();
        n.len() != numbers.len() || n.first().is_some_and(|f| *f > 1) || n.windows(2).any(|w| w[1] != w[0] + 1)
    }) {
        problems.push("track_number_gaps".into());
    }
    problems
}
