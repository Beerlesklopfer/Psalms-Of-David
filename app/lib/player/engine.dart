// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import '../audio/audio_backend.dart';
import '../server/api_client.dart' show ApiException;
import '../server/server_source.dart';
import 'state_machine.dart';
import 'transport.dart';

/// The playback engine: a thin shell around the pure state machine in `state_machine.dart`. It turns commands into events, feeds them
/// to [transition], carries out the effects against the [AudioBackend], the [MediaSource] and one timer, and publishes the result as
/// events (snapshots, position ticks, ...). It knows no widget and no `ChangeNotifier`; it talks to the UI only through
/// [PlayerCommand] in and [PlayerEvent] out (see `PlayerTransport`).
///
/// Service-side objects (the backend, the media source) are given to it directly; in a later isolate stage the service builds them itself.
class PlayerEngine {
  PlayerEngine({this.backend, MediaSource? source, required this.emit}) : _source = source {
    _state = Idle(Core(env: Env(hasBackend: backend != null, hasSource: source != null, gapless: backend?.supportsGapless ?? false)));
    // The system may start or pause the engine (notification, headset, a call, unplugged headphones): the machine follows.
    final b = backend;
    if (b is ExternalControl) {
      (b as ExternalControl).onExternalChange = _followBackend;
      (b as ExternalControl).onAdvanced = _backendAdvanced;
    }
  }

  final AudioBackend? backend;
  final void Function(PlayerEvent event) emit;

  MediaSource? _source;
  late EngineState _state;
  Timer? _timer;
  int _seq = 0;
  bool _disposed = false;

  /// The state of the machine (read only; for tests and the snapshot).
  EngineState get state => _state;

  MediaSource? get source => _source;

  set source(MediaSource? value) {
    _source = value;
    _dispatch(EnvChanged(_now(), hasSource: value != null));
  }

  static int _now() => DateTime.now().millisecondsSinceEpoch;

  /// Handles one command from the UI.
  void handle(PlayerCommand c) {
    final now = _now();
    switch (c) {
      case OpenCommand():
        _dispatch(UserOpen(now, c.book, trackIndex: c.trackIndex, startMs: c.startMs, forceReload: c.forceReload));
      case ToggleCommand():
        _dispatch(UserToggle(now));
      case PlayCommand():
        _dispatch(UserPlay(now));
      case SeekCommand():
        _dispatch(UserSeek(now, c.toMs));
      case SelectTrackCommand():
        _dispatch(UserSelectTrack(now, c.trackIndex, c.positionMs));
      case SkipCommand():
        _dispatch(UserSkip(now, c.deltaMs));
      case NextCommand():
        _dispatch(UserNext(now));
      case PreviousCommand():
        _dispatch(UserPrevious(now));
      case SetSpeedCommand():
        _dispatch(SetSpeed(now, c.speed));
      case SetPlaceCommand():
        _dispatch(SetPlace(now, book: c.book, clearItem: c.clearItem, trackIndex: c.trackIndex, positionMs: c.positionMs));
      case HaltCommand():
        _dispatch(Halt(now));
      case ClearHeardCommand():
        _dispatch(ClearHeard(now));
      case SetEnvCommand():
        _dispatch(EnvChanged(now, offlineActive: c.offlineActive, writesLast: c.writesLast));
      case FlushCommand():
        _dispatch(Flush(now));
      case DisposeCommand():
        _dispatch(Dispose(now));
        _disposed = true;
    }
  }

  void _dispatch(EngineEvent e) {
    final r = transition(_state, e);
    _state = r.state;
    _run(r.effects);
  }

  /// Carries out the effects in order. A call of the backend that throws stops the list and becomes a [BackendFailed] event.
  void _run(List<Effect> effects) {
    for (final fx in effects) {
      try {
        _do(fx);
      } on AudioException catch (e) {
        final unload = fx is! SeekBackend && fx is! SetBackendSpeed;
        _dispatch(BackendFailed(_now(), e.message, kind: e.kind, unload: unload));
        return;
      }
    }
  }

  void _do(Effect fx) {
    final b = backend;
    final c = _state.core;
    switch (fx) {
      case LoadTrack():
        b!.load(fx.path, start: Duration(milliseconds: fx.startMs), play: fx.play, title: fx.title);
      case FetchAndLoad():
        _fetchAndLoad(fx);
      case StartPlayback():
        b!.play();
      case PausePlayback():
        b!.pause();
      case SeekBackend():
        b!.seek(Duration(milliseconds: fx.positionMs));
      case StopBackend():
        b?.stop();
      case DisposeBackend():
        b?.dispose();
      case EnsureSpeed():
        if (b!.speed != fx.speed) b.setSpeed(fx.speed);
      case SetBackendSpeed():
        b!.setSpeed(fx.speed);
      case StartTick():
        _timer = Timer.periodic(const Duration(milliseconds: 500), (_) => _onTick());
      case StopTick():
        _timer?.cancel();
        _timer = null;
      case WriteLast():
        if (fx.playlist) emit(PlaylistPosition(fx.bookId, fx.trackIndex, fx.positionMs));
        emit(LastWritten(fx.bookId, fx.trackIndex, fx.positionMs, fx.atMs));
      case SaveProgress():
        _save(fx);
      case QueueProgress():
        emit(fx.event);
      case NoteEpisodeState():
        emit(PodcastState(fx.episodeId, fx.positionMs, fx.finished));
      case MarkHeard():
        emit(TrackHeard(fx.bookId, fx.trackIndex));
      case TrackEnded():
        emit(TrackFinished(fx.bookId, fx.trackIndex));
      case PrefetchNext():
        _prefetch(fx);
      case EnqueueNext():
        b!.enqueueNext(fx.path);
      case ClearQueuedNext():
        b?.clearNext();
      case EmitSnapshot():
        emit(_snapshot(c));
      case EmitPosition():
        emit(PositionTick(++_seq, c.trackIndex, c.positionMs));
    }
  }

  PlayerSnapshot _snapshot(Core c) {
    final s = _state;
    return PlayerSnapshot(
      seq: ++_seq,
      bookId: c.book?.id,
      trackIndex: c.trackIndex,
      positionMs: c.positionMs,
      playing: s.wantsPlay,
      speed: c.speed,
      loading: s is Loading,
      error: s is Failed ? s.message : null,
      errorKind: s is Failed ? s.kind : null,
      unplayableIndex: s is Unplayable ? s.index : null,
      streamTitle: c.streamTitle,
      streamDetails: c.streamDetails,
    );
  }

  Observation? _observe() {
    final b = backend;
    if (b == null) return null;
    final info = b is StreamInfo ? b as StreamInfo : null;
    return Observation(
      positionMs: b.position.inMilliseconds,
      state: b.state,
      streamTitle: info?.streamTitle.value,
      streamDetails: info?.streamDetails.value,
      errorMessage: info?.errorMessage,
      errorKind: info?.errorKind ?? AudioErrorKind.generic,
    );
  }

  void _onTick() {
    final o = _observe() ?? const Observation(positionMs: 0, state: BackendState.idle);
    _dispatch(Tick(_now(), o));
  }

  void _followBackend() {
    final o = _observe();
    if (o == null || _disposed) return;
    _dispatch(ExternalChange(_now(), o));
  }

  /// The backend switched to the queued next track by itself (gapless).
  void _backendAdvanced() {
    if (_disposed) return;
    _dispatch(BackendAdvanced(_now(), _state.core.trackIndex + 1));
  }

  /// Fetches the file of the current track, then loads it, if the player is still on that track by then.
  Future<void> _fetchAndLoad(FetchAndLoad fx) async {
    final book = _state.core.book!.toItem();
    final t = book.tracks[fx.trackIndex];
    bool stale() => _disposed || _state.core.key != fx.key;
    try {
      final path = await _source!.fetchPart(t);
      if (stale()) return;
      final c = _state.core;
      backend!.load(path, start: Duration(milliseconds: c.positionMs), play: _state.wantsPlay, title: t.title);
      _dispatch(BackendLoaded(_now(), fx.key));
    } on Object catch (e) {
      if (stale()) return;
      // The server could not make an MP3 of the file: the person is told that the format cannot be played here, as without transcoding.
      final transcode = e is ApiException && e.code == 'transcode_failed';
      _dispatch(LoadFailed(
        _now(),
        fx.key,
        e is AudioException ? e.message : '$e',
        kind: e is AudioException ? e.kind : AudioErrorKind.generic,
        transcodeFailed: transcode,
      ));
    }
  }

  void _save(SaveProgress fx) {
    final data = fx.book;
    final book = data.toItem();
    final t = book.tracks[fx.trackIndex];
    _source!.saveProgress(book, t, Duration(milliseconds: fx.positionMs), finished: fx.finished).catchError((Object e) {
      if (e is ApiException && e.code == 'network') {
        final q = progressQueue(data, data.tracks[fx.trackIndex], fx.positionMs, fx.finished);
        if (q != null) emit(q);
      }
    });
  }

  void _prefetch(PrefetchNext fx) {
    final data = _state.core.book;
    if (data == null) return;
    final key = '${data.id}:${fx.trackIndex}';
    _source!.fetchPart(data.toItem().tracks[fx.trackIndex]).then<void>((path) {
      if (!_disposed && _state.core.env.gapless) _dispatch(PrefetchReady(_now(), key, path));
    }, onError: (Object _) => _dispatch(PrefetchFailed(_now(), key)));
  }
}
