// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:audio_service/audio_service.dart';
import 'package:flutter/foundation.dart';

import '../models.dart';
import '../player_controller.dart';
import '../server/app_paths.dart';

/// What the system sees of the player: the notification, the lock screen, the buttons of headphones and cars. It is the
/// `AudioHandler` of `audio_service` (a foreground service on Android, the remote command centre on iOS) and does no playing itself:
/// the commands go to the [PlayerController], and what the controller does is published here. The controller stays the one owner of the state.
///
/// There is one handler for the whole run of the app; the controller is [attach]ed when the app has made it.
class PsalmAudioHandler extends BaseAudioHandler with SeekHandler {
  PsalmAudioHandler();

  /// The handler of this run, or null where the platform has none (Linux uses PipeWire and the desktop shell, the tests make their own).
  static PsalmAudioHandler? instance;

  /// Starts the service on Android, iOS and macOS (Now Playing and the media keys). Call once, before the app runs; elsewhere it does nothing.
  static Future<void> init() async {
    if (instance != null || !(Platform.isAndroid || Platform.isIOS || Platform.isMacOS)) return;
    instance = await AudioService.init(
      builder: PsalmAudioHandler.new,
      config: const AudioServiceConfig(
        androidNotificationChannelId: 'com.the-abraham-box.psalm.channel.audio',
        // The name of the channel is shown in the settings of the system; the app name is the same in every language.
        androidNotificationChannelName: 'Psalm',
        // A paused notification can be swiped away, and the service stops with it.
        androidNotificationOngoing: false,
        androidStopForegroundOnPause: true,
        fastForwardInterval: Duration(seconds: 30),
        rewindInterval: Duration(seconds: 10),
      ),
    );
  }

  PlayerController? _player;

  /// What was published last, so that the ticks of the controller (twice a second) do not become messages to the system: only a
  /// change of the state or a jump of the position is one.
  _Shown? _shown;

  /// Follows [player]; the previous controller, if any, is let go.
  void attach(PlayerController player) {
    _player?.removeListener(_publish);
    _player = player;
    player.addListener(_publish);
    _publish();
  }

  /// The controller is gone (the app closes).
  void detach(PlayerController player) {
    if (_player != player) return;
    player.removeListener(_publish);
    _player = null;
    _shown = null;
    playbackState.add(PlaybackState());
    mediaItem.add(null);
  }

  void _publish() {
    final p = _player;
    if (p == null) return;
    final book = p.shownBook, track = p.track;
    if (book == null || track == null) {
      if (_shown != null) {
        _shown = null;
        playbackState.add(PlaybackState());
        mediaItem.add(null);
      }
      return;
    }
    final live = track.live || (p.book?.isStream ?? false);
    final shown = _Shown(
      key: '${book.id}:${p.trackIndex}:${track.partId}',
      playing: p.playing,
      loading: p.loading,
      failed: p.error != null,
      speed: p.speed,
      position: p.position,
      at: DateTime.now(),
    );
    final last = _shown;
    if (last != null && !shown.differsFrom(last)) return;
    if (last == null || last.key != shown.key) mediaItem.add(_itemOf(book, p));
    _shown = shown;
    final last2 = (p.book?.tracks.length ?? 1) - 1;
    playbackState.add(
      PlaybackState(
        controls: [
          if (!live) MediaControl.skipToPrevious,
          if (!live) MediaControl.rewind,
          if (p.playing) MediaControl.pause else MediaControl.play,
          if (!live) MediaControl.fastForward,
          if (!live) MediaControl.skipToNext,
        ],
        systemActions: {
          MediaAction.seek,
          MediaAction.seekForward,
          MediaAction.seekBackward,
          MediaAction.skipToNext,
          MediaAction.skipToPrevious,
        },
        androidCompactActionIndices: live ? const [0] : const [0, 2, 4],
        processingState: p.error != null
            ? AudioProcessingState.error
            : p.loading
                ? AudioProcessingState.buffering
                : AudioProcessingState.ready,
        playing: p.playing,
        updatePosition: p.position,
        speed: p.speed,
        queueIndex: p.trackIndex.clamp(0, last2 < 0 ? 0 : last2),
      ),
    );
  }

  /// The "now playing" of one track: the title of the track, the book as album, its author as artist and the picture.
  MediaItem _itemOf(Book book, PlayerController p) {
    final track = p.track!;
    final stream = track.live || book.isStream;
    return MediaItem(
      id: '${p.book?.id ?? book.id}:${p.trackIndex}',
      title: track.title,
      album: stream ? null : book.title,
      artist: stream ? book.title : book.author,
      duration: stream ? null : track.length,
      artUri: _coverFile(book),
    );
  }

  String? _coverKey;
  Uri? _coverUri;

  /// The notification reads the picture from a file: one per book, the older one is removed.
  Uri? _coverFile(Book book) {
    final bytes = book.cover;
    if (bytes == null || bytes.isEmpty) return null;
    if (_coverKey == book.id && _coverUri != null) return _coverUri;
    try {
      final dir = Directory('${AppPaths.base.path}/now_playing')..createSync(recursive: true);
      for (final f in dir.listSync()) {
        if (f is File) f.deleteSync();
      }
      final file = File('${dir.path}/cover-${book.id.replaceAll(RegExp(r'[^A-Za-z0-9_-]'), '_')}')..writeAsBytesSync(bytes);
      _coverKey = book.id;
      return _coverUri = file.uri;
    } on Object {
      return null;
    }
  }

  // The commands of the system: all of them go through the controller.

  @override
  Future<void> play() async {
    final p = _player;
    if (p != null && !p.playing) p.toggle();
  }

  @override
  Future<void> pause() async {
    final p = _player;
    if (p != null && p.playing) p.toggle();
  }

  @override
  Future<void> stop() async {
    await pause();
    _player?.flush();
  }

  @override
  Future<void> seek(Duration position) async => _player?.seek(position);

  @override
  Future<void> skipToNext() async => _player?.next();

  @override
  Future<void> skipToPrevious() async => _player?.previous();

  @override
  Future<void> fastForward() async => _player?.skip(const Duration(seconds: 30));

  @override
  Future<void> rewind() async => _player?.skip(const Duration(seconds: -10));

  // ---- Extension point for the car (CarPlay, Android Auto): see docs/spec-car.md. -------------------------------------------------
  //
  // A car asks for a tree of media ("Continue listening", "Library", "Playlists") and starts an item by its id. The three methods
  // below are wired to the system already (audio_service answers the MediaBrowserService of Android and the content tree of CarPlay
  // with them) and answer nothing yet: the tree is the next step. The controller (this handler's [_player]) knows the library, the
  // playlists and what was heard last, so the answers come from it, never from a second copy of the data.

  @override
  Future<List<MediaItem>> getChildren(String parentMediaId, [Map<String, dynamic>? options]) async => const [];

  @override
  Future<MediaItem?> getMediaItem(String mediaId) async => null;

  @override
  Future<void> playFromMediaId(String mediaId, [Map<String, dynamic>? extras]) async {}
}

@immutable
class _Shown {
  const _Shown({required this.key, required this.playing, required this.loading, required this.failed, required this.speed, required this.position, required this.at});

  final String key;
  final bool playing;
  final bool loading;
  final bool failed;
  final double speed;
  final Duration position;
  final DateTime at;

  /// Another track or state, or a position that is not where the last one would have moved to (a seek, a skip).
  bool differsFrom(_Shown last) {
    if (key != last.key || playing != last.playing || loading != last.loading || failed != last.failed || speed != last.speed) return true;
    final expected = last.playing ? last.position + at.difference(last.at) * last.speed : last.position;
    return (position - expected).abs() > const Duration(seconds: 2);
  }
}
