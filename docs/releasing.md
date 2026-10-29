<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Building and releasing

`.github/workflows/ci.yml` (also runs on Forgejo/Gitea Actions) does the work.

| Job | When | Result |
|---|---|---|
| `server`, `audio`, `app` | every push and pull request | clippy, tests, `flutter analyze`, `flutter test` |
| `package-server` | push to main, tags | David: `.deb`, `.rpm`, `.tar.gz`; the sound engine as `.tar.gz` |
| `package-app-linux` | push to main, tags | Psalm for Linux: `.deb`, `.rpm`, `.tar.gz` (built with nfpm from `app/linux/packaging/nfpm.yaml`) |
| `package-android` | push to main, tags | Psalm for Android: one `.apk` per processor type (`arm64-v8a`, `armeabi-v7a`, `x86_64`) |
| `release` | tags `v*` | a release with all of the above, `SHA256SUMS` and, if a key is set, `SHA256SUMS.asc` |

The packages of a normal push are kept as workflow artifacts for 14 days. The version is the tag without
its `v` (`v1.2.3` gives `1.2.3`), otherwise the one in `app/pubspec.yaml`. The version of David's packages comes
from `server/Cargo.toml`: change both before tagging.

Locally: `scripts/package.sh` (David), `scripts/package-app.sh` (Psalm for Linux, needs `nfpm`),
`flutter build apk --release --split-per-abi` (Android), `scripts/checksums.sh` (sums and signature).

## Signing

Nothing is signed unless the secrets exist; the build then still works and says so in a warning.

- **Android**: create an upload key once, keep the file safe and never commit it:
  `keytool -genkeypair -v -keystore release.keystore -alias psalm -keyalg RSA -keysize 4096 -validity 10000`
  Secrets: `ANDROID_KEYSTORE_BASE64` (`base64 -w0 release.keystore`), `ANDROID_KEYSTORE_PASSWORD`,
  `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. CI writes `android/key.properties` (ignored by git) from them;
  `android/app/build.gradle.kts` uses it when it exists and the debug key otherwise. An app signed with the
  debug key can be installed on a phone to try it, but an update needs the same key.
- **Checksums and packages**: secret `GPG_PRIVATE_KEY` (`gpg --armor --export-secret-keys <id>`, ideally a
  signing sub-key). The release gets `SHA256SUMS.asc`; users check with `gpg --verify SHA256SUMS.asc SHA256SUMS`
  and `sha256sum -c SHA256SUMS`. Signing the `.rpm` itself (`rpmsign`) and a signed apt repository are not set up.
- Windows and macOS/iOS builds and their certificates are not part of this workflow.

Not verified here: the workflow has not run on GitHub; the Linux client packages need `nfpm` (downloaded in
CI), which could not be tried on this machine.

## The David service and its folders

The packaged unit (`server/packaging/david-deb.service`) starts David as root, in a sandbox
(`ProtectSystem=strict`, `ProtectHome=read-only`). Before it serves anything, David gives the fixed system user `david` (created by the
package via `sysusers.d` and the post-install script) read and write rights on every audiobook and music folder of the import
settings, with POSIX ACLs through libacl (the same as `setfacl -R -m u:UID:rwX` plus a default entry for new files), hands the
data folder to that user and drops to it for good. No `setfacl` is needed; a folder added in the admin pages is covered at the next start
(`systemctl restart david`). The admin pages warn about folders where the rights are still missing and name the command that fixes them.

`/etc/david.toml` holds what belongs to the machine: the account (`user`, optional numeric `uid` and `gid`, which the post-install script uses when it
creates the account), `acl = false` to switch the ACL step off, and `data_dir`, `listen`, `mcp`. A value comes from the command line, else the
environment (`DAVID_...`), else the file, else the default; what the family decides in the pages is in the database. The package marks the file as
configuration, so an update keeps it. A process that is not root (a development run) skips the rights and the switch of user.

The sandbox must let the writes through: `ProtectSystem=strict` makes everything but the state directory read-only, for root's ACL step too.
The library lives in the service's state directory, `/var/lib/david/library` (`StateDirectory=david david/library`):
it belongs to the service user and is writable, so nothing more is needed there; put albums into it with the
upload page or `rsync` as root and set it as the import folder. For other folders (and for a library below `/home`: add `ProtectHome=false`)
use a drop-in: `systemctl edit david` with `ReadWritePaths=/srv/Musik`.

### Network

The packaged unit sets `DAVID_LISTEN=[::]:8484`: the server listens on every interface, over IPv6 and over IPv4 (on Linux `[::]` serves both; a machine without IPv6 falls back to `0.0.0.0` by itself, with a warning in the log), and speaks TLS with a
certificate it makes itself (the pairing code carries the fingerprint, so the apps accept only this server; browsers
warn once). Open port 8484 in the firewall. To keep it on the machine, for example behind nginx with a real
certificate, set `DAVID_LISTEN=127.0.0.1:8484` in a drop-in. The pairing page offers the address of the machine in the
network, not `0.0.0.0`. The "David Administration" menu entry opens `https://localhost:8484/admin/`.

### Health check

`GET /healthz` needs no sign-in and sets no cookie. It answers `200 {"status":"ok","version":"…"}` when the database
answers for reading and writing and `503 {"status":"unavailable",…}` otherwise, and tells nothing else (no library, no
users, no folders). For a monitor, a load balancer or `curl -k https://<host>:8484/healthz`. The result of the last
import, with the places that were skipped and why, stays on the Import page of the administration, so a folder without
read permission can be seen without the server log.

### First start

`--library` exists only in development builds, so the unit starts plain `david run`; the folders to import are set in
the admin pages (Settings, Import; `/var/lib/david/library` is the one the unit creates). Before the first start create the first administrator. The state
directory belongs to `david`, so run the command as that user:

    sudo systemctl stop david
    sudo -u david env DAVID_DATA_DIR=/var/lib/david /usr/bin/david init --name Papa     # asks for a PIN
    sudo systemctl start david

A forgotten PIN or password is replaced with `david reset-pin --name <name>` (same data directory as above); the user
is signed out everywhere and a lock after wrong attempts is lifted.

## MCP: a remote LLM corrects the tags

`david run --mcp` (or `DAVID_MCP=1`, e.g. in a drop-in of the unit) serves `POST /mcp` on the same port and with the same
TLS as the rest. Without the switch the path does not exist. Access needs a token from Settings, tab *LLM access*; it is
shown once, only its hash is kept, and a token either only reads or may change tags too. The LLM sends it as
`Authorization: Bearer <token>`.

Tools: `list_albums`, `get_album`, `get_tags`, `field_names`, `set_tags` (with `dry_run`), `set_cover_from_folder`,
`set_cover_from_url` (an https picture of a public host, at most 8 MiB, JPEG or PNG: written as `cover.jpg`/`cover.png`
in the album folder and embedded in every track), `find_problems` (missing artist/album/year/genre/narrator/cover, tracks
that disagree, stray spaces, gaps in the track numbers) and `change_log`. `set_tags` answers in groups (field, old,
new, number of files), not one line per file. Every changed value is logged with its old and new value (table `tag_change`, shown in the tab,
and in the server log). Fields have the same names for every file format (`TrackTitle`, `AlbumArtist`, `Genre`, ...);
a format that cannot keep a field refuses it, and then nothing is written to that file.

## Desktop entries

- `app/linux/io.onesimus.psalm.desktop`: the Psalm launcher; also registers the `david://` links. Shipped in the bundle
  and in the `.deb`; `install-desktop-entry.sh` registers it for the current user.
- `server/packaging/io.onesimus.david-admin.desktop`: "David Administration", opens `http://127.0.0.1:8484/admin/` in
  the browser (change `Exec=` if the server listens elsewhere). Shipped in David's `.deb` and `.rpm` with its icons.
