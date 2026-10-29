# Spec: administration interface

Status: **built and tested** (`server/src/admin/`, `server/src/web/`, `server/templates/`): sign-in with PIN and lockout, sessions, CSRF protection, security headers, HTMX, and these pages and procedures: overview, users (create, change role, age limit, device limit, PIN, delete), books (release with age rating, withdraw, quota-exempt flag, guard rail below the recommendation), settings (confirm proposed values), import (criteria, unlock, lock, run), the weekly planner editor, and pairing with a QR code plus the device list. TLS is built (section 7 of the roles spec is settled: HTTPS with a pinned self-signed certificate for everything but loopback). **Not built:** the WireGuard tunnel itself, device marking as shared after pairing, avatars in the apps' profile list. Open items are marked **open**.

## Decision (project owner, 2026-10-27)

The server has a **template engine for the administration interface**: the admin pages are rendered on the server and delivered as HTML. They are not a part of the Flutter app. The engine is **Jinja** (decision of 2026-10-27: "a Jinja engine"), implemented by **MiniJinja**. Pages are **HTML templates**: real `.html` files in Jinja syntax.

## Why server-side pages fit

- Users, devices, release and import are maintained **by the server** (spec `spec-discovery-profiles-roles.md`). A web page on the server needs no app, no pairing and no store, and works from any browser on the home network, from Linux and Windows alike.
- It solves the first start: before any app is paired there is already something to open. The first admin is created on the command line (`david init`), everything after that can be done in the browser.
- One binary: the templates are embedded, so the Windows service and the Linux systemd unit need no extra files.

## Screens

| Screen | Role | Purpose |
|---|---|---|
| Sign in | any user with a PIN | session cookie, then the pages below |
| Users | `admin` | list, create, rename, change role, age limit, device limit, expiry (guests), reset PIN, delete (the last admin is refused) |
| Weekly planner | `admin` | one grid per user: 7 columns by 96 rows of 15 minutes, tap to allow; presets such as "every day 07:00 to 19:00" |
| Devices | `admin` | per user: list, revoke, mark as shared; create a pairing code |
| Pairing | `admin` | shows the `david://pair?...` address as a **QR code** (rendered on the server as SVG) and as text; valid for 15 minutes, single use |
| Library | `admin` | books waiting for a release, the release dialog (age rating prefilled from the recommendation, confirmation when releasing below it), exceptions per user or role, quota-exempt flag, withdraw a release |
| Import | `admin` | the release criteria (import roots, rating rule, automatic rules); unlocking and locking; the result of the last import |
| Settings | `admin` | role policies and settings; **proposals are shown as unconfirmed** with a button to confirm; time zone |
| Dashboard | `admin` | (formerly "Overview"; the menu item and the page are the dashboard) number of unreleased books, unconfirmed proposals, import state, server id and address |

Members, children and guests get no admin pages, but **every user has a profile page** (`/profile`, decided 2026-10-27): a picture and the PIN. Signing in is open to every user with a PIN; an administrator lands in the administration, everybody else on their profile. The session cookie is valid for `/`, so the profile and the pictures work outside `/admin`.

## Rules for every page

- **Authorization on the server**, from the session role, on every request. A page that is hidden in the menu is still refused when its address is typed.
- **Admin actions need a recent admin session or the PIN again** (step-up, section 3 of the roles spec).
- Forms that change data are **POST only** and carry a **CSRF token** tied to the session. Session cookies are `HttpOnly`, `SameSite=Strict`, and `Secure` once HTTPS is on.
- A strict **Content-Security-Policy**: scripts and styles only from the server itself, no inline scripts, no third-party requests. The pages work **without JavaScript** (forms fall back to full-page posts); HTMX and a little script for the planner grid are an enhancement.
- Everything the template prints is **escaped** by default. Raw HTML output is not used, except for the server-rendered QR SVG, which comes from our own code.
- No data leaves the server: no fonts, analytics or libraries loaded from the internet.

## Language

The administration interface follows the rule of the project: **English is the first language**, German the second. The browser's `Accept-Language` picks the language; anything unsupported falls back to English. Texts live in message catalogs, not in the templates, so that the templates stay readable and a new language is a new catalog.

## Template engine (decided)

**MiniJinja 3**, the Rust implementation of the Jinja syntax (`{{ }}`, `{% %}`, `extends`/`block`, `for`, `if`). It reads the templates at run time, but they are **embedded in the program** with `include_str!`, so the Windows service and the Linux unit stay a single file.

How it is set up (as built):

- One `Environment` is created at start and parses every template. A syntax error stops the start instead of failing when somebody opens the page; a test covers this.
- **Undefined variables are errors** (`UndefinedBehavior::Strict`), not empty strings. A typo in a template shows up in the tests.
- **Auto-escaping is on for `.html`** templates. A test puts `<script>` and `&` and quotes into user names and checks the output.
- Data is handed over as plain structs through `Serde` wrappers; templates do not call the database.
- Texts come from message catalogs through a function `t("key")` that reads the language of the page being rendered. English is complete, German must have exactly the same keys (a test checks it), and an unknown key shows the key itself so that a gap is visible.
- Language: `Accept-Language` picks German or English; anything else gives English.

Why not the compile-time engines: a runtime Jinja engine lets the pages be adjusted and previewed from files while developing, and Jinja is a syntax many people already know. The price is that errors in a template are found by tests and at start, not by the compiler. Hence the render tests for every page.

## Lists through sub-templates

Decision (project owner, 2026-10-27): **lists are built from sub-templates.**

- A list is a **frame** (`fragments/<name>_list.html`) around **rows** (`fragments/<name>_row.html`). The frame loops over the data and includes the row template for each entry. The page includes the frame.
- The row template is the **only** place that knows how one entry looks. A procedure that changes one entry answers with that row alone, and HTMX replaces it; one that changes the list (create, delete) answers with the whole frame.
- Every row carries a stable `id` (`user-<id>`), and the frame carries one too (`user-list`), including its empty state, so that HTMX always has a target.
- The page and the answer of a procedure therefore cannot drift apart: the page is made of the same fragments.
- Built for the user list: `fragments/user_list.html` and `fragments/user_row.html`, with `AdminUi::user_list_fragment` and `AdminUi::user_row_fragment`. The same pattern applies to books, devices, releases and settings.
- Checked by tests: the page contains the list fragment unchanged; the list is made of the same row; a row fragment is only a row (no table, no page); the frame keeps its id when the list is empty; names are escaped in rows too.

## HTMX and RPC endpoints

Decision (project owner, 2026-10-27): the pages use **HTMX**, and the server offers **RPC-style endpoints**.

**HTMX.** A click or a form sends a request; the server answers with an **HTML fragment** and HTMX swaps it into the page. There is no JavaScript application and no JSON between the page and the server for the administration interface. Fragments are templates of their own (`templates/fragments/`), rendered by the same engine and the same catalogs.

- `htmx.min.js` is **embedded in the program and served by us**. Nothing comes from a CDN.
- HTMX is configured in a `<meta name="htmx-config">`: no `eval`, no script tags in answers, requests only to our own origin.
- **Progressive enhancement:** every form is a normal `<form method="post" action="...">` that also works without JavaScript and then returns a whole page. With HTMX the same address returns only the fragment. The server tells them apart by the `HX-Request` header. This keeps the earlier rule that the pages work with JavaScript switched off.
- The CSRF token travels in a header set once on the body (`hx-headers`) and as a hidden field in plain forms.

**RPC endpoints.** Calls are named after what they do, not after a resource:

```
POST /admin/rpc/<noun>.<verb>      form-encoded input, HTML fragment as the answer
GET  /admin/fragments/<name>       read-only fragments (lists, dialogs)
```

| Procedure | Does |
|---|---|
| `user.create`, `user.rename`, `user.set_role`, `user.set_age_limit`, `user.set_device_limit`, `user.delete` | maintain users |
| `schedule.save` | store the weekly planner of a user (the grid sends its 672 cells) |
| `book.release`, `book.unrelease`, `book.set_exempt`, `book.set_recommendation` | release with an age rating, withdraw, quota-exempt flag |
| `book.grant_user`, `book.grant_role`, `book.revoke_grant` | exceptions |
| `pairing.create` | creates the one-time code and answers with the QR code fragment |
| `device.revoke`, `device.set_shared` | devices |
| `import.save_criteria`, `import.unlock`, `import.lock` | the release criteria and the import lock |
| `setting.confirm` | confirms a proposed value |

- State-changing procedures are **POST only**. Reads are GET fragments.
- **Typed errors map to answers.** The errors the data layer already knows become status codes with a readable fragment: `LastAdmin`, `DeviceLimitReached`, `ProfileLimitReached` give `409`; `ChildRequiresAgeAndSchedule` and `ReleaseRequiresAge` give `422`. HTMX is told to swap the error fragment into the dialog (`HX-Retarget` or a response with the status handled), so the user sees the reason where they clicked.
- A procedure can send `HX-Trigger` events, for example to close a dialog or refresh a list.
- Every procedure checks the **session role first** (admin) and the **step-up** rule for sensitive ones, then validates its input, then calls the `Store`. The data layer's rules (triggers) remain the last line of defence; the endpoint never trusts the form.
- Calls are **not idempotent**; a double click must not create two users. The submit button is disabled while a request runs (`hx-disabled-elt`), and creating a user with an existing name fails cleanly.

**Open:** whether the app uses the same naming with JSON bodies (one set of procedures, two kinds of answer) or a separate interface.

## How it is checked

- **Render tests** for every page and every role: an admin gets the page, a member and a child get `403`, an anonymous visitor is sent to sign-in.
- **No unescaped output**: a test creates a user named `<script>alert(1)</script>` and checks that it appears escaped on every page that lists users.
- **CSRF**: a POST without the token is refused; with a token from another session it is refused.
- **Languages**: each page renders in English and German, and an unsupported language gives English.
- **Planner**: saving the grid produces the same 84 bytes as `Schedule` in code (round trip).
- **QR**: the SVG decodes to the exact pairing address (checked with a QR reader library in the test).
- **Fragment or page**: the same procedure returns a fragment with the `HX-Request` header and a full page without it.
- **Error mapping**: each typed error of the data layer gives the documented status and a fragment with the reason.
- **Double submission**: sending `user.create` twice creates one user and one clean error.
- A browser check by hand on Linux, with JavaScript switched off and on, to confirm that nothing breaks.

## Open decisions

Decided: Jinja templates in HTML files (MiniJinja), HTMX, RPC-style endpoints.

1. Which HTTP framework serves it. `axum` is the natural choice (the project uses `tokio` and `tower`); not chosen yet.
2. Does the app use the same procedure names with JSON bodies, or a separate interface?
3. Does a `member` get a page for their own PIN and avatar, or is that only in the app?
4. Sign-in: PIN only, or name plus PIN? With up to 150 users a name list is not nice on a login page.
5. Is the administration interface reachable from outside the home network (through the tunnel) or only on the LAN?
6. Styles: a small hand-written stylesheet served by the server (assumed), or a framework?

## Wording: "confirm" (project owner, 2026-10-27)

"Confirm" does **not** apply to settings in general. In the project's vocabulary it means one thing: **imported files that are assigned to the rights system** (released with an age rating, exceptions, quota-exempt flag) are confirmed by an administrator. That is the release of books in the library, with the guard rail "release below the recommendation".

The proposed default values of `role_policy` and `setting` (column `confirmed = 0`, section 3a of the roles spec) are a different matter. The interface calls taking one over **"Accept"** (German "Übernehmen"), and the overview says "proposed values not accepted yet". The database column keeps its name `confirmed`; it is an internal flag and not what the owner means by confirming.

## Frame, menu and screens (2026-10-27)

- **A fixed frame in the middle:** the pages sit in a column of at most 72 rem, centered, on a backdrop; below that width the frame is the whole screen. The top bar is sticky.
- **Menu:** pill links with an icon (inline SVG, nothing loaded) and a label, the current page marked (`aria-current="page"`; a page says which with `{% set active = "..." %}`). Tablet: icons only, except the current page. Phone: the menu moves to the bottom edge. The user's picture and name (a link to the profile) and the sign-out button sit at the right.
- **Tables as cards on phones:** a table with class `stack` becomes one card per row; its cells show their `data-label`. The user list is the first.
- **Profile page** (`/profile`, every signed-in user of every role; title "Your profile", the user chip is its current menu entry; it is never the administrator's dashboard, whose heading is not used here). Top: "At a glance" (role, age limit only if there is one, devices against the limit, listening hours per week and "allowed now" for children, books in progress with a percentage bar, own paired devices). Below, "Settings" with one card and one primary **Save** per action: picture, names (nickname = shown name and sign-in name; first and last name optional), language (Automatic, English, Deutsch; takes effect at once and the message is shown in the new language) and PIN. The age or birthday is set by administrators only. Picture: PNG, JPEG or WebP, at most 256 KiB; the type is decided by the first bytes, never by name or header; SVG and GIF are refused; without a picture a lyre on a warm colour. The PIN needs the current PIN; wrong ones count towards the lock; other browser sessions end, paired devices keep theirs. Pictures are served to signed-in users at `/avatar/<id>` with `nosniff` and an ETag. The upload form carries its CSRF token in the address (`?csrf=`), because the body is not buffered. Everything is plain POST forms and works without JavaScript.

## Templates per distribution (project owner, 2026-10-27)

The templates are meant to be adapted **per distribution** (and per machine). They stay built into the program, so nothing needs to be installed, but a file of the same name in one of these folders replaces the built-in one, in this order:

1. the folder in `DAVID_TEMPLATES_DIR`,
2. `/etc/david/templates` (Windows: `%PROGRAMDATA%\David\templates`), for the machine's administrator,
3. `/usr/share/david/templates`, for the package of a distribution.

`static/admin.css` in those folders replaces the stylesheet. `david export-templates --to DIR` writes all built-in files as a starting point. A template that does not parse stops the start and names the file; the log lists the templates in use. A replaced template sees the same variables as the built-in one (Strict undefined), so a missing variable is an error, not an empty string. **Open:** catalogs of texts per distribution (the messages are still in the program), and whether a distribution's package should ship its templates under `/usr/share/david/templates`.

## Sign-in protection and "remember this device" (project owner, 2026-10-27)

- **A cookie protects the sign-in itself.** Before a session exists there is no session token to derive a CSRF token from, and a page of another site could otherwise sign a visitor in as an account of its choosing (login CSRF). The sign-in page sets `david_login` (random, `HttpOnly`, `SameSite=Strict`, `Secure` with TLS, `Path=/admin`, one hour). Its form carries a token derived from that cookie (`csrf`); the sign-in is refused with `403` before any PIN is tried, unless cookie and token belong together. A refused request does not count towards the PIN lock; the answer carries a fresh token so the user can try again at once. The cookie is used up (cleared) by a successful sign-in. A second tab reuses the same cookie.
- **Nothing the visitor typed is reflected** on the sign-in page: messages come from the catalog by key, the name is not echoed back. The templates escape everything else (`Strict` and auto-escape).
- **Remember this device:** a checkbox on the sign-in form. Ticked: the session cookie lives as long as the session of the role (30 days proposed) and the session ends then. Not ticked: a cookie of the browser session (gone when the browser closes) and a session that ends within 12 hours on the server as well.

## Popup menus, the Media menu and the planner grid (project owner, 2026-10-27)

- **The templates can do popup menus.** `templates/fragments/ui.html` has two macros: `icon(name)` and `popup(id, icon, label, current, extra)`, used as `{% call popup(...) %} ...items... {% endcall %}`. A popup is a `<details>` element, so it opens without JavaScript; `static/ui.js` adds closing on a click elsewhere and on Escape (focus goes back to the button) and keeps one popup open at a time. On phones the popups of the bottom bar open upwards across the screen. A distribution can replace `ui.html` to change menus everywhere.
- **Menus built with it:** the **user menu** (picture, name, role, Profile, Sign out) and the **Media menu** (Books, Music, Videos). Music and Videos are listed as "soon" and are not links: the kinds are designed (`spec-app-api.md`) but not built.
- **The logo is the link to the dashboard.** There is no separate "Dashboard" menu item; the logo is marked as the current page on the dashboard.
- **Weekly planner as a grid** (like the logon hours of Windows or the work-week grid of Outlook): a table of quarter hours (rows) by days (columns). Drag over cells to allow them; start on an allowed cell to block instead; shrinking the rectangle undoes. Click a day, an hour label or the corner to switch it as a whole. The check boxes remain the truth that is sent, so the page works without JavaScript (the boxes are then visible), and a keyboard can reach every cell (Space toggles). The allowed hours per week are counted live. On touch screens a tap toggles a cell; vertical dragging scrolls the page, so painting by touch works across days, not down the hours (**open**).
- **First administrator:** `david init` names the administrator **Papa** unless `--name` says otherwise.

## The audiobook page (project owner, 2026-10-27)

- The page and its menu item are called **Audiobooks** (German "Hörbücher"); it is the first entry of the Media menu, which will also hold Music and Videos.
- One row per book: a **cover thumbnail** (route `/admin/cover/<id>`, the same lookup as the apps' `/api/v1/items/<id>/cover`: `cover.jpg` in the folder, else the picture embedded in the first file), title with author, a **status badge** ("Released from 12" / "Not released", plus "Learning content"), the content ("12 discs, 200 tracks, 14 h 49 min") and the age that the medium recommends (information only).
- **One window per book** (button "Edit"): the box "Released", the age rating, the box "Learning content: not bound by the weekly planner" and, if the medium has a recommendation, a second box to release below it anyway. Cancel and Save. One procedure, `book.update`, checks everything first (an age is needed for a release; nothing changes if it is refused).
- The older procedures `book.release`, `book.unrelease` and `book.set_exempt` stay.
- **Errors in windows:** a refusal of the server (4xx) is shown **inside** the window (`.form-error`), because the message area of the page lies behind it. A form marked `data-close-window` closes its window on success, before the swap (the answer often replaces the form itself).
- **Lists have no visible "Actions" heading** (project owner: wrong label); the column keeps a label for screen readers only.

## One form, one Save; PIN or password (project owner, 2026-10-28)

- **The profile has one form with one Save** (and Cancel, which reloads the page): picture, nickname, first name, last name, language and PIN or password are saved together (`POST /profile?csrf=...`, multipart). Everything is checked before anything changes; a refused save changes nothing. The older single-purpose procedures (`/profile/pin`, `/profile/names`, `/profile/language`, `/profile/avatar`, `/profile/avatar.remove`) stay for the tests and for clients without the form. The same rule holds for every window: **one Cancel and one Save**.
- **Devices are shown once on the profile:** the card "Your devices" with the count in its title ("1 / 3"); no second figure. For administrators it links to the pairing page, where every device of every user is listed and managed.
- **A PIN is numbers, a password is alphanumeric.** A PIN has 4 to 12 digits. A password has 8 to 64 characters with at least one letter and one digit (spaces inside, symbols and umlauts are fine, control characters are not). The sign-in field is one field, "PIN or password", with the normal text keyboard; the same field is used to set one (profile, edit window, new-user window, `david init`). Anything that is neither is refused with the reason. A password and a PIN are stored the same way (Argon2) and are locked after wrong tries the same way. Children will usually have a PIN, adults may choose a password.

## Settings, import and pairing (2026-10-28)

- **Settings has three tabs** (`/admin/settings?tab=import|roles|values`, plain links, no JavaScript needed):
  *Import* (folders, rating rule, lock/unlock; the window "Folders and rule"), *Roles* (device limit and
  session length per role) and *Values* (sign-in, pairing and devices, guests, time zone).
- **Every value is a line edit** with its unit (minutes and days, not seconds). One form per tab, one
  Cancel/Save. Saving decides the values: the old "Accepted/Proposal" state and the per-row Accept buttons
  are gone (`setting.confirm` and `policy.confirm` were removed; `settings.save` and `policies.save`
  replace them). Input is checked before anything is written.
- **The Import page** only runs the import (and shows the folders and the report); setting up the folders and
  the lock lives in Settings, and the page links there.
- **The pairing code stays in the pairing window**: the form is replaced by the QR code and the address, and
  the foot becomes "Close" (out-of-band swap `#pair-foot`). Without JavaScript the page answers with the code
  below the form as before.

## Tags and cover, archive upload (2026-10-28)

- **Tags window** (button "Tags" in the book list): album, artist (written as artist and album artist), year, genre
  and a cover picture. Saving writes them into **every file** of the album (ID3v2 in MP3, Vorbis comments, MP4
  `ilst`); a field left empty stays as it is in the files. The picture comes from a file, from the clipboard
  ("Paste from clipboard" uses the async clipboard API where the page is a secure context and the user allows
  it; **Ctrl+V** inside the open window always works) and becomes `cover.jpg`/`cover.png` in the album folder
  and is embedded in every file. All files are located before anything is written. The title and author that David
  lists still come from the folder names (`Author/Title`), not from the tags; changing them is a rename of folders
  and is not done here.
- **Archive upload** (Import page): ZIP, TAR, TAR.GZ/TGZ, TAR.BZ2, RAR and 7z. The file is streamed to disk while it
  arrives (never held in memory), up to 32 GB, with a progress bar. A row in `upload_job` (migration 0011) puts it
  in a **queue**; one worker lists the archive (refusing more than 20 000 entries or 64 GB unpacked), unpacks it
  with `7z` (package `7zip` or `p7zip-full`, a recommended dependency of the deb), keeps only audio files and
  pictures, drops links, and moves every album into `<first import folder>/<Author>/<Title>/` (author and title from
  the tags, else the folder names), then runs the import if it is unlocked. States: waiting, unpacking, done,
  failed (with the reason). Jobs that were open at a restart are taken up again. Space is needed for the archive and
  its content at the same time. Not verified: a real RAR file (the tool here cannot make one); the 7-Zip build
  reads RAR and RAR5.

## Delete, automatic import, disk protection (2026-10-28)

- **Delete** (book window, bottom left): removes the book with its tracks, progress and release exceptions
  (database cascade) and authors without a book. The check box "Also delete the files from the disk" (on by
  default) deletes the audio files and cover files the player finds for the book, never anything else; a folder is
  removed only if it is then empty. Without it the files stay and the next import finds the book again. A native
  confirmation comes first.
- **Automatic import**: setting `import_interval_min` (default 10, 0 = never; Settings, Values, Library). The apps
  see changes through `GET /api/v1/library/version` (a hash of books, tracks, releases, blurbs) and reload.
- **Disk protection**: setting `min_free_gb` (default 2, 0 = off). An upload is refused before it travels if twice
  its announced size plus the reserve does not fit, and is stopped while it arrives if the disk runs below the
  reserve; unpacking checks the unpacked size on the staging disk (and on the import folder's disk if that is
  another one); writing a cover into every file of an album checks the bytes it adds.

## Folders: music, left-out folders, tolerant import (2026-10-28)

- **Two lists of folders** in "Folders and rule": audiobook folders (`Author/Title/files`) and music folders
  (`Artist/Album/files`). A folder is one or the other.
- **Left-out folders** are written into the list they belong to with a `!` in front, one per line
  (`!/srv/Musik/Audiobooks`); the syntax is explained in the window. They are stored per list
  (`import_excludes`, `music_excludes`), shown with a dashed "left out" line, and need to be absolute paths.
  Another import folder that lies inside a folder is left out of that folder's scan automatically, so
  `/srv/Musik` (music) with `/srv/Musik/Audiobooks` (audiobooks) needs no extra line, but the `!` line makes it explicit.
- **The import does not stop at a bad place:** a folder without read permission or a file with broken tags is
  skipped and listed in the report with the reason ("no permission to read it"); an album that cannot be stored
  (two files with the same track number) is skipped and counted. A renamed file keeps its track.
- **Upload** asks whether the archive holds audiobooks or music and unpacks it into the first folder of that kind.
- **Any depth** (2026-10-28, "deeper hierarchies are wanted, especially in families"): every folder that directly holds
  audio files is a book or an album, at any depth up to 8 levels: `Follett/Winter der Welt`, `NORA/Thea`,
  `JÖRG/Dania König/Versprochen`, `Musik/Rock/1998/Band/Album`. Title = the name of the folder, author or artist = the
  name of the folder above it; for a folder directly below the import folder the artist comes from the tags (else
  "Unknown"). The identity of a book is its path below the import folder, so existing two-level libraries keep
  their identities. Disc folders (`CD 1`, `Disc 2`, `Disk3`, `Teil 4`) of a folder without audio files of its own are
  one book with those discs. Hidden folders (`.recyclebin`), `@eaDir` and `lost+found` are never read.

## Visibility matrix in the media list (2026-10-29)

The lists "Audiobooks" and "Music" have a narrow column for every member of the family that is **no administrator** (administrators see everything) and a
checkbox in every row: ticked = the member sees the book. The mark is what the rule of the lists gives (`VISIBLE`: release, age, exceptions to the person or
the role), minus what is closed to the member (`book_block`, schema 0039). A change posts `book.visibility` (book, profile, visible) and draws the row again:
**opening** takes away a closing and, if release and age would not show the book, adds an exception for the member (`book_release`); **closing** takes away
that exception and closes the book to the member, whatever release, age or an exception of the role says. Every change is written to the change log
(`Visibility`, the member, `open`/`closed`). This is an administrator's decision and applies to that member's lists on every device; what a member hides for
themselves is another thing (`docs/spec-library-hiding.md`).

### Two kinds of columns, and the search (2026-10-29)

The matrix has **two groups of columns** with their own header: **Owner** (one box per row: whom the book belongs to; this is not access) and, under
**May see** (access), a checkbox for **every person**. Administrators are there too: they see everything **unless a box is cleared for them** (decision of the project owner, 2026-10-29: a couple that are both administrators must be able to keep their own things apart; without this an administrator would see everything whether they want it or not). The box of an administrator is ticked by default and can be cleared like any other; the administration pages themselves still list every book (they manage the library), only the lists of the apps follow the boxes. A change of the owner
posts `book.owner`; a change of a box posts `book.visibility`.

The **search** of the media lists (and `list_albums` of the MCP interface) knows more than plain words: `*` is any run of characters, `?` is one character, and
text between slashes is a regular expression (`/^der .* welt$/`, `/(bach|händel)/`); case is ignored. The text searched is title, author and narrator joined by
spaces. A broken expression finds nothing and says why. The search while typing in the browser (`static/ui.js`) follows the same rules as the server
(`server/src/search.rs`).

### Folders of a profile (schema 0041, 2026-10-29)

In the window of a profile (Users → Edit) every member has **a folder for every kind of medium**: audiobooks, music and pictures. Behind each input line there is a
button that opens a **dialog** with the folders; the dialog lists **only what the server can read, write and enter** (`access(2)` with `R_OK | W_OK | X_OK`, the
"0777" of the user the server runs as); hidden folders (a name with a leading dot) are left out. It starts with the places the server works in (the import
folders, the development library and the folders chosen already), so that the way is found even where the parents of those places are closed to the server;
one level up is offered only to a folder that is usable itself. "Use this folder" puts the path into the line. On saving the path is checked again (absolute,
an existing folder, usable, symbolic links resolved); a wrong one answers `422 err.folder` and nothing of the form changes; an empty line removes the folder; a form
that does not send the fields leaves them alone. The folders are stored in `profile_folder` (one per profile and kind). They are the ground of the personal
structure: the music and audiobooks of a member, the target of an import for them, and the folder of the picture sync (`docs/spec-photo-sync.md`).

### Collection folders as a list, with pictures and e-books (schema 0043, 2026-10-29)

Project owner: "the collection folders of the media must be manageable; pictures and e-books are missing". The import settings (Settings → Import → "Folders and
rule") no longer have text fields with `!` lines. The window has **a block for every kind of medium: audiobooks, music, pictures, e-books**. A block is a list of
rows; a row is the folder (an input with the button of the folder dialog inside it), a box **"Left out"** (the row then is an exception of its kind: it is read
nowhere below this block) and a button that removes the row. "Add a folder" fetches one more empty row (`GET /admin/rpc/import.row?kind=`, the number of the row is
the time in milliseconds, so that no row of the page has it). The form sends `rows=1`, `r_<kind>_<n>` (the path) and `x_<kind>_<n>=1` (left out); empty paths are
dropped. The older form with the lines `roots` and `music_roots` (`!/path` leaves out) is still understood.

Rules: a folder belongs to **one kind** (a second kind with the same folder is refused with the other kind named); the audiobook folders may be empty when
music folders are saved with them (before, one audiobook folder was always required); nothing at all is not a set of criteria. **Pictures and e-books are only kept
for now** (settings `photo_roots`, `photo_excludes`, `ebook_roots`, `ebook_excludes`): the import of e-books and the synchronisation of pictures
(`docs/spec-photo-sync.md`) follow. The folders of a profile get the fourth kind `ebook` (migration 0043 makes the table again, because a CHECK cannot be changed).

### Pages of the media lists (schema 0042, 2026-10-29)

The lists "Audiobooks" and "Music" come in **pages**. The books of the list (after the search) are cut into pages; the tree of folders/authors is built from the books of
the page. A bar above and below the list has the count (`21-25 of 25`), "previous" / "next", the numbers of the first, the last and the pages around the current one,
and a box for the **size of a page**. A page beyond the last shows the last, a nonsense number the first; the search runs over **all** pages (the server searches;
while a list is in pages the browser does not filter the rows while typing, because the rows of one page are not all there are) and the links carry it.
**The size of a page is set per browser, by cookie:** the box (or `?per=`, 10 to 500) sets the cookie `david_page_size` (`/admin`, a year, `HttpOnly`, `SameSite=Lax`);
without `?per=` the cookie decides, and without a valid cookie the setting `list_page_size` (default 50, 10 to 500) of the settings page.

