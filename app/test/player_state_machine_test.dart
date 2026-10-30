// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/file_formats.dart';
import 'package:psalm/player/state_machine.dart';
import 'package:psalm/player/transport.dart';

const _now = 1000000;
const _env = Env(hasBackend: true, hasSource: true);

TrackData _t(int n, {String? path, int? part, String? url, bool live = false, String? format, int len = 60000}) =>
    TrackData(disc: 1, number: n, title: 'T$n', lengthMs: len, path: path, partId: part, url: url, live: live, format: format, itemId: '7');

/// A book of the server: three tracks, fetched through the source (no path).
final _server = BookData(id: '7', title: 'Book', kind: 'audiobook', tracks: [_t(1, part: 11), _t(2, part: 12), _t(3, part: 13)]);

/// A book with files on the device.
final _files = BookData(id: '8', title: 'Files', kind: 'audiobook', tracks: [_t(1, path: '/a.mp3'), _t(2, path: '/b.mp3')]);

/// A podcast episode (`ep:5`): one track, fetched through the source.
final _episode = BookData(id: 'ep:5', title: 'Episode', kind: 'podcast', tracks: [_t(1, url: 'http://x/e.mp3', len: 600000)]);

/// A radio station: a live track, no length.
final _radio = BookData(id: 'r1', title: 'Station', kind: 'radio', tracks: [_t(1, url: 'http://x/live', live: true, len: 0)]);

Core _core(BookData? b, {int index = 0, int pos = 0, String? loaded, Env env = _env, int lastReport = _now}) =>
    Core(book: b, trackIndex: index, positionMs: pos, loadedKey: loaded, env: env, lastReportMs: lastReport, lastWrittenMs: _now);

Observation _obs(int pos, [BackendState s = BackendState.playing, AudioErrorKind k = AudioErrorKind.generic, String? msg]) =>
    Observation(positionMs: pos, state: s, errorKind: k, errorMessage: msg);

const _emit = EmitSnapshot();

void main() {
  tearDown(() => runsOnApple = () => false);

  group('track', () {
    test('Idle ignores the commands that need a book', () {
      final s = Idle(_core(null));
      for (final e in <EngineEvent>[const UserToggle(_now), const UserPlay(_now), const UserSeek(_now, 5), const UserSkip(_now, 5), const UserNext(_now), const UserPrevious(_now), const Tick(_now, Observation(positionMs: 0, state: BackendState.idle)), const BackendLoaded(_now, 'x:0')]) {
        final r = transition(s, e);
        expect(r.state, same(s), reason: '${e.runtimeType}');
        expect(r.effects, isEmpty);
      }
    });

    test('open a book of the server: Loading, fetch, then Playing on BackendLoaded', () {
      final r = transition(Idle(_core(null)), UserOpen(_now, _server, startMs: 5000));
      expect(r.state, isA<Loading>());
      expect((r.state as Loading).wantPlay, isTrue);
      expect(r.state.core.positionMs, 5000);
      expect(r.effects, [const StopTick(), const EnsureSpeed(1.0), const FetchAndLoad('7:0', 0), const StartTick(), _emit]);
      expect(r.state.wantsPlay, isTrue);

      final loaded = transition(r.state, const BackendLoaded(_now, '7:0'));
      expect(loaded.state, isA<Playing>());
      expect(loaded.state.core.loadedKey, '7:0');
      expect(loaded.effects, [_emit]);
    });

    test('open a book with files: load at once', () {
      final r = transition(Idle(_core(null)), UserOpen(_now, _files, trackIndex: 1));
      expect(r.state, isA<Playing>());
      expect(r.effects, [const StopTick(), const EnsureSpeed(1.0), const LoadTrack('/b.mp3', 0, true, 'T2'), const StartTick(), _emit]);
    });

    test('pause reports first, then pauses', () {
      final s = Playing(_core(_server, pos: 9000, loaded: '7:0'));
      final r = transition(s, const UserToggle(_now));
      expect(r.state, isA<Paused>());
      expect(r.effects, [const StopTick(), SaveProgress(_server, 0, 9000, false), const EnsureSpeed(1.0), const PausePlayback(), _emit]);
    });

    test('play on a paused loaded track starts the backend and the clock', () {
      final r = transition(Paused(_core(_server, loaded: '7:0')), const UserToggle(_now));
      expect(r.state, isA<Playing>());
      expect(r.effects, [const StopTick(), const EnsureSpeed(1.0), const StartPlayback(), const StartTick(), _emit]);
    });

    test('seek moves the place and the backend, and clamps', () {
      final s = Playing(_core(_server, loaded: '7:0'));
      expect(transition(s, const UserSeek(_now, 7000)).effects, [const SeekBackend(7000), _emit]);
      expect(transition(s, const UserSeek(_now, 999999)).state.core.positionMs, 60000);
      expect(transition(s, const UserSeek(_now, -5)).state.core.positionMs, 0);
      // Not loaded yet: only the place moves.
      expect(transition(Loading(_core(_server), wantPlay: true, reason: LoadReason.open), const UserSeek(_now, 7000)).effects, [_emit]);
    });

    test('seek after the end leaves Ended', () {
      final r = transition(Ended(_core(_server, index: 2, pos: 60000, loaded: '7:2')), const UserSeek(_now, 1000));
      expect(r.state, isA<Paused>());
    });

    test('skip across a track border loads the next track and seeks in it', () {
      final r = transition(Playing(_core(_files, pos: 50000, loaded: '8:0')), const UserSkip(_now, 30000));
      expect(r.state.core.trackIndex, 1);
      expect(r.state.core.positionMs, 20000);
      expect(r.effects.whereType<LoadTrack>().single, const LoadTrack('/b.mp3', 20000, true, 'T2'));
      expect(r.effects.whereType<SeekBackend>().single, const SeekBackend(20000));
    });

    test('next and previous', () {
      final p = Playing(_core(_files, loaded: '8:0'));
      final n = transition(p, const UserNext(_now));
      expect(n.state.core.trackIndex, 1);
      expect(transition(Playing(_core(_files, index: 1, loaded: '8:1')), const UserNext(_now)).state.core.trackIndex, 1, reason: 'the last track has no next');
      expect(transition(Playing(_core(_files, index: 1, loaded: '8:1')), const UserNext(_now)).effects, isEmpty);
      // After the first seconds previous goes to the start, otherwise back.
      expect(transition(Playing(_core(_files, index: 1, pos: 5000, loaded: '8:1')), const UserPrevious(_now)).state.core, isA<Core>().having((c) => c.trackIndex, 'track', 1));
      expect(transition(Playing(_core(_files, index: 1, pos: 1000, loaded: '8:1')), const UserPrevious(_now)).state.core.trackIndex, 0);
    });

    test('tick: position comes from the backend, nothing else changes: only a position message', () {
      final r = transition(Playing(_core(_files, loaded: '8:0')), Tick(_now, _obs(2000)));
      expect(r.state, isA<Playing>());
      expect(r.state.core.positionMs, 2000);
      expect(r.effects, [const EmitPosition()]);
    });

    test('tick while the file is still being fetched, or in a state without a clock, is ignored', () {
      for (final s in <EngineState>[
        Loading(_core(_server), wantPlay: true, reason: LoadReason.open),
        Paused(_core(_server, loaded: '7:0')),
        Ended(_core(_server)),
        Failed(_core(_server), 'x'),
        Unplayable(_core(_server), 0),
      ]) {
        final r = transition(s, Tick(_now, _obs(1)));
        expect(r.state, same(s), reason: '$s');
        expect(r.effects, isEmpty);
      }
      // Playing, but the loaded track is another one (the place was moved by hand).
      final s = Playing(_core(_server, loaded: '7:1'));
      expect(transition(s, Tick(_now, _obs(1))).state, same(s));
    });

    test('tick near the end marks heard and prefetches the next track', () {
      final r = transition(Playing(_core(_server, loaded: '7:0')), Tick(_now, _obs(58000)));
      expect(r.effects, containsAll(<Effect>[const MarkHeard('7', 0), const PrefetchNext(1)]));
      expect(r.state.core.heard, {'7:0'});
      expect(r.state.core.prefetchedKey, '7:1');
      expect(transition(r.state, const PrefetchFailed(_now, '7:1')).state.core.prefetchedKey, isNull);
    });

    test('tick: the backend ended on a middle track: reports the end and goes on with the next', () {
      final r = transition(Playing(_core(_server, loaded: '7:0')), Tick(_now, _obs(60000, BackendState.ended)));
      expect(r.state, isA<Loading>());
      expect(r.state.core.trackIndex, 1);
      expect(r.state.core.positionMs, 0);
      expect(r.effects, containsAllInOrder(<Effect>[SaveProgress(_server, 0, 60000, true), const TrackEnded('7', 0), const FetchAndLoad('7:1', 1)]));
    });

    test('tick: the backend ended on the last track: Ended, clock stopped', () {
      final r = transition(Playing(_core(_server, index: 2, loaded: '7:2')), Tick(_now, _obs(60000, BackendState.ended)));
      expect(r.state, isA<Ended>());
      expect(r.state.wantsPlay, isFalse);
      expect(r.effects, containsAll(<Effect>[SaveProgress(_server, 2, 60000, true), const TrackEnded('7', 2), const StopTick()]));
    });

    test('tick: the backend failed: Failed with its message, clock stopped; play tries again', () {
      final r = transition(Playing(_core(_files, loaded: '8:0')), Tick(_now, _obs(1, BackendState.failed, AudioErrorKind.generic, 'boom')));
      expect(r.state, isA<Failed>());
      expect((r.state as Failed).message, 'boom');
      expect(r.effects, [const StopTick(), _emit]);
      final again = transition(r.state, const UserToggle(_now));
      expect(again.state, isA<Playing>());
    });

    test('tick: the backend buffers, then plays again', () {
      final b = transition(Playing(_core(_files, loaded: '8:0')), Tick(_now, _obs(1, BackendState.buffering)));
      expect(b.state, isA<Buffering>());
      expect(b.state.wantsPlay, isTrue);
      expect(transition(b.state, Tick(_now, _obs(2))).state, isA<Playing>());
    });

    test('without a backend the clock runs on and the track list advances', () {
      const env = Env();
      final s = Playing(_core(_files, pos: 59800, env: env));
      final r = transition(s, const Tick(_now, Observation(positionMs: 0, state: BackendState.idle)));
      expect(r.state.core.trackIndex, 1);
      expect(r.state.core.positionMs, 0);
      final end = transition(Playing(_core(_files, index: 1, pos: 59800, env: env)), const Tick(_now, Observation(positionMs: 0, state: BackendState.idle)));
      expect(end.state, isA<Ended>());
      expect(end.effects, contains(const StopTick()));
    });

    test('a format this platform cannot play is Unplayable and nothing is loaded', () {
      runsOnApple = () => true;
      final book = BookData(id: '9', title: 'Ogg', kind: 'audiobook', tracks: [_t(1, part: 1, format: 'ogg')]);
      final r = transition(Paused(_core(book)), const UserToggle(_now));
      expect(r.state, isA<Unplayable>());
      expect(r.state.wantsPlay, isFalse);
      expect(r.effects, [const StopTick(), _emit]);
    });

    test('the server could not transcode: Unplayable; another error: Failed', () {
      final l = Loading(_core(_server), wantPlay: true, reason: LoadReason.open);
      expect(transition(l, const LoadFailed(_now, '7:0', 'x', transcodeFailed: true)).state, isA<Unplayable>());
      final f = transition(l, const LoadFailed(_now, '7:0', 'no net'));
      expect(f.state, isA<Failed>());
      // A late answer for another track is ignored.
      expect(transition(l, const LoadFailed(_now, '7:1', 'x')).state, same(l));
      expect(transition(l, const BackendLoaded(_now, '7:1')).state, same(l));
    });

    test('BackendLoaded outside Loading is illegal', () {
      final s = Paused(_core(_server, loaded: '7:0'));
      expect(transition(s, const BackendLoaded(_now, '7:0')).state, same(s));
    });

    test('a failed backend call is Failed and unloads', () {
      final r = transition(Playing(_core(_files, loaded: '8:0')), const BackendFailed(_now, 'disk'));
      expect(r.state, isA<Failed>());
      expect(r.state.core.loadedKey, isNull);
      expect(r.effects, [const StopTick(), _emit]);
    });

    test('the system pauses and starts the engine', () {
      final p = transition(Playing(_core(_files, pos: 100, loaded: '8:0')), ExternalChange(_now, _obs(3000, BackendState.paused)));
      expect(p.state, isA<Paused>());
      expect(p.state.core.positionMs, 3000);
      expect(p.effects.first, const StopTick());
      final s = transition(Paused(_core(_files, loaded: '8:0')), ExternalChange(_now, _obs(3000)));
      expect(s.state, isA<Playing>());
      expect(s.effects, [const StopTick(), const StartTick(), _emit]);
      // Not loaded: not our business.
      final other = Paused(_core(_files));
      expect(transition(other, ExternalChange(_now, _obs(1))).state, same(other));
    });

    test('place and halt', () {
      final i = transition(Idle(_core(null)), SetPlace(_now, book: _server, trackIndex: 1, positionMs: 5));
      expect(i.state, isA<Paused>());
      expect(i.state.core.trackIndex, 1);
      expect(transition(i.state, const SetPlace(_now, clearBook: true)).state, isA<Idle>());
      final h = transition(Playing(_core(_files, loaded: '8:0')), const Halt(_now));
      expect(h.state, isA<Paused>());
      expect(h.state.core.loadedKey, isNull);
      expect(h.effects, [const StopTick(), const StopBackend(), _emit]);
    });

    test('progress goes to the outbox when offline, and the place is written when somebody wants it', () {
      final env = _env.copyWith(offlineActive: true, writesLast: true);
      final r = transition(Playing(_core(_server, pos: 4000, loaded: '7:0', env: env, lastReport: 0)), const UserToggle(_now));
      expect(r.effects.whereType<WriteLast>().single, const WriteLast('7', 0, 4000, _now, false));
      final q = r.effects.whereType<QueueProgress>().single.event;
      expect((q.item, q.part, q.positionMs, q.finished), (7, 11, 4000, false));
    });

    test('dispose reports, stops, disposes the backend and closes; a closed machine ignores everything', () {
      final r = transition(Playing(_core(_server, loaded: '7:0')), const Dispose(_now));
      expect(r.state, isA<Closed>());
      expect(r.effects, containsAll(<Effect>[const StopTick(), const DisposeBackend()]));
      for (final e in <EngineEvent>[const UserToggle(_now), UserOpen(_now, _server), const BackendLoaded(_now, '7:0'), const Flush(_now)]) {
        final c = transition(r.state, e);
        expect(c.state, same(r.state));
        expect(c.effects, isEmpty);
      }
    });
  });

  group('episode', () {
    test('an episode is its own kind, and its place is noted for the list of downloaded episodes', () {
      expect(mediaKindOf(_episode, _episode.tracks.first), MediaKind.episode);
      final r = transition(Playing(_core(_episode, pos: 30000, loaded: 'ep:5:0')), const UserToggle(_now));
      expect(r.state, isA<Paused>());
      expect(r.effects, containsAll(<Effect>[const NoteEpisodeState(5, 30000, false), SaveProgress(_episode, 0, 30000, false)]));
    });

    test('an episode is fetched through the source (Loading), then plays', () {
      final r = transition(Idle(_core(null)), UserOpen(_now, _episode, startMs: 120000));
      expect(r.state, isA<Loading>());
      expect(r.effects, contains(const FetchAndLoad('ep:5:0', 0)));
      expect(transition(r.state, const BackendLoaded(_now, 'ep:5:0')).state, isA<Playing>());
    });

    test('offline the episode place waits in the outbox', () {
      final env = _env.copyWith(offlineActive: true);
      final r = transition(Playing(_core(_episode, pos: 5000, loaded: 'ep:5:0', env: env, lastReport: 0)), const UserToggle(_now));
      final q = r.effects.whereType<QueueProgress>().single.event;
      expect((q.episodeId, q.positionMs), (5, 5000));
      expect(r.effects, contains(const NoteEpisodeState(5, 5000, false)));
    });

    test('the end of an episode is reported as finished and noted', () {
      final r = transition(Playing(_core(_episode, pos: 599000, loaded: 'ep:5:0')), Tick(_now, _obs(600000, BackendState.ended)));
      expect(r.state, isA<Ended>());
      expect(r.effects, containsAll(<Effect>[const NoteEpisodeState(5, 600000, true), SaveProgress(_episode, 0, 600000, true), const TrackEnded('ep:5', 0)]));
    });

    test('an episode can be sought and skipped', () {
      final s = Playing(_core(_episode, loaded: 'ep:5:0'));
      expect(transition(s, const UserSeek(_now, 1000)).effects, [const SeekBackend(1000), _emit]);
      expect(transition(s, const UserSkip(_now, 30000)).state.core.positionMs, 30000);
    });
  });

  group('live', () {
    test('a station is its own kind', () {
      expect(mediaKindOf(_radio, _radio.tracks.first), MediaKind.live);
    });

    test('connect: Loading, then Playing; no progress is ever reported', () {
      final r = transition(Idle(_core(null)), UserOpen(_now, _radio));
      expect(r.state, isA<Loading>());
      expect(r.effects, contains(const FetchAndLoad('r1:0', 0)));
      final p = transition(r.state, const BackendLoaded(_now, 'r1:0'));
      expect(p.state, isA<Playing>());
      final pause = transition(p.state, const UserToggle(_now));
      expect(pause.effects.whereType<SaveProgress>(), isEmpty);
      expect(pause.effects.whereType<QueueProgress>(), isEmpty);
    });

    test('seek, skip, next and previous by time are explicit no-ops', () {
      final s = Playing(_core(_radio, loaded: 'r1:0'));
      for (final e in <EngineEvent>[const UserSeek(_now, 5000), const UserSkip(_now, 30000), const UserNext(_now)]) {
        final r = transition(s, e);
        expect(r.state, same(s), reason: '${e.runtimeType}');
        expect(r.effects, isEmpty);
      }
    });

    test('the title and the details of the stream arrive with the tick and make a snapshot', () {
      const details = StreamDetails(name: 'Jazz', bitrateKbps: 128);
      final r = transition(
        Playing(_core(_radio, loaded: 'r1:0')),
        const Tick(_now, Observation(positionMs: 100, state: BackendState.playing, streamTitle: 'Artist - Song', streamDetails: details)),
      );
      expect(r.state.core.streamTitle, 'Artist - Song');
      expect(r.state.core.streamDetails, details);
      expect(r.effects, [_emit]);
      // The same again: only the position.
      final again = transition(r.state, const Tick(_now, Observation(positionMs: 600, state: BackendState.playing, streamTitle: 'Artist - Song', streamDetails: details)));
      expect(again.effects, [const EmitPosition()]);
    });

    test('a stream error keeps its kind in Failed (connection, server, certificate, unsupported)', () {
      for (final k in AudioErrorKind.values) {
        final r = transition(Playing(_core(_radio, loaded: 'r1:0')), Tick(_now, _obs(0, BackendState.failed, k, 'down')));
        expect((r.state as Failed).kind, k);
      }
    });

    test('a stream that underruns is Buffering and the clock goes on', () {
      final r = transition(Playing(_core(_radio, loaded: 'r1:0')), Tick(_now, _obs(0, BackendState.buffering)));
      expect(r.state, isA<Buffering>());
      expect(r.effects, [_emit]);
    });
  });
}
