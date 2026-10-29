// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/cd/cd_import_controller.dart';
import 'package:psalm/cd/cd_models.dart';
import 'package:psalm/server/api_client.dart';

import 'cd_fakes.dart';

const _sturz = CdCandidate(
  author: 'Ken Follett',
  title: 'Sturz der Titanen',
  disc: 2,
  discsTotal: 12,
  mbid: 'mb-1',
  narrator: 'Johannes Steck',
  year: '2010',
  trackTitles: ['Teil 1', '', 'Teil 3'],
);

void main() {
  late FakeCdServer server;
  late FakeEvents events;
  late CdJobTracker tracker;

  setUp(() {
    server = FakeCdServer();
    events = FakeEvents();
    tracker = CdJobTracker(events)..start();
  });
  tearDown(() => tracker.dispose());

  CdImportController make({String? resume, Duration poll = kCdJobPoll}) => CdImportController(
    server: server,
    tracker: tracker,
    drive: audioDrive,
    resumeJobId: resume,
    pollInterval: poll,
  );

  Future<CdImportController> ready([List<CdCandidate> c = const [_sturz]]) async {
    server.candidates = c;
    final ctl = make();
    await ctl.identify();
    return ctl;
  }

  void push(CdJobState s) {
    events.connections.first.add(s);
  }

  Future<void> tick() => Future<void>.delayed(Duration.zero);

  group('the search by hand', () {
    const other = CdCandidate(author: 'Ken Follett', title: 'Ein anderes Buch', mbid: 'mb-2');

    test('a search adds what MusicBrainz finds to the matches and leaves the form alone', () async {
      final c = await ready();
      server.searchResults = const [other];
      await c.search('  Ken Follett Buch ');
      expect(server.lookups.last.query, 'Ken Follett Buch');
      expect(c.candidates.map((e) => e.title), ['Sturz der Titanen', 'Ein anderes Buch']);
      expect((c.title, c.searchError, c.searching), ('Sturz der Titanen', null, false));
      c.dispose();
    });

    test('a release from a search gets its track titles for this disc when it is chosen', () async {
      final c = await ready();
      server.searchResults = const [other];
      server.fetched['mb-2'] = const CdCandidate(
        author: 'Ken Follett',
        title: 'Ein anderes Buch',
        disc: 2,
        discsTotal: 5,
        mbid: 'mb-2',
        trackTitles: ['Eins', 'Zwei', 'Drei'],
      );
      await c.search('Buch');
      c.selectCandidate(1);
      await tick();
      await tick();
      expect(server.lookups.last.releaseId, 'mb-2');
      expect((c.title, c.disc, c.discsTotal), ('Ein anderes Buch', 2, 5));
      expect(c.trackTitles, ['Eins', 'Zwei', 'Drei']);
      c.dispose();
    });

    test('a search that finds nothing new leaves the matches as they are', () async {
      final c = await ready();
      await c.search('zzz');
      expect(c.candidates.length, 1);
      c.dispose();
    });
  });

  group('the form', () {
    test('a found disc fills the form; the server reads the table of contents itself, so only the drive is sent', () async {
      final c = await ready();
      expect(c.phase, CdPhase.confirm);
      expect(
        (c.author, c.title, c.disc, c.discsTotal),
        ('Ken Follett', 'Sturz der Titanen', 2, 12),
      );
      expect(c.trackTitles, [
        'Teil 1',
        '',
        'Teil 3',
      ], reason: 'no title: the dialog says "Track 02" in the language of the user');
      expect(server.lookedUpDevice, 'drive0');
      c.dispose();
    });

    test('no match or a failing lookup: empty form, Save needs author and title', () async {
      server.lookupError = ApiException(409, 'no_contact');
      final c = make();
      await c.identify();
      expect(
        (c.phase, c.lookupFound, c.lookupError, c.canSave),
        (CdPhase.confirm, false, 'no_contact', false),
      );
      c.update(author: 'A', title: 'T');
      expect(c.canSave, isTrue);
      c.dispose();
    });

    test(
      'the kind and the eject checkbox start from the server settings and can be changed',
      () async {
        server.settingsValue = const CdSettings(kind: 'music', eject: true);
        final c = await ready();
        expect((c.kind, c.eject), ('music', true));
        c.update(kind: 'audiobook', eject: false);
        expect((c.kind, c.eject), ('audiobook', false));
        c.dispose();
      },
    );

    test('settings that cannot be read leave the documented fallbacks', () async {
      server.settingsError = Exception('x');
      final c = await ready();
      expect((c.kind, c.eject), (CdDefaults.kind, CdDefaults.eject));
      c.dispose();
    });

    test('cancelling the form starts nothing', () async {
      final c = await ready();
      await c.cancel();
      expect(c.phase, CdPhase.cancelled);
      expect(server.started, isNull);
      c.dispose();
    });
  });

  group('the track list', () {
    test('all tracks are chosen at first; the count and the playing time follow', () async {
      final c = await ready();
      expect(
        (c.selectedCount, c.allSelected, c.selectedDuration),
        (3, true, const Duration(seconds: 365)),
      );
      c.toggle(2);
      expect(
        (c.selectedCount, c.allSelected, c.selectedDuration),
        (2, false, const Duration(seconds: 245)),
      );
      c.dispose();
    });

    test('All, None, Invert', () async {
      final c = await ready();
      c.selectNone();
      expect((c.selectedCount, c.canSave), (0, false));
      c.toggle(1);
      c.invertSelection();
      expect(c.selected, {2, 3});
      c.selectAll();
      expect(c.selected, {1, 2, 3});
      c.dispose();
    });

    test('with no title chosen Save is off, even with author and title', () async {
      final c = await ready();
      expect(c.canSave, isTrue);
      c.selectNone();
      expect(c.canSave, isFalse);
      await c.save();
      expect(server.started, isNull);
      c.dispose();
    });

    test(
      'all tracks: no "only" is sent; some tracks: the numbers on the disc, with gaps',
      () async {
        var c = await ready();
        await c.save();
        expect(server.started!.only, isNull);
        expect(server.started!.toJson().containsKey('only'), isFalse);
        c.dispose();

        c = await ready();
        c.toggle(2);
        await c.save();
        expect(server.started!.only, [1, 3]);
        expect(server.started!.toJson()['only'], [1, 3]);
        expect(server.started!.tracks, [
          'Teil 1',
          '',
          'Teil 3',
        ], reason: 'the titles of all tracks go along, by position');
        c.dispose();
      },
    );

    test('a drive that only says how many tracks it has gives tracks 1 to n', () async {
      final c = CdImportController(
        server: server,
        tracker: tracker,
        drive: const CdDriveInfo(
          device: 'd',
          tray: CdTray.closed,
          medium: CdMedium.audio,
          tracks: 2,
        ),
      );
      await c.identify();
      expect(c.tracks.map((t) => t.number), [1, 2]);
      expect(c.selected, {1, 2});
      c.dispose();
    });

    test('a track list with gaps in the numbers is kept as it is', () async {
      final c = CdImportController(
        server: server,
        tracker: tracker,
        drive: const CdDriveInfo(
          device: 'd',
          tray: CdTray.closed,
          medium: CdMedium.audio,
          tracks: 2,
          trackList: [CdTrackInfo(number: 3, seconds: 10), CdTrackInfo(number: 5, seconds: 20)],
        ),
      );
      await c.identify();
      c.update(author: 'A', title: 'T');
      c.toggle(3);
      await c.save();
      expect(server.started!.only, [5]);
      c.dispose();
    });

    test('the drive state: tracks is the count, track_list the numbers and lengths; no list means no lengths', () {
      final a = CdDriveInfo.fromJson({
        'device': 'd',
        'tracks': 2,
        'track_list': [
          {'number': 1, 'seconds': 30},
          {'number': 2, 'seconds': 40.5},
        ],
      });
      expect(a.tracks, 2);
      expect(a.trackList.map((t) => t.number), [1, 2]);
      expect(a.trackList[1].seconds, 40.5);
      final b = CdDriveInfo.fromJson({'device': 'd', 'tracks': 14});
      expect(b.tracks, 14);
      expect(b.trackList, isEmpty);
    });

    test('the lookup says audiobook with a bool', () {
      expect(CdCandidate.fromJson({'title': 'T', 'audiobook': true}).audiobook, isTrue);
      expect(CdCandidate.fromJson({'title': 'T'}).audiobook, isFalse);
    });

    test('empty_selection and invalid_selection are the codes of the server', () async {
      for (final code in ['empty_selection', 'invalid_selection']) {
        server.startError = ApiException(422, code);
        final c = await ready();
        await c.save();
        expect(c.failureCode, code);
        c.dispose();
      }
    });

    test('tracks that are in the library already: the numbers come with the refusal', () async {
      server.startError = ApiException(409, 'disc_exists', {
        'error': 'disc_exists',
        'taken': [3, 5],
      });
      final c = await ready();
      await c.save();
      expect(c.failureCode, 'disc_exists');
      expect(c.failureTaken, [3, 5]);
      c.dispose();
    });

    test('an invalid selection is the failure with its code', () async {
      server.startError = ApiException(422, 'invalid_selection');
      final c = await ready();
      await c.save();
      expect(c.failureCode, 'invalid_selection');
      c.dispose();
    });
  });

  group('the kind', () {
    test('follows the server settings; a release that is an audiobook sets it', () async {
      server.settingsValue = const CdSettings(kind: 'music');
      var c = await ready([const CdCandidate(author: 'A', title: 'T')]);
      expect(c.kind, 'music');
      c.dispose();
      c = await ready([const CdCandidate(author: 'A', title: 'T', audiobook: true)]);
      expect(c.kind, 'audiobook');
      c.dispose();
    });
  });

  group('the import', () {
    test('Save asks the server to rip: the data of the form and of the match go along', () async {
      final c = await ready();
      c.update(eject: true);
      await c.save();
      final r = server.started!;
      expect(
        (
          r.kind,
          r.author,
          r.title,
          r.disc,
          r.discsTotal,
          r.mbid,
          r.narrator,
          r.year,
          r.eject,
          r.device,
        ),
        (
          'audiobook',
          'Ken Follett',
          'Sturz der Titanen',
          2,
          12,
          'mb-1',
          'Johannes Steck',
          '2010',
          true,
          'drive0',
        ),
      );
      expect(r.tracks, hasLength(3));
      expect((c.phase, c.jobId), (CdPhase.running, '12'));
      c.dispose();
    });

    test('progress and the end come from the job events, not from a local guess', () async {
      final c = await ready();
      await c.save();
      push(
        const CdJobState(
          id: '12',
          state: 'working',
          phase: 'ripping',
          track: 2,
          tracksTotal: 3,
          fraction: 0.4,
        ),
      );
      await tick();
      expect((c.phase, c.job?.track, c.job?.fraction), (CdPhase.running, 2, 0.4));
      push(const CdJobState(id: '99', state: 'failed', error: 'busy'));
      await tick();
      expect(c.phase, CdPhase.running, reason: 'another job does not matter');
      push(const CdJobState(id: '12', state: 'done', phase: 'done', message: 'ok'));
      await tick();
      expect(c.phase, CdPhase.done);
      c.dispose();
    });

    test('a failed job shows the server\'s code', () async {
      final c = await ready();
      await c.save();
      push(const CdJobState(id: '12', state: 'failed', error: 'verify', message: 'track 2'));
      await tick();
      expect((c.phase, c.failureCode, c.failureDetail), (CdPhase.failed, 'verify', 'track 2'));
      c.dispose();
    });

    test('a job that fails without a code is an import failure', () async {
      final c = await ready();
      await c.save();
      push(const CdJobState(id: '12', state: 'failed'));
      await tick();
      expect(c.failureCode, 'import_failed');
      c.dispose();
    });

    test(
      'the refusals of the server are the failure: busy, tray open, no disc, no network',
      () async {
        for (final (err, code) in [
          (ApiException(409, 'busy'), 'busy'),
          (ApiException(409, 'tray_open'), 'tray_open'),
          (ApiException(409, 'no_disc'), 'no_disc'),
          (ApiException(403, 'forbidden'), 'forbidden'),
          (ApiException(0, 'network'), 'network'),
        ]) {
          server.startError = err;
          final c = await ready();
          await c.save();
          expect((c.phase, c.failureCode), (CdPhase.failed, code));
          c.dispose();
        }
      },
    );

    test(
      'Cancel while it runs tells the server; the end of the job closes the dialog state',
      () async {
        final c = await ready();
        await c.save();
        await c.cancel();
        expect(server.cancelled, ['12']);
        expect(c.phase, CdPhase.running, reason: 'the server decides when it has stopped');
        push(const CdJobState(id: '12', state: 'failed', error: 'cancelled'));
        await tick();
        expect(c.phase, CdPhase.cancelled);
        c.dispose();
      },
    );

    test('Cancel on a job that just ended is not an error', () async {
      server.cancelError = ApiException(409, 'job_closed');
      final c = await ready();
      await c.save();
      await c.cancel();
      expect(c.phase, CdPhase.running);
      c.dispose();
    });

    test('a dialog opened on a running import (also of another device) follows it', () async {
      events.open = [const CdJobState(id: '5', state: 'working', phase: 'ripping', fraction: 0.2)];
      final c = make(resume: '5');
      await tracker.refresh('5');
      tracker.start();
      await c.identify();
      expect(c.phase, CdPhase.running);
      push(const CdJobState(id: '5', state: 'done'));
      await tick();
      expect(c.phase, CdPhase.done);
      expect(server.started, isNull, reason: 'it starts nothing');
      c.dispose();
    });

    test('without news over the channel the job is asked for directly', () {
      fakeAsync((async) {
        events.jobs['12'] = const CdJobState(id: '12', state: 'done');
        final c = make(poll: const Duration(seconds: 3));
        c.identify();
        async.flushMicrotasks();
        c.update(author: 'A', title: 'T');
        c.save();
        async.flushMicrotasks();
        async.elapse(const Duration(seconds: 4));
        expect(c.phase, CdPhase.done);
        c.dispose();
      });
    });
  });

  group('job tracker', () {
    test('open jobs from another device show up, finished ones are not open', () async {
      final e = FakeEvents()..open = [const CdJobState(id: '1', state: 'working', fraction: 0.2)];
      final t = CdJobTracker(e)..start();
      await tick();
      expect(t.open.map((j) => j.id), ['1']);
      e.connections.first.add(const CdJobState(id: '1', state: 'done'));
      await tick();
      expect(t.open, isEmpty);
      t.dispose();
    });

    test('reconnects after the delay when the connection breaks, tells the owner, and not after dispose', () {
      fakeAsync((async) {
        final e = FakeEvents();
        var again = 0;
        final t = CdJobTracker(
          e,
          retryDelay: const Duration(seconds: 3),
          onReconnected: () => again++,
        )..start();
        async.flushMicrotasks();
        expect(e.connections.length, 1);
        e.connections.first.addError('broken');
        async.elapse(const Duration(seconds: 2));
        expect(e.connections.length, 1);
        async.elapse(const Duration(seconds: 2));
        expect((e.connections.length, again), (2, 1));
        t.dispose();
        async.elapse(const Duration(minutes: 1));
        expect(e.connections.length, 2);
      });
    });

    test('messages of the server: one job, the snapshot, the drives; noise is ignored', () {
      expect(
        parseCdMessage(
          '{"v":1,"type":"progress","job":{"id":5,"state":"working","fraction":0.5,"phase":"importing","track":2,"tracks_total":9}}',
        ).jobs.single.tracksTotal,
        9,
      );
      final snap = parseCdMessage(
        '{"type":"snapshot","jobs":[{"id":1,"state":"working"},{"id":2,"state":"done"}]}',
      );
      expect(snap.jobs.map((j) => j.id), ['1', '2']);
      expect(
        parseCdMessage('{"type":"failed","job":{"id":3,"state":"failed","error":"disc_exists"}}')
            .jobs
            .single
            .error,
        'disc_exists',
      );
      expect(parseCdMessage('{"type":"hello"}').jobs, isEmpty);
      expect(parseCdMessage('not json').jobs, isEmpty);
    });
  });
}
