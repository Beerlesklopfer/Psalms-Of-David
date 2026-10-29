// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/cd/cd_drive_watcher.dart';
import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/cd/cd_models.dart';

import 'cd_fakes.dart';

CdDrivesState _one(CdTray t, CdMedium m, {bool denied = false, bool ripping = false}) =>
    CdDrivesState([CdDriveInfo(device: 'd', tray: t, medium: m, denied: denied, ripping: ripping)]);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('what the symbols may show', () {
    test('no drive: nothing', () {
      expect(CdDrivesState.none.importable, isNull);
      expect(CdDrivesState.none.ejectable, isNull);
    });

    test('open tray: neither, whatever was in it before', () {
      for (final m in CdMedium.values) {
        final s = _one(CdTray.open, m);
        expect((s.importable, s.ejectable), (null, null), reason: '$m');
      }
    });

    test('shut tray without a disc: neither', () {
      final s = _one(CdTray.closed, CdMedium.none);
      expect((s.importable, s.ejectable), (null, null));
    });

    test('shut tray with a data disc: eject only', () {
      final s = _one(CdTray.closed, CdMedium.other);
      expect((s.importable, s.ejectable?.device), (null, 'd'));
    });

    test('shut tray with an audio CD: both', () {
      final s = _one(CdTray.closed, CdMedium.audio);
      expect((s.importable?.device, s.ejectable?.device), ('d', 'd'));
    });

    test('a state that is not known yet shows nothing', () {
      final s = _one(CdTray.unknown, CdMedium.unknown);
      expect((s.importable, s.ejectable), (null, null));
    });

    test(
      'a drive the server may not use shows nothing; a drive that rips cannot be imported again',
      () {
        expect(_one(CdTray.closed, CdMedium.audio, denied: true).importable, isNull);
        expect(_one(CdTray.closed, CdMedium.audio, denied: true).ejectable, isNull);
        final r = _one(CdTray.closed, CdMedium.audio, ripping: true);
        expect((r.importable, r.ripping, r.ejectable?.device), (null, true, 'd'));
      },
    );

    test('the state is read from the JSON of the server', () {
      final s = CdDrivesState.fromJson({
        'mode': 'poll',
        'drives': [
          {
            'device': 'a',
            'tray': 'closed',
            'medium': 'audio',
            'tracks': 12,
            'seconds': 3600,
            'denied': false,
            'ripping': false,
            'disc_id': 'x',
          },
        ],
      });
      expect((s.importable?.tracks, s.importable?.seconds), (12, 3600));
      expect(CdDrivesState.fromJson({}).hasDrive, isFalse);
    });
  });

  group('drive watcher', () {
    test('reads the state, follows pushes, keeps the last state when the server fails', () async {
      final events = FakeEvents()..drivesValue = _one(CdTray.closed, CdMedium.audio);
      final w = CdDriveWatcher(events)..start();
      await Future<void>.delayed(Duration.zero);
      expect(w.value.importable, isNotNull);

      events.driveStream.add(_one(CdTray.open, CdMedium.none));
      await Future<void>.delayed(Duration.zero);
      expect(w.value.importable, isNull);

      events.drivesError = Exception('offline');
      await w.reload();
      expect(w.value.hasDrive, isTrue, reason: 'last known');
      w.dispose();
    });

    test('a server that cannot be reached from the start shows nothing and says nothing', () async {
      final events = FakeEvents()..drivesError = Exception('offline');
      final w = CdDriveWatcher(events)..start();
      await Future<void>.delayed(Duration.zero);
      expect(w.value, CdDrivesState.none);
      w.dispose();
    });

    test('returning to the foreground reads again', () async {
      final events = FakeEvents();
      final w = CdDriveWatcher(events)..start();
      await Future<void>.delayed(Duration.zero);
      final before = events.driveReads;
      w.didChangeAppLifecycleState(AppLifecycleState.resumed);
      await Future<void>.delayed(Duration.zero);
      expect(events.driveReads, before + 1);
      w.dispose();
    });

    test('the drive message of the server is parsed', () {
      final m = parseCdMessage(
        '{"v":1,"type":"drive","mode":"udev","drives":[{"device":"a","tray":"closed","medium":"audio"}]}',
      );
      expect(m.drives?.importable?.device, 'a');
      expect(parseCdMessage('{"type":"hello"}').drives, isNull);
    });
  });
}
