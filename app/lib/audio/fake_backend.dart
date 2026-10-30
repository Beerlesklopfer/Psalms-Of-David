// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'audio_backend.dart';

/// A sound engine that makes no sound: position runs by a clock that a test can
/// move. Behaves like the real one in the things the player depends on.
class FakeBackend implements AudioBackend, ExternalControl {
  FakeBackend({Duration Function()? clock, this.fileLength = const Duration(minutes: 10), this.gapless = false})
    : _clock = clock ?? _systemClock();

  static Duration Function() _systemClock() {
    final watch = Stopwatch()..start();
    return () => watch.elapsed;
  }

  final Duration Function() _clock;

  /// Length given to every file that is loaded.
  final Duration fileLength;

  /// Whether this fake plays the queued next track without a gap (the contract of [AudioBackend.enqueueNext]).
  final bool gapless;

  /// Paths that fail to load, to try the error path.
  final Set<String> missing = {};

  BackendState _state = BackendState.idle;
  Duration _base = Duration.zero;
  Duration _since = Duration.zero;
  double _volume = 1.0;
  double _speed = 1.0;
  String? _path;
  bool _disposed = false;

  String? get loadedPath => _path;

  String? _next;

  /// The track queued with [enqueueNext], or null.
  String? get queuedPath => _next;

  /// How often the fake switched to the queued track by itself, and how often [state] was `ended` (for tests: a gapless switch never ends).
  int advances = 0;
  int endedSeen = 0;

  void Function()? _onAdvanced;

  @override
  set onAdvanced(void Function()? callback) => _onAdvanced = callback;

  @override
  bool get supportsGapless => gapless;

  @override
  void enqueueNext(String path) {
    _check();
    if (!gapless || _path == null) return;
    _next = path;
  }

  @override
  void clearNext() {
    _check();
    _next = null;
  }

  void Function()? _onExternalChange;

  @override
  set onExternalChange(void Function()? callback) => _onExternalChange = callback;

  /// The system pauses the engine (a call, unplugged headphones, the notification) and says so.
  void pauseFromOutside() {
    pause();
    _onExternalChange?.call();
  }

  /// The system starts the engine (the notification, a headset button) and says so.
  void playFromOutside() {
    play();
    _onExternalChange?.call();
  }

  void _check() {
    if (_disposed) throw StateError('the backend was disposed');
  }

  Duration _now() => _state == BackendState.playing ? _base + (_clock() - _since) * _speed : _base;

  void _settle() {
    var p = _now();
    var advanced = false;
    // Gapless: the queued track takes over at the exact end, the overshoot carries into it, and the state never becomes `ended`.
    while (_state == BackendState.playing && p >= fileLength && _next != null) {
      p -= fileLength;
      _path = _next;
      _next = null;
      advanced = true;
      advances++;
    }
    if (_state == BackendState.playing && p >= fileLength) {
      _base = fileLength;
      _state = BackendState.ended;
      endedSeen++;
    } else {
      _base = p;
      _since = _clock();
    }
    if (advanced) _onAdvanced?.call();
  }

  @override
  void load(String path, {Duration start = Duration.zero, bool play = true, String title = ''}) {
    _check();
    if (missing.contains(path)) {
      _state = BackendState.failed;
      throw AudioException('cannot open $path');
    }
    _path = path;
    _next = null;
    _base = start > fileLength ? fileLength : start;
    _since = _clock();
    _state = play ? BackendState.playing : BackendState.paused;
  }

  @override
  void play() {
    _check();
    _settle();
    if (_state == BackendState.paused) {
      _since = _clock();
      _state = BackendState.playing;
    }
  }

  @override
  void pause() {
    _check();
    _settle();
    if (_state == BackendState.playing) _state = BackendState.paused;
  }

  @override
  void stop() {
    _check();
    _state = BackendState.idle;
    _base = Duration.zero;
    _path = null;
    _next = null;
  }

  @override
  void seek(Duration to) {
    _check();
    if (_path == null) return;
    final wasPaused = _state == BackendState.paused;
    _base = to > fileLength ? fileLength : to;
    _since = _clock();
    _state = wasPaused ? BackendState.paused : BackendState.playing;
  }

  @override
  void setVolume(double volume) {
    _check();
    _volume = volume.clamp(0.0, 1.0);
  }

  @override
  void setSpeed(double speed) {
    _check();
    _settle();
    _speed = speed.clamp(0.5, 3.0);
  }

  @override
  BackendState get state {
    _check();
    _settle();
    return _state;
  }

  @override
  Duration get position {
    _check();
    _settle();
    return _base;
  }

  @override
  Duration? get duration => _path == null ? null : fileLength;

  @override
  double get volume => _volume;

  int _bufferSeconds = kBufferDefaultSeconds;

  @override
  void setBufferSeconds(int seconds) {
    _check();
    _bufferSeconds = seconds.clamp(kBufferMinSeconds, kBufferMaxSeconds);
  }

  @override
  int get bufferSeconds => _bufferSeconds;

  @override
  bool get bufferAdjustable => true;

  @override
  double get speed => _speed;

  @override
  void dispose() => _disposed = true;
}
