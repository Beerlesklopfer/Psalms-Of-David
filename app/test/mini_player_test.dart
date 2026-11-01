// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/demo_data.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/main.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/about_info.dart';
import 'package:psalm/screens/settings_screen.dart';
import 'package:psalm/server/credentials.dart';
import 'package:psalm/server/server_source.dart' show MediaSource, PassageHit;
import 'package:psalm/widgets.dart';

Widget _app(PlayerController p, Widget child, {double width = 800}) => PlayerScope(
  controller: p,
  child: MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: Scaffold(
      body: Align(
        alignment: Alignment.bottomCenter,
        child: SizedBox(width: width, child: child),
      ),
    ),
  ),
);

void main() {
  testWidgets('the mini player has previous, back, forward, next and a speed menu', (tester) async {
    final p = PlayerController(demoItems)..seek(const Duration(minutes: 1));
    await tester.pumpWidget(_app(p, const MiniPlayer()));
    final before = p.trackIndex;
    final at = p.position;

    await tester.tap(find.byTooltip('Forward 30 seconds'));
    expect(p.position, at + const Duration(seconds: 30));
    await tester.tap(find.byTooltip('Back 10 seconds'));
    expect(p.position, at + const Duration(seconds: 20));
    await tester.tap(find.byTooltip('Next track'));
    expect(p.trackIndex, before + 1);
    await tester.tap(find.byTooltip('Previous track'));
    await tester.pump();
    expect(p.trackIndex, before);

    await tester.tap(find.byTooltip('Playback speed'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('1.5×').last);
    await tester.pumpAndSettle();
    expect(p.speed, 1.5);
    p.dispose();
  });

  testWidgets('on a narrow screen only play and pause stay', (tester) async {
    final p = PlayerController(demoItems)..seek(Duration.zero);
    await tester.pumpWidget(_app(p, const MiniPlayer(), width: 400));
    expect(find.byTooltip('Next track'), findsNothing);
    expect(find.byTooltip('Forward 30 seconds'), findsNothing);
    expect(find.byTooltip('Play'), findsOneWidget);
    p.dispose();
  });

  test('the speed reaches the sound engine and stays across loads', () {
    final backend = FakeBackend();
    final book = LibraryItem(
      id: 'b',
      title: 'Book',
      author: 'Author',
      color: const Color(0xFF000000),
      tracks: [
        for (var i = 1; i <= 2; i++)
          Track(disc: 1, number: i, title: 'T$i', length: const Duration(minutes: 1), path: '/music/$i.mp3'),
      ],
    );
    final p = PlayerController([book], backend: backend);
    p.open(book, track: 0);
    p.setSpeed(1.5);
    expect(backend.speed, 1.5);
    p.next();
    expect(backend.speed, 1.5);
    p.dispose();
  });

  test('a radio station is played by the engine from its address and does not end at once', () async {
    final backend = FakeBackend();
    final station = LibraryItem(
      id: 'st:1',
      title: 'Station',
      author: '',
      kind: 'radio',
      color: const Color(0xFF000000),
      tracks: [Track(disc: 1, number: 1, title: 'Station', length: Duration.zero, url: 'https://radio.example.org/live.mp3', live: true)],
    );
    final p = PlayerController([station], backend: backend, source: _AddressSource());
    p.open(station);
    await Future<void>.delayed(const Duration(milliseconds: 1300));
    expect(backend.loadedPath, 'https://radio.example.org/live.mp3');
    expect(p.playing, isTrue, reason: 'a station of length zero is not over');
    expect(p.error, isNull);
    p.dispose();
  });

  testWidgets('settings show the server and ask before disconnecting', (tester) async {
    var disconnected = 0;
    await tester.pumpWidget(
      MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: SettingsScreen(
            credentials: Credentials(
              serverId: 'x',
              address: 'https://192.168.1.5:8484',
              fingerprint: 'ab',
              token: 't',
              deviceName: 'Laptop',
            ),
            onDisconnect: () => disconnected++,
          ),
        ),
      ),
    );
    expect(find.text('https://192.168.1.5:8484'), findsOneWidget);
    expect(find.text('Laptop'), findsOneWidget);
    await tester.tap(find.text('Disconnect'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(disconnected, 0);
    await tester.tap(find.text('Disconnect'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Disconnect'));
    await tester.pumpAndSettle();
    expect(disconnected, 1);
  });

  testWidgets('the navigation has Settings and About, in English and German', (tester) async {
    await tester.pumpWidget(const PsalmApp(demo: true, locale: Locale('en')));
    await tester.pump();
    // The rail scrolls in a low window, so the last entry has to be brought into view first.
    await tester.ensureVisible(find.text('About'));
    await tester.tap(find.text('About'));
    await tester.pumpAndSettle();
    expect(find.text('Version $appVersion'), findsOneWidget);
    await tester.ensureVisible(find.text(securityContact).first);
    expect(find.textContaining('Cyber Resilience Act'), findsOneWidget);
    expect(find.text(manufacturerName), findsOneWidget);
    expect(find.text(manufacturerWebsite), findsOneWidget);
    expect(
      find.text(manufacturerEmail),
      findsNWidgets(2),
      reason: 'as the contact and as the address for security reports',
    );
    expect(find.textContaining('has not been set yet'), findsOneWidget, reason: 'no invented support date');
    await tester.ensureVisible(find.text('License'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('License'));
    await tester.pumpAndSettle();
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 200)));
    await tester.pumpAndSettle();
    expect(find.byType(SelectableText), findsOneWidget);
    expect(
      tester.widget<SelectableText>(find.byType(SelectableText)).data,
      contains('GNU AFFERO GENERAL PUBLIC LICENSE'),
    );
    await tester.pageBack();
    await tester.pumpAndSettle();
    await tester.tap(find.text('Settings'));
    await tester.pumpAndSettle();
    expect(find.textContaining('demo library'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await tester.pumpWidget(const PsalmApp(demo: true, locale: Locale('de')));
    await tester.pump();
    expect(find.text('Einstellungen'), findsOneWidget);
    expect(find.text('Über'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });
}

/// Hands the address of a station back as it is (the real source does the same for a live track).
class _AddressSource extends MediaSource {
  @override
  Future<String> fetchPart(Track track) async => track.url!;

  @override
  Future<void> saveProgress(LibraryItem book, Track track, Duration position, {required bool finished}) async {}

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}
