// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/audio/playback_settings.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/settings_screen.dart';

Widget page(PlayerController p, {Locale locale = const Locale('en')}) => PlayerScope(
  controller: p,
  child: MaterialApp(
    locale: locale,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: const Scaffold(body: SettingsScreen()),
  ),
);

void main() {
  late Directory dir;
  late PlaybackSettingsStore store;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('psalm-playback-');
    store = PlaybackSettingsStore(File('${dir.path}/playback.json'));
  });
  tearDown(() => dir.deleteSync(recursive: true));

  test('the store gives the default for nothing or nonsense and keeps what is written within the range', () {
    expect(store.readBufferSeconds(), kBufferDefaultSeconds);
    File(store.file.path).writeAsStringSync('not json');
    expect(store.readBufferSeconds(), kBufferDefaultSeconds);
    store.writeBufferSeconds(12);
    expect(store.readBufferSeconds(), 12);
    store.writeBufferSeconds(1000);
    expect(
      store.readBufferSeconds(),
      kBufferMaxSeconds,
      reason: 'a value out of range is cut to the range',
    );
  });

  test('the player gives the stored size to the engine when it starts and sets and remembers a new one', () {
    store.writeBufferSeconds(7);
    final backend = FakeBackend();
    final p = PlayerController(const [], backend: backend, playbackStore: store);
    expect(backend.bufferSeconds, 7);
    p.setBufferSeconds(20);
    expect(backend.bufferSeconds, 20);
    expect(store.readBufferSeconds(), 20);
    p.setBufferSeconds(0);
    expect(backend.bufferSeconds, kBufferMinSeconds);
    p.dispose();
  });

  testWidgets(
    'the slider in the settings sets the buffer when it is let go, in English and German',
    (tester) async {
      final backend = FakeBackend();
      final p = PlayerController(const [], backend: backend, playbackStore: store);
      await tester.pumpWidget(page(p));
      expect(find.text('Audio buffer'), findsOneWidget);
      expect(find.text('2 s'), findsOneWidget);
      await tester.drag(find.byKey(const Key('buffer-slider')), const Offset(2000, 0));
      await tester.pumpAndSettle();
      expect(backend.bufferSeconds, kBufferMaxSeconds);
      expect(find.text('$kBufferMaxSeconds s'), findsOneWidget);
      expect(store.readBufferSeconds(), kBufferMaxSeconds);
      await tester.pumpWidget(page(p, locale: const Locale('de')));
      await tester.pumpAndSettle();
      expect(find.text('Audiopuffer'), findsOneWidget);
      p.dispose();
    },
  );

  testWidgets('without an engine that has a buffer there is no slider', (tester) async {
    final p = PlayerController(const []);
    await tester.pumpWidget(page(p));
    expect(find.byKey(const Key('buffer-slider')), findsNothing);
    p.dispose();
  });
}
