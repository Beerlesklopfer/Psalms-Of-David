# Background playback, lock screen and the car (CarPlay, Android Auto)

Status: background playback and the lock screen are built (Android, iOS); the browse tree for the car is **not** built, only its extension point.

## What exists

- `lib/audio/psalm_audio_handler.dart`: the `AudioHandler` of `audio_service` (a foreground service of the type media playback on Android, the remote command centre on iOS). It plays nothing itself. The `PlayerController` stays the one owner of the state: the commands of the system (play, pause, stop, seek, next, previous, fast forward 30 s, rewind 10 s, headset and car buttons) go to the controller, and the state of the controller (track, album, author, cover, position, speed, loading, error) is published to the system. Ticks of the controller are not messages: only a change of state or a jump of the position is one.
- The engine (`JustAudioBackend`) tells the controller when the **system** paused or started it (phone call, unplugged headphones, another app, the notification) through `ExternalControl.onExternalChange`, so the screen, the notification and the server (progress report on pause) never disagree.
- The next track is fetched about 45 s before the current one ends, so that the next one starts at once and the service never waits for the network with the screen locked.
- Android: `AndroidManifest.xml` has the permissions (WAKE_LOCK, FOREGROUND_SERVICE, FOREGROUND_SERVICE_MEDIA_PLAYBACK, POST_NOTIFICATIONS), the service `com.ryanheise.audioservice.AudioService` with the `MediaBrowserService` intent filter, the `MediaButtonReceiver`, and `MainActivity` extends `AudioServiceActivity`. The permission for the notification (Android 13+) is asked once at the first playback; playback works without it.
- iOS: `UIBackgroundModes: audio` is in `Info.plist`; `audio_service` fills the lock screen and the remote command centre.
- Linux keeps PipeWire and MPRIS (`audio/mpris.dart`); there is no handler there (`PsalmAudioHandler.instance` is null).

## Extension point for the car

`PsalmAudioHandler` overrides three methods that the system calls when a car browses: `getChildren(parentMediaId)`, `getMediaItem(mediaId)` and `playFromMediaId(mediaId)`. They are wired to the system already and **answer nothing yet**. The next step fills them from the controller (library, playlists, what was heard last), never from a second copy of the data:

- root (`AudioService.browsableRootId`): "Continue listening" (the books in `recentBooks`), "Library" (music, audiobooks; by owner as in the app), "Playlists";
- an item id is `bookId:trackIndex`, the same as the id of the published `MediaItem`; `playFromMediaId` opens that book at that track through `PlayerController.open`;
- only what the user may see (the library of the controller is already filtered by the server), no podcasts and radio in the first step, no blacklist label.

## What each car platform needs

Android Auto:
- the meta-data `com.google.android.gms.car.application` pointing to `res/xml/automotive_app_desc.xml` (`<automotiveApp><uses name="media"/></automotiveApp>`) in the manifest;
- the media browser service is the `AudioService` of `audio_service` (already declared);
- testing without a car: the Desktop Head Unit of the Android SDK (`desktop-head-unit`, started with "Start head unit server" in the developer settings of the phone and `adb forward tcp:5277 tcp:5277`). The app must be installed from a source that Android Auto accepts (developer mode "unknown sources" in the Android Auto settings).

CarPlay:
- the entitlement `com.apple.developer.carplay-audio` must be requested from Apple (a form in the developer account; approval takes time and is not guaranteed) and added to the provisioning profile and `Runner.entitlements`;
- Xcode on macOS is needed (the CarPlay simulator is part of Xcode: "I/O > External Displays > CarPlay");
- `audio_service` provides the content tree for CarPlay through the same `getChildren` calls; the scene delegate for CarPlay (`CPTemplateApplicationSceneDelegate`) must be declared in `Info.plist` (`UIApplicationSceneManifest`), which changes how the iOS app starts and needs a careful test on a real iPhone.

## What to check on a phone (nothing of this is tested on a device yet)

1. Start a book, lock the screen: the sound goes on, the lock screen shows track, album, author and cover, play/pause, seek bar, previous/next, rewind 10 s, fast forward 30 s.
2. Ten minutes with the screen locked, over at least one change of track: no gap longer than a second, the progress arrives on the server.
3. A phone call (or another app that plays): the player pauses and resumes after a short interruption; the screen of the app follows.
4. Unplug the headphones: it pauses.
5. Notification permission denied (Android 13+): playback works, only the notification is hidden.
6. Swipe the paused notification away: the service stops; starting playback again brings it back.
7. Headset button and car buttons: play/pause, next, previous.
