# Log of the autonomous session

Started 2026-10-27 after the instruction "the next 120 minutes without me". Everything below was decided by the assistant without asking, following the specs and the owner's earlier decisions. Each entry says what was decided, why, and how to change it. Nothing here was committed, pushed or installed outside the project directory; no `sudo` was used.

Ground rules the assistant set for itself:

- Follow the specs in `docs/` and the decisions already recorded there. Where they leave a choice open, take the option the specs recommend and mark it here.
- Safe by default: the server listens on `127.0.0.1` only (there is no TLS yet), imports stay locked, unconfirmed values stay marked `confirmed = 0`.
- No commits, no pushes, no changes outside the project, no changes on the Windows VM.
- Every piece gets tests; failing tests are reported, not hidden.

## Decisions

1. **HTTP framework: axum 0.8.** The spec recommended it (the project already uses tokio). Change: only `server/src/web/` depends on it.
2. **Sign-in: typed name plus PIN, administrators only.** Resolves open item 4 of `spec-admin-ui.md`. A list of users on the sign-in page would show who exists, and with up to 150 users it is unwieldy. Wrong name, wrong PIN and "no PIN set" give the same answer. A correct PIN of a non-administrator opens no session and says so. Change: `web/handlers.rs::login_submit`.
3. **PIN: 4 to 12 digits, Argon2id with the library defaults.** A proposal: nothing was decided by the owner. Change: `auth::PIN_MIN_LEN` / `PIN_MAX_LEN`.
4. **Lockout: 5 wrong PINs lock the user for 15 minutes.** Stored as the settings `pin_max_failures` and `pin_lock_s` with `confirmed = 0` (proposals). Confirm or change them on the settings page. Needed because a 4-digit PIN falls to guessing otherwise.
5. **Sessions:** 256 random bits in a cookie; only the SHA-256 is stored. The lifetime comes from `role_policy`. A demotion takes effect at once because the role is read from the user on every request. Browser sessions have no device: migration `0004` makes `session.device_id` optional (the table was unused).
6. **CSRF token derived from the session token** (SHA-256 of a label plus the token), so no extra column. Checked by one middleware for every POST except sign-in, from the `X-CSRF-Token` header (HTMX) or the form field `csrf`. Sign-in itself is not CSRF-protected because there is no session yet; the cookie is `SameSite=Strict`.
7. **HTMX 2.0.4 is vendored** in `server/static/htmx.min.js`, downloaded from jsdelivr once, SHA-256 `e209dda5c8235479f3166defc7750e1dbcd5a5c1808b7792fc2e6733768fb447`, served by the server. Configured without `eval`, without script tags in answers, same origin only, and with 4xx/5xx answers swapped so that error messages appear where the user clicked.
8. **Strict Content-Security-Policy** on every response (scripts and styles from the server only, no framing), plus `nosniff`, `no-referrer`, `no-store`.
9. **Without JavaScript** every form still works: it posts normally, the procedure redirects to the page with `?msg=<catalog key>`, and the page shows the message. Only keys that exist in our own catalog and start with `ok.` or `err.` are shown, so a link cannot print arbitrary text.
10. **Listening address defaults to `127.0.0.1:8484`.** There is no TLS yet; `--listen` accepts any address but logs a warning for a non-loopback one. Change: `app::DEFAULT_LISTEN`.
11. **`david init --name NAME`** creates the first administrator and refuses if one exists. The PIN is read hidden at a terminal, or as one line from standard input otherwise. A bad PIN is checked before anything is created.
12. **The release below the recommendation needs a tick** ("release anyway"). The family decides, but the second look is the guard rail of spec 3b.
13. **Unconfirmed proposals stay `confirmed = 0` and are listed on the settings page** with a button to confirm each.

14. **The import is unlocked through criteria, as in spec 3c.** The criteria are the import folders and the rating rule; saving them is the administrator's confirmation of the rule. Unlocking needs both. Folders must exist, be absolute, and are stored canonical (no `..`, no duplicates); the filesystem root is refused. An import reads only these folders. Change: `server/src/importer.rs`.
15. **Rating rule "recommendation" releases only books that arrive in the run, and only if they carry a recommendation.** Everything else stays unreleased. A book an administrator withdrew is not released again by a later import. The default rule is "manual".
16. **A bug found by a test and fixed:** the first version recognised "new in this run" by timestamp (`added_at >= start of the run`). Two runs close together can fall into the same millisecond, so a withdrawn book was released again. Now a book counts as new if its id is higher than the highest id before the run. The test ran six times in a row afterwards.
17. **The age recommendation is read from the tags of the first track of a book**, per format (ID3v2 `TXXX`, Vorbis comment, MP4 freeform atom; WAV cannot hold it). Checked with real files made by ffmpeg in MP3, FLAC, Ogg, Opus, M4A (freeform atom written with lofty) and WAV. Other tag fields (author, album) are still not used for the library structure; the folders decide.
18. **`--library` and `DAVID_LIBRARY` exist only in development builds**, like `--allow-import`, because they only make sense together. In a release build the help does not mention them and the flag is unknown. Both build profiles have a test for this.
19. **The server starts and scans according to the database.** If the import is unlocked there, it imports all saved folders at start; if not, it reads nothing. A failing import is logged and does not keep the interface from starting.

20. **Weekly planner editor:** a grid of 7 days by 96 quarters of an hour with checkboxes, a preset ("every day from-until") and removal of the planner. Saving the grid gives the same 84 bytes as `Schedule` in code, and posting back an untouched grid changes nothing (both tested). A child cannot lose its planner (the data layer refuses). Slots outside 0-6 / 0-95 are refused. Without JavaScript it works with a plain form.
21. **Pairing page with QR code.** An administrator picks a user and an address; the code is 128 random bits, stored only as a hash, valid by the setting `enrollment_code_ttl_s` (proposal: 15 minutes), single use even when two devices try at once (a test does exactly that). Refused up front if the user already owns as many devices as allowed, and again at redeeming (then the code stays usable, because the refusal rolls back). Tunnel addresses come from `10.77.0.0/16`, one per device, no reuse of a free number while it is taken. Keys are checked for the canonical shape of 32 bytes in base64.
22. **The QR code contains** `david://pair?v=1&id=<server id>&addr=<address>&code=<code>`. `fp` (certificate fingerprint) is **left out because there is no TLS yet**; the page says so in plain words. A test draws the code, reads it back with an independent reader (rqrr) and compares it with the address; another checks that the page holds exactly that picture.
23. **Without JavaScript the answer to `pairing.create` is the whole page with the code on it**, not a redirect, because the code is shown only once and would be lost.
24. **A missing row is a 404, not a 500.** A test had allowed both; it found that changing a user or device that is gone gave a server error. Fixed and the test made strict.
25. **Revoking a device does not delete it**; the row stays with its state "Revoked" and frees the place under the limit.

26. **mDNS announcement, only when the server is reachable from the network.** The server announces `_david._tcp` with the records `v`, `id`, `name`, `api` (nothing about users or library); the instance name is `<name> (<first 8 of the id>)` so two servers called "David" differ. A server that listens only on loopback (the default) announces nothing. Withdrawn with a goodbye when the server stops. The name comes from the setting `server_name`, default "David". Library: `mdns-sd` 0.21.
27. **Checked on the real network:** `avahi-browse` saw the service with the right address and port, the announced address answered with `200`, and after stopping the server the entry was gone. For this a throwaway server with a throwaway database and a test PIN listened on `192.168.24.127:18486`, `18488` and `18490` for a few seconds each. That is the only time the interface was reachable from the network, and it has no TLS (the log said so).
28. **A finding about the test, not the server:** two `mdns-sd` daemons inside one process do not see each other, so the test runs the real `david` as its own process. A second trap: on this machine the default route goes through the VPN (`tun0`), so "the address towards the outside" is a tunnel address where mDNS does not reach the neighbours. The test therefore picks a real LAN interface (not loopback, not tun/wg/virbr/docker, not a /32) and honours `DAVID_TEST_LAN_IP`. Without such an interface the test says so and stops.

29. **TLS for everything but loopback.** A server that others can reach speaks HTTPS; one on loopback (the default) stays plain HTTP, so local tools and tests need no trust setup. The session cookie gets `Secure` whenever TLS is on. This replaces the warning "no TLS yet".
30. **The certificate is self-signed and made at the first start** (`<data dir>/tls/cert.pem` and `key.pem`, ten years, names `localhost`, `127.0.0.1` and the listen address; the key file has mode 600). **It is never replaced silently**: if only one file exists, or a file is damaged, the server refuses to start rather than make a new one, because every paired device would stop trusting it. Crypto provider: `ring` (no `aws-lc`, which needs more tools to build, notably on Windows).
31. **The QR code carries the fingerprint** (`fp=`, SHA-256 of the certificate in DER form, 64 hex digits), in front of the one-time code. A test client behaves as a paired device will: it checks the handshake signatures but replaces the chain of trust by the pinned fingerprint. It is accepted by the right server and refused, in the handshake, with another fingerprint. `openssl s_client` confirmed that the certificate the live program shows has the fingerprint from the log and from the file.
32. **`--no-announce`** switches the mDNS announcement off (default: on whenever the server is reachable from the network). The tests use it so that they do not announce anything.

33. **A data folder made by the server has mode 700 on Unix.** Found in a review of my own code: the folder with the database (PIN hashes, session hashes) and the private key was created with the default rights, which on many systems let other users of the machine read it. An existing folder is left alone, because its owner chose its rights. On Windows the folder inherits the rights of `C:\ProgramData`; that was not looked at (see the gaps below).

34. **The audio engine got pause, seek, volume and position** (see `spec-audio-pipewire.md`, spike 2) and **a C interface with a Dart backend** (spike 3). Seeking restarts the session at the new place (a 25 ms break was measured) instead of discarding buffers; pausing is `set_active(false)`; the volume is the stream's own control. All of it was checked against recordings of a null sink; nothing was listened to.
35. **The audio code is a separate crate in `audio/`**, not a part of `server/`, because it belongs to the app (Linux) and has no use on the server. The two share nothing.

36. **An incident I caused and repaired: the remembered stream volume.** My volume test set a stream to 0.5. WirePlumber, the session manager, **remembers the volume of a stream per role** (file `~/.local/state/wireplumber/stream-properties`, entry `Output/Audio:media.role:Music`) and restores it for the next stream with that role. So for a while every program playing as `Music` (not just mine) would have started at half volume. I noticed because a later check measured the level at 0.515 of the file. I restored the value to 1.0 through a stream with that role (not by editing the file), and verified it in the file. To prevent a repeat: the engine takes its role from `EngineOptions.role` or `PSALM_AUDIO_ROLE` (default `Music`); every check script sets the role `Test`; the volume check begins and ends by setting 1.0. The checks were repeated and the entry for `Music` stayed at 1.0.
37. **The engine now reports the actual volume** instead of assuming it: a second stream listener receives `control_info` and updates the stored volume, so a volume restored by the system or changed in a mixer shows. `set_volume` updates the stored value at once. A volume the caller never set is left to the system.
38. **A caveat for the real player:** because the system remembers the volume per role, the app's volume is shared with other `Music` programs. That may be wanted; it should be a decision, not an accident.

### Side effects on this machine (all outside the project folder)

- **WirePlumber state** (`~/.local/state/wireplumber/stream-properties`): new entries for the sink `psalm-verify` and the role `Test`, both at volume 1.0. `Music` is back at 1.0. These entries are harmless litter that WirePlumber manages; they are not removed.
- **Windows VM** (`vm-win11-dev`, reached over SSH in the earlier part of the session, before the autonomous period): `D:\psalm-build\` (a copy of the server sources with a build), `D:\psalm-server.tgz` and `D:\david-server.tgz`. Not removed, because of the rule not to change the VM during this period.
- **mDNS:** a throwaway server announced itself for a few seconds three times, and the discovery test announces a server for about ten seconds when it runs.
- **Started earlier, still running:** the Flutter window (`./psalm`, process 3175370) from the look at the app.
- **Not a side effect but worth knowing:** nothing was committed or pushed, no `sudo` was used, and no package was installed except crates and the one JavaScript file in `server/static/`.

### Known gaps found in a review of the security code (not fixed)

- **Step-up is not built.** The spec asks for a recent session or the PIN again before sensitive administration actions. Every action of a signed-in administrator is allowed for the whole session, which lasts as long as the role policy says (30 days for an administrator, a proposal). A shorter administrator session and the step-up need to be decided and built.
- **The lockout can be used against its owner.** Five wrong PINs lock a user for 15 minutes, so anyone who can reach the sign-in page can keep the administrator out. And the lock answer (`429`, "too many wrong PINs") is only given for names that exist, so it tells which names exist. Both are accepted for now: the first is the cost of the lockout, the second leaks little on a family server. A uniform answer would remove the leak at the price of confusing a real administrator.
- **There is no limit per address** for sign-in attempts, only per user.
- **(Closed on 2026-10-27: the `david_login` cookie, see `spec-admin-ui.md`.) Sign-in was not protected against login CSRF** (an attacker making a victim's browser sign in as the attacker). It needs the attacker's own PIN, so it has no use against an administrator, but it is a gap.
- **There is no audit log** of who changed what.
- **Signing in again does not end the older sessions of the same user.** Signing out ends only the current one.
- **Windows:** the rights of `C:\ProgramData\David` and the service account (`LocalService`) were never tried.

### Known gaps of the TLS setup

- A browser warns about the self-signed certificate for the administration pages; an administrator accepts it once. There is no HSTS.
- The names in the certificate are fixed when it is made. If the address of the server changes, browsers warn again; paired devices are not affected because they pin the fingerprint, not the name.
- There is no certificate renewal or rotation. A rotation would have to re-pair or re-pin every device; it needs a design.

### Not verified here

- The Windows code (`platform/windows.rs`) was edited for the new `listen` option and the `on_ready` signature but cannot be compiled on this machine. It needs a build on the Windows VM. The shutdown future must be `Send + 'static`; whether `ServiceStatusHandle` is `Send` was not checked.
- Nobody has looked at the pages in a browser. They were checked through HTTP tests and `curl` only.

