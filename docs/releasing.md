<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Building and releasing

`.github/workflows/ci.yml` (also runs on Forgejo/Gitea Actions) does the work.

| Job | When | Result |
|---|---|---|
| `server`, `audio`, `app` | every push and pull request | clippy, tests, `flutter analyze`, `flutter test` |
| `package-server` | push to main, tags | David for Linux on `amd64` and `arm64` (two runners, `ubuntu-latest` and `ubuntu-24.04-arm`): `.deb`, `.rpm`, `.tar.gz`; the sound engine as `.tar.gz` |
| `package-app-linux` | push to main, tags | Psalms for Linux on `amd64` and `arm64`: `.deb`, `.rpm`, `.tar.gz` (built with nfpm from `app/linux/packaging/nfpm.yaml`) |
| `package-android` | push to main, tags | Psalms for Android: one `.apk` per processor type (`arm64-v8a`, `armeabi-v7a`, `x86_64`) |
| `package-app-macos` | push to main and development, tags, by hand | Psalms for macOS: `.zip` and `.dmg` of the app (macOS runner); signed and notarized only with the Developer ID secrets |
| `package-server-macos` | push to main and development, tags, by hand | David for macOS: an installer `.pkg` that sets the server up as a launchd daemon (macOS runner); signed, notarized only with the Developer ID secrets |
| `package-server-windows` | push to main and development, tags, by hand | David for Windows: `david-server-VERSION-windows-x64.zip` with `david.exe` and `install-service.ps1` (sets up the service `DavidServer`, started with the computer; not signed) |
| `package-app-windows` | push to main and development, tags, by hand | Psalms for Windows: `psalms-VERSION-windows-x64.zip` (the folder `Release` of `flutter build windows`; unsigned, Windows SmartScreen asks the first time) |
| `release` | tags `v*` | a release with all of the above, `SHA256SUMS` and, if a key is set, `SHA256SUMS.asc` |

The packages of a normal push are kept as workflow artifacts for 14 days. The version is the tag without
its `v` (`v1.2.3` gives `1.2.3`), otherwise the one in `app/pubspec.yaml`. The version of David's packages comes
from `server/Cargo.toml`: change both before tagging.

Locally: `scripts/package.sh` (David), `scripts/package-app.sh` (Psalms for Linux, needs `nfpm`),
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
- **iOS** (iPhone and iPad): see the next section. Windows and macOS builds are not part of this workflow.

Not verified here: the workflow has not run on GitHub; the Linux client packages need `nfpm` (downloaded in
CI), which could not be tried on this machine.

## Psalms for iPhone and Psalms for iPad (iOS)

Two products come out of one Xcode project: **Psalms** for iPhone (bundle id `com.the-abraham-box.psalm`, device family iPhone) and **Psalms for iPad**
(`com.the-abraham-box.psalm.ipad`, device family iPad). They are two apps on the App Store with two listings, two builds and two profiles. The project
reads three settings from `app/ios/Flutter/Product.xcconfig` (`PSALM_BUNDLE_ID`, `PSALM_DEVICE_FAMILY`, `PSALM_NAME`); CI copies
`Product-iphone.xcconfig` or `Product-ipad.xcconfig` to it before the build. Without that file a build is universal (`com.the-abraham-box.psalm`,
iPhone and iPad), for a quick try with Xcode.

CI (`ios-build`, macOS runner, matrix iphone/ipad): `flutter build ios --release --no-codesign` on every push and pull request, and a check of the
bundle id and the device family. It needs no Apple account. On a private repository macOS minutes cost ten times as much as Linux minutes; a
public repository has them free.
`package-ios` (every push and every run by hand) builds the signed `.ipa` of both products, **only if the secrets exist**, and uploads them to TestFlight when it makes sense: for tags and `main`, for a run by hand (input `upload`), and for a push whose commits change the files of the app (`app/lib`, `app/ios`, `app/assets`, `pubspec.yaml`, `pubspec.lock`, `l10n.yaml`); a push that changes only the server or the documentation uploads nothing, and `[skip testflight]` in the commit message keeps a push out of TestFlight:

| Secret | What |
|---|---|
| `IOS_CERTIFICATE_P12_BASE64`, `IOS_CERTIFICATE_PASSWORD` | the "Apple Distribution" certificate with its private key as .p12 (`base64 -w0`) |
| `IOS_TEAM_ID` | the ten characters of the team (developer account, membership) |
| `IOS_PROFILE_IPHONE_BASE64`, `IOS_PROFILE_IPAD_BASE64` | the App Store provisioning profile of each bundle id (`base64 -w0 x.mobileprovision`) |
| `APPSTORE_API_KEY_ID`, `APPSTORE_API_ISSUER_ID`, `APPSTORE_API_KEY_P8_BASE64` | the App Store Connect API key (role App Manager) for the upload; without it the .ipa stays an artifact |

One-time setup (everything works from Linux, a Mac is not needed):
1. Apple Developer Program (99 EUR a year, an individual or a company account). The free Apple ID is not enough for TestFlight, CarPlay or a profile that lasts.
2. Developer portal: two App IDs (`com.the-abraham-box.psalm`, `com.the-abraham-box.psalm.ipad`).
3. A certificate: make a key and a request with `openssl req -new -newkey rsa:2048 -nodes -keyout dist.key -out dist.csr -subj "/CN=Psalms"`,
   upload `dist.csr` as "Apple Distribution", download the `.cer`, then `openssl x509 -inform der -in dist.cer -out dist.pem` and
   `openssl pkcs12 -export -inkey dist.key -in dist.pem -out dist.p12`. Keep the key safe: whoever has it can sign as the team.
4. Two App Store provisioning profiles (one per App ID, type "App Store Connect").
5. App Store Connect: two app records (for example "Psalms" and "Psalms for iPad", the names must be free), and an API key (users and access, integrations).
6. Put the secrets into the repository settings. Tag or push to main: the builds appear in TestFlight after Apple has processed them (about ten minutes).

How the family gets it: TestFlight. Internal testers (up to 100 people of the team, no review) need an invitation by Apple ID; external
testers (up to 10 000, a public link) need one beta review of the first build. A TestFlight build runs for 90 days. For a longer life without the
public App Store: "unlisted" distribution after a normal review. Ad hoc builds (up to 100 devices by UDID a year) are possible but need a profile per device list.
Trying it with a free Apple ID: only with Xcode on a Mac (the app lasts seven days); there is no way from Linux alone.

CarPlay needs more: the entitlement `com.apple.developer.carplay-audio` has to be requested from Apple for the iPhone app (a form, the answer can take
weeks), then an entitlements file in the project and a profile that contains it. See `docs/spec-car.md`.

Not verified here: the workflow has not run on GitHub (no Mac runner available), and the Xcode project was edited by hand on Linux. The first CI run
will show if `flutter build ios` accepts the product settings. The CI runs on pushes to `main` and `development`, on tags and on pull requests; the iOS apps are signed in every run; the upload to TestFlight follows the rules above (the repository has no `main` yet: its default branch is `development`); a run can also be started by hand (Actions, ci, Run workflow).

## Psalms for macOS

`package-app-macos` builds the desktop app (`app/macos/`, bundle id `com.the-abraham-box.psalm`, macOS 12 or later) on a macOS runner with
`flutter build macos --release` and packs it as `psalm-<version>-macos.zip` and `psalm-<version>-macos.dmg`. A tag publishes both with the release.
The app plays through `just_audio` (AVPlayer, as on iOS), with Now Playing and the media keys through `audio_service`; the app sandbox
(`macos/Runner/*.entitlements`) allows the network client and the file chooser. There is no mDNS search on the Mac (it would need
`com.apple.security.network.server` and gets nothing in the sandbox otherwise): the app keeps the address it was paired with. Ogg Vorbis and the other formats AVPlayer
cannot decode come as an MP3 from the server (`?as=mp3`), as on the iPhone. The camera button is not offered; the profile picture is chosen with the file chooser.

**Unsigned** (no secrets): the build works, the job warns. macOS refuses to open such a download with a double click: right click, "Open", or
`xattr -dr com.apple.quarantine Psalm.app` in the terminal.

**Signed and notarized** (what a user can simply open): a certificate "Developer ID Application" (not "Apple Distribution"; Developer portal, Certificates, "+",
"Developer ID Application", G2 Sub-CA). Make the key and the request as in the iOS section
(`openssl req -new -newkey rsa:2048 -nodes -keyout devid.key -out devid.csr -subj "/CN=Psalms/C=DE"`), upload `devid.csr`, download the `.cer`, then
`openssl x509 -inform der -in developerID_application.cer -out devid.pem` and
`openssl pkcs12 -export -inkey devid.key -in devid.pem -out devid.p12 -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1`.
Secrets: `MACOS_CERTIFICATE_P12_BASE64` (`base64 -w0 devid.p12`) and `MACOS_CERTIFICATE_PASSWORD`. The job signs with the hardened runtime and
`macos/Runner/Release.entitlements`, and notarizes with the App Store Connect API key that the iOS builds use already (`APPSTORE_API_KEY_ID`,
`APPSTORE_API_ISSUER_ID`, `APPSTORE_API_KEY_P8_BASE64`: `xcrun notarytool submit --wait`, then `xcrun stapler staple`). Without that key the app is signed but not notarized.

**Mac App Store** (not set up): it needs a third app record (platform macOS, the bundle id `com.the-abraham-box.psalm` may be shared with the iPhone app
as a universal purchase or a separate id), the certificates "Apple Distribution" and "Mac Installer Distribution", a profile for macOS and a signed `.pkg`.

Not verified: nothing of this macOS part could be built on the machine where it was written (Linux); the first run of the job on GitHub will show it.

## David for macOS (installer package)

The job `package-server-macos` builds `david-server-VERSION-macos.pkg` (a universal binary for Apple silicon and Intel; if the Intel build fails in CI
the package is for Apple silicon only and the job says so). The files are in `server/packaging/macos/`; `build-pkg.sh` makes the package from a binary
(`pkgbuild`, `productbuild`, optional `productsign`) and checks what is inside. Opening the .pkg runs the installer (German and English text, license,
read-me); macOS 11 or newer.

What it installs:

| Where | What |
|---|---|
| `/usr/local/bin/david` | the server |
| `/Library/LaunchDaemons/com.the-abraham-box.david.plist` | the daemon (RunAtLoad, KeepAlive; starts at boot, restarts after a crash) |
| `/Library/LaunchDaemons/com.the-abraham-box.david-reindex.plist` | the hourly import (`david reindex`, as the service account), like `david-reindex.timer` |
| `/etc/david.toml` | the machine's settings; written only if there is none (an update keeps yours); the default is also in `/usr/local/share/david/david.toml.default` |
| `/Library/Application Support/David` | the data (database, certificate), owner `_david`, mode 700 |
| `/var/log/david.log`, `/var/log/david-reindex.log` | the logs, turned over by newsyslog (`/etc/newsyslog.d/david.conf`) |
| the account `_david` | a hidden service account (shell `/usr/bin/false`, no home), created by the post-install script with an id below 500 |
| `/usr/local/bin/david-uninstall.sh` | removes it all again |

The daemon starts as root, hands the data folder to `_david` and drops to that account for good (`user = "_david"` in `/etc/david.toml`), like the systemd unit.
POSIX ACLs exist on Linux only (`acl = false` in the macOS file): the music folders have to be readable (and writable, if tags and covers are edited or uploads are taken)
by `_david`; macOS also protects `Documents`, `Desktop`, `Downloads`, `Music` and external disks, so either the library lives in a plain folder such as `/Users/Shared/Musik`
or `/usr/local/bin/david` gets *Full Disk Access* (System Settings, Privacy & Security). The read-me of the installer says how.
The CD import and the kernel's drive events are Linux only: on macOS the server reports "no drive". The sound engine (`audio/`) is not part of the package.

First start: the installer does **not** start the daemon on a fresh install (there is no administrator yet). `sudo -u _david /usr/local/bin/david init`, then
`sudo launchctl bootstrap system /Library/LaunchDaemons/com.the-abraham-box.david.plist`; the administration pages are at `https://localhost:8484/admin`.
An update installs over the old files, keeps data and config and starts the daemon again.

Everyday: restart `sudo launchctl kickstart -k system/com.the-abraham-box.david`, stop `sudo launchctl bootout system/com.the-abraham-box.david`, log
`tail -f /var/log/david.log`, remove `sudo /usr/local/bin/david-uninstall.sh` (the data folder and `/etc/david.toml` stay; `--purge` removes them, the account and the logs too;
the music files are never touched).

Signing and notarization (optional; without them the package opens with a right click, Open): two certificates of the Developer ID kind, made like the distribution
certificate of the iOS section (a key and a request with `openssl`, uploaded under Certificates as **Developer ID Application** (signs the program) and **Developer ID Installer**
(signs the package), each downloaded as `.cer`, turned into a `.p12`). Secrets: `MACOS_CERTIFICATE_P12_BASE64` (Application), `MACOS_INSTALLER_CERT_P12_BASE64` (Installer), both
with the same password `MACOS_CERTIFICATE_PASSWORD`; for the notarization the App Store Connect key of the iOS section (`APPSTORE_API_KEY_ID`, `APPSTORE_API_ISSUER_ID`,
`APPSTORE_API_KEY_P8_BASE64`) is used, and the stapled package opens without a warning.

Not verified here: nothing of this ran on a real Mac (the code only type-checks for `x86_64-apple-darwin` on Linux; the installer, the launchd files, the
scripts and the CI job are untested). The first CI run is the test.

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

- `app/linux/com.the-abraham-box.psalm.desktop`: the Psalms launcher; also registers the `david://` links. Shipped in the bundle
  and in the `.deb`; `install-desktop-entry.sh` registers it for the current user.
- `server/packaging/io.onesimus.david-admin.desktop`: "David Administration", opens `http://127.0.0.1:8484/admin/` in
  the browser (change `Exec=` if the server listens elsewhere). Shipped in David's `.deb` and `.rpm` with its icons.

## The name of the device when it is paired

Psalms proposes a name for the device in the field "Name of this device". Android: the name the person gave the device (the setting `device_name`),
else the maker and the model. iOS and iPadOS: from iOS 16 an app gets the model ("iPhone", "iPad") and not the name the person gave the device, unless
Apple has granted the entitlement `com.apple.developer.device-information.user-assigned-device-name`. To get it: ask Apple at
https://developer.apple.com/contact/request/device-name (the account holder fills in the form, the answer can take days), then enable the capability
"User Assigned Device Name" for both App IDs, make new App Store profiles, replace the secrets `IOS_PROFILE_*_BASE64`, and add the entitlement to
the repository variable `IOS_DEVICE_NAME_ENTITLEMENT` set to `true` (Settings, Secrets and variables, Actions, Variables): then the CI writes
`ios/Runner/Runner.entitlements` and sets `CODE_SIGN_ENTITLEMENTS` for that build. Do not set the variable before the profiles contain the entitlement: signing fails.
The person can always change the name in the field.

## Windows

- **David:** unzip `david-server-VERSION-windows-x64.zip`, open PowerShell **as administrator** in the folder and run `.\install-service.ps1 -MediaFolder D:\Audiobooks, D:\Music` (the folders are only given read access for the service; they are chosen per user afterwards in the administration). The service `DavidServer` runs as `NT AUTHORITY\LocalService`, stores its data in `C:\ProgramData\David` (`-DataDir` changes it) and starts with the computer. Without a certificate the administration listens on `127.0.0.1:8484` over plain HTTP, as on the other systems; remove the service with `david.exe uninstall`. Tried on Windows 11: the build, the start and the service (install, start, remove).
- **Psalms:** unzip `psalms-VERSION-windows-x64.zip` and start `psalm.exe`. The sound comes from Media Foundation through `just_audio_windows`; there is no QR camera on Windows (pair with the address and the code, or with a picture of the QR code). **Not tried on a real Windows yet**: the build on the VM failed because Flutter wants the generator "Visual Studio 18 2026" and the CMake of Visual Studio 2026 does not know it; the CI job (Visual Studio 2022) is the way to build it.
- Not done yet: an installer (`.msi` or Inno Setup) and signing with a certificate.
