<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Backlog

Things the project owner has decided to do **later**. Each line says where the concept lives. Order is not priority; the owner picks.

## Next up

- **Speech to text for sermons** (David as a client of Eliezer): `docs/spec-transcription.md`. Eliezer has no speech model yet; both halves are in the spec.

## Later

- **App Group for the Siri extension, so that "Frag Eliezer" can answer by voice** (decided 2026-10-30, put back on the backlog the same day).
  Why: the Intents extension has to reach the server without the app running, so the pairing (address, token, certificate fingerprint) must be readable
  by the extension; that is an App Group and a shared keychain group. Concept: `docs/spec-voice.md`.
  Steps for the owner (Apple portal): create the App Group `group.com.the-abraham-box.psalm`; enable "App Groups" on the four App IDs
  (`com.the-abraham-box.psalm`, `.psalm.ipad`, `.psalm.intents`, `.psalm.ipad.intents`) and select the group; edit and save the four App Store profiles
  and replace the four GitHub secrets `IOS_PROFILE_*_BASE64`. For the code: `com.apple.security.application-groups` in `Runner.entitlements` and
  `PsalmsIntents.entitlements`, the app writes the pairing into the group, the CI checks that the profiles contain the group (like it does for Siri).
  Do not push the entitlements before the new profiles are in the secrets (signing fails).
  Open point on the server side: whether the way "ask Eliezer" (David to MultiGPT to Eliezer) exists; the spec says David passes the text on.
- **Voice loop on a Raspberry Pi** (a microphone and a speaker as a thin client of Eliezer, raw PCM in both directions): the speech output exists
  (`eliezer speak --format pcm | aplay -t raw -f S16_LE -r 24000 -c 1`); the speech **input** as a stream is specified in the Eliezer repository
  (`docs/Specification.md`, section 5.3, `POST /v1/audio/transcriptions/stream` and the command `eliezer listen`, 2026-10-31) and not built.
  The loop is `arecord ... | eliezer listen | <assistant> | eliezer speak --format pcm | aplay ...`; the assistant (the language model and the MCP
  tools of David) is the part of `docs/spec-voice.md`. The models stay on the GPU machine: a Pi with 1 GB RAM cannot hold Whisper large or Chatterbox
  (estimates, not measured on a Pi).
- **Read-aloud books in the parents' voices** (synthetic audiobooks): `docs/spec-read-aloud.md`. First step is a listening test with 30 seconds of a parent.
- **More Siri commands** (radio, podcasts, next title, "what is playing"): `docs/spec-voice.md`, phase 2; no new profiles needed.
- **Android voice** (Google Assistant / Gemini App Actions): `docs/spec-voice.md`.
- **Real chapter titles for the Follett audiobooks**: none exist for these editions (MusicBrainz has only "Teil NNN", "Part N", "Titel N"); the albums carry
  "Teil N" counted over all CDs as the publisher counts. Revisit if a source with printed titles turns up.
- **Genre and year for imported CDs that MusicBrainz does not know** (for example *Keine halben Sachen*, Duo Camillo): by hand in the tags window.

## Drafts (2026-10-31, not yet decided)

- **Video** (family films and series, age ratings, direct play, discs through MakeMKV as a hidden feature; PDC cannot transcode): `docs/spec-video.md`. First step: the scanner and the listing.
- **E-books** (host, read, read aloud, PDF to EPUB with a labelling language model): `docs/spec-ebooks.md`. First step: sample PDFs from the owner.
- **Abraham Box** (David on an RK3588, night jobs, publisher edition): `docs/spec-abraham-box.md`. First step: the benchmark on the board.
- **Read aloud through Eliezer** (push one chapter per request, voices, night window, rule 6): `docs/spec-read-aloud.md`, section "Update 2026-10-31".
- **CDs in the family** (rights per person, approval, inbox on the server, spoken messages): `docs/spec-cd-inbox.md` (draft
  2026-10-31, branch `feature/cd-inbox`).
- **Real push messages** when the app is closed (APNs through a small relay without contents; UnifiedPush / ntfy on Android):
  `docs/spec-cd-inbox.md`, "Delivery". Not needed now (owner, 2026-10-31).
