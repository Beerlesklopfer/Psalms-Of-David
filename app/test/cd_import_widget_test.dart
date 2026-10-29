// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/cd/cd_drive_watcher.dart';
import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/cd/cd_import_ui.dart';
import 'package:psalm/cd/cd_kit.dart';
import 'package:psalm/cd/cd_models.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/library_screen.dart';
import 'package:psalm/server/api_client.dart';

import 'cd_fakes.dart';

CdDrivesState _state(CdTray t, CdMedium m, {bool ripping = false}) => CdDrivesState([
  CdDriveInfo(
    device: 'drive0',
    tray: t,
    medium: m,
    tracks: 3,
    trackList: audioDrive.trackList,
    seconds: 365,
    ripping: ripping,
  ),
]);

void main() {
  late FakeCdServer server;
  late FakeEvents events;
  late CdKit kit;
  late PlayerController player;

  setUp(() {
    server = FakeCdServer()
      ..candidates = const [
        CdCandidate(author: 'Ken Follett', title: 'Sturz der Titanen', disc: 1, discsTotal: 12),
      ];
    events = FakeEvents();
    final watcher = CdDriveWatcher(events);
    final tracker = CdJobTracker(events);
    kit = CdKit(server: server, drives: watcher, tracker: tracker);
    player = PlayerController([]);
    player.refresher = () async {};
  });
  tearDown(() => kit.dispose());

  Future<void> pump(WidgetTester tester, {CdKit? cd, Locale locale = const Locale('en')}) =>
      tester.pumpWidget(
        PlayerScope(
          controller: player,
          child: MaterialApp(
            locale: locale,
            localizationsDelegates: AppLocalizations.localizationsDelegates,
            supportedLocales: AppLocalizations.supportedLocales,
            home: Scaffold(body: LibraryScreen(cd: cd)),
          ),
        ),
      );

  Future<void> show(WidgetTester tester, CdDrivesState s) async {
    kit.drives.value = s;
    await tester.pump();
  }

  final importButton = find.byKey(const Key('cd-import-button'));
  final ejectButton = find.byKey(const Key('cd-eject-button'));
  final progress = find.byKey(const Key('cd-progress'));

  group('the symbols', () {
    testWidgets('without a server there are none, and nothing is said', (tester) async {
      await pump(tester);
      expect(find.byIcon(Icons.album), findsNothing);
      expect(find.byIcon(Icons.eject), findsNothing);
    });

    testWidgets('no drive, or a server that said nothing: no symbols', (tester) async {
      await pump(tester, cd: kit);
      expect(find.byIcon(Icons.album), findsNothing);
      expect(find.byIcon(Icons.eject), findsNothing);
      expect(find.byType(SnackBar), findsNothing);
    });

    testWidgets('open tray: neither symbol (and so nothing that could close it)', (tester) async {
      await pump(tester, cd: kit);
      for (final m in [CdMedium.none, CdMedium.audio, CdMedium.other]) {
        await show(tester, _state(CdTray.open, m));
        expect(importButton, findsNothing, reason: '$m');
        expect(ejectButton, findsNothing, reason: '$m');
      }
    });

    testWidgets('shut tray without a disc: neither', (tester) async {
      await pump(tester, cd: kit);
      await show(tester, _state(CdTray.closed, CdMedium.none));
      expect(importButton, findsNothing);
      expect(ejectButton, findsNothing);
    });

    testWidgets('shut tray with a data disc: eject only', (tester) async {
      await pump(tester, cd: kit);
      await show(tester, _state(CdTray.closed, CdMedium.other));
      expect(importButton, findsNothing);
      expect(ejectButton, findsOneWidget);
    });

    testWidgets(
      'shut tray with an audio CD: both, as icons with tooltips, import, eject, then refresh',
      (tester) async {
        await pump(tester, cd: kit);
        await show(tester, _state(CdTray.closed, CdMedium.audio));
        expect(find.byIcon(Icons.album), findsOneWidget);
        expect(find.byIcon(Icons.eject), findsOneWidget);
        expect(find.text('Import CD'), findsNothing, reason: 'an icon, not a text button');
        expect(tester.widget<IconButton>(importButton).tooltip, 'Import CD');
        expect(tester.widget<IconButton>(ejectButton).tooltip, 'Eject CD');
        final x = [
          for (final f in [importButton, ejectButton, find.byIcon(Icons.refresh)])
            tester.getCenter(f).dx,
        ];
        expect(
          x[0] < x[1] && x[1] < x[2],
          isTrue,
          reason: 'import, eject, refresh from left to right: $x',
        );
      },
    );

    testWidgets('the tooltips are German in German', (tester) async {
      await pump(tester, cd: kit, locale: const Locale('de'));
      await show(tester, _state(CdTray.closed, CdMedium.audio));
      expect(tester.widget<IconButton>(importButton).tooltip, 'CD importieren');
      expect(tester.widget<IconButton>(ejectButton).tooltip, 'CD auswerfen');
    });

    testWidgets('a push from the server changes them at once; taking the disc out hides both', (
      tester,
    ) async {
      await pump(tester, cd: kit);
      kit.drives.start();
      await tester.runAsync(() => Future<void>.delayed(Duration.zero));
      events.driveStream.add(_state(CdTray.closed, CdMedium.audio));
      await tester.runAsync(() => Future<void>.delayed(Duration.zero));
      await tester.pump();
      expect(importButton, findsOneWidget);
      events.driveStream.add(_state(CdTray.closed, CdMedium.none));
      await tester.runAsync(() => Future<void>.delayed(Duration.zero));
      await tester.pump();
      expect(importButton, findsNothing);
      expect(ejectButton, findsNothing);
    });

    testWidgets(
      'while an import runs: progress instead of import, eject disabled; after the end both are back',
      (tester) async {
        await pump(tester, cd: kit);
        await show(tester, _state(CdTray.closed, CdMedium.audio, ripping: true));
        expect(importButton, findsNothing);
        expect(progress, findsOneWidget);
        expect(
          tester.widget<IconButton>(ejectButton).onPressed,
          isNull,
          reason: 'eject is off while it rips',
        );
        expect(tester.widget<IconButton>(progress).tooltip, 'Importing the CD');

        kit.tracker.value = {'12': const CdJobState(id: '12', state: 'working', fraction: 0.4)};
        await tester.pump();
        expect(tester.widget<IconButton>(progress).tooltip, 'Importing the CD 40 %');

        kit.tracker.value = {'12': const CdJobState(id: '12', state: 'done')};
        await show(tester, _state(CdTray.closed, CdMedium.audio));
        expect(progress, findsNothing);
        expect(importButton, findsOneWidget);
        expect(
          tester.widget<IconButton>(ejectButton).onPressed,
          isNotNull,
          reason: 'eject shows after the end of the import',
        );
      },
    );

    testWidgets('an import of another device shows the progress here too', (tester) async {
      await pump(tester, cd: kit);
      await show(tester, _state(CdTray.closed, CdMedium.audio));
      kit.tracker.value = {
        '7': const CdJobState(id: '7', state: 'working', fraction: 0.1, device: 'other'),
      };
      await tester.pump();
      expect(progress, findsOneWidget);
      expect(importButton, findsNothing);
    });
  });

  group('eject', () {
    testWidgets('sends the eject to the server for that drive and says so', (tester) async {
      await pump(tester, cd: kit);
      await show(tester, _state(CdTray.closed, CdMedium.other));
      await tester.tap(ejectButton);
      await tester.pump();
      await tester.pump();
      expect(server.ejects, ['drive0']);
      expect(find.text('The CD was ejected.'), findsOneWidget);
    });

    testWidgets('nothing to eject is told as it is', (tester) async {
      server.ejectResult = false;
      await pump(tester, cd: kit);
      await show(tester, _state(CdTray.closed, CdMedium.audio));
      await tester.tap(ejectButton);
      await tester.pump();
      await tester.pump();
      expect(find.text('There is nothing to eject.'), findsOneWidget);
    });

    testWidgets('a refusal is shown in words, not as a code', (tester) async {
      server.ejectError = ApiException(409, 'busy');
      await pump(tester, cd: kit);
      await show(tester, _state(CdTray.closed, CdMedium.audio));
      await tester.tap(ejectButton);
      await tester.pump();
      await tester.pump();
      expect(find.text('The drive is busy with another CD.'), findsOneWidget);
      expect(find.text('busy'), findsNothing);
    });

    testWidgets('no answer from the server: shown in words', (tester) async {
      server.ejectError = ApiException(0, 'network');
      await pump(tester, cd: kit);
      await show(tester, _state(CdTray.closed, CdMedium.audio));
      await tester.tap(ejectButton);
      await tester.pump();
      await tester.pump();
      expect(find.text('The server does not answer.'), findsOneWidget);
    });
  });

  group('the import dialog', () {
    Future<void> open(WidgetTester tester) async {
      // The form with its track list is tall.
      tester.view.physicalSize = const Size(1000, 1800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await pump(tester, cd: kit);
      await show(tester, _state(CdTray.closed, CdMedium.audio));
      await tester.tap(importButton);
      await tester.pump();
      await tester.pump();
      await tester.pump();
    }

    testWidgets('shows the match, one Cancel/Save pair, then follows the job to the end', (
      tester,
    ) async {
      await open(tester);
      expect(find.text('Ken Follett'), findsOneWidget);
      expect(find.text('Sturz der Titanen'), findsOneWidget);
      expect(find.text('Audiobook'), findsOneWidget);
      expect(find.byKey(const Key('cd-save')), findsOneWidget);
      expect(find.byKey(const Key('cd-cancel')), findsOneWidget);

      await tester.tap(find.byKey(const Key('cd-save')));
      await tester.pump();
      await tester.pump();
      expect(server.started?.author, 'Ken Follett');
      expect(find.text('Reading track 1 of 3'), findsOneWidget);

      kit.tracker.value = {
        '12': const CdJobState(
          id: '12',
          state: 'working',
          phase: 'ripping',
          track: 1,
          tracksTotal: 3,
          fraction: 0.5,
        ),
      };
      await tester.pump();
      expect(
        find.text('Reading track 2 of 3'),
        findsOneWidget,
        reason: 'track is the number done, so the second is being read',
      );
      kit.tracker.value = {
        '12': const CdJobState(id: '12', state: 'working', phase: 'importing', fraction: 0.9),
      };
      await tester.pump();
      expect(find.text('Adding the CD to the library…'), findsOneWidget);

      kit.tracker.value = {'12': const CdJobState(id: '12', state: 'done')};
      await tester.pump();
      expect(find.text('The CD is now in the library.'), findsOneWidget);
      await tester.tap(find.byKey(const Key('cd-close')));
      await tester.pumpAndSettle();
      expect(find.byType(CdImportDialog), findsNothing);
    });

    testWidgets(
      'lists every track with a checkbox, number, title and length; all chosen; the count and time',
      (tester) async {
        server.candidates = const [
          CdCandidate(author: 'A', title: 'T', trackTitles: ['Anfang', 'Mitte', '']),
        ];
        await open(tester);
        for (final n in [1, 2, 3]) {
          expect(
            tester.widget<CheckboxListTile>(find.byKey(Key('cd-track-$n'))).value,
            isTrue,
            reason: 'track $n',
          );
        }
        expect(find.text('01  Anfang'), findsOneWidget);
        expect(find.text('Track 03'), findsOneWidget, reason: 'no title: localized default');
        expect(find.text('1:00'), findsOneWidget);
        expect(find.text('3:05'), findsOneWidget);
        expect(find.text('3 of 3 tracks, 6:05'), findsOneWidget);
      },
    );

    testWidgets('the default title is German in German', (tester) async {
      tester.view.physicalSize = const Size(1000, 1800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      server.candidates = const [CdCandidate(author: 'A', title: 'T')];
      await pump(tester, cd: kit, locale: const Locale('de'));
      await show(tester, _state(CdTray.closed, CdMedium.audio));
      await tester.tap(importButton);
      await tester.pump();
      await tester.pump();
      await tester.pump();
      expect(find.text('Titel 01'), findsOneWidget);
      expect(find.text('3 von 3 Titeln, 6:05'), findsOneWidget);
    });

    testWidgets(
      'a tap on a row, then All, None and Invert (icon buttons with tooltips) change the choice',
      (tester) async {
        await open(tester);
        expect(
          tester.widget<IconButton>(find.byKey(const Key('cd-select-all'))).tooltip,
          'Select all tracks',
        );
        await tester.tap(find.byKey(const Key('cd-track-2')));
        await tester.pump();
        expect(find.text('2 of 3 tracks, 4:05'), findsOneWidget);
        await tester.tap(find.byKey(const Key('cd-select-invert')));
        await tester.pump();
        expect(find.text('1 of 3 tracks, 2:00'), findsOneWidget);
        await tester.tap(find.byKey(const Key('cd-select-none')));
        await tester.pump();
        expect(find.text('0 of 3 tracks, 0:00'), findsOneWidget);
        expect(
          tester.widget<FilledButton>(find.byKey(const Key('cd-save'))).onPressed,
          isNull,
          reason: 'no track, no import',
        );
        await tester.tap(find.byKey(const Key('cd-select-all')));
        await tester.pump();
        expect(tester.widget<FilledButton>(find.byKey(const Key('cd-save'))).onPressed, isNotNull);
      },
    );

    testWidgets('Save sends only the chosen tracks; with all chosen it sends none of "only"', (
      tester,
    ) async {
      await open(tester);
      await tester.tap(find.byKey(const Key('cd-track-1')));
      await tester.pump();
      await tester.tap(find.byKey(const Key('cd-save')));
      await tester.pump();
      await tester.pump();
      expect(server.started?.only, [2, 3]);
      kit.tracker.value = {'12': const CdJobState(id: '12', state: 'done')};
      await tester.pump();
      await tester.tap(find.byKey(const Key('cd-close')));
      await tester.pumpAndSettle();
    });

    testWidgets('a long list scrolls inside the dialog', (tester) async {
      final many = CdDriveInfo(
        device: 'drive0',
        tray: CdTray.closed,
        medium: CdMedium.audio,
        tracks: 40,
        trackList: [for (var i = 1; i <= 40; i++) CdTrackInfo(number: i, seconds: 100)],
      );
      tester.view.physicalSize = const Size(1000, 1800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await pump(tester, cd: kit);
      await show(tester, CdDrivesState([many]));
      await tester.tap(importButton);
      await tester.pump();
      await tester.pump();
      await tester.pump();
      expect(find.byKey(const Key('cd-track-1')), findsOneWidget);
      expect(
        find.byKey(const Key('cd-track-40')),
        findsNothing,
        reason: 'built lazily, outside the visible part',
      );
      await tester.drag(find.byKey(const Key('cd-tracks')), const Offset(0, -3000));
      await tester.pump();
      expect(find.byKey(const Key('cd-track-40')), findsOneWidget);
      expect(find.byKey(const Key('cd-save')), findsOneWidget);
    });

    testWidgets('cancel while it runs asks the server to stop', (tester) async {
      await open(tester);
      await tester.tap(find.byKey(const Key('cd-save')));
      await tester.pump();
      await tester.pump();
      await tester.tap(find.byKey(const Key('cd-cancel')));
      await tester.pump();
      expect(server.cancelled, ['12']);
      kit.tracker.value = {'12': const CdJobState(id: '12', state: 'failed', error: 'cancelled')};
      await tester.pump();
      expect(
        find.text('The import was cancelled. Nothing was added to the library.'),
        findsOneWidget,
      );
    });

    testWidgets(
      'a running import can be closed without stopping it, and the strip opens it again',
      (tester) async {
        await open(tester);
        await tester.tap(find.byKey(const Key('cd-save')));
        await tester.pump();
        await tester.pump();
        kit.tracker.value = {
          '12': const CdJobState(
            id: '12',
            state: 'working',
            phase: 'ripping',
            track: 0,
            tracksTotal: 13,
            fraction: 0.02,
          ),
        };
        await tester.pump();
        // Two buttons while it runs: one stops the import, the other only closes the dialog.
        expect(find.text('Stop import'), findsOneWidget);
        expect(find.byKey(const Key('cd-hide')), findsOneWidget);
        await tester.tap(find.byKey(const Key('cd-hide')));
        await tester.pumpAndSettle();
        expect(find.byType(CdImportDialog), findsNothing);
        expect(server.cancelled, isEmpty, reason: 'closing the dialog does not stop the import');
        // The import goes on and the strip is the way back to it.
        expect(find.byKey(const Key('cd-strip')), findsOneWidget);
        await tester.tap(find.byKey(const Key('cd-strip')));
        await tester.pump();
        await tester.pump();
        expect(find.byType(CdImportDialog), findsOneWidget);
        expect(server.started, isNotNull);
        // End the test cleanly: the dialog is closed and the job ends, so that no timer is left.
        await tester.tap(find.byKey(const Key('cd-hide')));
        await tester.pumpAndSettle();
        kit.tracker.value = {'12': const CdJobState(id: '12', state: 'done', tracksTotal: 13)};
        await tester.pump();
        await tester.pump(const Duration(seconds: 10));
      },
    );

    testWidgets('cancel in the form closes it and starts nothing', (tester) async {
      await open(tester);
      await tester.tap(find.byKey(const Key('cd-cancel')));
      await tester.pumpAndSettle();
      expect(find.byType(CdImportDialog), findsNothing);
      expect(server.started, isNull);
    });

    testWidgets(
      'not found online: Save waits for author and title; the reason of a failed lookup is in words',
      (tester) async {
        server.candidates = const [];
        server.lookupError = ApiException(409, 'no_contact');
        await open(tester);
        expect(find.textContaining('contact address for MusicBrainz'), findsOneWidget);
        FilledButton save() => tester.widget<FilledButton>(find.byKey(const Key('cd-save')));
        expect(save().onPressed, isNull);
        await tester.enterText(find.byKey(const Key('cd-author')), 'A');
        await tester.enterText(find.byKey(const Key('cd-title')), 'T');
        await tester.pump();
        expect(save().onPressed, isNotNull);
      },
    );

    testWidgets('a refusal of the server (busy) is shown in words with a Close button', (
      tester,
    ) async {
      server.startError = ApiException(409, 'tray_open');
      await open(tester);
      await tester.tap(find.byKey(const Key('cd-save')));
      await tester.pump();
      await tester.pump();
      expect(find.byKey(const Key('cd-error')), findsOneWidget);
      expect(find.text('The tray of the drive is open. Close it by hand first.'), findsOneWidget);
      expect(find.byKey(const Key('cd-close')), findsOneWidget);
    });

    testWidgets('tracks already in the library are named by number, in both languages', (
      tester,
    ) async {
      server.startError = ApiException(409, 'disc_exists', {
        'taken': [3, 5],
      });
      await open(tester);
      await tester.tap(find.byKey(const Key('cd-save')));
      await tester.pump();
      await tester.pump();
      expect(find.text('Tracks 3, 5 are already in the library.'), findsOneWidget);
    });

    testWidgets('an empty choice that reaches the server is told in words', (tester) async {
      server.startError = ApiException(422, 'empty_selection');
      await open(tester);
      await tester.tap(find.byKey(const Key('cd-save')));
      await tester.pump();
      await tester.pump();
      expect(find.text('No track is chosen.'), findsOneWidget);
    });

    testWidgets('a failed job shows the code of the server in words', (tester) async {
      await open(tester);
      await tester.tap(find.byKey(const Key('cd-save')));
      await tester.pump();
      await tester.pump();
      kit.tracker.value = {'12': const CdJobState(id: '12', state: 'failed', error: 'disc_exists')};
      await tester.pump();
      expect(
        find.text('This CD is already in the library; the server does not overwrite it.'),
        findsOneWidget,
      );
    });

    testWidgets(
      'the progress symbol opens the dialog on the running import, without starting another',
      (tester) async {
        await pump(tester, cd: kit);
        await show(tester, _state(CdTray.closed, CdMedium.audio, ripping: true));
        kit.tracker.value = {
          '12': const CdJobState(
            id: '12',
            state: 'working',
            phase: 'ripping',
            track: 0,
            tracksTotal: 3,
            fraction: 0.2,
          ),
        };
        await tester.pump();
        await tester.tap(progress);
        await tester.pump();
        await tester.pump();
        expect(find.text('Reading track 1 of 3'), findsOneWidget);
        expect(server.started, isNull);
        await tester.tap(find.byKey(const Key('cd-hide')));
        await tester.pumpAndSettle();
        expect(find.byType(CdImportDialog), findsNothing);
      },
    );
  });
}
