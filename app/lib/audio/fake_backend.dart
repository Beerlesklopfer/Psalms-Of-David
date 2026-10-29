// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'audio_backend.dart';

/// A sound engine that makes no sound: position runs by a clock that a test can
/// move. Behaves like the real one in the things the player depends on.
class FakeBackend implements AudioBackend {
  FakeBackend({Duration Function()? clock, this.fileLength = const Duration(minutes: 10)})
    : _clock = clock ?? _systemClock();

  static Duration Function() _systemClock() {
    final watch = Stopwatch()..start();
    return () => watch.elapsed;
  }

  final Duration Function() _clock;

  /// Length given to every file that is loaded.
  final Duration fileLength;

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

  void _check() {
    if (_disposed) throw StateError('the backend was disposed');
  }

  Duration _now() => _state == BackendState.playing ? _base + (_clock() - _since) * _speed : _base;

  void _settle() {
    final p = _now();
    if (_state == BackendState.playing && p >= fileLength) {
      _base = fileLength;
      _state = BackendState.ended;
    } else {
      _base = p;
      _since = _clock();
    }
  }

  @override
  void load(String path, {Duration start = Duration.zero, bool play = true, String title = ''}) {
    _check();
    if (missing.contains(path)) {
      _state = BackendState.failed;
      throw AudioException('cannot open $path');
    }
    _path = path;
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
