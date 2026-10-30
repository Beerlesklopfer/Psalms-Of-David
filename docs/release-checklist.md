<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Checklist: Psalms in the stores

Status 2026-10-30. What has to be done, in this order, to publish Psalms for iPhone and iPad in the public App Store, and the other systems. The
technical side (signing, TestFlight upload) is in `docs/releasing.md`. The texts below are proposals in German and English: change them, but keep
every sentence true (Apple and the review check the app against them).

## 0. Decide first

- [ ] **Who is the seller?** As an individual the App Store shows the name of the account holder. Since the EU Digital Services Act a developer who
      sells or distributes in the EU must say whether they are a **trader**. A trader's **address, telephone number and e-mail are shown publicly** on
      the product page, and the websites need an imprint (Impressum). Alternatives: an association (Verein) or a company with a business address
      (a D-U-N-S number is needed for the organisation account). Decide before the first submission: it is a setting in App Store Connect (Business
      > Compliance) and part of the Apple account.
- [ ] **Impressum** for the websites (`site/`): name, address, e-mail (§ 5 DDG). Not written yet because it needs an address.
- [ ] **Is the public store wanted now?** For the family and the first congregations TestFlight is enough: internal testers (up to 100) need no review,
      external testers (up to 10 000, a public link) need one beta review of the first build.
- [ ] **A demo server** for the reviewer (see 4). The app is empty without a David server: this is the most common reason for a rejection.

## 1. Name and texts (App Store Connect > The Abraham Box / Psalms > Version 1.0)

Limits: name 30 characters, subtitle 30, promotional text 170, keywords 100 (separated by commas, no spaces), description 4000.
The store name is the "App Name" of the record ("The Abraham Box / Psalms" today, 23 characters; it can be changed before the first submission).
The name under the icon on the phone is "Psalms" (set in the app).

| Field | Deutsch | English |
|---|---|---|
| Name (alternatives) | `Psalms – Familien-Mediathek` (25) | `Psalms – Family Media Player` (27) |
| Subtitle | `Hörbücher, Musik, Podcasts` (26) | `Audiobooks, music, podcasts` (27) |
| Promotional text | `Der Player für den eigenen Familienserver David: Hörbücher, Musik, Podcasts und Radio, mit Profilen und Altersgrenzen für Kinder.` | `The player for your family's own David server: audiobooks, music, podcasts and radio, with profiles and age limits for children.` |
| Keywords | `Hörbuch,Musik,Podcast,Radio,Familie,Kinder,Predigt,Bibel,Server,Mediathek,Offline` | `audiobook,music,podcast,radio,family,kids,sermon,bible,server,library,offline` |
| Support URL | `https://beerlesklopfer.github.io/Psalms-Of-David/support.html` | same |
| Marketing URL (optional) | `https://beerlesklopfer.github.io/Psalms-Of-David/` | same |
| Privacy policy URL | `https://beerlesklopfer.github.io/Psalms-Of-David/privacy.html` | same |
| Copyright | `2026 Jörg Bernau` | same |
| Category | Music (secondary: Lifestyle or Education) | same |

**Description (Deutsch):**

> Psalms ist der Player für **David**, den Medienserver deiner Familie. Hörbücher, Musik, Podcasts und Radio liegen auf einem eigenen Rechner zu Hause, nicht bei einem fremden Anbieter, und Psalms bringt sie auf iPhone und iPad.
>
> **Wichtig:** Psalms braucht einen David-Server, den deine Familie selbst betreibt. Ohne Server hat die App nichts abzuspielen. David ist freie Software und kostenlos.
>
> • Hörbücher und Musik hören, mit Hörfortschritt auf allen Geräten
> • Titel herunterladen und ohne Netz hören, Änderungen werden später abgeglichen
> • Podcasts abonnieren, Folgen laden, Radio hören
> • Schlagwörter, Filter und Suche über die ganze Sammlung
> • Profile für alle in der Familie, Altersgrenzen und Wochenplan für Kinder
> • Koppeln per QR-Code mit der Kamera oder per Adresse, ohne Konto bei einem Dienst
> • Kein Tracking, keine Werbung, keine Analyse: Die App spricht nur mit deinem Server
>
> Mehr unter beerlesklopfer.github.io/Psalms-Of-David

**Description (English):**

> Psalms is the player for **David**, your family's media server. Audiobooks, music, podcasts and radio live on a computer at home, not with a stranger, and Psalms brings them to your iPhone and iPad.
>
> **Important:** Psalms needs a David server that your family runs itself. Without a server the app has nothing to play. David is free software and costs nothing.
>
> • Listen to audiobooks and music, with your progress on every device
> • Download titles and listen without a network; changes are synchronised later
> • Subscribe to podcasts, download episodes, listen to radio
> • Labels, filters and search across the whole collection
> • Profiles for everybody in the family, age limits and a weekly schedule for children
> • Pair with the camera by QR code or by address, no account with a service
> • No tracking, no ads, no analytics: the app talks only to your server
>
> More at beerlesklopfer.github.io/Psalms-Of-David

**What's new (1.0):** "Erste Version." / "First version."

## 2. Screenshots

- [ ] iPhone: the set for **6.9 inch** (1320 x 2868) or **6.5 inch** (1284 x 2778 or 1242 x 2688); one of them is enough, Apple scales the others. Screenshots
      of a 6.1 inch iPhone (1170 x 2532) are **not** accepted as they are.
- [ ] iPad (for the iPad record): **13 inch** (2064 x 2752 or 2048 x 2732).
- [ ] 3 to 10 per size. Proposal: Start page, Library with covers, Player, Search with labels, Podcasts, Settings > Pair a further device. Show only content
      that is free to show (no cover of a commercial album that you do not own the rights to show; take free or own content for the demo server).
- [ ] No other devices, no real names, no private addresses in the picture.

## 3. App information and privacy

- [ ] **Age rating** (questionnaire): the app plays content that the family or publishers provide. Propose **12+** to be safe: "Infrequent/Mild" for
      unrestricted media content, "No" for user generated content in the app, "No" for unrestricted web access. Answer truthfully; Apple may ask.
- [ ] **App Privacy ("nutrition label")**: the developer collects **no data** (the data goes to the server of the user's family, not to the developer).
      Answer "Data Not Collected" for tracking and collection, and keep the privacy policy URL. Re-read the policy before you answer: if a later
      version adds analytics or a crash service, the label must change.
- [ ] **Export compliance**: only standard HTTPS: `ITSAppUsesNonExemptEncryption` is false in `Info.plist` (done).
- [ ] **Advertising identifier**: not used.
- [ ] **Content rights**: the app has no content of its own; the user brings the content.
- [ ] **Account deletion** (guideline 5.1.1): the app has no sign-up of an account with the developer; profiles are made by the administrator of the
      family server. Say so in the notes if asked.
- [ ] **Permissions** have texts in `Info.plist`: camera (pairing QR code, profile picture), photo library (a chosen picture), local network
      (reach the server). Check that every text says what the app does with it.

## 4. Notes for the reviewer and the demo server

The reviewer must be able to use the app. Put into "App Review Information > Notes" (English):

> Psalms is the player app for David, a media server that a family runs itself. The app has no content of its own. To try it, pair it with our demo server:
> 1. Open the app, tap the QR icon (or paste the address) on the first screen.
> 2. Paste this pairing address into the field "Pairing address": `david://pair?v=1&id=…&addr=…&code=…` (valid until <date>; four more codes are
>    in the attachment, each works once).
> 3. Tap "Connect". The library of the demo server (public domain and Creative Commons audio) appears.
> "Try with demo data" on the first screen only shows the screens without any interactions (no server, nothing can be played or changed).
> The camera is used only to scan the pairing QR code and for an optional profile picture. The local network permission is used to reach the server.

To prepare:
- [ ] A **demo David server** on the internet with a valid certificate (the pairing address carries a fingerprint or the server has a certificate from a
      certificate authority) and **only free content** (public domain audiobooks, Creative Commons music, own recordings).
- [ ] A **member profile** (not an administrator) for the reviewer.
- [ ] **Long-lived pairing codes**: a code is valid 15 minutes and works once (`enrollment_code_ttl_s`, default 900). For the review set the setting to
      something like 14 days, make **five codes** (the admin page "Pairing" shows them as open codes), and put them in the notes. Set the value back
      afterwards. Check that the codes were not used by your own tests.
- [ ] Keep the demo server running until the review is finished and for later updates.

## 5. Submit

- [ ] A build from the CI is in TestFlight and has been tried on an iPhone and an iPad: **pairing with the camera, a title plays, downloaded titles
      play offline, background playback with the screen locked, radio, podcast**.
- [ ] Select the build in the version (App Store Connect > version 1.0 > Build), fill in everything above, press "Add for review".
- [ ] Release: manual (after the approval you press the button) or phased over seven days (can be paused).
- [ ] Repeat for the iPad record.
- [ ] Review takes about one to two days. If it is rejected, answer in the Resolution Center; most rejections are fixed with a note or a small change.

## 6. After the release

- [ ] Tag `v1.0.0` in git (the CI makes the release with all packages).
- [ ] Look at crashes and feedback in App Store Connect (TestFlight and App Analytics).
- [ ] An update is a new build from the CI (version from the tag, build number counted), a new version in App Store Connect, the "What's new" text, and a
      new submission.
- [ ] When the profiles or the certificate run out (one year after 2026-10-30: the Apple Distribution certificate and the profiles; the membership
      renews on 2027-10-31), make them again as described in `docs/releasing.md` and replace the secrets.

## 7. The other systems

| System | Way | To do |
|---|---|---|
| Android | APK from the CI (arm64-v8a, armeabi-v7a, x86_64) as a download, or Google Play | Play: developer account (one time 25 US dollars). A new personal account must run a **closed test with at least 12 testers for 14 days** before production. F-Droid is not possible while `mobile_scanner` brings Google ML Kit. Sign the APK with the release key (secrets `ANDROID_KEYSTORE_*`). |
| macOS | `.dmg` and `.zip` from the CI | Unsigned works with "right click > Open". For a download without a warning: certificate "Developer ID Application" and notarization (secrets in `docs/releasing.md`). The Mac App Store needs a macOS record and the certificates "Apple Distribution" and "Mac Installer Distribution". |
| Linux | `.deb`, `.rpm`, `.tar.gz` from the release | Run the window with `GDK_BACKEND=x11` under GNOME Wayland until the cause is fixed. |
| David (server) | `.deb`, `.rpm`, `.tar.gz`, macOS `.pkg` | Not in a store; a release on GitHub with `SHA256SUMS` (signed if `GPG_PRIVATE_KEY` exists). |
