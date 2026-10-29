// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/cd/cd_drive_watcher.dart';
import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/cd/cd_import_ui.dart';
import 'package:psalm/cd/cd_kit.dart';
import 'package:psalm/cd/cd_models.dart';
import 'package:psalm/cd/cd_progress_bar.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/library_screen.dart';

import 'cd_fakes.dart';

const _running = CdJobState(
  id: '12',
  state: 'working',
  phase: 'ripping',
  track: 2,
  tracksTotal: 11,
  fraction: 0.4,
  title: 'Sturz der Titanen',
  disc: 3,
  discsTotal: 12,
);

void main() {
  late FakeCdServer server;
  late FakeEvents events;
  late CdKit kit;
  late PlayerController player;

  setUp(() {
    server = FakeCdServer();
    events = FakeEvents();
    kit = CdKit(server: server, drives: CdDriveWatcher(events), tracker: CdJobTracker(events));
    player = PlayerController([]);
    player.refresher = () async {};
  });
  tearDown(() => kit.dispose());

  Future<void> pump(
    WidgetTester tester, {
    Locale locale = const Locale('en'),
    double width = 1000,
  }) {
    tester.view.physicalSize = Size(width, 1800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    return tester.pumpWidget(
      PlayerScope(
        controller: player,
        child: MaterialApp(
          locale: locale,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(body: LibraryScreen(cd: kit)),
        ),
      ),
    );
  }

  void jobs(Map<String, CdJobState> m) => kit.tracker.value = m;

  /// Shows the change: the strip grows in 150 ms, and an unknown share animates for ever (no settling).
  Future<void> shown(WidgetTester tester) async {
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
  }

  final strip = find.byKey(const Key('cd-strip'));
  final text = find.byKey(const Key('cd-strip-text'));
  final bar = find.byKey(const Key('cd-strip-bar'));

  testWidgets('no job: no strip and no room taken', (tester) async {
    await pump(tester);
    expect(strip, findsNothing);
    expect(bar, findsNothing);
  });

  test('with the list of the tracks the place is where the rip reads, not how many are done', () {
    // The first reading goes through the disc with the second one still to come: none is done, track 5 is being read.
    const job = CdJobState(
      id: '12',
      state: 'working',
      phase: 'ripping',
      track: 0,
      tracksTotal: 13,
      fraction: 0.16,
      title: 'Sturz der Titanen',
      trackStates: [
        CdTrackState(number: 1, state: 'read'),
        CdTrackState(number: 2, state: 'read'),
        CdTrackState(number: 3, state: 'read'),
        CdTrackState(number: 4, state: 'read'),
        CdTrackState(number: 5, state: 'reading', fraction: 0.31),
        CdTrackState(number: 6),
      ],
    );
    expect(job.currentTrack, 5);
    expect(
      const CdJobState(id: '1', state: 'working', track: 2, tracksTotal: 4).currentTrack,
      3,
      reason: 'without the list: the server\'s count',
    );
  });

  testWidgets('a running job: a bar with the share done and the one line in English', (
    tester,
  ) async {
    await pump(tester);
    jobs({'12': _running});
    await tester.pumpAndSettle();
    expect(strip, findsOneWidget);
    expect(tester.widget<LinearProgressIndicator>(bar).value, 0.4);
    expect(
      tester.widget<Text>(text).data,
      'Importing: Sturz der Titanen, CD 3: Track 3 of 11 · Reading · 40 %',
    );
  });

  testWidgets('the line in German', (tester) async {
    await pump(tester, locale: const Locale('de'));
    jobs({'12': _running});
    await tester.pumpAndSettle();
    expect(
      tester.widget<Text>(text).data,
      'Import von: Sturz der Titanen, CD 3: Titel 3 von 11 · Lesen · 40 %',
    );
  });

  testWidgets('the phases have words, an unknown one is neutral; CD 1 of 1 has no "CD" part', (
    tester,
  ) async {
    await pump(tester);
    for (final (phase, word) in [
      ('verifying', 'Checking'),
      ('encoding', 'Converting'),
      ('placing', 'Filing'),
      ('importing', 'Importing'),
      ('mystery', 'Working'),
      (null, 'Working'),
    ]) {
      jobs({
        '12': CdJobState(
          id: '12',
          state: 'working',
          phase: phase,
          track: 0,
          tracksTotal: 2,
          title: 'Solo',
          disc: 1,
          discsTotal: 1,
        ),
      });
      await shown(tester);
      expect(
        tester.widget<Text>(text).data,
        'Importing: Solo: Track 1 of 2 · $word · 0 %',
        reason: '$phase',
      );
    }
  });

  testWidgets('without a share the bar is indeterminate; the share can come from the tracks done', (
    tester,
  ) async {
    await pump(tester);
    jobs({'12': const CdJobState(id: '12', state: 'working', title: 'T')});
    await shown(tester);
    expect(tester.widget<LinearProgressIndicator>(bar).value, isNull);
    expect(tester.widget<Text>(text).data, 'Importing: T: Working');
    jobs({
      '12': const CdJobState(id: '12', state: 'working', title: 'T', track: 1, tracksTotal: 4),
    });
    await shown(tester);
    expect(tester.widget<LinearProgressIndicator>(bar).value, 0.25);
  });

  testWidgets('a job of another device shows too', (tester) async {
    await pump(tester);
    jobs({
      '7': const CdJobState(
        id: '7',
        state: 'working',
        device: 'other',
        title: 'Fremd',
        fraction: 0.1,
      ),
    });
    await shown(tester);
    expect(tester.widget<Text>(text).data, startsWith('Importing: Fremd: '));
  });

  testWidgets('end, success: green "Done: 11 tracks", gone after the time', (tester) async {
    await pump(tester);
    jobs({'12': _running});
    await shown(tester);
    jobs({
      '12': const CdJobState(id: '12', state: 'done', tracksTotal: 11, title: 'Sturz der Titanen'),
    });
    await shown(tester);
    expect(tester.widget<Text>(text).data, 'Done: 11 tracks');
    expect(tester.widget<Text>(text).style?.color, Colors.green);
    expect(tester.widget<LinearProgressIndicator>(bar).value, 1.0);
    await tester.pump(kCdResultLinger - const Duration(seconds: 1));
    expect(strip, findsOneWidget);
    await tester.pump(const Duration(seconds: 2));
    await tester.pumpAndSettle();
    expect(strip, findsNothing);
  });

  testWidgets('end, failure: red with the localized reason, in German too', (tester) async {
    await pump(tester, locale: const Locale('de'));
    jobs({'12': _running});
    await shown(tester);
    jobs({'12': const CdJobState(id: '12', state: 'failed', error: 'disc_exists')});
    await shown(tester);
    expect(
      tester.widget<Text>(text).data,
      'Diese CD ist schon in der Bibliothek; der Server überschreibt sie nicht.',
    );
    expect(
      tester.widget<Text>(text).style?.color,
      Theme.of(tester.element(strip)).colorScheme.error,
    );
    await tester.pump(kCdResultLinger + const Duration(seconds: 1));
    await tester.pumpAndSettle();
    expect(strip, findsNothing);
  });

  testWidgets('the close icon removes a result at once; a result is not brought back', (
    tester,
  ) async {
    await pump(tester);
    jobs({'12': _running});
    await shown(tester);
    expect(
      find.byKey(const Key('cd-strip-close')),
      findsNothing,
      reason: 'nothing to close while it runs',
    );
    jobs({'12': const CdJobState(id: '12', state: 'done', tracksTotal: 11)});
    await shown(tester);
    await tester.tap(find.byKey(const Key('cd-strip-close')));
    await tester.pumpAndSettle();
    expect(strip, findsNothing);
    jobs({'12': const CdJobState(id: '12', state: 'done', tracksTotal: 11, message: 'again')});
    await shown(tester);
    expect(strip, findsNothing);
    await tester.pump(kCdResultLinger);
  });

  testWidgets('a finished job that was never seen running (an old one in the list) shows nothing', (
    tester,
  ) async {
    await pump(tester);
    jobs({'3': const CdJobState(id: '3', state: 'done', tracksTotal: 5)});
    await shown(tester);
    expect(strip, findsNothing);
  });

  testWidgets('a tap on the strip opens the dialog on that import, which offers Cancel', (
    tester,
  ) async {
    await pump(tester);
    jobs({'12': _running});
    await shown(tester);
    await tester.tap(strip);
    await tester.pump();
    await tester.pump();
    expect(find.byType(CdImportDialog), findsOneWidget);
    expect(server.started, isNull);
    expect(find.text('Reading track 3 of 11'), findsOneWidget);
    await tester.tap(find.byKey(const Key('cd-hide')));
    await tester.pumpAndSettle();
  });

  testWidgets(
    'a narrow phone: the text is cut with dots, the bar stays full width, the value is read out',
    (tester) async {
      await pump(tester, width: 320);
      jobs({'12': _running});
      await tester.pumpAndSettle();
      final t = tester.widget<Text>(text);
      expect((t.maxLines, t.overflow), (1, TextOverflow.ellipsis));
      expect(tester.getSize(bar).width, 320);
      expect(tester.takeException(), isNull);
      final sem = tester.getSemantics(find.byKey(const Key('cd-strip')));
      expect(sem.value, '40 %');
      expect(sem.label, contains('Sturz der Titanen'));
    },
  );
}
