// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/cd/cd_drive_watcher.dart';
import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/cd/cd_import_controller.dart';
import 'package:psalm/cd/cd_kit.dart';
import 'package:psalm/cd/cd_models.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/library_screen.dart';

import 'cd_fakes.dart';

/// Jobs as the server sends them (shapes of docs/spec-cd-import-api.md and the server tests).
Map<String, dynamic> _json(
  int id,
  String csState, {
  String? state,
  bool? active,
  String phase = 'ripping',
  String error = '',
  int track = 0,
  double? fraction,
  List<Map<String, dynamic>>? trackStates,
}) => {
  'id': id,
  'cd_state': ?csState.isEmpty ? null : csState,
  'state': state ?? 'working',
  'active': ?active,
  'phase': phase,
  'track': track,
  'tracks_total': 15,
  'album': 'Leuchtfeuer',
  'error': error,
  'fraction': ?fraction,
  'track_states': ?trackStates,
};

final _present = CdJobState.fromJson(
  _json(75, 'present', state: 'present', active: false, phase: 'present'),
);
final _skipped = CdJobState.fromJson(
  _json(
    78,
    'skipped',
    state: 'skipped',
    active: false,
    phase: 'skipped',
    error: 'ambiguous_match',
  ),
);
final _real = CdJobState.fromJson(
  _json(
    80,
    'ripping',
    active: true,
    track: 5,
    fraction: 0.63,
    trackStates: [
      for (var n = 1; n <= 5; n++) {'number': n, 'state': 'read'},
      {'number': 6, 'state': 'verifying', 'fraction': 0.67},
      {'number': 7, 'state': 'waiting'},
    ],
  )..['album'] = 'Rüm Hart',
);

void main() {
  group('the kind of a job', () {
    CdJobKind kind(Map<String, dynamic> j) => CdJobState.fromJson(j).kind;

    test('every state of the server', () {
      expect(_present.kind, CdJobKind.ended);
      expect(_skipped.kind, CdJobKind.ended);
      expect(_real.kind, CdJobKind.running);
      expect(
        kind(
          _json(
            81,
            'awaiting_approval',
            state: 'awaiting_approval',
            active: false,
          ),
        ),
        CdJobKind.waitingApproval,
      );
      expect(
        kind(_json(82, 'retry_wait', state: 'retry_wait', active: false)),
        CdJobKind.waitingRetry,
      );
      expect(
        kind(_json(83, 'imported', state: 'done', phase: 'done')),
        CdJobKind.ended,
      );
      expect(
        kind(_json(84, 'rejected', state: 'rejected', active: false)),
        CdJobKind.ended,
      );
      expect(
        kind(_json(85, 'cancelled', state: 'failed', error: 'cancelled')),
        CdJobKind.ended,
      );
      expect(
        kind(_json(86, 'failed', state: 'failed', error: 'no_space')),
        CdJobKind.ended,
      );
      expect(kind(_json(87, 'ripped', active: true)), CdJobKind.running);
      expect(
        kind(
          _json(
            88,
            'imported',
            state: 'working',
            active: true,
            phase: 'importing',
          ),
        ),
        CdJobKind.running,
      );
    });

    test('an unknown future state is never running', () {
      expect(
        kind(_json(90, 'teleporting', state: 'teleporting')),
        CdJobKind.ended,
      );
      expect(kind({'id': 91, 'state': 'teleporting'}), CdJobKind.ended);
    });

    test('a running state with active false has ended', () {
      expect(kind(_json(92, 'ripping', active: false)), CdJobKind.ended);
    });

    test('an old server: only state, no cd_state, no active', () {
      expect(
        kind({'id': 1, 'state': 'working', 'phase': 'ripping'}),
        CdJobKind.running,
      );
      expect(
        CdJobState.fromJson({'id': 1, 'state': 'done'}).kind,
        CdJobKind.ended,
      );
      expect(CdJobState.fromJson({'id': 1, 'state': 'done'}).succeeded, isTrue);
      expect(
        CdJobState.fromJson({'id': 1, 'state': 'failed'}).succeeded,
        isFalse,
      );
      expect(CdJobState.fromJson({'id': 1}).kind, CdJobKind.running);
    });

    test('an ended job has no current track and no bar', () {
      expect(_present.currentTrack, isNull);
      expect(_real.currentTrack, 6);
      expect(_real.overall, 0.63);
    });
  });

  group('the tracker', () {
    test('BUG 2026-10-31 (ghost job): the real job 80 is chosen over the ended job 75', () {
      final tracker = CdJobTracker(FakeEvents());
      tracker.value = {'75': _present, '78': _skipped, '80': _real};
      expect(tracker.open.map((j) => j.id), ['80']);
      expect(tracker.running.single.id, '80');
      tracker.dispose();
    });

    test('running first, then waiting; the newest first in each group', () {
      final tracker = CdJobTracker(FakeEvents());
      final wait = CdJobState.fromJson(
        _json(95, 'retry_wait', state: 'retry_wait', active: false),
      );
      final older = CdJobState.fromJson(_json(70, 'ripping'));
      tracker.value = {'95': wait, '70': older, '80': _real};
      expect(tracker.open.map((j) => j.id), ['80', '70', '95']);
      expect(tracker.running.map((j) => j.id), ['80', '70']);
      tracker.dispose();
    });
  });

  group('strip and dialog', () {
    late FakeCdServer server;
    late FakeEvents events;
    late CdKit kit;
    late PlayerController player;

    setUp(() {
      server = FakeCdServer();
      events = FakeEvents();
      kit = CdKit(
        server: server,
        drives: CdDriveWatcher(events),
        tracker: CdJobTracker(events),
      );
      player = PlayerController([]);
      player.refresher = () async {};
    });
    tearDown(() => kit.dispose());

    Future<void> pump(
      WidgetTester tester, {
      Locale locale = const Locale('en'),
    }) {
      tester.view.physicalSize = const Size(1000, 1800);
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
    final strip = find.byKey(const Key('cd-strip'));
    final text = find.byKey(const Key('cd-strip-text'));
    final bar = find.byKey(const Key('cd-strip-bar'));

    testWidgets(
      'BUG 2026-10-31: the strip shows the real job 80, not the ghost 75',
      (tester) async {
        await pump(tester);
        jobs({'75': _present, '80': _real});
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 300));
        expect(tester.widget<Text>(text).data, contains('Rüm Hart'));
        expect(tester.widget<Text>(text).data, contains('63 %'));
        expect(tester.widget<LinearProgressIndicator>(bar).value, 0.63);
      },
    );

    testWidgets(
      'ended jobs that arrive without having been seen running stay silent',
      (tester) async {
        await pump(tester);
        final all = [
          _present,
          _skipped,
          CdJobState.fromJson(
            _json(83, 'imported', state: 'done', phase: 'done'),
          ),
          CdJobState.fromJson(
            _json(84, 'rejected', state: 'rejected', active: false),
          ),
          CdJobState.fromJson(
            _json(85, 'cancelled', state: 'failed', error: 'cancelled'),
          ),
          CdJobState.fromJson(
            _json(86, 'failed', state: 'failed', error: 'no_space'),
          ),
          CdJobState.fromJson(_json(90, 'teleporting', state: 'teleporting')),
        ];
        jobs({for (final j in all) j.id: j});
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 300));
        expect(strip, findsNothing);
      },
    );

    testWidgets('a job seen running and then present shows its result', (
      tester,
    ) async {
      await pump(tester);
      jobs({'75': CdJobState.fromJson(_json(75, 'ripping'))});
      await tester.pump();
      jobs({'75': _present});
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      expect(
        tester.widget<Text>(text).data,
        'This CD is in the library already (ejected): Leuchtfeuer',
      );
      await tester.pump(const Duration(seconds: 9));
    });

    testWidgets('waiting states: no bar share, own words', (tester) async {
      await pump(tester, locale: const Locale('de'));
      jobs({
        '82': CdJobState.fromJson(
          _json(82, 'retry_wait', state: 'retry_wait', active: false),
        ),
      });
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      expect(
        tester.widget<Text>(text).data,
        'Leuchtfeuer: Lesefehler: Die CD liegt noch im Laufwerk',
      );
      expect(tester.widget<LinearProgressIndicator>(bar).value, 0);
      jobs({
        '81': CdJobState.fromJson(
          _json(
            81,
            'awaiting_approval',
            state: 'awaiting_approval',
            active: false,
          ),
        ),
      });
      await tester.pump();
      expect(
        tester.widget<Text>(text).data,
        'Leuchtfeuer: Wartet auf Freigabe',
      );
    });

    testWidgets('the dialog follows the real job and shows its progress', (
      tester,
    ) async {
      await pump(tester);
      jobs({'75': _present, '80': _real});
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      await tester.tap(strip);
      await tester.pump();
      await tester.pump();
      expect(find.text('Reading track 6 of 15'), findsOneWidget);
      final dialogBar = find.descendant(
        of: find.byType(AlertDialog),
        matching: find.byType(LinearProgressIndicator),
      );
      expect(
        tester.widget<LinearProgressIndicator>(dialogBar.first).value,
        0.63,
      );
      await tester.tap(find.byKey(const Key('cd-hide')));
      await tester.pumpAndSettle();
    });

    test('the controller of a ghost job ends instead of reading', () async {
      kit.tracker.value = {'75': _present};
      final c = kit.controllerFor(
        const CdDriveInfo(device: 'd'),
        resumeJobId: '75',
      );
      await c.identify();
      expect(c.phase, CdPhase.ended);
      c.dispose();
    });

    test(
      'the controller maps skipped to an error, cancelled to cancelled',
      () async {
        kit.tracker.value = {'78': _skipped};
        var c = kit.controllerFor(
          const CdDriveInfo(device: 'd'),
          resumeJobId: '78',
        );
        await c.identify();
        expect((c.phase, c.failureCode), (CdPhase.failed, 'ambiguous_match'));
        c.dispose();
        kit.tracker.value = {
          '85': CdJobState.fromJson(
            _json(85, 'cancelled', state: 'failed', error: 'cancelled'),
          ),
        };
        c = kit.controllerFor(
          const CdDriveInfo(device: 'd'),
          resumeJobId: '85',
        );
        await c.identify();
        expect(c.phase, CdPhase.cancelled);
        c.dispose();
      },
    );
  });
}
