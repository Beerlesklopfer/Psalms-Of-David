// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'audio/audio_backend.dart';
import 'cd/cd_kit.dart';
import 'audio/just_audio_backend.dart';
import 'audio/mpris.dart';
import 'audio/pipewire_backend.dart';
import 'audio/psalm_audio_handler.dart';
import 'demo_data.dart';
import 'theme_mode.dart';
import 'models.dart';
import 'l10n/app_localizations.dart';
import 'labels.dart';
import 'library_hiding.dart';
import 'offline/library_cache.dart';
import 'offline/offline_controller.dart';
import 'offline/offline_view.dart';
import 'offline/outbox.dart';
import 'player_controller.dart';
import 'playlists.dart';
import 'profile.dart';
import 'server/profile_api.dart';
import 'screens/share_button.dart';
import 'screens/connect_screen.dart';
import 'screens/home_screen.dart';
import 'server/api_client.dart';
import 'server/app_paths.dart';
import 'server/credentials.dart';
import 'server/device_name.dart';
import 'server/device_pairing.dart';
import 'server/hiding_api.dart';
import 'server/labels_api.dart';
import 'server/playlist_api.dart';
import 'server/discovery.dart';
import 'audio/playback_settings.dart';
import 'server/last_played.dart';
import 'server/outbox_api.dart';
import 'server/server_source.dart';
import 'screens/about_screen.dart';
import 'screens/library_screen.dart';
import 'screens/playlists_screen.dart';
import 'podcast_downloads.dart';
import 'screens/podcasts_screen.dart';
import 'screens/radio_screen.dart';
import 'screens/search_screen.dart';
import 'screens/settings_screen.dart';
import 'theme.dart';
import 'voice/voice_commands.dart';
import 'widgets.dart';

Future<void> main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();
  await AppPaths.init();
  await PsalmAudioHandler.init();
  runApp(PsalmApp(pairing: _pairingAddress(args)));
}

/// A pairing address given on the command line: `psalm "david://pair?..."`.
String? _pairingAddress(List<String> args) {
  for (final a in args) {
    if (a.startsWith('david://pair?')) return a;
  }
  return null;
}

/// The sound engine, where there is one: PipeWire on Linux, ExoPlayer and AVPlayer on Android
/// and iOS, Media Foundation on Windows. Null elsewhere or if the library or PipeWire is missing; the app then runs silently
/// on demo data.
AudioBackend? _openBackend() {
  if (Platform.isAndroid || Platform.isIOS || Platform.isMacOS || Platform.isWindows) return JustAudioBackend();
  if (!Platform.isLinux) return null;
  try {
    return PipeWireBackend.open();
  } on AudioException catch (e) {
    debugPrint('No sound: ${e.message}');
    return null;
  }
}

/// Development aid: with `PSALM_DEMO_FILE=<audio file>` every demo track plays
/// that file, so the player can be heard before the server delivers books.
List<Book> _demoLibrary() {
  final file = Platform.environment['PSALM_DEMO_FILE'];
  if (file == null || file.isEmpty) return demoBooks;
  return [
    for (final b in demoBooks)
      Book(
        id: b.id,
        title: b.title,
        author: b.author,
        color: b.color,
        tracks: [
          for (final t in b.tracks)
            Track(disc: t.disc, number: t.number, title: t.title, length: t.length, path: file),
        ],
      ),
  ];
}

class PsalmApp extends StatefulWidget {
  const PsalmApp({super.key, this.locale, this.demo = false, this.pairing});

  /// A pairing address that the program was started with; it is put into the connect screen.
  final String? pairing;

  /// For tests: demo data, no sound engine, and no look at the stored pairing.
  final bool demo;

  /// For tests and trying things out only; otherwise the system language applies.
  final Locale? locale;

  @override
  State<PsalmApp> createState() => _PsalmAppState();
}

enum _Phase { starting, connect, ready }

class _PsalmAppState extends State<PsalmApp> with WidgetsBindingObserver {
  late final PlayerController _player = PlayerController(
    _demoLibrary(),
    backend: widget.demo ? null : _openBackend(),
    lastStore: widget.demo ? null : LastPlayedStore.standard(),
    playbackStore: widget.demo ? null : PlaybackSettingsStore.standard(),
  );
  // The playlists of the profile, with the copy on this device (not in the demo).
  late final HidingController _hiding = HidingController(
    onChanged: () => unawaited(_reloadLibrary(force: true)),
  );
  late final LabelsController _labels = LabelsController(
    onChanged: () => unawaited(_reloadLibrary(force: true)),
  );
  // The own profile: names, picture, language, PIN (not in the demo); the copy on this device serves the offline view.
  late final ProfileController _profile = ProfileController(
    store: widget.demo ? null : ProfileStore.standard(),
  );
  late final PlaylistController _playlists = PlaylistController(
    store: widget.demo ? null : PlaylistStore.standard(),
  );
  // Offline mode: the switch and the changes that wait for the server, and the library as it was last loaded.
  late final OfflineController _offline = OfflineController(
    outbox: widget.demo ? Outbox.memory() : Outbox(File('${AppPaths.base.path}/outbox.json')),
    store: widget.demo ? null : OfflineSettingsStore.standard(),
  );
  final _cache = LibraryCache.standard();
  bool _wasManual = false;
  final _credentials = CredentialStore.standard();
  _Phase _phase = _Phase.starting;
  ConnectError? _connectError;
  bool _canRetry = false;
  ApiClient? _api;
  String? _libraryVersion;
  String? _playlistsVersion;
  String? _hiddenVersion;
  Timer? _libraryTimer;
  MprisService? _mpris;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    // A Siri media intent that reaches the running app (iOS): the native side says so, the command is taken at once.
    if (!widget.demo) _voice.listen(() => unawaited(_runVoice(retry: true)));
    if (!widget.demo) loadThemeMode();
    themeModeNotifier.addListener(_themeChanged);
    _player.onPlaylistPosition = _playlists.remember;
    PsalmAudioHandler.instance?.attach(_player);
    _player.offline = _offline;
    // The podcast episodes on this device: the offline view lists them and the player reports their state to it.
    if (!widget.demo) _player.podcastDownloads = PodcastDownloads.standard();
    _labels
      ..offline = _offline
      ..bookOf = _bookOf
      ..patchBook = _player.patchBook;
    _hiding
      ..offline = _offline
      ..bookOf = _bookOf
      ..booksNow = (() => _player.library)
      ..removeBook = _player.removeBookLocally;
    _wasManual = _offline.manual;
    _offline.addListener(_onOfflineChanged);
    // The media keys of the keyboard and the sound menu of the desktop (Linux): the player announces itself on the session bus.
    if (Platform.isLinux && !widget.demo) {
      unawaited(MprisService.start(_player).then((s) => _mpris = s));
    }
    unawaited(defaultDeviceName().then((n) {
      if (mounted) setState(() => _deviceName = n);
    }));
    _start();
  }

  Book? _bookOf(String id) => _player.fullLibrary.where((b) => b.id == id).firstOrNull;

  /// The lists follow the offline mode; switching it off sends what waited and loads the library again.
  void _onOfflineChanged() {
    _player.setOfflineView(_offline.active);
    if (_wasManual && !_offline.manual) unawaited(_reloadLibrary(force: true));
    _wasManual = _offline.manual;
  }

  /// Sends the changes made without the server. True if the server took or refused any (the library is then loaded again).
  Future<bool> _syncOutbox() async {
    final api = _api;
    if (api == null || _offline.manual) return false;
    final r = await _offline.sync(ServerOutboxApi(api));
    if (!identical(api, _api)) return false;
    return r != null && (r.sent > 0 || r.rejected.isNotEmpty);
  }

  /// The button "try again" of the offline bar: looks for the server now.
  Future<void> _retryOnline() => _reloadLibrary(force: true);

  /// The library as it was loaded last, for the start without the server (or in the offline mode): the person can listen to what is downloaded.
  Future<bool> _openCached(Credentials creds, {required bool unreachable}) async {
    final cached = _cache.read();
    if (cached == null) return false;
    final api = ApiClient(address: creds.address, fingerprint: creds.fingerprint, ca: creds.ca, token: creds.token);
    _player.attachServer(ServerSource(api, partCache()), cached);
    _offline.setUnreachable(unreachable);
    _player.setOfflineView(_offline.active);
    _watchLibrary(api);
    return true;
  }

  /// When the app leaves the screen or is closed, where the listening is gets written down first.
  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state != AppLifecycleState.resumed) {
      _player.flush();
    } else {
      // Back in the foreground: what the other devices did meanwhile.
      // The voice command waits for the sync, so "continue" starts from the newest listening of the account.
      unawaited(_checkLibrary().then((_) => _runVoice(retry: true)));
    }
  }

  final VoiceChannel _voice = VoiceChannel();

  /// Runs the command that Siri left for the app (iOS); nothing happens without one or while the library is not loaded.
  /// If nothing matches, the app stays silent: Siri has already answered.
  /// [retry]: the intent may write its command just after the app came to the front, so it looks once more a moment later.
  Future<void> _runVoice({bool retry = false}) async {
    if (widget.demo || _phase != _Phase.ready) return;
    var command = await _voice.takePending();
    if (command == null && retry) {
      await Future<void>.delayed(const Duration(milliseconds: 800));
      command = await _voice.takePending();
    }
    if (command == null || !mounted) return;
    switch (command.action) {
      case VoiceAction.continueListening:
        _player.resumeListening();
      case VoiceAction.play:
        _player.playByVoice(command.query);
      case VoiceAction.next:
        _player.next();
      case VoiceAction.previous:
        _player.previous();
      case VoiceAction.showNew:
        showStartRequests.value++;
    }
  }

  Timer? _retryTimer;
  int _retries = 0;

  /// What this device proposes as its name when it is paired (the model of a phone, the host name of a computer).
  String _deviceName = Platform.localHostname;

  Future<void> _start() async {
    final demo = Platform.environment['PSALM_DEMO_FILE'];
    if (widget.demo || (demo != null && demo.isNotEmpty)) {
      setState(() => _phase = _Phase.ready);
      return;
    }
    final stored = _credentials.load();
    if (stored == null) {
      setState(() => _phase = _Phase.connect);
      return;
    }
    _retryTimer?.cancel();
    final error = await _signIn(stored);
    if (!mounted) return;
    // The server was not reachable: look again by itself, after 5 s, 10 s, 20 s ... at most a minute.
    if (error == ConnectError.network) {
      _retries++;
      final wait = Duration(seconds: (5 * (1 << (_retries - 1).clamp(0, 4))).clamp(5, 60));
      _retryTimer = Timer(wait, () {
        if (mounted && _phase == _Phase.connect) unawaited(_start());
      });
    } else {
      _retries = 0;
    }
    setState(() {
      _connectError = error;
      _canRetry = error == ConnectError.network || error == ConnectError.certificate;
      _phase = error == null ? _Phase.ready : _Phase.connect;
    });
    if (error == null) unawaited(_followListening().then((_) => _runVoice()));
  }

  /// Loads the library with stored credentials. If the server does not answer
  /// where it was, it is looked for by mDNS under its id; a new address is kept.
  Future<ConnectError?> _signIn(Credentials stored) async {
    var creds = stored;
    // The offline mode is on: the server is not asked; the library of the last time is shown.
    if (_offline.manual && await _openCached(creds, unreachable: false)) return null;
    for (var attempt = 0; attempt < 2; attempt++) {
      final api = ApiClient(
        address: creds.address,
        fingerprint: creds.fingerprint,
        ca: creds.ca,
        token: creds.token,
      );
      try {
        final loaded = await loadLibrary(api);
        _player.attachServer(ServerSource(api, partCache()), loaded);
        _offline.setUnreachable(false);
        _player.setOfflineView(_offline.active);
        unawaited(_cache.write(loaded));
        _watchLibrary(api);
        // What was changed without the server the last time goes now; the library is loaded again afterwards.
        unawaited(_syncOutbox().then((changed) => changed ? _reloadLibrary(force: true) : null));
        return null;
      } on ApiException catch (e) {
        api.close();
        // The server does not know the token: the pairing stays until a new one succeeds (a short fault of the server must not unpair the device).
        if (e.status == 401) return ConnectError.signedOut;
        if (e.code == 'certificate') return ConnectError.certificate;
        // The name of a server with a certificate from a certificate authority has to stay: another
        // address found by mDNS would not match the certificate.
        if (e.code != 'network' || attempt == 1 || creds.serverId.isEmpty || creds.ca) {
          return e.code == 'network' && await _openCached(creds, unreachable: true) ? null : ConnectError.network;
        }
        final found = await findServer(creds.serverId);
        if (found == null || found.address == creds.address) {
          return await _openCached(creds, unreachable: true) ? null : ConnectError.network;
        }
        creds = Credentials(
          serverId: creds.serverId,
          address: found.address,
          fingerprint: creds.fingerprint,
          ca: creds.ca,
          token: creds.token,
          deviceName: creds.deviceName,
        );
        _credentials.save(creds);
      }
    }
    return await _openCached(creds, unreachable: true) ? null : ConnectError.network;
  }

  Future<ConnectError?> _pair(String text, String deviceName) async {
    final pairing = PairingAddress.parse(text);
    if (pairing == null) return ConnectError.address;
    try {
      final (api, _) = await ApiClient.pair(
        pairing,
        deviceName.trim().isEmpty ? Platform.localHostname : deviceName.trim(),
      );
      final creds = Credentials(
        serverId: pairing.serverId,
        address: pairing.address,
        fingerprint: pairing.fingerprint,
        ca: pairing.ca,
        token: api.token!,
        deviceName: deviceName.trim(),
      );
      _credentials.save(creds);
      api.close();
      final error = await _signIn(creds);
      if (error == null && mounted) setState(() => _phase = _Phase.ready);
      return error;
    } on ApiException catch (e) {
      return switch (e.code) {
        'enrollment_invalid' => ConnectError.code,
        'device_limit_reached' => ConnectError.limit,
        'network' => ConnectError.network,
        'certificate' => ConnectError.certificate,
        _ => ConnectError.other,
      };
    }
  }

  /// Looks at the server now and then for a changed library (a new CD, a release) and loads it
  /// again by itself. The check is one small request; the reload happens only on a change.
  void _watchLibrary(ApiClient api) {
    _api = api;
    _libraryTimer?.cancel();
    _playlists.api = ServerPlaylistApi(api);
    unawaited(_playlists.load());
    _hiding.api = ServerHidingApi(api);
    _labels.api = ServerLabelApi(api);
    _profile.api = ServerProfileApi(api);
    unawaited(_profile.load());
    unawaited(_hiding.load());
    api.libraryVersions().then((v) {
      _libraryVersion = v.version;
      _playlistsVersion = v.playlists;
      _hiddenVersion = v.hidden;
    }, onError: (Object _) {});
    _libraryTimer = Timer.periodic(const Duration(seconds: 30), (_) => _checkLibrary());
    _player.refresher = () => _reloadLibrary(force: true);
  }

  Future<void> _checkLibrary() async {
    await _reloadLibrary(force: false);
    await _followListening();
  }

  /// What another device of the account listened to: the player shows it when it is newer than what this device did (the listening is not part
  /// of the fingerprint of the library, so it has to be asked for).
  Future<void> _followListening() async {
    final api = _api;
    if (api == null || !mounted || _offline.manual || _offline.unreachable) return;
    try {
      _player.followServerListening(await api.continueListening().timeout(const Duration(seconds: 4)));
    } on Object {
      // The next look tries again.
    }
  }

  Future<void> _reloadLibrary({required bool force}) async {
    final api = _api;
    // The offline mode (the person's switch) does not ask the server at all.
    if (api == null || !mounted || _offline.manual) return;
    // The pairing can be forgotten while this waits for the server: what comes back for the old one is dropped then,
    // or it would put the old account's library back into the player and the cache that `_disconnect` just cleared.
    bool stale() => !mounted || !identical(api, _api);
    try {
      final v = await api.libraryVersions();
      if (stale()) return;
      // The server answers again: what waits is sent first, so that the library that is loaded has it, and everything is loaded again.
      if (_offline.unreachable) {
        _offline.setUnreachable(false);
        force = true;
      }
      if (await _syncOutbox()) force = true;
      if (stale()) return;
      // The playlists of this profile have their own fingerprint: a change of another profile's lists is not seen here.
      final playlistsChanged = v.playlists != _playlistsVersion;
      _playlistsVersion = v.playlists;
      // What this profile hid (or the setting) decides which items the list has: another device of the profile may have changed it.
      final hiddenChanged = v.hidden != _hiddenVersion;
      _hiddenVersion = v.hidden;
      if (hiddenChanged) unawaited(_hiding.load());
      if (force || hiddenChanged || v.version != _libraryVersion) {
        final loaded = await loadLibrary(api);
        if (stale()) return;
        _libraryVersion = v.version;
        _player.updateLibrary(loaded);
        unawaited(_cache.write(loaded));
        // A change of the library can change which entries are available.
        unawaited(_playlists.load(force: force));
      } else if (playlistsChanged) {
        unawaited(_playlists.load());
      }
    } on ApiException catch (e) {
      // The server is not reachable right now; the next look tries again. The lists show what is downloaded meanwhile.
      if (e.code == 'network' && !stale()) _offline.setUnreachable(true);
    }
  }

  /// Forgets the pairing, stops the sound and shows the connect screen again.
  void _disconnect() {
    _libraryTimer?.cancel();
    _player.refresher = null;
    _playlists.api = null;
    _hiding.api = null;
    _labels.api = null;
    unawaited(_profile.forget());
    _api = null;
    if (_player.playing) _player.toggle();
    _credentials.clear();
    // What waits and the cached library belong to this pairing, not to the next one.
    _offline.outbox.clear();
    _player.podcastDownloads?.forgetList();
    _offline.setUnreachable(false);
    unawaited(_cache.clear());
    setState(() {
      _connectError = null;
      _canRetry = false;
      _phase = _Phase.connect;
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _voice.stopListening();
    _libraryTimer?.cancel();
    _retryTimer?.cancel();
    _offline.removeListener(_onOfflineChanged);
    _offline.dispose();
    unawaited(_mpris?.stop());
    PsalmAudioHandler.instance?.detach(_player);
    themeModeNotifier.removeListener(_themeChanged);
    _player.dispose();
    _playlists.dispose();
    _hiding.dispose();
    _labels.dispose();
    _profile.dispose();
    super.dispose();
  }

  void _themeChanged() => setState(() {});

  @override
  Widget build(BuildContext context) {
    return PlayerScope(
      controller: _player,
      child: PlaylistScope(
        controller: _playlists,
        child: HidingScope(
          controller: _hiding,
          child: LabelsScope(
            controller: _labels,
            child: ServerAddressScope(
            address: widget.demo ? null : _credentials.load()?.address,
            child: ProfileScope(
            controller: _profile,
            child: MaterialApp(
            onGenerateTitle: (context) => AppLocalizations.of(context).appTitle,
            debugShowCheckedModeBanner: false,
            locale: widget.locale,
            localizationsDelegates: const [
              AppLocalizations.delegate,
              GlobalMaterialLocalizations.delegate,
              GlobalWidgetsLocalizations.delegate,
              GlobalCupertinoLocalizations.delegate,
            ],
            // English comes first: for an unsupported system language Flutter falls
            // back to the first locale in this list.
            supportedLocales: [
              const Locale('en'),
              for (final l in AppLocalizations.supportedLocales)
                if (l.languageCode != 'en') l,
            ],
            theme: PsalmTheme.light,
            darkTheme: PsalmTheme.dark,
            themeMode: themeModeNotifier.value,
            home: switch (_phase) {
              _Phase.starting => const Scaffold(body: Center(child: CircularProgressIndicator())),
              _Phase.connect => ConnectScreen(
                deviceName: _deviceName,
                error: _connectError,
                searching: _canRetry && _connectError == ConnectError.network,
                initialAddress: widget.pairing,
                onPair: _pair,
                onDemo: () => setState(() => _phase = _Phase.ready),
                onRetry: _canRetry ? _start : null,
              ),
              _Phase.ready => Shell(
                credentials: widget.demo ? null : _credentials.load(),
                onDisconnect: widget.demo ? null : _disconnect,
                api: widget.demo ? null : _api,
                offline: _offline,
                onSync: widget.demo ? null : _retryOnline,
              ),
            },
          ),
          ),
          ),
          ),
        ),
      ),
    );
  }
}

/// Frame with navigation: at the bottom on phones, on the left on the desktop.
/// The mini player always sits above or below it.
class Shell extends StatefulWidget {
  const Shell({super.key, this.credentials, this.onDisconnect, this.api, this.offline, this.onSync});

  /// The offline mode (the bar, the switch, the changes that wait); null in tests that do not need it.
  final OfflineController? offline;

  /// Looks for the server and sends what waits.
  final Future<void> Function()? onSync;

  /// The server, for podcasts and radio; null in demo mode.
  final ApiClient? api;

  /// For the settings page; null in demo mode.
  final Credentials? credentials;
  final VoidCallback? onDisconnect;

  @override
  State<Shell> createState() => _ShellState();
}

class _ShellState extends State<Shell> {
  int _index = 0;

  /// The CD symbols: the server's drive and jobs; null without a server.
  late final CdKit? _cd = widget.api == null ? null : createCdKit(widget.api!);

  int _toldRejected = 0;

  @override
  void initState() {
    super.initState();
    widget.offline?.addListener(_tellRejected);
    showStartRequests.addListener(_showStart);
  }

  /// A voice command ("what is new") asks for the start page.
  void _showStart() {
    if (mounted) setState(() => _index = 0);
  }

  /// A change that the server did not take is told once, in a bar; the settings keep the list until the person has seen it.
  void _tellRejected() {
    final n = widget.offline?.rejected.length ?? 0;
    if (n > _toldRejected && mounted) {
      final l = AppLocalizations.of(context);
      ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text('${l.offlineRejectedTitle}: $n')));
    }
    _toldRejected = n;
  }

  @override
  void dispose() {
    widget.offline?.removeListener(_tellRejected);
    showStartRequests.removeListener(_showStart);
    _cd?.dispose();
    super.dispose();
  }

  late final _pages = [
    const HomeScreen(),
    SearchScreen(api: widget.api),
    LibraryScreen(cd: _cd, api: widget.api),
    PodcastsScreen(api: widget.api),
    OnlineOnly(child: RadioScreen(api: widget.api)),
    SettingsScreen(
      credentials: widget.credentials,
      onDisconnect: widget.onDisconnect,
      devicePairing: widget.api == null ? null : ServerDevicePairingApi(widget.api!),
      onSync: widget.onSync,
      onShareListening: widget.api?.setShareListening,
    ),
    const AboutScreen(),
  ];
  static const _icons = [
    (Icons.home_outlined, Icons.home),
    (Icons.search, Icons.search),
    (Icons.library_books_outlined, Icons.library_books),
    (Icons.podcasts_outlined, Icons.podcasts),
    (Icons.radio_outlined, Icons.radio),
    (Icons.settings_outlined, Icons.settings),
    (Icons.info_outline, Icons.info),
  ];

  @override
  Widget build(BuildContext context) {
    final offline = widget.offline;
    final shell = _build(context);
    return offline == null ? shell : OfflineScope(controller: offline, child: shell);
  }

  Widget _build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final labels = [
      l.navHome,
      l.navSearch,
      l.navLibrary,
      l.navPodcasts,
      l.navRadio,
      l.navSettings,
      l.navAbout,
    ];
    final wide = MediaQuery.sizeOf(context).width >= 800;
    final body = IndexedStack(index: _index, children: _pages);
    // The bar of the offline mode sits above the pages, over the whole width.
    final banner = OfflineBanner(onSync: widget.onSync == null ? null : () => unawaited(widget.onSync!()));

    if (wide) {
      return Scaffold(
        body: Column(
          children: [
            banner,
            Expanded(
              child: Row(
                children: [
                  // The rail scrolls when the window is too low for all of its entries.
                  LayoutBuilder(
                    builder: (context, constraints) => SingleChildScrollView(
                      child: ConstrainedBox(
                        constraints: BoxConstraints(minHeight: constraints.maxHeight),
                        child: IntrinsicHeight(
                          child: NavigationRail(
                            selectedIndex: _index,
                            onDestinationSelected: (i) => setState(() => _index = i),
                            labelType: NavigationRailLabelType.all,
                            leading: Padding(
                              padding: const EdgeInsets.symmetric(vertical: 20),
                              child: Column(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  Lyre(size: 44, color: Theme.of(context).colorScheme.primary),
                                  const SizedBox(height: 6),
                                  Text(
                                    l.appTitle,
                                    style: Theme.of(context).textTheme.titleLarge
                                        ?.copyWith(color: Theme.of(context).colorScheme.primary),
                                  ),
                                ],
                              ),
                            ),
                            destinations: [
                              for (final (i, (icon, selected)) in _icons.indexed)
                                NavigationRailDestination(
                                  icon: Icon(icon),
                                  selectedIcon: Icon(selected),
                                  label: Text(labels[i]),
                                ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                  const VerticalDivider(width: 1),
                  Expanded(child: body),
                ],
              ),
            ),
            const MiniPlayer(),
          ],
        ),
      );
    }

    return Scaffold(
      body: Column(children: [banner, Expanded(child: body)]),
      bottomNavigationBar: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const MiniPlayer(),
          // Six entries on a phone ("About" is a line in the settings): seven are too narrow for the German names, which broke in the middle of a word.
          NavigationBar(
            selectedIndex: _index < _icons.length - 1 ? _index : _icons.length - 2,
            onDestinationSelected: (i) => setState(() => _index = i),
            labelBehavior: NavigationDestinationLabelBehavior.onlyShowSelected,
            destinations: [
              for (final (i, (icon, selected)) in _icons.take(_icons.length - 1).toList().indexed)
                NavigationDestination(
                  icon: Icon(icon),
                  selectedIcon: Icon(selected),
                  label: labels[i],
                ),
            ],
          ),
        ],
      ),
    );
  }
}
