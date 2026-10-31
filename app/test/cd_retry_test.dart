// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/cd/cd_drive_watcher.dart';
import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/cd/cd_import_controller.dart';
import 'package:psalm/cd/cd_kit.dart';
import 'package:psalm/cd/cd_models.dart';
import 'package:psalm/inbox/inbox_controller.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/library_screen.dart';
import 'package:psalm/server/api_client.dart';

import 'cd_fakes.dart';
import 'inbox_fakes.dart';

CdJobState _job(int id, String cdState, {bool active = false}) =>
    CdJobState.fromJson({
      'id': id,
      'cd_state': cdState,
      'state': cdState == 'ripping' ? 'working' : cdState,
      'active': active,
      'phase': 'ripping',
      'tracks_total': 15,
      'album': 'Rüm Hart',
      'error': cdState == 'retry_wait' ? 'read' : '',
    });

void main() {
  late FakeCdServer server;
  late FakeEvents events;
  late FakeInboxApi inboxApi;
  late CdKit kit;

  setUp(() {
    server = FakeCdServer();
    events = FakeEvents();
    inboxApi = FakeInboxApi();
    kit = CdKit(
      server: server,
      drives: CdDriveWatcher(events),
      tracker: CdJobTracker(events),
      inbox: InboxController(api: inboxApi),
    );
  });
  tearDown(() => kit.dispose());

  Future<CdImportController> follow(CdJobState j) async {
    kit.tracker.value = {j.id: j};
    final c = kit.controllerFor(
      const CdDriveInfo(device: 'd'),
      resumeJobId: j.id,
    );
    await c.identify();
    return c;
  }

  group('controller', () {
    test('the buttons are possible only in retry_wait', () async {
      var c = await follow(_job(82, 'retry_wait'));
      expect(c.canMoveJob, isTrue);
      c.dispose();
      c = await follow(_job(83, 'ripping', active: true));
      expect(c.canMoveJob, isFalse);
      c.dispose();
    });

    test('retry runs through the inbox client; the move ends when the job runs again', () async {
      final c = await follow(_job(82, 'retry_wait'));
      final hold = inboxApi.hold = Completer<void>();
      final f = c.retryRead();
      expect(c.movePhase, CdMovePhase.retrying);
      hold.complete();
      await f;
      expect(inboxApi.calls, ['retry 82']);
      expect(
        c.movePhase,
        CdMovePhase.retrying,
        reason: 'until the server says it moved',
      );
      kit.tracker.value = {'82': _job(82, 'ripping', active: true)};
      expect(c.movePhase, CdMovePhase.idle);
      expect(c.canMoveJob, isFalse);
      c.dispose();
    });

    test(
      '403 forbidden: the code is kept and the button is on again',
      () async {
        inboxApi.failures['retry 82'] = ApiException(403, 'forbidden');
        final c = await follow(_job(82, 'retry_wait'));
        await c.retryRead();
        expect((c.movePhase, c.moveError), (CdMovePhase.idle, 'forbidden'));
        c.dispose();
      },
    );

    test('409 invalid_move: the job moved on, the error shows', () async {
      inboxApi.failures['retry 82'] = ApiException(409, 'invalid_move');
      final c = await follow(_job(82, 'retry_wait'));
      await c.retryRead();
      expect((c.movePhase, c.moveError), (CdMovePhase.idle, 'invalid_move'));
      c.dispose();
    });

    test('give up calls give_up; the job then ends as cancelled', () async {
      final c = await follow(_job(82, 'retry_wait'));
      await c.giveUp();
      expect(inboxApi.calls, ['give_up 82']);
      kit.tracker.value = {'82': _job(82, 'cancelled')};
      expect(c.phase, CdPhase.cancelled);
      c.dispose();
    });

    test('without an inbox client there is nothing to press', () async {
      kit.tracker.value = {'82': _job(82, 'retry_wait')};
      final c = CdImportController(
        server: server,
        tracker: kit.tracker,
        drive: const CdDriveInfo(device: 'd'),
        resumeJobId: '82',
      );
      await c.identify();
      expect(c.canMoveJob, isFalse);
      c.dispose();
    });
  });

  group('dialog', () {
    late PlayerController player;
    setUp(() {
      player = PlayerController([]);
      player.refresher = () async {};
    });

    Future<void> open(
      WidgetTester tester,
      CdJobState j, {
      Locale locale = const Locale('en'),
    }) async {
      tester.view.physicalSize = const Size(1000, 1800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
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
      kit.tracker.value = {j.id: j};
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));
      await tester.tap(find.byKey(const Key('cd-strip')));
      await tester.pump();
      await tester.pump();
    }

    Future<void> close(WidgetTester tester) async {
      await tester.tap(find.byKey(const Key('cd-hide')));
      // A running job animates for ever, so the page never settles.
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
    }

    testWidgets(
      'retry_wait: text, Give up and Try again; Try again is off while it runs',
      (tester) async {
        await open(tester, _job(82, 'retry_wait'));
        expect(find.byKey(const Key('cd-waiting')), findsOneWidget);
        expect(find.byKey(const Key('cd-give-up')), findsOneWidget);
        final hold = inboxApi.hold = Completer<void>();
        await tester.tap(find.byKey(const Key('cd-retry')));
        await tester.pump();
        expect(
          tester
              .widget<FilledButton>(find.byKey(const Key('cd-retry')))
              .onPressed,
          isNull,
        );
        expect(find.byKey(const Key('cd-moving')), findsOneWidget);
        hold.complete();
        await tester.pump();
        kit.tracker.value = {'82': _job(82, 'ripping', active: true)};
        await tester.pump();
        expect(find.byKey(const Key('cd-retry')), findsNothing);
        await close(tester);
      },
    );

    testWidgets('a running job has neither button', (tester) async {
      await open(tester, _job(83, 'ripping', active: true));
      expect(find.byKey(const Key('cd-retry')), findsNothing);
      expect(find.byKey(const Key('cd-give-up')), findsNothing);
      await close(tester);
    });

    testWidgets('403 shows the mapped text and the button is on again', (
      tester,
    ) async {
      inboxApi.failures['retry 82'] = ApiException(403, 'forbidden');
      await open(tester, _job(82, 'retry_wait'));
      await tester.tap(find.byKey(const Key('cd-retry')));
      await tester.pump();
      await tester.pump();
      expect(find.byKey(const Key('cd-move-error')), findsOneWidget);
      expect(
        tester
            .widget<FilledButton>(find.byKey(const Key('cd-retry')))
            .onPressed,
        isNotNull,
      );
      await close(tester);
    });

    testWidgets('Give up asks first; No does nothing, Yes gives up', (
      tester,
    ) async {
      await open(tester, _job(82, 'retry_wait'));
      await tester.tap(find.byKey(const Key('cd-give-up')));
      await tester.pump();
      expect(find.text('Give up on this CD?'), findsOneWidget);
      await tester.tap(find.byKey(const Key('cd-give-up-no')));
      await tester.pumpAndSettle();
      expect(inboxApi.calls, isEmpty);
      await tester.tap(find.byKey(const Key('cd-give-up')));
      await tester.pump();
      await tester.tap(find.byKey(const Key('cd-give-up-yes')));
      await tester.pump();
      await tester.pump();
      expect(inboxApi.calls, ['give_up 82']);
      await close(tester);
    });

    testWidgets('German', (tester) async {
      await open(tester, _job(82, 'retry_wait'), locale: const Locale('de'));
      expect(find.text('Aufgeben'), findsOneWidget);
      expect(find.text('Erneut versuchen'), findsOneWidget);
      await close(tester);
    });
  });
}
