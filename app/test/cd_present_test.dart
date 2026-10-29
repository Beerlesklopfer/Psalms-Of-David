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
import 'package:psalm/server/api_client.dart';

import 'cd_fakes.dart';

const _tracks = [
  CdTrackInfo(number: 1, seconds: 60),
  CdTrackInfo(number: 2, seconds: 120),
  CdTrackInfo(number: 3, seconds: 185),
  CdTrackInfo(number: 4, seconds: 90),
];

const _present = CdPresent(
  book: 'Sturz der Titanen',
  author: 'Ken Follett',
  disc: 2,
  via: 'disc_id',
  tracksInLibrary: 2,
  tracksOnDisc: 4,
  missing: [3],
  damaged: [
    CdDamaged(number: 2, reason: 'length', haveSeconds: 100, wantSeconds: 120),
    CdDamaged(number: 4, reason: 'empty'),
  ],
);

CdDriveInfo _drive({CdPresent? present, CdResume? resume, CdRipInfo? rip}) => CdDriveInfo(
  device: 'drive0',
  tray: CdTray.closed,
  medium: CdMedium.audio,
  tracks: 4,
  trackList: _tracks,
  present: present,
  resume: resume,
  rip: rip,
);

const _names = CdCandidate(
  author: 'Ken Follett',
  title: 'Sturz der Titanen',
  disc: 2,
  discsTotal: 12,
  trackTitles: ['Anfang', 'Mitte', 'Dritter Teil', 'Ende'],
);

void main() {
  group('the state of a drive from the server', () {
    test('resume, present and rip are read; nothing is null', () {
      final d = CdDriveInfo.fromJson({
        'device': 'd',
        'tray': 'closed',
        'medium': 'audio',
        'tracks': 11,
        'resume': {
          'disc_id': 'X',
          'tracks_done': [1, 2, 3],
          'tracks_total': 11,
          'only': [],
          'interrupted_at': 1760000000000,
          'keep_until': 1760100000000,
          'settings_match': false,
        },
        'present': {
          'book': 'Sturz',
          'author': 'Follett',
          'disc': 2,
          'via': 'book',
          'tracks_in_library': 9,
          'tracks_on_disc': 11,
          'missing': [4, 9],
          'damaged': [
            {'number': 7, 'reason': 'length', 'have_s': 192, 'want_s': 245},
          ],
          'complete': false,
        },
        'rip': {
          'job_id': 5,
          'finished': false,
          'error': '',
          'fraction': 0.4,
          'buffer_seconds': 41,
          'buffer_max': 230,
          'buffer_percent': 18,
          'reader_waits': 3,
          'reader_wait_max_s': 6.0,
          'track_states': [
            {
              'number': 3,
              'title': 'Teil 031',
              'seconds': 245,
              'state': 'reading',
              'fraction': 0.4,
              'error': '',
            },
          ],
        },
      });
      expect(d.resume!.tracksDone, [1, 2, 3]);
      expect((d.resume!.settingsMatch, d.resume!.tracksTotal), (false, 11));
      expect(d.resume!.interruptedAt, DateTime.fromMillisecondsSinceEpoch(1760000000000));
      expect(d.present!.missing, [4, 9]);
      expect(
        (d.present!.book, d.present!.damaged.single.wantSeconds, d.present!.via),
        ('Sturz', 245, 'book'),
      );
      expect(
        (d.rip!.jobId, d.rip!.buffer!.seconds, d.rip!.buffer!.max, d.rip!.buffer!.percent),
        ('5', 41, 230, 18),
      );
      expect(
        (d.rip!.trackStates.single.state, d.rip!.trackStates.single.fraction),
        ('reading', 0.4),
      );
      final none = CdDriveInfo.fromJson({
        'device': 'd',
        'resume': null,
        'present': null,
        'rip': null,
      });
      expect((none.resume, none.present, none.rip), (null, null, null));
    });

    test('a changed present or rip is a changed state (so the symbols and the dialog update)', () {
      final a = CdDriveInfo.fromJson({
        'device': 'd',
        'present': {
          'book': 'B',
          'missing': [1],
        },
      });
      final b = CdDriveInfo.fromJson({
        'device': 'd',
        'present': {'book': 'B', 'missing': []},
      });
      expect(a == b, isFalse);
      expect(
        a ==
            CdDriveInfo.fromJson({
              'device': 'd',
              'present': {
                'book': 'B',
                'missing': [1],
              },
            }),
        isTrue,
      );
    });

    test('a job carries the states of the tracks and the buffer', () {
      final j = CdJobState.fromJson({
        'id': 1,
        'state': 'working',
        'track_states': [
          {'number': 1, 'title': 'A', 'seconds': 10, 'state': 'done'},
          {'number': 2, 'title': 'B', 'seconds': 10, 'state': 'failed', 'error': 'read'},
        ],
        'buffer_seconds': 5,
        'buffer_max': 20,
      });
      expect(j.trackStates.map((t) => t.state), ['done', 'failed']);
      expect(j.trackStates[1].error, 'read');
      expect(j.buffer?.max, 20);
      expect(CdJobState.fromJson({'id': 2, 'state': 'working'}).buffer, isNull);
    });
  });

  group('controller', () {
    late FakeCdServer server;
    late FakeEvents events;
    late CdJobTracker tracker;

    setUp(() {
      server = FakeCdServer()..candidates = const [_names];
      events = FakeEvents();
      tracker = CdJobTracker(events)..start();
    });
    tearDown(() => tracker.dispose());

    CdImportController make(CdDriveInfo d) =>
        CdImportController(server: server, tracker: tracker, drive: d);
    Future<void> tick() => Future<void>.delayed(Duration.zero);

    test(
      'a CD that is in the library gets no import form but the check, with the names of the tracks',
      () async {
        final c = make(_drive(present: _present));
        await c.identify();
        expect(c.phase, CdPhase.present);
        expect(c.titleOf(3), 'Dritter Teil');
        expect(server.started, isNull);
        c.dispose();
      },
    );

    test('Check and repair starts the repair with what the library knows, nothing typed', () async {
      final c = make(_drive(present: _present));
      await c.identify();
      await c.heal();
      final r = server.started!;
      expect(
        (r.mode, r.title, r.author, r.disc, r.device, r.force, r.only),
        ('heal', 'Sturz der Titanen', 'Ken Follett', 2, 'drive0', false, null),
      );
      expect(r.toJson()['mode'], 'heal');
      expect(c.phase, CdPhase.running);
      c.dispose();
    });

    test('"Import anyway" is a deliberate step: the form comes, and Save sends force', () async {
      final c = make(_drive(present: _present));
      await c.identify();
      expect(server.allStarted, isEmpty);
      await c.importAnyway();
      expect(c.phase, CdPhase.confirm);
      await c.save();
      expect(server.started!.force, isTrue);
      expect(server.started!.toJson()['force'], isTrue);
      expect(server.started!.mode, isNull);
      c.dispose();
    });

    test('a normal start does not send force, mode or resume', () async {
      final c = make(_drive());
      await c.identify();
      await c.save();
      final j = server.started!.toJson();
      expect(j.containsKey('force') || j.containsKey('mode') || j.containsKey('resume'), isFalse);
      c.dispose();
    });

    test('the server turning down a start with disc_present leads to the same view', () async {
      server.startError = ApiException(409, 'disc_present', {
        'error': 'disc_present',
        'book': 'Sturz der Titanen',
        'author': 'Ken Follett',
        'disc': 2,
        'present': {
          'book': 'Sturz der Titanen',
          'author': 'Ken Follett',
          'disc': 2,
          'missing': [3],
          'tracks_in_library': 3,
          'tracks_on_disc': 4,
        },
      });
      final c = make(_drive());
      await c.identify();
      await c.save();
      expect(c.phase, CdPhase.present);
      expect(c.present?.missing, [3]);
      server.startError = null;
      await c.heal();
      expect(server.started!.mode, 'heal');
      c.dispose();
    });

    test('heal that is not needed or not possible shows the code', () async {
      for (final code in ['not_present', 'nothing_to_heal']) {
        server.startError = ApiException(409, code);
        final c = make(_drive(present: _present));
        await c.identify();
        await c.heal();
        expect((c.phase, c.failureCode), (CdPhase.failed, code));
        c.dispose();
      }
    });

    test('an interrupted rip with the same choice is offered; Continue sends nothing special, Read again sends resume false', () async {
      const r = CdResume(discId: 'X', tracksDone: [1, 2], tracksTotal: 4, settingsMatch: true);
      var c = make(_drive(resume: r));
      await c.identify();
      expect(c.resumeOffer, isNotNull);
      await c.save();
      expect(server.started!.resume, isNull, reason: 'the server continues by default');
      c.dispose();

      c = make(_drive(resume: r));
      await c.identify();
      c.setReread(true);
      await c.save();
      expect(server.started!.resume, isFalse);
      expect(server.started!.toJson()['resume'], false);
      c.dispose();
    });

    test('the offer stands while a chosen track was read then (the choice may differ); other settings mean it is read again', () async {
      var c = make(
        _drive(
          resume: const CdResume(
            discId: 'X',
            tracksDone: [1],
            tracksTotal: 2,
            only: [1, 2],
            settingsMatch: true,
          ),
        ),
      );
      await c.identify();
      expect(
        c.resumeOffer,
        isNotNull,
        reason: 'track 1 was read and is chosen, though the earlier rip was for 1 and 2 only',
      );
      c.toggle(1);
      expect(c.resumeOffer, isNull, reason: 'the track that was read is not chosen any more');
      c.toggle(1);
      c.toggle(3);
      expect(c.resumeOffer, isNotNull, reason: 'a track more or less does not matter');
      c.dispose();

      c = make(_drive(resume: const CdResume(discId: 'X', settingsMatch: false)));
      await c.identify();
      expect((c.resumeOffer, c.resumeLost), (null, true));
      c.dispose();
    });

    test('the answer of the server about the resume is kept: resumed, fresh, mismatch', () async {
      server.resumeAnswer = 'mismatch';
      final c = make(_drive());
      await c.identify();
      await c.save();
      expect(c.resumeAnswer, 'mismatch');
      c.dispose();
    });

    test('the states of the tracks come from the job, else from the rip of the drive', () async {
      final c = make(
        _drive(
          rip: const CdRipInfo(
            jobId: '12',
            trackStates: [CdTrackState(number: 1, title: 'From drive')],
          ),
        ),
      );
      await c.identify();
      await c.save();
      expect(c.trackStates.single.title, 'From drive');
      events.connections.first.add(
        const CdJobState(
          id: '12',
          state: 'working',
          trackStates: [
            CdTrackState(number: 1, title: 'From job', state: 'reading', fraction: 0.5),
          ],
        ),
      );
      await tick();
      expect((c.trackStates.single.title, c.trackStates.single.state), ('From job', 'reading'));
      c.dispose();
    });

    test('a job that ends well with a hint: unreadable_track is kept', () async {
      final c = make(_drive());
      await c.identify();
      await c.save();
      events.connections.first.add(
        const CdJobState(id: '12', state: 'done', error: 'unreadable_track', message: 'tracks 7'),
      );
      await tick();
      expect((c.phase, c.doneNote, c.doneDetail), (CdPhase.done, 'unreadable_track', 'tracks 7'));
      c.dispose();
    });

    test('closing the result tells the server', () async {
      final c = make(_drive());
      await c.identify();
      await c.save();
      await c.dismiss();
      expect(server.dismissed, ['drive0']);
      c.dispose();
    });
  });

  group('dialog', () {
    late FakeCdServer server;
    late FakeEvents events;
    late CdKit kit;
    late PlayerController player;

    setUp(() {
      server = FakeCdServer()..candidates = const [_names];
      events = FakeEvents();
      kit = CdKit(server: server, drives: CdDriveWatcher(events), tracker: CdJobTracker(events));
      player = PlayerController([]);
      player.refresher = () async {};
    });
    tearDown(() => kit.dispose());

    Future<void> open(
      WidgetTester tester,
      CdDriveInfo d, {
      Locale locale = const Locale('en'),
    }) async {
      tester.view.physicalSize = const Size(1000, 2200);
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
      kit.drives.value = CdDrivesState([d]);
      await tester.pump();
      await tester.tap(find.byKey(const Key('cd-import-button')));
      for (var i = 0; i < 4; i++) {
        await tester.pump();
      }
    }

    void jobs(Map<String, CdJobState> m) => kit.tracker.value = m;

    testWidgets(
      'a CD in the library: what is missing and what is damaged, by name; Repair and Cancel',
      (tester) async {
        await open(tester, _drive(present: _present));
        expect(find.byKey(const Key('cd-present')), findsOneWidget);
        expect(find.text('This CD is in the library already'), findsOneWidget);
        expect(find.text('Sturz der Titanen by Ken Follett, CD 2'), findsOneWidget);
        expect(find.byKey(const Key('cd-save')), findsNothing, reason: 'no normal import');
        expect(find.text('03  Dritter Teil'), findsOneWidget);
        expect(find.text('02  Mitte: the file is 1:40 long, the CD has 2:00'), findsOneWidget);
        expect(find.text('04  Ende: the file is empty'), findsOneWidget);
        expect(find.byKey(const Key('cd-heal')), findsOneWidget);
        expect(find.byKey(const Key('cd-cancel')), findsOneWidget);
        expect(server.started, isNull);
      },
    );

    testWidgets('in German', (tester) async {
      await open(tester, _drive(present: _present), locale: const Locale('de'));
      expect(find.text('Diese CD ist schon in der Bibliothek'), findsOneWidget);
      expect(find.text('Prüfen und reparieren'), findsOneWidget);
      expect(find.text('Fehlt'), findsOneWidget);
      expect(find.text('Defekt'), findsOneWidget);
      expect(find.text('04  Ende: die Datei ist leer'), findsOneWidget);
    });

    testWidgets('without titles the numbers show, in the language of the user', (tester) async {
      server.candidates = const [];
      await open(tester, _drive(present: _present));
      expect(find.text('Track 03'), findsOneWidget);
    });

    testWidgets(
      'Check and repair starts the repair and follows the job with the states of the tracks',
      (tester) async {
        await open(tester, _drive(present: _present));
        await tester.tap(find.byKey(const Key('cd-heal')));
        await tester.pump();
        await tester.pump();
        expect(server.started?.mode, 'heal');
        jobs({
          '12': const CdJobState(
            id: '12',
            state: 'working',
            phase: 'ripping',
            track: 0,
            tracksTotal: 3,
            trackStates: [
              CdTrackState(number: 2, title: 'Mitte', state: 'reading', fraction: 0.4),
              CdTrackState(number: 3, title: 'Dritter Teil'),
              CdTrackState(number: 4, title: 'Ende', state: 'failed', error: 'read'),
            ],
          ),
        });
        await tester.pump();
        expect(find.text('Reading 40 %'), findsOneWidget);
        expect(find.text('Waiting'), findsOneWidget);
        expect(
          find.text('The disc could not be read reliably. It may be scratched or dirty.'),
          findsOneWidget,
        );
        expect(find.text('02  Mitte'), findsOneWidget);
        // Each state has its own colour, in the text and in the symbol: the track that is read has the accent, waiting is quiet, an error is red.
        Color? colorOf(int n) =>
            tester.widget<Text>(find.byKey(Key('cd-state-text-$n'))).style?.color;
        final scheme = Theme.of(tester.element(find.byKey(const Key('cd-state-text-2'))))
            .colorScheme;
        expect(colorOf(2), scheme.primary);
        expect(colorOf(3), scheme.onSurfaceVariant);
        expect(colorOf(4), scheme.error);
        expect({colorOf(2), colorOf(3), colorOf(4)}.length, 3);
        // A track that is read a third time because the readings differed says so with its progress, and the speed is the measured one.
        jobs({
          '12': const CdJobState(
            id: '12',
            state: 'working',
            phase: 'ripping',
            track: 0,
            tracksTotal: 3,
            buffer: CdBuffer(seconds: 0, max: 600, percent: 0, speedX: 7.84),
            trackStates: [
              CdTrackState(number: 2, title: 'Mitte', state: 'rereading', fraction: 0.3),
            ],
          ),
        });
        await tester.pump();
        expect(find.text('Reading again 30 %'), findsOneWidget);
        expect(find.textContaining('Drive reads at 7.8x'), findsOneWidget);
        // A track that was filed without a confirmation says so, with its own colour.
        jobs({
          '12': const CdJobState(
            id: '12',
            state: 'working',
            phase: 'ripping',
            track: 1,
            tracksTotal: 3,
            trackStates: [
              CdTrackState(number: 2, title: 'Mitte', state: 'unverified', fraction: 1),
            ],
          ),
        });
        await tester.pump();
        expect(find.text('Taken, not confirmed'), findsOneWidget);
        jobs({'12': const CdJobState(id: '12', state: 'done')});
        await tester.pump();
        await tester.tap(find.byKey(const Key('cd-close')));
        await tester.pumpAndSettle();
      },
    );

    testWidgets('a whole CD says so and offers no repair', (tester) async {
      const whole = CdPresent(
        book: 'Sturz',
        author: 'Follett',
        tracksInLibrary: 4,
        tracksOnDisc: 4,
        complete: true,
      );
      await open(tester, _drive(present: whole));
      expect(find.text('The CD is whole in the library (4 of 4 tracks).'), findsOneWidget);
      expect(find.byKey(const Key('cd-heal')), findsNothing);
    });

    testWidgets('Import anyway asks first; No leaves it, Yes goes to the form', (tester) async {
      await open(tester, _drive(present: _present));
      await tester.tap(find.byKey(const Key('cd-force')));
      await tester.pump();
      expect(find.text('Import the CD again?'), findsOneWidget);
      await tester.tap(find.byKey(const Key('cd-force-no')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('cd-present')), findsOneWidget);
      expect(find.byKey(const Key('cd-save')), findsNothing);

      await tester.tap(find.byKey(const Key('cd-force')));
      await tester.pump();
      await tester.tap(find.byKey(const Key('cd-force-yes')));
      for (var i = 0; i < 4; i++) {
        await tester.pump();
      }
      expect(find.byKey(const Key('cd-save')), findsOneWidget);
      await tester.tap(find.byKey(const Key('cd-save')));
      await tester.pump();
      await tester.pump();
      expect(server.started?.force, isTrue);
      jobs({'12': const CdJobState(id: '12', state: 'done')});
      await tester.pump();
      await tester.tap(find.byKey(const Key('cd-close')));
      await tester.pumpAndSettle();
    });

    testWidgets(
      'an interrupted rip: the note with the time, Continue instead of Import, and one box to read again',
      (tester) async {
        final r = CdResume(
          discId: 'X',
          tracksDone: const [1, 2, 3],
          tracksTotal: 4,
          interruptedAt: DateTime(2026, 10, 28, 14, 5),
        );
        await open(tester, _drive(resume: r));
        expect(
          find.textContaining('This CD was read to 3 of 4 tracks already (interrupted'),
          findsOneWidget,
        );
        expect(find.widgetWithText(FilledButton, 'Continue'), findsOneWidget);
        await tester.tap(find.byKey(const Key('cd-reread')));
        await tester.pump();
        expect(
          find.widgetWithText(FilledButton, 'Import'),
          findsOneWidget,
          reason: 'Read again is a new import',
        );
        await tester.tap(find.byKey(const Key('cd-save')));
        await tester.pump();
        await tester.pump();
        expect(server.started?.resume, isFalse);
        jobs({'12': const CdJobState(id: '12', state: 'done')});
        await tester.pump();
        await tester.tap(find.byKey(const Key('cd-close')));
        await tester.pumpAndSettle();
      },
    );

    testWidgets('settings changed: it says the CD is read again, and no Continue', (tester) async {
      await open(tester, _drive(resume: const CdResume(discId: 'X', settingsMatch: false)));
      expect(find.byKey(const Key('cd-resume-lost')), findsOneWidget);
      expect(find.widgetWithText(FilledButton, 'Continue'), findsNothing);
      expect(find.byKey(const Key('cd-reread')), findsNothing);
    });

    testWidgets('the server\'s mismatch hint is shown while it reads', (tester) async {
      server.resumeAnswer = 'mismatch';
      await open(tester, _drive());
      await tester.tap(find.byKey(const Key('cd-save')));
      await tester.pump();
      await tester.pump();
      expect(
        find.text(
          'The settings or the choice of tracks changed, so the CD is read again from the start.',
        ),
        findsOneWidget,
      );
      jobs({'12': const CdJobState(id: '12', state: 'done')});
      await tester.pump();
      await tester.tap(find.byKey(const Key('cd-close')));
      await tester.pumpAndSettle();
    });

    testWidgets(
      'the states of the tracks, the quiet buffer line, and the end with a hint; Close tells the server',
      (tester) async {
        await open(tester, _drive());
        await tester.tap(find.byKey(const Key('cd-save')));
        await tester.pump();
        await tester.pump();
        jobs({
          '12': const CdJobState(
            id: '12',
            state: 'working',
            phase: 'ripping',
            track: 1,
            tracksTotal: 4,
            buffer: CdBuffer(seconds: 41, max: 230, percent: 18),
            trackStates: [
              CdTrackState(number: 1, title: 'Anfang', state: 'done'),
              CdTrackState(number: 2, title: 'Mitte', state: 'verifying'),
              CdTrackState(number: 3, title: 'Dritter Teil', state: 'encoding'),
              CdTrackState(number: 4, title: 'Ende'),
            ],
          ),
        });
        await tester.pump();
        expect(find.text('Done'), findsOneWidget);
        expect(find.text('Checking'), findsOneWidget);
        expect(find.text('Converting'), findsOneWidget);
        expect(find.text('Waiting'), findsOneWidget);
        expect(find.text('Buffer 41 s of 230 s'), findsOneWidget);

        jobs({
          '12': const CdJobState(
            id: '12',
            state: 'done',
            error: 'unreadable_track',
            message: 'track 4',
          ),
        });
        await tester.pump();
        expect(
          find.text('Some tracks could not be read from the CD (it may be scratched).'),
          findsOneWidget,
        );
        await tester.tap(find.byKey(const Key('cd-close')));
        await tester.pumpAndSettle();
        expect(server.dismissed, ['drive0']);
      },
    );

    testWidgets('the new texts exist in German too', (tester) async {
      await open(
        tester,
        _drive(
          resume: CdResume(
            discId: 'X',
            tracksDone: const [1],
            tracksTotal: 4,
            interruptedAt: DateTime(2026, 10, 28, 14, 5),
          ),
        ),
        locale: const Locale('de'),
      );
      expect(find.textContaining('Diese CD wurde schon bis Titel 1 von 4 gelesen'), findsOneWidget);
      expect(find.widgetWithText(FilledButton, 'Fortsetzen'), findsOneWidget);
    });
  });
}
