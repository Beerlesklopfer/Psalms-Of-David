<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Voice assistants (Siri, Google): controlling the library by voice

**Eliezer** is the family's own model server (`~/Dokumente/Projekte/eliezer`: a replacement for LM Studio with an OpenAI-compatible interface), driven by
**MultiGPT**, which holds permissions, ranking and billing. **Neither has any voice control yet** (project owner, 2026-10-30). Psalms therefore brings the
voice in: the system assistant (Siri, Google) turns the speech into text, the text goes to David, and David asks Eliezer through MultiGPT and acts with the
MCP tools. The spoken phrase is "Ask Eliezer ..." / "Frag Eliezer ..." (with Siri: "Frag Eliezer in Psalms ..."); the section of the administration is
"Eliezer". (Eliezer is the servant of Abraham who carried out his errands: he does what he is asked, within what the master allows.)

Status 2026-10-30: **concept; phase 1 for iOS (play and look) is written but not yet tried on a device** (see "Status of phase 1 (iOS)" below). Project owner: "Psalms needs Siri, or Google on Android. It should take over the control of the media
library. The MCP commands are the template." This text says what that can mean, what each platform allows, and in which order to build it. Statements about
Apple and Google are from the documentation as known to the author and **have to be checked on a device** before they are relied on.

## What "control of the library" means here

Two groups of commands, both modelled on the tools of the MCP server (`server/src/mcp.rs`, `list` and `help_for` give the catalogue):

| Group | Examples (spoken) | MCP tool as the model | Who |
|---|---|---|---|
| **Play** | "Play the last audiobook", "Play *Frohe Weihnachten*", "Next title", "Play the radio station DWG Lyra", "Continue where I stopped" | `list_albums`, `get_album`, `list_stations`, `list_podcasts` and the playback of the app | everybody, within the age limit and the weekly schedule of the profile |
| **Look** | "What is new?", "How many albums are labelled School?", "Which albums have problems?" | `list_albums`, `list_labels`, `find_problems`, `list_formats` | adults; a child hears only what it may see |
| **Change** | "Label this album Prayer", "Hide the School albums for Beerchen", "Release the audiobooks for age 8" | `set_labels`, `hide_albums`, `release_albums`, `set_owner`, `edit_podcasts`, `edit_stations`, `edit_playlists` | **administrators only** (project owner, 2026-10-30); every change is confirmed first |

The same rules as everywhere apply: the server decides (role, age limit, planner), the voice command is only another way to ask.

## Two ways to understand a sentence

1. **Fixed commands (intents).** The platform understands the sentence and gives the app a command with parameters ("play" + album name). Deterministic, works
   without a language model, but the set of sentences is limited to what the platform offers.
2. **Free text to a language model on the server.** The app sends the spoken text to David; David lets a language model use the MCP tools (it has
   the MCP server already, `david run --mcp`, with tokens for a remote model) and answers in a sentence. Flexible ("hide everything for school from Beerchen"), but
   it needs a model: **Eliezer at home through MultiGPT** (decided 2026-10-30), over its OpenAI-compatible interface (`base_url` and a key are settings of the
   administrator). It costs time (a local model on a scarce GPU may need seconds to load) but no money and no data leaves the house.

**Proposal: both, in this order.** Phase 1 with fixed commands (play and look, a few safe changes). Phase 2 adds one command "Ask Eliezer" that takes any text
to the server. The model of phase 2 never gets more rights than the person who speaks (see "Rules").

## Rules for changes (both ways)

- The voice belongs to the **paired device**, so to a profile. Its role limits the tools: a child and a member play and look (within their rights); **only an
  administrator changes anything** by voice, with every writing tool.
- **Nothing is changed in one step.** The server first answers what *would* happen (the `dry_run` of the tools: "62 albums would be hidden for Beerchen"); the
  assistant reads it back and asks; only a "yes" runs the change with `dry_run=false`. A change that touches many items (more than a handful) says the number.
- Every change goes to the change log with the profile and "by voice" (as the MCP tools log the token).
- What the language model of phase 2 sees: names and counts of the library, not the files and not the passwords. The model is a setting of the
  administrator (address of Eliezer or MultiGPT); **the default is none** (phase 2 off), as with `--mcp`. MultiGPT keeps its own permissions and billing: David
  uses one account for the house, and its own rules (above) decide what a person may ask.

## What each platform needs

**iPhone and iPad (Siri, Shortcuts).**
- *App Intents* (Swift, iOS 16 and later): the app declares intents such as `PlayAlbum(name)`, `ContinueListening`, `NextTrack`, `WhatIsNew`, `AskEliezer(text)`
  and an `AppShortcutsProvider` with the spoken phrases ("Play *name* in Psalms"). They also appear in the Shortcuts app and on the lock screen. The author
  expects that **no special permission from Apple** is needed (unlike CarPlay), only the usage text for speech; to be confirmed when the first build runs.
- *SiriKit media intents* (`INPlayMediaIntent`) would give "Play X" without the app name, but need an *Intents extension* and the capability "Siri" on the App
  IDs (as the device name needed a request, this is a switch in the Apple Developer account and new profiles). Worth it only if the short phrase matters. **Built 2026-10-30:** see "SiriKit media intents" at the end.
- The intents are native code (Swift) that calls the same API as the Flutter app (address and token are in the app folder; the intent runs in the app's
  process and can read them) or tells the Flutter side through the existing platform channel. The CI builds it with the app; it **cannot be run on this
  computer**: every try is a TestFlight build.

**Android (Google Assistant, Gemini).**
- *Playing by voice* ("Hey Google, play X on Psalms") works through the media session that already exists (`audio_service`): the handler answers
  `playFromSearch(query)`; the app looks the query up on the server (`/search`) and plays the hit. Small and the same code serves Android Auto.
- *App Actions / App Functions* (`shortcuts.xml`, built-in intents; for Gemini the newer *App Functions*) are for the other commands. Google is changing this
  area (Assistant to Gemini); what an app may declare, and on which Android versions, has to be looked up again when the work starts. The free-text command
  "Ask Eliezer" is the most robust way here too: one function with one text parameter.
- Native Kotlin again; tried on a phone or the emulator (the project owner works the emulator by hand).

**Both:** the microphone belongs to the system assistant; the app never listens by itself and needs no microphone permission.

## Server side

- Phase 1: nothing new for *play* and *look* (the API has search, items, labels, continue). For *change* a small endpoint `POST /api/v1/assistant/plan` and
  `/confirm`, which runs a named tool with the rules above and returns the sentence to read back. The tools themselves are the ones of the MCP server,
  called inside the server (no second implementation).
- Phase 2: `POST /api/v1/assistant/ask {text, language}` -> the model with the tools allowed for the role -> `{say, needs_confirmation, plan_id}`. The
  administration gets a section "Eliezer" (off, or the address and key of Eliezer or MultiGPT), next to "LLM". The call is an OpenAI-style chat request with
  the tools of the role (`tools` / `tool_calls`); Eliezer has to offer tool calling for the chosen model (to be checked in `docs/Specification.md` of Eliezer).
  **Speech input and output are not part of Eliezer yet**: the phone does both (the assistant listens and speaks the answer).

## Order of work

1. Android: `playFromSearch` (small, also needed for Android Auto).
2. iOS: App Intents for play, continue, next, what is new (App Shortcuts with phrases in German and English). **Written, untested on a device** (below).
3. Server: `plan`/`confirm` for the safe writes (labels, hide, release), with tests; intents for them on both platforms.
4. Phase 2: "Ask Eliezer" and the model setting.

## Decided and open

Decided (2026-10-30): the model is Eliezer through MultiGPT; **only administrators change** by voice.

Open:
1. Which comes first, Siri or Android? (The proposal above starts with the small Android part and then iOS.)
2. Does the chosen model of Eliezer do tool calls reliably (the "plan" must be exact)? To be tried with a real call before phase 2 is built.
3. Which sentences matter most? (The table above is a guess.)

## Status of phase 1 (iOS)

Built (2026-10-30), play and look only, no change of the library, no Eliezer:

- `app/ios/Runner/PsalmsIntents.swift`: App Intents `ContinueListeningIntent`, `PlayIntent` (parameter `query`, Siri asks "What would you like to hear?"),
  `NextTrackIntent`, `PreviousTrackIntent`, `WhatIsNewIntent`, all `@available(iOS 16.0, *)` (the deployment target is 15), all with `openAppWhenRun`.
  Each writes a JSON command `{"action":"continue|play|next|previous|new","query":"..."}` to UserDefaults (`psalms.voice.pending`) and answers with a
  short spoken sentence. `PsalmsShortcuts` (an `AppShortcutsProvider`) holds the phrases; each contains the app name.
- `AppDelegate.swift`: channel `com.the-abraham-box.psalm/voice`, method `takePending` (returns the JSON text or nothing and clears it).
- `en.lproj` / `de.lproj` with `Localizable.strings` (titles, answers) and `AppShortcuts.strings` (phrases, keyed by the English phrase with
  `${applicationName}`); added to `project.pbxproj` by hand (German is a known region; `CFBundleLocalizations` en/de in `Info.plist`).
- Flutter: `lib/voice/voice_commands.dart` (`VoiceCommand`, `VoiceChannel`), `lib/voice/voice_match.dart` (best match by title, author, narrator, labels;
  case and umlauts ignored, all words must match, title first), `PlayerController.resumeListening()` and `playByVoice(query)`, and in `main.dart` the
  command is taken after the start (library loaded) and when the app returns to the front. "What is new" selects the start tab. When nothing matches the app
  stays silent (Siri has already answered). Tests: `test/voice_commands_test.dart`, `test/voice_match_test.dart`.

Not possible here, to be checked with a TestFlight build (nothing of the Swift side or the project file was compiled; the CI on macOS is the first compiler):

1. The Swift code compiles with the Xcode of the CI (App Intents API names, `static let` for `title`/`openAppWhenRun`, `registrar(forPlugin:)` on the
   implicit engine bridge) and `project.pbxproj` opens (it was only checked for balanced braces, unique and existing ids).
2. **Whether the intents need a capability or entitlement in the Apple Developer account: unverified.** The author expects none for App Intents (unlike
   CarPlay or SiriKit media intents), and no entitlement was added. If the build or the Shortcuts app says otherwise, that is the place to look.
3. The phrases are recognised, in German and in English, and Siri uses the right app name (it is `CFBundleDisplayName`, from `PSALM_NAME`).
4. `PlayIntent`: Siri asks for the title after "Play in Psalms" (the parameter cannot be inside the phrase) and the title is understood well enough.
5. Cold start (app closed) and warm start (app in the background) both run the command; the command may be written a moment after the app came to the
   front, so the app looks once more after 0.8 s. Does this suffice? Does the app come to the front by itself (`openAppWhenRun`) from the lock screen?
6. The German spoken answers and the localised titles show in the Shortcuts app on a German phone.

### How to test Siri (TestFlight)

1. Install the build, open the app once, pair it. (Siri learns the phrases after the first start; this may take a minute.)
2. Shortcuts app -> search "Psalms" (or "All Shortcuts" -> Psalms): the five actions appear (Continue listening, Play, Next title, Previous title, What is
   new). Run each one by tapping it; Play asks for the title.
3. By voice: "Hey Siri, Weiterhören in Psalms" / "Hey Siri, continue listening in Psalms"; "Hey Siri, Abspielen in Psalms" (Siri asks what, answer with a
   title, an author or a label); "Hey Siri, Nächster Titel in Psalms"; "Hey Siri, Was gibt es Neues in Psalms".
4. Try with the app closed, in the background and with the phone locked; try a title that does not exist (Siri answers, the app stays quiet).

## Siri chooses another service (project owner, 2026-10-30: "it prefers Spotify")

- A request **without the name of an app** ("Spiele X") goes to the default music service of the person (Spotify, Apple Music). An app cannot take that over by its own
  choice: the person has to say the name of the app, or set the service as default where the system offers it.
- With the name ("... in Psalms") Siri uses the phrases of the shortcuts. "Psalms" is an unusual word and may be understood as something else. Since 2026-10-30 the
  `Info.plist` lists **alternative names** (`INAlternativeAppNames`: "Psalm", "Psalmen", "Davids Mediathek"), which Siri accepts in the phrases of the shortcuts.
  Not tried on a device; the pronunciation hints that the key allows are not set.
- The person can also make a **shortcut with a phrase of their own** (Shortcuts app > the action of Psalms > "Add to Siri"), for example "Mein Hörbuch": no app name is
  needed then and nothing is confused with Spotify. Worth telling the family.
- To be tried on a device: what Siri says when "in Psalms" is spoken, and whether "in Psalm" works.

## SiriKit media intents: "Spiele <Titel>" without the name of the app (project owner, 2026-10-30)

Goal: Siri starts playback with the short phrase, as with Spotify. The App Intents above need the app name in the phrase; `INPlayMediaIntent` does not.

**What was written (2026-10-30, nothing compiled or run; the CI on macOS is the first compiler):**

- Xcode target **PsalmsIntents** (app extension, Swift, iOS 15) in `app/ios/Runner.xcodeproj/project.pbxproj` (by hand, checked by a script for balanced
  braces, unique and defined ids), embedded in Runner (phase "Embed Foundation Extensions", `dstSubfolderSpec = 13`, plus a target dependency). Bundle id
  `$(PSALM_BUNDLE_ID).intents`, device family and version from the same settings as the app (`FLUTTER_BUILD_NUMBER`, `FLUTTER_BUILD_NAME`), because Apple
  rejects an extension whose version differs from the app.
- `app/ios/PsalmsIntents/`: `IntentHandler.swift` (`INExtension`, returns the handler), `PlayMediaIntentHandler.swift` (`INPlayMediaIntentHandling`:
  `resolveMediaItems` takes the spoken text from the media search - name, album, artist - and returns **one** `INMediaItem` whose identifier and title are that
  text, or `needsValue` if nothing was spoken; `confirm` answers `.ready`; `handle` answers `.handleInApp`), `Info.plist` (`NSExtension`, point
  `com.apple.intents-service`, `IntentsSupported` = `INPlayMediaIntent`, `IntentsRestrictedWhileLocked` empty),
  `PsalmsIntents.entitlements` (`com.apple.developer.siri`; no app group: the intent itself carries the text).
- The app: `Runner/Runner.entitlements` (`com.apple.developer.siri`, set in the project for Runner; the CI only adds the device-name key when its variable is
  true), `Runner/SiriMediaHandoff.swift` (turns the intent into the same JSON as the App Intents: `{"action":"play","query":"..."}`, or `{"action":"continue"}` when
  nothing was spoken, into the key `psalms.voice.pending`, and tells Flutter), `AppDelegate.swift` (`application(_:handle:completionHandler:)`, the path for apps
  that play audio in the background) and `SceneDelegate.swift` (`scene(_:willConnectTo:options:)` and `scene(_:continue:)`, the path as an `NSUserActivity` whose
  `interaction.intent` is the `INPlayMediaIntent`). The same text within five seconds is taken once, because both paths could fire.
- Flutter: `VoiceChannel.listen` answers a call `pendingChanged` from the native side by taking the command at once (`main.dart`: `_runVoice`, with the existing
  retry); a cold start is covered by the existing take after the start and on resume. Test in `test/voice_commands_test.dart`.
- The extension resolves to the spoken text; the app searches the library (`playByVoice`). The extension cannot know the library, so Siri says "OK" even for a
  title that does not exist; the app then stays silent.

**What the owner did in the Apple account:** enabled the capability Siri on the App IDs `com.the-abraham-box.psalm` and `com.the-abraham-box.psalm.ipad`;
created the App IDs `com.the-abraham-box.psalm.intents` and `com.the-abraham-box.psalm.ipad.intents` (Siri on); made four App Store profiles.
**Secrets expected:** `IOS_PROFILE_IPHONE_BASE64`, `IOS_PROFILE_IPAD_BASE64` (the apps, made again with Siri), `IOS_PROFILE_IPHONE_INTENTS_BASE64`,
`IOS_PROFILE_IPAD_INTENTS_BASE64` (the extensions), team `RRT2J4HCC5` (`IOS_TEAM_ID`). Profile names are read from the decoded profiles, not written in the repository.
Signing: see `docs/releasing.md`.

**Unverified (a Mac or an iPhone is needed), highest risk first:**

1. Signing two targets in one `flutter build ipa` with the profile name of the extension given through the project setting `PSALM_INTENTS_PROFILE` and an
   `ExportOptions.plist` with both bundle ids. Look at the log of `package-ios` for "provisioning profile" errors.
2. The exact callback for `.handleInApp` in an app with scenes: both `application(_:handle:completionHandler:)` (written without `override`; if
   `FlutterAppDelegate` declares it the compiler will say so) and the scene methods are implemented; which one fires is unknown. Also whether `override` of
   the two `FlutterSceneDelegate` methods compiles with the Flutter of the CI.
3. `INSupportedMediaCategories`: **removed again from the app's `Info.plist`** (TestFlight upload, 2026-10-30: "Invalid Bundle. The SupportedMediaCategories key in the 'Runner.app' Info.plist cannot be present unless the INPlayMediaIntent intent is supported", code 90775). Apple's key page only says the key is set with the app's Supported Intents (the plist key that Xcode writes for that list is not named there; only `INMediaCategoryGeneral` is documented). The extension's `IntentsSupported = INPlayMediaIntent` carries Siri's routing meanwhile. To bring the categories back: find out in Xcode what adding `INPlayMediaIntent` under Supported Intents of the app target writes into `Info.plist`, and set both.
4. `INPlayMediaMediaItemResolutionResult.successes(with:)` (array form) and `INMediaItem(identifier:title:type:artwork:)`; the item type is the type
   of the search or `.unknown`.
5. The keys of the extension `Info.plist` (`IntentsSupported`, `IntentsRestrictedWhileLocked` empty meaning "allowed on the lock screen",
   `NSExtensionPrincipalClass` = `$(PRODUCT_MODULE_NAME).IntentHandler`) and the embed phase name/placement before "Thin Binary" (a build cycle would show
   up as "Cycle inside Runner").
6. Whether Siri prefers Spotify anyway for "Spiele X": the person may have to choose Psalms as the app for the media category (Settings > Siri, or by answering
   Siri once); a name like "Psalms" may need the alternative names (`INAlternativeAppNames`, already there).
7. That "Spiele" with no words starts "continue" (the extension answers `needsValue`, so Siri asks what to play; the app-side `continue` is only reached when the
   intent arrives empty).
