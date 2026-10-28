// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:just_audio/just_audio.dart';

import 'audio_backend.dart';

/// The sound engine on Android and iOS: ExoPlayer and AVPlayer through `just_audio`. They
/// change the speed without changing the pitch.
///
/// [AudioBackend] is synchronous and `just_audio` is not. The commands go to the player one
/// after the other through a chain of futures, so their order is kept, and what the getters
/// return is what the player last said. A file that cannot be opened is reported by the state
/// `failed` a moment later, not by an exception from [load].
class JustAudioBackend implements AudioBackend {
  JustAudioBackend() : _player = AudioPlayer() {
    _subs.add(_player.playerStateStream.listen((s) {
      if (_loaded && s.processingState == ProcessingState.completed) _ended = true;
    }));
    _subs.add(_player.durationStream.listen((d) => _duration = d));
  }

  final AudioPlayer _player;
  final _subs = <StreamSubscription<Object?>>[];
  Future<void> _chain = Future.value();
  bool _loaded = false;
  bool _wantPlaying = false;
  bool _ended = false;
  bool _failed = false;
  bool _disposed = false;
  Duration? _duration;
  double _volume = 1.0;
  double _speed = 1.0;

  /// Runs [step] after everything asked before it. A failure marks the engine as failed.
  void _then(Future<void> Function() step) {
    _chain = _chain.then((_) async {
      if (_disposed) return;
      try {
        await step();
      } on Object {
        _failed = true;
        _wantPlaying = false;
      }
    });
  }

  @override
  void load(String path, {Duration start = Duration.zero, bool play = true, String title = ''}) {
    _loaded = true;
    _ended = false;
    _failed = false;
    _wantPlaying = play;
    _duration = null;
    _then(() async {
      await _player.setFilePath(path, initialPosition: start);
      await _player.setSpeed(_speed);
      await _player.setVolume(_volume);
      if (_wantPlaying) unawaited(_player.play());
    });
  }

  @override
  void play() {
    _wantPlaying = true;
    _then(() async {
      if (_ended) {
        await _player.seek(Duration.zero);
        _ended = false;
      }
      unawaited(_player.play());
    });
  }

  @override
  void pause() {
    _wantPlaying = false;
    _then(() => _player.pause());
  }

  @override
  void stop() {
    _loaded = false;
    _wantPlaying = false;
    _ended = false;
    _then(() => _player.stop());
  }

  @override
  void seek(Duration to) {
    _ended = false;
    _then(() => _player.seek(to));
  }

  @override
  void setVolume(double volume) {
    _volume = volume.clamp(0.0, 1.0);
    _then(() => _player.setVolume(_volume));
  }

  @override
  void setSpeed(double speed) {
    _speed = speed.clamp(0.5, 3.0);
    _then(() => _player.setSpeed(_speed));
  }

  @override
  BackendState get state {
    if (_failed) return BackendState.failed;
    if (!_loaded) return BackendState.idle;
    if (_ended) return BackendState.ended;
    return _wantPlaying ? BackendState.playing : BackendState.paused;
  }

  @override
  Duration get position => _player.position;

  @override
  Duration? get duration => _duration;

  @override
  double get volume => _volume;

  @override
  double get speed => _speed;

  @override
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    for (final s in _subs) {
      s.cancel();
    }
    _player.dispose();
  }
}
