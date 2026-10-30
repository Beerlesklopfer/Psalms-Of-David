// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'dart:io';

import 'package:flutter/foundation.dart' show ValueListenable, ValueNotifier;
import 'package:flutter/services.dart';
import 'package:just_audio/just_audio.dart';

import 'audio_backend.dart';

/// The sound engine on Android and iOS: ExoPlayer and AVPlayer through `just_audio`. They
/// change the speed without changing the pitch.
///
/// [AudioBackend] is synchronous and `just_audio` is not. The commands go to the player one
/// after the other through a chain of futures, so their order is kept, and what the getters
/// return is what the player last said. A file that cannot be opened is reported by the state
/// `failed` a moment later, not by an exception from [load].
class JustAudioBackend implements AudioBackend, ExternalControl, StreamInfo {
  static const _notifications = MethodChannel('com.the-abraham-box.psalm/notifications');

  /// Asks once, at the first playback, for the permission to show the notification with the controls (Android 13 and later). Playback
  /// goes on without it, only the notification of the service stays hidden.
  static bool _asked = false;
  static void _askForNotifications() {
    if (_asked || !Platform.isAndroid) return;
    _asked = true;
    _notifications.invokeMethod<void>('request').catchError((Object _) {});
  }

  JustAudioBackend() : _player = AudioPlayer(useLazyPreparation: false) {
    _subs.add(_player.currentIndexStream.listen(_onIndex));
    _subs.add(
      _player.playerStateStream.listen((s) {
        if (_loaded && s.processingState == ProcessingState.completed) _ended = true;
        _followOutside(s);
      }),
    );
    _subs.add(_player.durationStream.listen((d) => _duration = d));
    // What a radio station announces ("Artist - Song") and says about itself: the ICY data of the stream (ExoPlayer and AVPlayer read them).
    _subs.add(
      _player.icyMetadataStream.listen((m) {
        final t = m?.info?.title?.trim();
        _title.value = t == null || t.isEmpty ? null : t;
        final h = m?.headers;
        if (h != null) _details.value = StreamDetails(name: h.name, genre: h.genre, website: h.url, bitrateKbps: h.bitrate);
      }),
    );
    _subs.add(_player.positionDiscontinuityStream.listen((d) {
      // A jump that the app did not ask for (the seek bar of the lock screen, a headset button).
      if (d.reason == PositionDiscontinuityReason.seek && _pending == 0 && _loaded) _onExternalChange?.call();
    }));
  }

  void Function()? _onExternalChange;
  void Function()? _onAdvanced;

  @override
  set onAdvanced(void Function()? callback) => _onAdvanced = callback;

  // ---- Gapless: the playlist of the player is [current] or [current, next] ----

  /// Counts the loads and stops; a queued step of an older generation must not touch the playlist of a newer one.
  int _gen = 0;

  /// The next track is in the playlist at index 1.
  bool _nextQueued = false;
  int _lastIndex = 0;
  bool _isStream = false;

  @override
  bool get supportsGapless => true;

  @override
  void enqueueNext(String path) {
    if (!_loaded || _isStream || _failed || path.startsWith('http://') || path.startsWith('https://')) return;
    final gen = _gen;
    _then(() async {
      if (gen != _gen || _disposed || _ended) return;
      try {
        await _removeNext();
        await _player.addAudioSource(AudioSource.file(path));
        if (gen == _gen) _nextQueued = true;
      } on Object {
        // The next track could not be queued: playback goes on as without gapless, the controller loads it at the end.
        _nextQueued = false;
      }
    });
  }

  @override
  void clearNext() {
    if (!_nextQueued) return;
    _nextQueued = false;
    final gen = _gen;
    _then(() async {
      if (gen != _gen || _disposed) return;
      try {
        await _removeNext();
      } on Object {
        // Nothing to drop.
      }
    });
  }

  /// Removes everything behind the playing item (only while the playing item is the first one).
  Future<void> _removeNext() async {
    final n = _player.audioSources.length;
    if (n > 1 && (_player.currentIndex ?? 0) == 0) await _player.removeAudioSourceRange(1, n);
  }

  /// The player moved to another item. Going from the first to the queued second one is the gapless switch: update the bookkeeping,
  /// drop the finished item so that the current one is index 0 again, and tell the controller.
  void _onIndex(int? i) {
    if (i == null || _disposed || !_loaded) return;
    final prev = _lastIndex;
    _lastIndex = i;
    if (i != 1 || prev != 0 || !_nextQueued) return;
    _nextQueued = false;
    _ended = false;
    _duration = _player.duration;
    final gen = _gen;
    _then(() async {
      if (gen != _gen || _disposed) return;
      try {
        if (_player.audioSources.length > 1 && _player.currentIndex == 1) await _player.removeAudioSourceAt(0);
      } on Object {
        // Harmless: the finished item stays in front, the next switch is not detected until the next load.
      }
    });
    _onAdvanced?.call();
  }

  final ValueNotifier<String?> _title = ValueNotifier<String?>(null);
  final ValueNotifier<StreamDetails?> _details = ValueNotifier<StreamDetails?>(null);

  @override
  ValueListenable<String?> get streamTitle => _title;

  @override
  ValueListenable<StreamDetails?> get streamDetails => _details;

  @override
  String? get errorMessage => _failed ? 'playback failed' : null;

  @override
  AudioErrorKind get errorKind => AudioErrorKind.generic;

  @override
  set onExternalChange(void Function()? callback) => _onExternalChange = callback;

  /// Commands of the app that are on their way to the player; while there are some, a change of the player is the app's own doing.
  int _pending = 0;

  /// The system started or paused the player (notification, headset, phone call, unplugged headphones): the engine says so and the
  /// player of the app follows through [onExternalChange].
  void _followOutside(PlayerState s) {
    if (!_loaded || _disposed || _pending > 0 || _ended || _failed) return;
    final playingNow = s.playing && s.processingState != ProcessingState.idle;
    if (playingNow == _wantPlaying) return;
    // Loading and buffering are not a pause: only a settled player counts.
    if (s.processingState == ProcessingState.loading) return;
    _wantPlaying = playingNow;
    _onExternalChange?.call();
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
    _pending++;
    _chain = _chain.then((_) async {
      try {
        if (_disposed) return;
        try {
          await step();
        } on Object {
          _failed = true;
          _wantPlaying = false;
        }
      } finally {
        // The player reports its own change a moment after the command; the app's own doing must not count as an outside one.
        Future<void>.delayed(const Duration(milliseconds: 400), () => _pending = _pending > 0 ? _pending - 1 : 0);
      }
    });
  }

  @override
  void load(String path, {Duration start = Duration.zero, bool play = true, String title = ''}) {
    if (play) _askForNotifications();
    _gen++;
    _nextQueued = false;
    _lastIndex = 0;
    _isStream = path.startsWith('http://') || path.startsWith('https://');
    _loaded = true;
    _ended = false;
    _failed = false;
    _wantPlaying = play;
    _duration = null;
    _title.value = null;
    _details.value = null;
    _then(() async {
      // A radio stream comes as an address, everything else as a file.
      final stream = path.startsWith('http://') || path.startsWith('https://');
      if (stream) {
        await _player.setUrl(path, initialPosition: start);
      } else {
        await _player.setFilePath(path, initialPosition: start);
      }
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
    _gen++;
    _nextQueued = false;
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

  // The player of the platform has a buffer of its own that cannot be set.
  @override
  void setBufferSeconds(int seconds) {}

  @override
  int get bufferSeconds => kBufferDefaultSeconds;

  @override
  bool get bufferAdjustable => false;

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
    _title.dispose();
    _details.dispose();
  }
}
