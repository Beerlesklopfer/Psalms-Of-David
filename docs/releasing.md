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

The packaged unit (`server/packaging/david-deb.service`) runs David as a dynamic user in a sandbox
(`ProtectSystem=strict`, `ProtectHome=read-only`). David writes into the library: uploads are put there, tags and
covers are written into the files, deleted books lose their files. Two things must then be true for every library
folder: the dynamic user has write permission on it (an owning group added with `SupplementaryGroups=`, or an ACL),
and the sandbox lets the write through. The unit opens `/srv/david` (`ReadWritePaths=-/srv/david`); for other
folders, and for a library below `/home` (add `ProtectHome=false`), use a drop-in: `systemctl edit david`.

## Desktop entries

- `app/linux/io.onesimus.psalm.desktop`: the Psalm launcher; also registers the `david://` links. Shipped in the bundle
  and in the `.deb`; `install-desktop-entry.sh` registers it for the current user.
- `server/packaging/io.onesimus.david-admin.desktop`: "David Administration", opens `http://127.0.0.1:8484/admin/` in
  the browser (change `Exec=` if the server listens elsewhere). Shipped in David's `.deb` and `.rpm` with its icons.
