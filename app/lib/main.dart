// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'audio/audio_backend.dart';
import 'audio/just_audio_backend.dart';
import 'audio/pipewire_backend.dart';
import 'demo_data.dart';
import 'models.dart';
import 'l10n/app_localizations.dart';
import 'player_controller.dart';
import 'screens/connect_screen.dart';
import 'screens/home_screen.dart';
import 'server/api_client.dart';
import 'server/app_paths.dart';
import 'server/credentials.dart';
import 'server/discovery.dart';
import 'server/last_played.dart';
import 'server/server_source.dart';
import 'screens/about_screen.dart';
import 'screens/library_screen.dart';
import 'screens/search_screen.dart';
import 'screens/settings_screen.dart';
import 'theme.dart';
import 'widgets.dart';

Future<void> main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();
  await AppPaths.init();
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
/// and iOS. Null elsewhere or if the library or PipeWire is missing; the app then runs silently
/// on demo data.
AudioBackend? _openBackend() {
  if (Platform.isAndroid || Platform.isIOS) return JustAudioBackend();
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
  );
  final _credentials = CredentialStore.standard();
  _Phase _phase = _Phase.starting;
  ConnectError? _connectError;
  bool _canRetry = false;
  ApiClient? _api;
  String? _libraryVersion;
  Timer? _libraryTimer;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _start();
  }

  /// When the app leaves the screen or is closed, where the listening is gets written down first.
  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state != AppLifecycleState.resumed) _player.flush();
  }

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
    final error = await _signIn(stored);
    if (!mounted) return;
    setState(() {
      _connectError = error;
      _canRetry = error == ConnectError.network || error == ConnectError.certificate;
      _phase = error == null ? _Phase.ready : _Phase.connect;
    });
  }

  /// Loads the library with stored credentials. If the server does not answer
  /// where it was, it is looked for by mDNS under its id; a new address is kept.
  Future<ConnectError?> _signIn(Credentials stored) async {
    var creds = stored;
    for (var attempt = 0; attempt < 2; attempt++) {
      final api = ApiClient(address: creds.address, fingerprint: creds.fingerprint, token: creds.token);
      try {
        _player.attachServer(ServerSource(api, partCache()), await loadLibrary(api));
        _watchLibrary(api);
        return null;
      } on ApiException catch (e) {
        api.close();
        if (e.status == 401) {
          _credentials.clear();
          return ConnectError.code;
        }
        if (e.code == 'certificate') return ConnectError.certificate;
        if (e.code != 'network' || attempt == 1 || creds.serverId.isEmpty) return ConnectError.network;
        final found = await findServer(creds.serverId);
        if (found == null || found.address == creds.address) return ConnectError.network;
        creds = Credentials(
          serverId: creds.serverId,
          address: found.address,
          fingerprint: creds.fingerprint,
          token: creds.token,
          deviceName: creds.deviceName,
        );
        _credentials.save(creds);
      }
    }
    return ConnectError.network;
  }

  Future<ConnectError?> _pair(String text, String deviceName) async {
    final pairing = PairingAddress.parse(text);
    if (pairing == null) return ConnectError.address;
    try {
      final (api, _) = await ApiClient.pair(pairing, deviceName.trim().isEmpty ? Platform.localHostname : deviceName.trim());
      final creds = Credentials(
        serverId: pairing.serverId,
        address: pairing.address,
        fingerprint: pairing.fingerprint,
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
    api.libraryVersion().then((v) => _libraryVersion = v, onError: (Object _) {});
    _libraryTimer = Timer.periodic(const Duration(seconds: 30), (_) => _checkLibrary());
    _player.refresher = () => _reloadLibrary(force: true);
  }

  Future<void> _checkLibrary() => _reloadLibrary(force: false);

  Future<void> _reloadLibrary({required bool force}) async {
    final api = _api;
    if (api == null || !mounted) return;
    try {
      final version = await api.libraryVersion();
      if (!force && version == _libraryVersion) return;
      final loaded = await loadLibrary(api);
      if (!mounted) return;
      _libraryVersion = version;
      _player.updateLibrary(loaded);
    } on ApiException {
      // The server is not reachable right now; the next look tries again.
    }
  }

  /// Forgets the pairing, stops the sound and shows the connect screen again.
  void _disconnect() {
    _libraryTimer?.cancel();
    _player.refresher = null;
    _api = null;
    if (_player.playing) _player.toggle();
    _credentials.clear();
    setState(() {
      _connectError = null;
      _canRetry = false;
      _phase = _Phase.connect;
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _libraryTimer?.cancel();
    _player.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return PlayerScope(
      controller: _player,
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
        themeMode: ThemeMode.dark,
        home: switch (_phase) {
          _Phase.starting => const Scaffold(body: Center(child: CircularProgressIndicator())),
          _Phase.connect => ConnectScreen(
              deviceName: Platform.localHostname,
              error: _connectError,
              initialAddress: widget.pairing,
              onPair: _pair,
              onDemo: () => setState(() => _phase = _Phase.ready),
              onRetry: _canRetry ? _start : null,
            ),
          _Phase.ready => Shell(
              credentials: widget.demo ? null : _credentials.load(),
              onDisconnect: widget.demo ? null : _disconnect,
            ),
        },
      ),
    );
  }
}

/// Frame with navigation: at the bottom on phones, on the left on the desktop.
/// The mini player always sits above or below it.
class Shell extends StatefulWidget {
  const Shell({super.key, this.credentials, this.onDisconnect});

  /// For the settings page; null in demo mode.
  final Credentials? credentials;
  final VoidCallback? onDisconnect;

  @override
  State<Shell> createState() => _ShellState();
}

class _ShellState extends State<Shell> {
  int _index = 0;

  late final _pages = [
    const HomeScreen(),
    const SearchScreen(),
    const LibraryScreen(),
    SettingsScreen(credentials: widget.credentials, onDisconnect: widget.onDisconnect),
    const AboutScreen(),
  ];
  static const _icons = [
    (Icons.home_outlined, Icons.home),
    (Icons.search, Icons.search),
    (Icons.library_books_outlined, Icons.library_books),
    (Icons.settings_outlined, Icons.settings),
    (Icons.info_outline, Icons.info),
  ];

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final labels = [l.navHome, l.navSearch, l.navLibrary, l.navSettings, l.navAbout];
    final wide = MediaQuery.sizeOf(context).width >= 800;
    final body = IndexedStack(index: _index, children: _pages);

    if (wide) {
      return Scaffold(
        body: Column(
          children: [
            Expanded(
              child: Row(
                children: [
                  NavigationRail(
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
                            style: Theme.of(context).textTheme.titleLarge?.copyWith(
                                  color: Theme.of(context).colorScheme.primary,
                                ),
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
      body: body,
      bottomNavigationBar: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const MiniPlayer(),
          NavigationBar(
            selectedIndex: _index,
            onDestinationSelected: (i) => setState(() => _index = i),
            destinations: [
              for (final (i, (icon, selected)) in _icons.indexed)
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
