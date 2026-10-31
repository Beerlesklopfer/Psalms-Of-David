<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# CDs in the family: rights, approval, inbox, spoken messages

Status 2026-10-31: **phases 1 to 3 built** (rights, `cd_auto_approval`, the state machine, the held folder, approve / reject, `retry_wait`, `present`, expiry; `server/src/cdrights.rs`, `server/src/cdflow.rs`, migrations 0058, 0059 and 0060, `server/src/inbox.rs`, `server/src/api_inbox.rs`, tests `server/tests/cd_inbox.rs`, `tests/web.rs`, `tests/mcp_cd.rs`); phase 4 (the app) is built, see the end of this document; phases 5 and 6 are not built. Decided by the project owner in a conversation on 2026-10-31 (quotes in German are his).
Builds on `spec-cd-import-api.md` (jobs, the drive of the server, automatic import) and `spec-state-machines.md`.

## Why

"CD rein, Meldung, alles gut": automatic ripping exists so that people who are not at home with technology can put a CD in and be
done. Today the apps say nothing when something goes differently: a CD that is in the library already, a CD MusicBrainz does not
know, a track that cannot be read. That is only in the log of the administration. And in a Christian household a CD the parents do
not want must not reach the library just because someone put it into the drive ("Intrusion-Möglichkeit"). At the same time a
couple without children or a single person must not be slowed down by approvals they do not need: **it is a matter of settings.**

## Rights per person

Three rights on every profile (owner, 2026-10-31: "in die moderne Kultur muss das anpassbar sein"; the defaults are the careful
"alte Kultur", every household changes them):

| Right | Column | Means | Default |
|---|---|---|---|
| May put in CDs | `cd_may_insert` | may start a rip from the app or the web, and gets the messages of their own CDs | admins yes, others no |
| Needs no approval | `cd_no_approval` | a CD of theirs goes into the library when it is read; otherwise it waits for approval | admins yes, others no |
| May approve CDs of others | `cd_may_approve` | gets the tasks "CD waits for approval" and may decide them; gets the notices of CDs put in at the server | admins yes, others no |

- Children and guests never get the first two (the CD API refuses them already, `403`); `cd_may_approve` is refused for them too.
- An administrator can take rights from another administrator, but at least one profile keeps `cd_may_approve` (otherwise nobody
  could ever decide); the users page says so when it refuses.
- Shown on the users page of the administration as three tick boxes in the edit window ("CDs einlegen", "ohne Freigabe",
  "CDs anderer freigeben"), and in the MCP tool for profiles. Migration: the three columns, admins set to 1, everybody else 0.
- A member without `cd_may_insert` is treated like a child for CDs: no import symbol in the app, `403 forbidden` from `/cd/rip`.

**A CD put in at the server** (automatic import) belongs to nobody: the server cannot know who stands at the drive. A setting of the
tab "CD import" decides: `cd_auto_approval` = **wait for approval** (default) or **import at once** (for a single or a couple:
"Ehepaare ohne Kinder oder Singles"). With "wait", the task goes to everybody with `cd_may_approve`.

## The flow of one CD (state machine of a job)

One state machine per CD, i.e. per job (`spec-state-machines.md`: states are stored, the GUI only shows them, every change goes
through one function that checks the move and who may make it). The rights decide **who may make which move** and **who is told**.

```
                     ┌────────────── present ──────────────▶ PRESENT  (eject at once, notice; end)
 inserted / started ─┤
                     └─▶ RIPPING ──read error──▶ RETRY_WAIT ──retry──▶ RIPPING
                            │                        └──eject / give up──▶ CANCELLED (end)
                            ▼
                         RIPPED ──(no approval needed)──▶ IMPORTED (end)
                            │
                            └─▶ AWAITING_APPROVAL ──approve──▶ IMPORTED (end)
                                                  └──reject───▶ REJECTED  (files deleted; end)
```

| State | What the server does | The CD |
|---|---|---|
| `present` | the Disc ID (or author, title and CD number) shows the CD is whole in the library: nothing is read | **ejected at once** (owner: "eine CD, die schon vorhanden ist, wird gleich wieder ejected"); notice "already in the library" with a link to the album |
| `ripping` | reads, verifies, encodes as today | in the drive |
| `retry_wait` | a track could not be read (`read`, `verify`); what was read stays in the resume folder | **stays in the drive** (owner: "das Fehler auswerfen macht keinen Sinn"), so it can be read again without putting it in again; notice with "try again". The existing eject button of the app takes it out for cleaning |
| `ripped` | all chosen tracks are files in the job folder; decides at once between `imported` and `awaiting_approval` | ejected (as `cd_eject_after` says) |
| `awaiting_approval` | the files wait in `<data dir>/cd-import/held-<job>/`, **outside the library**: nobody sees them in any list, no app, no MCP tool other than the approval ones | out |
| `imported` | the files are placed and imported as today (`finalize`); the new album is released or not by the import criteria as every other album | out |
| `rejected` | the held files are deleted; the job log keeps the decision (who, when) | out |
| `cancelled` | as today | out |

- Ripping starts at once, also when an approval will be needed (owner: rip, then wait locked). A parent on a business trip may
  decide two weeks later without the CD lying in the drive.
- **Who decides `ripped`:** the starter's `cd_no_approval` for a rip from an app or the web; the setting `cd_auto_approval` for
  a CD put in at the server.
- **Moves and who may make them:** `retry` (from `retry_wait`) by the starter or anyone with `cd_may_approve`; `give up` by the
  same (the app's eject also gives up); `approve` / `reject` only by someone with `cd_may_approve`. Everything else is the
  server's own move. A move that is not allowed answers `409 invalid_move` or `403 forbidden`.
- A held CD expires after `cd_held_keep_days` (default 60, 1..365): it becomes `rejected` with the reason `expired`, and the people
  who could have decided get a notice. (Decided by the owner on 2026-10-31: it expires, after 60 days by default; there is no "never".)
- Approval does not replace the release of albums for children (`book_release`, import criteria): it only decides whether the
  CD enters the library at all.

## The inbox

Messages are kept on the server, not only sent (owner: "ein Elternteil ist im Urlaub oder auf Dienstreise und bekommt die ganzen
Meldungen nicht mit"). The app fetches the inbox when it opens, however long someone was away.

- **Notices** ("12 tracks imported", "already in the library", "track 7 cannot be read", "Papa approved the CD"): every recipient
  has their own read mark; a notice stays until it is read (and is removed after `inbox_keep_days`, default 90).
- **Tasks** ("new CD: *Paulus*; import?"): one decision for everybody. When one person decides, the task is done for all, and the
  others see a notice "Papa approved the CD *Paulus*" instead.
- Recipients: the starter of a rip (their own CDs) and everybody with `cd_may_approve` (all tasks; the notices of CDs put in at the
  server and of CDs of people who need approval). Children and guests receive nothing.
- **The starter sees their own held CD** (decided on 2026-10-31): a member with `cd_may_insert` but without `cd_no_approval` finds the CD in
  the app as an entry of the job, "waits for approval", with author, title, the number of tracks and since when, **without the buttons**
  (only someone with `cd_may_approve` decides). It is an entry about the job, not an album: the files stay out of every list of the
  library, of the search, of streams, of MCP answers and of OPDS feeds until approved (see Tests), and the entry disappears when the CD is
  approved (then it is an album), rejected or expired (then the starter gets the notice `cd_expired` or the decision notice).

Tables (migration after 0057): `inbox_item(id, kind 'notice'|'task', topic 'cd', job_id, code, args JSON, created_at, resolved_at,
resolved_by, resolution)`, `inbox_recipient(item_id, profile_id, read_at)`, both with the audit fields (0028). `code` is one of the
message keys below; the apps make the text in their language from `code` and `args` (author, title, CD n of m, tracks, track
numbers, who decided).

| Code | Kind | Args |
|---|---|---|
| `cd_present` | notice | album id, author, title, disc |
| `cd_imported` | notice | album id, author, title, disc, tracks |
| `cd_read_error` | notice (with "try again" while the job is in `retry_wait`) | job id, tracks that failed |
| `cd_no_match`, `cd_ambiguous` | notice (the CD is not read; the web panel chooses) | disc id, candidates |
| `cd_awaiting_approval` | task | job id, author, title, disc, tracks, who put it in (`auto` for the drive) |
| `cd_decided` | notice | job id, approved or rejected, by whom |
| `cd_expired` | notice | job id |

API (`/api/v1`, bearer token; everybody, the list is empty for children and guests):

| Request | Answer |
|---|---|
| `GET /inbox` | `{"items":[{id,kind,code,args,created_ms,read,resolved?,resolution?,by?}],"unread":n,"tasks_open":n}`, newest first |
| `POST /inbox/{id}/read` | `204` |
| `POST /inbox/read_all` | `204` |
| ~~`POST /inbox/{id}/decide`~~ | **not built, on purpose:** a task is decided through `POST /cd/jobs/{job_id}/approve` or `/reject` (phase 2), the one way; the slower decider gets `409 decided` with `{by, resolution}` |
| `POST /cd/jobs/{id}/retry` | `202`; from `retry_wait` only (`409 invalid_move`) |

The exact shapes (item, paging, WebSocket messages) are in `spec-app-api.md` ("The inbox of the CDs").

## Delivery

- **While the app runs** (open, or playing in the background): the server sends the message over the WebSocket the app has open
  already (`/cd/events`, new types `inbox` and `inbox_resolved`); the app shows it as a system notification with the button of the
  task ("Import" / "Reject") or "Open album". No outside service.
- **When the app is closed:** nothing is pushed. The next start shows "1 CD waits for approval" (badge on the profile and a card at
  the top of the library). Real push (APNs, UnifiedPush/ntfy) is on the backlog: the owner decided it is not needed now, because a
  CD waits locked anyway ("das genügt, denn die CD bleibt in Familien gesperrt").
- The channel `/cd/events` is open to Admin and Member today; that stays, and a member without any CD right only gets an empty
  inbox. (When other topics use the inbox, it moves to a general event channel.)

## Spoken messages

While something plays, a voice says what happened (owner: "ein Elternteil hört Musik. Die CD wird fertig. Dann sollte eine Stimme
genau das auditiv ankündigen"): the music is ducked, the device's own speech engine (Android TextToSpeech, iOS AVSpeechSynthesizer,
on Linux speech-dispatcher if present) says e.g. "Die CD Paulus ist fertig und wartet auf deine Freigabe.", the music comes back.

Which device speaks:

| The rip was started | Speaks |
|---|---|
| from an app | **that device** (owner: "wenn das Rippen von dem Handy aus bedient wurde, ist das das Gate") |
| at the server (CD put in) | the devices of people with `cd_may_approve` **that are playing now and are on the home network**: whoever put the CD in is at home; the parent away is not interrupted and gets the silent message (owner: "Rippen dauert bis zu einer halben Stunde", so the finished message is needed here too) |

- The server marks each message with `speak: "starter" | "home"` and the starter's device; the app decides the rest: it speaks
  only when it is playing (otherwise a normal notification) and, for `home`, only when it reaches the server on its LAN address
  (today always, because the WireGuard tunnel is not built; then: not over the tunnel).
- What is spoken: `cd_imported`, `cd_awaiting_approval`, `cd_read_error`, `cd_present` for an app-started rip; not the notices of
  decisions.
- A switch in the app settings ("Ansagen zu CDs", on by default for people with a CD right) and the system's own volume. A child's
  device never speaks: children receive nothing.
- Later the voice can come from Eliezer (`/v1/audio/speech`); the device's own voice is first because it needs no server and no
  waiting.

## What changes in what exists

- `cd_auto_import`: the `skipped` reasons that a person should know about become inbox notices to the approvers (`disc_present`
  with eject, `no_match`, `ambiguous_match`, `heal_needed`, `no_space`, `no_target`); the log of the tab stays as it is.
- `POST /cd/rip`: the starter needs `cd_may_insert`; `disc_present` ejects as the automatic path does when the request says
  `"eject": true` (the app sets it); the job has the new states.
- `read` / `verify` failures go to `retry_wait` instead of `failed`; the resume folder stays (as today) and `retry` uses it.
- The web panel `/admin/cd` shows held CDs with Approve / Reject (for `cd_may_approve`), and the job log the decisions.
- MCP: `cd_progress` lists held jobs; approving stays a thing for people, not for an LLM (no MCP tool decides).

## Tests

- State machine: every allowed move, every refused move (`invalid_move`), each right (`forbidden`), the faster of two deciders
  (`409 decided`), expiry, a restart in every state (held files survive, `ripping` becomes `retry_wait` with the resume folder).
- Held files are in no list, stream, search, MCP answer or OPDS feed until approved; rejected ones are deleted.
- The starter's own held CD is shown as a job entry without buttons (with title, author, number of tracks), and still gives no track, no stream and
  no search hit; another member without a CD right sees nothing of it; the entry goes away on approve, reject and expiry.
- Inbox: recipients by rights, read marks per person, a task resolved for all with the notice to the others, children and guests
  get nothing, the WebSocket messages, the inbox after a reconnect.
- Present CD: ejected, nothing read (the fake drive counts the reads), notice with the album.
- App: the card and badge, the notification buttons, speaking only while playing and only on the starter / at home (fake speech
  engine), the switch.

## Phases

1. **Done (2026-10-31).** Rights (columns, users page, API checks) and `cd_auto_approval`.
2. **Done (2026-10-31).** State machine with `awaiting_approval`, held folder, approve / reject, `retry_wait` / retry, present → eject.
3. **Done (2026-10-31).** Inbox (tables, API, WebSocket types), notices from the automatic import.
4. **Done (2026-10-31).** App: inbox card, badge, notifications with buttons, decide and retry.
5. Spoken messages.
6. Backlog: real push (APNs relay, UnifiedPush / ntfy); voice from Eliezer.

## Decided

- A held CD expires after `cd_held_keep_days` (default 60 days, 1 to 365); there is no "wait for ever". On expiry the files are deleted, the job is
  `rejected` with the reason `expired`, and the approvers and the starter get the notice `cd_expired` (owner, 2026-10-31).
- The starter sees their own held CD in the app as "waits for approval", without the buttons (owner, 2026-10-31).

## What phases 1 and 2 built, and what they leave for phase 3

Specified in `spec-cd-import-api.md` ("States of a job"), `spec-admin-ui.md` and `spec-app-api.md` (the interface of the app). Decisions taken while building, where the draft was silent:

- The state is a column of the job (`cd_job.cd_state`) besides the old `phase`; `failed` (a failure no new try mends) and `skipped` (a log entry of the automatic import) exist besides the states of the draft. Only `cdflow::Store::cd_move` changes it.
- **Approve** moves `awaiting_approval` -> `ripped` (the decision is stored: who, when) and the placing then moves it to `imported`; two deciders cannot both succeed (`409 decided`). A CD that could not be placed after an approval waits again.
- A CD from `POST /cd/import` (an upload by an app) is held like a rip of the server when its starter needs approval.
- `retry` needs the request of the rip, which is stored with the job (`cd_job.request`); a job of an upload cannot be retried.
- The rights are columns of the profile with triggers (a child or guest never holds one; the last `cd_may_approve` cannot be taken or deleted); a profile that becomes an administrator gets all three.
- No MCP tool changes the rights or decides a CD; `list_profiles` only shows the rights.

**Hooks for phase 3** (all in `server/src/cdflow.rs`): `pub enum CdEvent { Present, Imported, ReadError { tracks }, AwaitingApproval, Decided { approved, by }, Expired, NotStarted { reason } }` and `pub fn notify(event: &CdEvent)`, which does nothing yet. Every place that makes one of these events already calls it: `finish_present` (`Present`, also for the automatic path), `conclude` (`Imported`), the end of a rip (`ReadError` for `retry_wait`), `hold` (`AwaitingApproval`), `approve` / `reject` (`Decided`), `expire_held` (`Expired`), `log_skipped` (`NotStarted` for `no_match` and `ambiguous_match`). `Store::cd_approvers()` lists the profiles a task goes to; `Store::cd_rights(profile)` the recipients by right; the starter is `Job::user_id` (0 = the drive). `cdflow::recover` and `expire_held(store, cd, now_ms)` are the entry points of restart and expiry. (Phase 3 turned `notify` into `async fn notify(store, cd, &CdEvent)`, which calls `inbox::handle`.)

## Open

Nothing at the moment. (OPDS feeds are named in the draft's tests; the server has no OPDS feed yet, so there is nothing to hide from.)

## What phase 3 built, and what it leaves for phases 4 and 5

- Tables (migration 0060): `inbox_item(id, kind, topic 'cd', job_id, code, args JSON, speak, device, resolved_at, resolved_by, resolved_name, resolution)` and `inbox_recipient(item_id, profile_id, read_at, decide)`, both with the audit fields (0028; `created_at` is the time of the item); `cd_job.starter_device`; the setting `inbox_keep_days` (90, 1 to 3650; the settings page, group "library"). Code: `server/src/inbox.rs` (`inbox::handle` is the one place that turns a `CdEvent` into items; `Store::inbox_*`), `server/src/api_inbox.rs`.
- Cleanup: the periodic round of `cdflow::expiry_task` (every ten minutes, also expires held CDs) removes the items older than `inbox_keep_days`. **A task nobody decided is never removed** (a held CD may wait longer than the inbox keeps notices: `cd_held_keep_days` goes up to 365).
- Decisions where the draft was silent: (1) the starter's entry of a held CD is **the same task item**, with `decide = 0` in his row of `inbox_recipient` (API: `can_decide: false`), so that one task stays one decision; when it is decided, it disappears from his list and the decision notice (`cd_decided`, or `cd_expired`) takes its place. (2) A decided task is marked read for everybody (nothing is left to do). (3) The decider gets no notice about their own decision. (4) `cd_imported` and `cd_read_error` go to the starter, and also to the approvers when the CD was put in at the server or the starter needs approval; an approved CD gives `cd_decided` instead of `cd_imported`. (5) The skipped reasons `no_match`, `ambiguous_match`, `heal_needed`, `no_space`, `no_target` give `cd_no_match`, `cd_ambiguous`, `cd_heal_needed`, `cd_no_space`, `cd_no_target`; `disc_present` of the automatic import is `cd_present` (the job `present`); the other reasons (`busy`, `drive_denied`, `udev_off`, ...) stay in the log only. (6) Without any CD right (neither `cd_may_insert` nor `cd_may_approve`) the inbox is always empty, whatever it held before. (7) Only these four codes may be spoken, `speak` is set for them only.
- **The starter's device.** Devices are identified by `cdimport::device_label(token)`, the `device` of the `hello` message. `POST /cd/rip` and `POST /cd/import` store it in `cd_job.starter_device`; a rip started in the web panel, by MCP or at the drive has none, so its messages carry `speak: "home"` and `device: ""`. The body of a request cannot set it.
- Left for phase 4: the card, the badge, the notifications with buttons (against `spec-app-api.md`); for phase 5: speaking by `speak` / `device`, "playing" and "on the home network" (the server knows neither).

## What phase 4 built (the app), and what it leaves for phase 5

There is no separate document for the screens of the app; they are described here. Code: `app/lib/inbox/` (`inbox_models.dart`, `inbox_api.dart`, `inbox_machine.dart`, `inbox_controller.dart`, `inbox_text.dart`, `inbox_notifier.dart`, `inbox_ui.dart`, `inbox_demo.dart`); the event channel is the existing `/cd/events` (`app/lib/cd/cd_events.dart` parses the new types); tests `app/test/inbox_*_test.dart`.

- **State machine** (`inbox_machine.dart`, pure Dart: `inboxTransition(state, event) -> (state, effects)`): phases `idle`, `loading`, `ready`, `refreshing`, `error`; the list, the counts of the whole inbox (`unread`, `tasks_open`), `deciding` and `retrying` per item, `failures` per item (`forbidden`, `network`, `busy`, `decided` ...). `InboxController` is the shell that carries out the effects against `ServerInboxApi` and feeds the answers back; the pages only show the state. Nothing is replayed after a reconnect: every `snapshot` (it follows each connect) sets the counts and makes the machine read `GET /inbox` again. An `inbox` message that arrives while a page is on its way is kept if the page is older than it.
- **Decision**: `approve` / `reject` go to `POST /cd/jobs/{job_id}/...` with the `job_id` of the item. The item shows "deciding" (buttons off) until the response or the message `inbox_resolved` settles it. `409 decided` gives "Already decided by <name>" with the server's `by` and `resolution`; `403 forbidden` removes the buttons; `409 invalid_move` and `404` read the list again.
- **Card** (top of the library, under the progress strip): "N CDs wait for approval" and "N unread"; only when there is something and the profile has `cd_rights.may_insert` or `may_approve` (from `GET /me`, kept in the offline copy of the profile). A child, a guest or a member without a CD right never sees it, whatever the server sends.
- **Badge**: the unread count on the avatar of the profile entry (settings) and on the avatar of the start page; an entry "CD inbox" in the settings under the profile opens the page.
- **Page** "CD inbox": newest first, a card per message, text from `code` and `args` (an l10n key per code; an unknown code shows a generic text; `knownInboxCodes` is checked against the table of `spec-app-api.md` by a test). Tasks with `can_decide` have Import and Reject; Reject asks "Reject this CD? Its files are deleted." (`confirmRemoval`, as other destructive actions). The starter's own held CD (`can_decide: false`) reads "Waits for approval" without buttons. "Try again" only while `job_state` is `retry_wait`. "Open album" only when `album_id` is not null (and says so when the album is not in the library of the device yet). A tap on a message marks it read; "Mark all read" in the bar; pull to refresh; the list loads the next page near its end (`next`). Offline mode: "The inbox needs the server" instead of the list.
- **Demo mode**: an example inbox (a task, a notice, an expired CD; `inbox_demo.dart`); reads and decisions succeed in memory.
- **System notification** while the app runs: `InboxController.onInboxMessage` calls the notifier with the text and, for a task with `can_decide`, the buttons Import / Reject, for `retry_wait` Try again. On **Linux** this is the desktop notification over D-Bus (`org.freedesktop.Notifications`, with actions if the desktop supports them). **Other platforms have no mechanism in the app yet** (no notification plugin is a dependency, and the channel `com.the-abraham-box.psalm/notifications` only asks for the permission): `NoInboxNotifier`, the card and badge still work. The button Reject of a notification does not delete: it opens the inbox page, which asks first. A tap opens the page. A notification goes away when the task is decided or read. When the app is closed nothing is pushed.
- **The seam for phase 5**: `InboxController.messageHandlers` (a list of `void Function(InboxItem item, String? speak, String? device)`) and `onInboxMessage(item, speak, device)`, called once for every message that arrives over the channel and is new to the list (not for the list read at start). Today the one handler is the notification; speaking adds a handler (the item has `speak` and `device`).
- Interpretations: opening the page reads the list again but marks nothing read (a tap on an item does, and "Mark all read"); a task decided by the person counts as read at once (as the server does).
- **Import dialog, CD already in the library** (`app/lib/cd/cd_import_controller.dart`, `CdEjectPhase`; owner, 2026-10-31): the dialog is reached by the drive state `present` (the lookup answers it) or by `409 disc_present` of a rip. A CD that is **whole** (`present.complete`) is ejected at once by the controller (`POST /cd/eject` with the drive's `device`, once); the dialog says "The CD was ejected." with one Close button and no "Import anyway". If the server ejected it already (`ejected: true` of the 409) there is no second eject. If the eject fails, the error shows and Eject, Import anyway and Cancel are offered. A CD with **gaps** stays in the drive (the server may repair it): Eject, Cancel, Import anyway and Check and repair; Eject ejects and shows the message. The rip request keeps `eject` from the checkbox "Eject after import"; the app does not force it, and ejects a whole CD itself when the server did not.
- **Job kinds in the app** (`CdJobState.kind`, `app/lib/cd/cd_events.dart`; fix of 2026-10-31): the state that counts is `cd_state`, else the old `state`. Running = `ripping`, `working`, `ripped` (or `imported` while the old `state` still says `working`) with `active` not false; waiting for a person = `awaiting_approval`, `retry_wait` (a line without a bar); ended = everything else, an unknown state too (`present`, `skipped`, `cancelled`, `rejected`, `imported`, `done`, `failed`). The strip and the dialog show the newest running job; an ended job gets a result only if the app saw it running.
- **Badge as a button** (owner, 2026-10-31: "Hier würde ich den Zugriff auf das Protokoll erwarten"): the profile avatar already opens the profile (start page and settings), so it keeps that. The unread badge on it (`InboxBadgeOverlay`, `app/lib/inbox/inbox_ui.dart`) is its own button of 40x40 dp that opens the CD inbox page (label "CD inbox, N unread", tooltip, click cursor, focus with Enter / Space); back returns to the page it came from. The entry "CD inbox" in the settings (with "N unread") is always there for a profile with a CD right, also with 0 unread; the empty page says "No messages about your CDs." No badge and no entry without a CD right.
- **Import dialog, read error** (`retry_wait`): next to the text "Read error: the CD is still in the drive" the dialog has "Try again" (`POST /cd/jobs/{id}/retry`) and "Give up" (`POST /cd/jobs/{id}/give_up`, after a question "Give up on this CD?"), both through the inbox client; the controller keeps `movePhase` (`retrying`, `givingUp`; the button is off with a spinner until the server's next message moves the job) and `moveError` (`forbidden`, `invalid_move`, ... shown with the usual texts). The app does not know the starter of a job, so the buttons show for everyone and the server answers `403`.
