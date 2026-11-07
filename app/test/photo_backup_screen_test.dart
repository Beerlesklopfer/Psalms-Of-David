// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/photos/photo_backup_api.dart';
import 'package:psalm/photos/photo_backup_controller.dart';
import 'package:psalm/photos/photo_backup_ui.dart';
import 'package:psalm/photos/photo_device.dart';
import 'package:psalm/photos/photo_sync.dart';

/// The server of the screen: the backup on, sync not allowed, and what was changed.
class _Api implements PhotoBackupApi {
  _Api({this.off = false});

  final bool off;
  final saved = <Map<String, String>>[];
  var choices = {'enabled': 'on', 'mode': 'uni', 'network': 'wifi', 'charging': 'charging', 'videos': 'on', 'deletes': 'never'};

  PhotoSettings get _settings => PhotoSettings(
        folder: true,
        active: choices['enabled'] == 'on',
        settings: Map.of(choices),
        effective: Map.of(choices),
        locked: const {
          'mode': {'bi': 'no_bisync'},
        },
      );

  @override
  Future<PhotoSettings?> settings() async => off ? null : _settings;

  @override
  Future<PhotoSettings> putSettings(Map<String, String> c) async {
    saved.add(c);
    if (c['mode'] == 'bi') throw PhotoChoiceRefused({'mode': 'no_bisync'});
    choices = {...choices, ...c};
    return _settings;
  }

  @override
  Future<PhotoServerState> state() async => PhotoServerState(count: 12, bytes: 34 * 1000 * 1000);

  @override
  Future<Set<String>> missing(List<({String sha256, int size})> files) async => const {};

  @override
  Future<int?> received(String clientId) async => null;

  @override
  Future<PieceAnswer> putPiece({
    required String clientId,
    required String sha256,
    required int size,
    required String mime,
    required int start,
    required List<int> bytes,
    String? name,
    DateTime? takenAt,
  }) async =>
      const PieceDone(1, known: false);

  @override
  Future<void> putThumb(int photo, List<int> jpeg) async {}

  @override
  Future<({List<RemotePhoto> photos, int next})> changes(int since, {int limit = 200}) async => (photos: <RemotePhoto>[], next: since);

  @override
  Future<List<int>?> thumb(int photo) async => null;

  @override
  Future<void> download(int photo, File dest, String sha256) async {}

  @override
  Future<List<int>> trash(List<String> sha256) async => const [];

  final inTrash = [TrashedPhoto(id: 5, sha256: 'a' * 64, name: 'IMG_5.jpg', mime: 'image/jpeg', size: 10, takenAt: DateTime(2026, 11, 1), purgeAt: DateTime(2026, 12, 7))];
  final restored = <int>[];

  @override
  Future<List<TrashedPhoto>> trashList() async => List.of(inTrash);

  @override
  Future<void> restore(int photo) async {
    restored.add(photo);
    inTrash.removeWhere((t) => t.id == photo);
  }

  @override
  Future<({List<PhotoEvent> events, int next})> removals(int? since) async => (events: <PhotoEvent>[], next: 0);
}

class _Library extends PhotoLibrary {
  bool allowed = true;
  int asked = 0;

  @override
  Future<bool> hasAccess() async => allowed;

  @override
  Future<bool> requestAccess() async {
    asked++;
    return allowed;
  }

  @override
  Future<List<LocalPhoto>> list({DateTime? since, required bool videos, required String sources}) async => const [];
}

void main() {
  late Directory dir;
  late PhotoDeviceStore store;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('photo-screen-');
    store = PhotoDeviceStore(File('${dir.path}/photo_backup.json'), File('${dir.path}/photo_device.json'));
  });
  tearDown(() => dir.deleteSync(recursive: true));

  PhotoBackupController controller(_Api api, _Library lib) =>
      PhotoBackupController(api: api, library: lib, store: store, schedule: (_, _, _) async {}, now: () => DateTime(2026, 11, 7, 15));

  setUp(() {
    // Tall enough that the list builds all its cards.
    final view = TestWidgetsFlutterBinding.instance.platformDispatcher.views.first;
    view.physicalSize = const Size(800, 4000);
    view.devicePixelRatio = 1;
  });
  tearDown(() => TestWidgetsFlutterBinding.instance.platformDispatcher.views.first.reset());

  Widget app(Widget child, {Locale locale = const Locale('en')}) => MaterialApp(
        locale: locale,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: child,
      );

  Future<void> pick(WidgetTester tester, String id, String option) async {
    final field = find.byWidgetPredicate((w) => w.key is ValueKey && '${(w.key as ValueKey).value}'.startsWith('photo-choice-$id-'));
    await tester.ensureVisible(field);
    await tester.pumpAndSettle();
    await tester.tap(field);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(ValueKey('photo-option-$id-$option')).last);
    await tester.pumpAndSettle();
  }

  String explain(WidgetTester tester, String id) => tester.widget<Text>(find.byKey(ValueKey('photo-explain-$id'))).data!;

  testWidgets('each choice shows the text of the chosen option only, and the text changes with the choice', (tester) async {
    final api = _Api();
    final c = controller(api, _Library());
    await c.load();
    await tester.pumpWidget(app(PhotoBackupScreen(controller: c)));
    await tester.pumpAndSettle();
    final l = AppLocalizations.of(tester.element(find.byType(PhotoBackupScreen)));

    expect(explain(tester, 'network'), l.photoExplainNetworkWifi);
    expect(find.text(l.photoExplainNetworkAny), findsNothing);
    await pick(tester, 'network', 'any');
    expect(api.saved.last, {'network': 'any'});
    expect(explain(tester, 'network'), l.photoExplainNetworkAny);
    expect(find.text(l.photoExplainNetworkWifi), findsNothing);
    expect(find.text(l.photoServerCount(12, '34 MB')), findsOneWidget);
  });

  testWidgets('an option the server does not allow is greyed out with the reason, and a refusal saves nothing', (tester) async {
    final api = _Api();
    final c = controller(api, _Library());
    await c.load();
    await tester.pumpWidget(app(PhotoBackupScreen(controller: c), locale: const Locale('de')));
    await tester.pumpAndSettle();
    final l = AppLocalizations.of(tester.element(find.byType(PhotoBackupScreen)));

    expect(find.text(l.photoWhyNoBisync), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('photo-choice-mode-uni')));
    await tester.pumpAndSettle();
    final item = tester.widget<DropdownMenuItem<String>>(find.ancestor(of: find.byKey(const ValueKey('photo-option-mode-bi')), matching: find.byType(DropdownMenuItem<String>)).last);
    expect(item.enabled, isFalse);
    await tester.tap(find.byKey(const ValueKey('photo-option-mode-bi')).last, warnIfMissed: false);
    await tester.pumpAndSettle();
    expect(api.saved, isEmpty, reason: 'a greyed-out option cannot be picked');
    await tester.tap(find.byKey(const ValueKey('photo-option-mode-uni')).last);
    await tester.pumpAndSettle();

    // Should an older value come through anyway: the server refuses, the choice stays and says why.
    await c.choose('mode', 'bi');
    await tester.pumpAndSettle();
    expect(api.choices['mode'], 'uni');
    expect(explain(tester, 'mode'), l.photoExplainModeUni);
    expect(find.text(l.photoWhyNoBisync), findsNWidgets(2));
  });

  testWidgets('switching the device on asks for the pictures and counts from today; refused, it stays off', (tester) async {
    final lib = _Library()..allowed = false;
    final c = controller(_Api(), lib);
    await c.load();
    await tester.pumpWidget(app(PhotoBackupScreen(controller: c)));
    await tester.pumpAndSettle();
    final l = AppLocalizations.of(tester.element(find.byType(PhotoBackupScreen)));
    expect(find.text(l.photoStatusDeviceOff), findsOneWidget);

    await pick(tester, 'device', 'on');
    expect(lib.asked, 1);
    expect(store.loadSettings().enabled, isFalse);
    expect(find.text(l.photoStatusNoAccess), findsOneWidget);

    lib.allowed = true;
    await pick(tester, 'device', 'on');
    expect(store.loadSettings().enabled, isTrue);
    expect(store.loadSettings().since, DateTime(2026, 11, 7));
    expect(explain(tester, 'device'), l.photoExplainDeviceOn);
    expect(find.text(l.photoStatusActive), findsOneWidget);

    await tester.ensureVisible(find.byKey(const ValueKey('photo-older')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('photo-older')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('photo-older-confirm')));
    await tester.pumpAndSettle();
    expect(store.loadSettings().olderToo, isTrue);
    expect(find.text(l.photoSinceAll), findsOneWidget);
  });

  testWidgets('a deletion on another device is only asked about; the trash lists what is in it and restores it', (tester) async {
    final api = _Api()..choices = {..._Api().choices, 'mode': 'bi', 'deletes': 'trash'};
    store.saveSettings(PhotoDeviceSettings(enabled: true, since: DateTime(2026, 11, 7)));
    store.save(PhotoDeviceState(toDelete: [PhotoGone('42@1', 'b' * 64, 'IMG_42.jpg')]));
    final c = controller(api, _Library());
    await c.load();
    await tester.pumpWidget(app(PhotoBackupScreen(controller: c)));
    await tester.pumpAndSettle();
    final l = AppLocalizations.of(tester.element(find.byType(PhotoBackupScreen)));

    expect(find.text(l.photoAskDelete(1)), findsOneWidget);
    await tester.tap(find.descendant(of: find.byKey(const ValueKey('photo-ask-delete')), matching: find.byKey(const ValueKey('ask-no'))));
    await tester.pumpAndSettle();
    expect(store.load().toDelete, isEmpty);
    expect(find.byKey(const ValueKey('photo-ask-delete')), findsNothing);

    await tester.ensureVisible(find.byKey(const ValueKey('open-trash')));
    await tester.tap(find.byKey(const ValueKey('open-trash')));
    await tester.pumpAndSettle();
    expect(find.text('IMG_5.jpg'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('restore-5')));
    await tester.pumpAndSettle();
    expect(api.restored, [5]);
    expect(find.byKey(const ValueKey('photo-trash-empty')), findsOneWidget);
  });

  testWidgets('on iOS the screen says that iOS decides when the backup runs in the background; elsewhere it does not', (tester) async {
    final c = controller(_Api(), _Library());
    await c.load();
    await tester.pumpWidget(MaterialApp(
      theme: ThemeData(platform: TargetPlatform.iOS),
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: PhotoBackupScreen(controller: c),
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('photo-ios-hint')), findsOneWidget);

    await tester.pumpWidget(app(PhotoBackupScreen(key: const ValueKey('android'), controller: c)));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('photo-ios-hint')), findsNothing);
  });

  testWidgets('with the backup off on the server the settings show no photo backup at all', (tester) async {
    final c = controller(_Api(off: true), _Library());
    await tester.pumpWidget(app(Scaffold(body: PhotoBackupCard(controller: c))));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('open-photo-backup')), findsNothing);

    final on = controller(_Api(), _Library());
    await tester.pumpWidget(app(Scaffold(body: PhotoBackupCard(key: const ValueKey('on'), controller: on))));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('open-photo-backup')), findsOneWidget);
  });
}
