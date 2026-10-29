# Spec: discovery, profiles and roles

Status: the **data model and its rules are built** (migration `0003`, `server/`), with tests. **Not built yet:** mDNS announcement and discovery, QR pairing and enrollment, sessions and the HTTP API, the PIN, enforcement of the planner in the streaming layer (the decision function `Store::access` exists, nothing calls it yet), the import unlock through criteria, reading the age recommendation from tags, and the apps. Open decisions are listed at the end and marked **open** in the text.

What migration `0003` contains, and which of its values are proposals, is described in section 5. Since then built: sign-in and sessions for the administration interface (migration `0004`), the import criteria and unlock (3c), the planner editor, creating and redeeming pairing codes with QR display (4), device limits at both points. Also built: the mDNS announcement and a browse function (section 1), verified with `avahi-browse`. Also built: TLS with a self-signed certificate and the fingerprint in the pairing code (`fp`), verified with a pinning client and with `openssl`. Still not built: the app-side discovery, the app-facing API and device sessions, the WireGuard tunnel, TLS and the certificate fingerprint in the QR code.

## Requirements (project owner, 2026-10-27)

1. Client and server both use **mDNS** to find each other.
2. The **profile is chosen in the client** and **maintained by the server**.
3. In a family, **roles act as credentials**.

## 1. Discovery (mDNS / DNS-SD)

- Service type: `_david._tcp`. Instance name: the display name of the server, for example `David`. Port: the API port.
- TXT records (no personal data, nothing about people or library content):

  | Key | Value |
  |---|---|
  | `v` | protocol version, `1` |
  | `id` | server id, a random UUID created at first start and stored in the database |
  | `name` | display name chosen by the family |
  | `api` | API base path, `/v1` |

- **Server**: the `mdns-sd` crate (pure Rust, no async runtime dependency, runs on Linux and Windows) registers the service, re-registers when network interfaces change, and sends a goodbye on shutdown.
- **Client**: browses `_david._tcp.local.`, shows the servers found, and remembers the `id` of the chosen one. On the next start it looks the `id` up again, because the address may have changed. Dart candidates: `multicast_dns` (pure Dart) or `nsd` (uses the platform service). **Open**: which one works on all five platforms.
- Platform requirements for the client:
  - Android: a multicast lock and the `CHANGE_WIFI_MULTICAST_STATE` permission.
  - iOS: `NSBonjourServices` containing `_david._tcp` and a text for the local network permission in `Info.plist`.
  - Linux: avahi may already be running; the client must coexist with it (shared port 5353).
  - Windows: works through the system multicast stack; check inside the VM, which is bridged.
- mDNS is **not authenticated** and **does not cross routers or the WireGuard tunnel**. Therefore:
  - Discovery only produces *candidates*. Trust comes from the identity pinned at enrollment (section 4), not from the announcement.
  - Away from home the client uses the stored address from enrollment (the tunnel address), not mDNS.
  - A manual address and a QR code from enrollment remain as fallbacks.

## 2. Profiles: who owns what

The server is the single source of truth.

| Data | Lives on | Notes |
|---|---|---|
| Name, avatar, role, age limit, PIN hash | server | never leaves the server except name, avatar, role and "has PIN" |
| Which server (`id`) and which profile this device uses | client | local settings |
| Cached profile list | client | for showing the picker offline; refreshed on every connection |
| Session token | client, in secure storage | Android Keystore, iOS Keychain, Linux Secret Service, Windows Credential Manager |
| PIN | nowhere on the client | typed, sent once, never stored |

Flow: find server, enroll the device once, fetch the profile list, pick a profile (PIN if one is set), the server issues a session token, the client uses it for all calls.

Maintenance: only admins create, rename and delete profiles, change roles and age limits and reset PINs. Anyone may change their own avatar and PIN, except children. A command line on the server (`david profile ...`) is the way back in when no admin client is available. At most 150 profiles. At least one admin must always exist; the database refuses to delete or demote the last one.

## 3. Users, roles and devices

Decision (project owner, 2026-10-27): **roles belong to users, and devices belong to users.** A device has no role of its own.

In this document a *user* is a person with a profile. The database table keeps the name `profile`; renaming it to `user` is possible but touches the entities, queries, tests and the trigger, so it is not part of this step (**open**).

```
user (role)  1 ----- n  device  (owned by exactly one user)
```

**Roles.** A fixed set, with the permissions defined in code (simple to test, enough for a family). Configurable roles can come later (**open**).

| Role | For | May |
|---|---|---|
| `admin` | the people who run it | everything: users, roles, devices and enrollment codes, library, age ratings; and listen |
| `member` | other adults | listen, favorites, own progress, change own PIN and avatar |
| `child` | children | listen within the age limit; no management; PIN optional |
| `guest` | visitors | listen; whether progress is kept and when the user expires is **open** |

**Devices.**

- A device is enrolled **for one user** and belongs to them. Its keys are revoked when the user is deleted.
- A personal device acts as its owner: the session is the owner's, with the owner's role. There is nothing to pick.
- A device can be marked **shared** by an admin (for example the family tablet). On a shared device the client shows the user picker. Whoever is picked proves themselves with their PIN (a child user without PIN needs none), and the session role is the **lower** of the picked user's role and the device owner's role. A tablet owned by a child therefore never opens an admin session, even if someone knows a parent's PIN. No extra field is needed for that: the ceiling is the owner's role.
- A user may own several devices. How many is not limited beyond the limit of 150 users.

**Credentials.**

- The *device* credential (enrollment, section 4) says this device may talk to the server and who owns it.
- The *PIN* proves a person where a PIN is required: on a shared device, and for admin actions.
- The session token holds `user_id` (the picked user), `device_id`, the effective `role` and an expiry.
- **Step-up for admin actions**: managing users or devices needs a recent admin session, or the PIN again.
- **Enforcement is on the server**, on every call, from the role in the session. Hiding menus in the client is only convenience.

## 3a. Limits and times

Decision (project owner, 2026-10-27): `max_devices` and times are defined. The values below are **proposals** to be confirmed.

**Device limit.** Every user has `max_devices`: how many active (not revoked) devices they may own. The role gives the default, a user may override it. The server refuses the enrollment that would exceed it with the error `device_limit_reached`. A trigger in the database enforces it, like the limit of 150 users, so no code path can bypass it. A revoked device frees its place.

**Times, in two groups.**

*Validity times* (proposed, per role in a policy table):

| Role | `max_devices` | Session lifetime | Notes |
|---|---|---|---|
| `admin` | 5 | 30 days | admin actions need a session younger than the step-up window or the PIN again |
| `member` | 5 | 30 days | |
| `child` | 3 | 90 days | long, so children are not asked for anything |
| `guest` | 1 | 24 hours | the user itself expires after 7 days |

Server-wide: step-up window 10 minutes; enrollment code valid for 15 minutes and usable once; a device not seen for 180 days is marked inactive (its place stays taken until an admin revokes it, **open**).

*Listening times for children* (decided, 2026-10-27): children have **fixed times**, kept in a **weekly planner with 15-minute slots**.

- A week has 7 days of 96 slots, 672 in total. A slot is either allowed or not. The planner is stored as a bitmap of 84 bytes (`profile.schedule`), which maps one to one onto a grid in the app: seven columns, 96 rows, one tap per slot.
- `NULL` means no restriction. A user with role `child` must have a schedule (the database refuses a child without one); other roles may have one if an admin wants it.
- The **server decides**, because a client can be changed. It evaluates "now" in the server's time zone (a setting) and refuses to start or continue a stream outside an allowed slot. The answer carries the reason and the next allowed start, so the app can say "listening is possible from 07:00".
- The app shows the planner and warns shortly before a window ends; it does not enforce anything.
- Daylight saving time: the planner is wall-clock time. On the day of a change the slots simply follow the clock.
- The earlier idea of a daily limit in minutes is dropped in favor of the planner.

*Content outside the quota* (decided, 2026-10-27): free, unlimited content, meant for learning (*Lerninhalte*), is **not** subject to the planner. A child may listen to it at any time.

- A book carries a flag `quota_exempt`. An admin sets it per book. As a convenience the scanner can set it for every book under a configured library folder (for example `Lernen/`); **open** whether that convention is wanted.
- The check on the server becomes: a stream is allowed if the current slot is allowed **or** the book is `quota_exempt`.
- The flag lifts only the time quota. Age ratings (`min_age`), role permissions and visibility still apply. Exempt content is not a back door past them.
- Outside the allowed window the app shows only the exempt content, with a note such as "right now only learning content is available". The server's answer for a refused stream names the next allowed start, as before.
- Progress is still kept for exempt content like for any other book.

**Where the values live.** A table `role_policy` holds the defaults per role (roles stay a fixed set in code, their parameters are data). A user's own `max_devices` overrides the role default when it is set. Server-wide times sit in a small `setting` table. Changing a value needs no new release.

## 3b. Visibility: only released content, with an age rating

Decisions (project owner, 2026-10-27): **only released content is shown**, and **an age rating (Altersfreigabe) is required.**

The rule is default-deny. Releasing a book means giving it an age rating.

- A newly scanned book starts **unreleased**. Nobody except an admin sees it. Admins see everything, with unreleased books marked as new, and the app tells them how many books wait for a release.
- **Releasing = assigning an age rating** (`book.min_age`). The app offers the usual steps: all ages (0), 6, 12, 16, 18. The database refuses a released book without a rating.
- Every child user has an age limit (`profile.max_age`, required for role `child`). A released book is visible to a user if the rating is **within** the user's limit. Users without a limit (adults) see all released books.
- **Exceptions on top**, for the cases where an age is not the whole story: an admin can release a book to **one user** or to **a role** regardless of the rating ("Anna may hear this 12+ book, she is 10"). Exceptions only add; they never hide a book that the age rule shows.
- Content outside the quota (section 3a) must be released and rated like everything else. The flag lifts only the time quota.
- **Enforced everywhere on the server, not just in lists.** The check applies to the book list, search, "continue listening", covers and above all the stream itself. A direct address of a book the user may not see answers as if it did not exist.
- Withdrawing a release or lowering a rating hides the book at once; the next request is refused. Listening progress is kept.
- Changing a user's limit changes what they see immediately.

**Two different ages** (decision of 2026-10-27: media have an age recommendation):

| | Altersempfehlung (recommendation) | Altersfreigabe (release age) |
|---|---|---|
| Whose | the medium's own, from its source | the family's decision |
| Column | `book.recommended_age`, may be empty | `book.min_age`, set when releasing |
| Purpose | information and starting value | decides who may see the book |
| Enforced | never | always |

- The recommendation is a property of the medium. It is read when the library is scanned, where the source has one, and an admin can correct it. It is shown in the app ("recommended from 8").
- When an admin releases a book, the dialog starts with the recommendation as the release age, so a normal release is one tap. The admin may choose another age.
- **Guard rail:** releasing below the recommendation (recommended 12, released at 6) asks the admin to confirm. The release age wins, because the family decides. A book without a recommendation can be released at any age.
- Where the recommendation comes from is **open**. Audio tags have no standard field for it. Candidates: a custom tag (for example `TXXX:AGE_RECOMMENDATION` in ID3), a small sidecar file in the book folder, or entry by hand. Nothing is looked up on the internet.

Rule in one sentence:

```
visible(user, book) = user is admin
    OR (book.released AND (user.max_age IS NULL OR book.min_age <= user.max_age))
    OR an exception exists for the user or the user's role
```

**Open:** should a child's limit be a fixed number (`max_age`) or a **birth date** from which the server computes the age, so the limit rises on its own every birthday? A birth date is more convenient and means storing a personal date; a fixed number is simpler.

## 3c. Import is locked until the release criteria are defined

Decision (project owner, 2026-10-27): **an import is locked by default until the release criteria (Freigabekriterien) are defined.**

- Out of the box the server imports nothing: no scan at start, no scheduled scan, no manual import, no upload. Every way of adding media to the library is an import and is locked the same way. The server answers an import request with `import_locked` and says what is missing.
- **Unlocking is an admin action.** It needs the criteria first. As a starting proposal the criteria are:
  1. **Import roots**: the folders that may be imported. Nothing outside them is read.
  2. **Rating rule**: how a medium gets its release age: `manual` (an admin rates each book, the default) or `recommendation` (take the medium's age recommendation when it has one; media without one stay unreleased).
  3. **Automatic release**: off by default. Without it every imported medium lands as *unreleased* (section 3b) and waits for an admin. If switched on, a rule names a folder and the release age that applies to everything below it (for example `Lernen/` at age 6, quota-exempt).
  4. **Who releases**: admins only. Fixed, not a setting.
- **Defined** means: at least the import roots and the rating rule have been saved by an admin, and the admin has confirmed unlocking. The server records who unlocked and when.
- The import can be locked again at any time. A running import stops at the next step.
- Changing the criteria later does not touch media that are already released. **Open** whether an admin can ask for a re-check.
- **The developer switch exists only in development builds.** Until the criteria are built, a debug build accepts `--allow-import` (or `DAVID_ALLOW_IMPORT=1`) to scan at start. This is compiled out with `debug_assertions`: a release build has no such flag, ignores the environment variable, and does not mention it in `david help`. Two tests (one per build profile) keep this true. In a release build the import can therefore never be unlocked except through the criteria.
- Because of the chicken-and-egg problem of a first start without any user, the server command line can create the first admin and perform the unlocking (`david init`, `david import unlock`).

Stored as settings: `import_enabled` (default `0`), `import_roots`, `rating_rule`, `auto_release_rules`, `import_unlocked_by`, `import_unlocked_at`. They live in the `setting` table of section 5.

## 4. Pairing by QR code

Decision (project owner, 2026-10-27): devices are **paired by QR code**.

An admin creates a one-time code **for a user** (table `enrollment` gets `user_id`). It is shown as a QR code, in the admin's app or by the server command line (`david enroll --user anna` prints it as text blocks in the terminal). The new device scans it.

Payload (one URI, around 150 characters):

```
david://pair?v=1&id=<server id>&addr=<host:port>[&addr=<host:port>]&fp=<certificate fingerprint>&code=<one-time code>
```

- `id`: the server id, as in the mDNS announcement. `addr`: one or more addresses, the LAN address first and the tunnel address after it. `fp`: SHA-256 fingerprint of the server certificate. `code`: single use, valid for 15 minutes.
- **The QR code is the trusted channel.** Scanning it gives the device the server's identity directly, so it does not have to trust whatever answers on the network. This closes the gap of mDNS, which anyone on the Wi-Fi can imitate.

Steps:

1. The device scans the code, connects to an `addr`, checks the certificate against `fp`, and sends the code, its public key and a device name.
2. The server checks the code and the owner's `max_devices` (section 3a), stores the device under the user (table `device`), marks the code as used, and answers with the tunnel settings if the device is outside the home network.
3. The device **pins** `id` and `fp`. From then on a server with another identity is refused.

Devices without a camera (desktop) paste the same URI or type the code; there is no other way in. Scanner and generator: a Dart scanner package in the app (for example `mobile_scanner`), and a QR generator both in the app and in the server's command line. **Open**: which packages.

## 5. Data model changes (migration `0003`, built)

Built as described below. **Values that the owner has not confirmed are stored with `confirmed = 0`** in `role_policy` and `setting`, and the server logs a warning at start while any exist (currently 9: four role policies and five settings). Confirming a value sets `confirmed = 1`.

- `profile.role TEXT NOT NULL CHECK (role IN ('admin','member','child','guest'))`, filled from `is_admin` (1 becomes `admin`, 0 becomes `member`). `is_admin` stays unused until a table rebuild removes it.
- Trigger: refuse to delete or demote the last `admin`.
- `device` is rebuilt (it holds no data yet): `user_id INTEGER NOT NULL REFERENCES profile (id) ON DELETE CASCADE` for the owner, `shared INTEGER NOT NULL DEFAULT 0`, and `created_by` stays as the admin who enrolled it. The idea of a `max_role` column is dropped.
- `enrollment` gets `user_id` (the future owner) and `shared`.
- `session (token_hash PRIMARY KEY, user_id, device_id, role, created_at, last_used, expires_at)`. Removed together with its user or device.
- `server_identity (id, created_at)`: the server id used in the announcement.
- `role_policy (role PRIMARY KEY, max_devices, session_ttl_s, step_up_s)`, filled with the proposed values above.
- `profile.max_devices INTEGER` (`NULL` = use the role default) and `profile.expires_at INTEGER` (`NULL` = never; used for guests).
- Trigger `device_limit` before insert on `device`: counts the owner's devices with `revoked_at IS NULL` and aborts with `device_limit_reached` when the owner's effective `max_devices` is reached.
- `setting (key PRIMARY KEY, value)`: enrollment code lifetime, device inactivity time, time zone, and the listening-window switch.
- `profile.schedule BLOB CHECK (schedule IS NULL OR length(schedule) = 84)`: the weekly planner, 672 bits (day * 96 + slot, bit set = allowed). `CHECK (role <> 'child' OR schedule IS NOT NULL)`: a child always has a schedule.
- `book.recommended_age INTEGER CHECK (recommended_age IS NULL OR recommended_age >= 0)`: the medium's age recommendation. Information only.
- `book.released INTEGER NOT NULL DEFAULT 0 CHECK (released IN (0,1))` and `CHECK (released = 0 OR min_age IS NOT NULL)`: a released book always has an age rating. `profile`: `CHECK (role <> 'child' OR max_age IS NOT NULL)`: a child always has an age limit.
- `book_release (id PRIMARY KEY, book_id REFERENCES book ON DELETE CASCADE, profile_id REFERENCES profile ON DELETE CASCADE, role TEXT CHECK (role IN ('member','child','guest')), created_by, created_at)` with `CHECK ((profile_id IS NULL) <> (role IS NULL))` and unique indexes so an exception exists only once. These are the **exceptions** of section 3b. Admins need none.
- `book.quota_exempt INTEGER NOT NULL DEFAULT 0 CHECK (quota_exempt IN (0,1))`: content outside the time quota.
- `setting`: `time_zone` (for example `Europe/Berlin`) so the server can evaluate the planner.

## 6. API sketch (to be defined with the HTTP layer)

| Call | Who | Purpose |
|---|---|---|
| `GET /v1/info` | anyone on the network | id, name, version only |
| `POST /v1/enroll` | holder of a code | register a device, receive the server identity |
| `GET /v1/profiles` | enrolled device | list for the picker: name, avatar, role, has PIN |
| `POST /v1/session` | enrolled device | profile id and PIN in, session token out |
| profile and device management | `admin` session | create, change, delete |

## 7. Transport security (decided and built)

Everything that other machines can reach is served over HTTPS with a self-signed certificate made at the first start. The fingerprint of that certificate travels in the pairing code, and a paired device accepts only a server that shows exactly this certificate; there is no certificate authority. Loopback stays plain HTTP. The WireGuard tunnel encrypts the traffic away from home in addition.

## 8. How it will be checked

- **Announcement**: `avahi-browse -rt _david._tcp` shows the instance, port and TXT records while the server runs and nothing after it stops. avahi is available on the development machine.
- **Discovery in code**: a Rust test that publishes with `avahi-publish` and finds the entry with `mdns-sd`, and the other way round.
- **Roles**: a test table, every role against every operation; the last-admin guard; token expiry; a child device that cannot open an admin profile.
- **Platforms**: Android, iPhone and the Windows VM by hand, because multicast behaves differently on each.

## Open decisions

Decided: roles belong to users, devices belong to users (section 3).

1. Fixed roles in code, or configurable roles? (recommended: fixed)
2. Which roles need a PIN; may child users have none?
3. Do we need **shared devices** (the family tablet)? The design allows them with a PIN per picked user; if not, every device is simply its owner's.
4. How users are maintained: through the client app, through the server command line, or both (recommended: both).
5. Transport on the LAN: HTTP or HTTPS with a pinned self-signed certificate (recommended: HTTPS with pinning).
6. What a `guest` user is allowed to keep, and for how long.
7. Which Dart package does discovery on all five platforms, and which packages scan and generate the QR code.
8. Rename the table `profile` to `user`, or keep the name?
9b. Decided: imports are locked until the release criteria are defined (section 3c). Still open: the exact set of criteria (the list in 3c is a proposal), whether automatic release rules are wanted at all, and re-checking existing media after a change.
9c. **Guests without an age limit.** As built, a user with no age limit sees every released book, and that includes a guest created without one (the test `a_user_without_an_age_limit_sees_every_released_book` documents it). Proposal: like a child, a guest must have an age limit, enforced by the database; adults (`admin`, `member`) may have none. Decision needed.
9a. Decided: only released content is shown, with a required release age and a separate age recommendation on the medium, plus exceptions per user or role (section 3b). The guard rail (confirm when releasing below the recommendation) is part of the design. Still open: where the recommendation is read from (custom tag, sidecar file, by hand), fixed `max_age` or a birth date, and whether exceptions can expire.
9. Decided: content outside the quota is exempt from the planner (section 3a). Still open there: whether it is marked per book only or also by a library folder such as `Lernen/`, whether such content has a limit of its own, and whether the earlier unclear wording "unlimited" was meant as "no limit at all" for it.
10a. Decided: children have a weekly planner with 15-minute slots (section 3a). Still open: the default planner of a new child user (nothing allowed until an admin sets it, or a template such as every day 07:00 to 19:00), what happens to a stream that is running when the window ends (stop at once, or finish the current chapter), whether an admin can grant extra time for today, and whether the planner may also be set for other roles.
10. The proposed numbers in section 3a (devices per role, session lifetimes, 15 minute code, 10 minute step-up, 7 day guest): keep, or change?
11. An inactive device (180 days): keep its place until an admin revokes it, or free it automatically?
