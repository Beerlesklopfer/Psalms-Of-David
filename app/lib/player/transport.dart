// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:ui' show Color;

import '../audio/audio_backend.dart' show AudioErrorKind, StreamDetails;
import '../models.dart';
import 'engine.dart';

// The wire between the UI facade (`PlayerController`) and the playback state machine (`PlayerEngine`).
//
// Every message is plain data (ints, strings, bools, lists, maps) with `toMap`/`fromMap`, so that a later `IsolateTransport` can send
// it. The only exception is [LibraryItemData.live]: the in-process transport hands the engine the live `LibraryItem` (so the identity of its tracks
// is kept for the source and the tests); it is never serialized, and the engine rebuilds the book from the data if it is null.

/// The part of a [Track] that the engine reads.
class TrackData {
  const TrackData({
    required this.disc,
    required this.number,
    required this.title,
    required this.lengthMs,
    this.path,
    this.partId,
    this.url,
    this.live = false,
    this.itemId,
    this.format,
    this.mime,
    this.transcode,
  });

  factory TrackData.of(Track t) => TrackData(
        disc: t.disc,
        number: t.number,
        title: t.title,
        lengthMs: t.length.inMilliseconds,
        path: t.path,
        partId: t.partId,
        url: t.url,
        live: t.live,
        itemId: t.itemId,
        format: t.format,
        mime: t.mime,
        transcode: t.transcode,
      );

  factory TrackData.fromMap(Map<String, dynamic> m) => TrackData(
        disc: m['disc'] as int,
        number: m['number'] as int,
        title: m['title'] as String,
        lengthMs: m['length_ms'] as int,
        path: m['path'] as String?,
        partId: m['part_id'] as int?,
        url: m['url'] as String?,
        live: m['live'] as bool? ?? false,
        itemId: m['item_id'] as String?,
        format: m['format'] as String?,
        mime: m['mime'] as String?,
        transcode: m['transcode'] as String?,
      );

  final int disc;
  final int number;
  final String title;
  final int lengthMs;
  final String? path;
  final int? partId;
  final String? url;
  final bool live;
  final String? itemId;
  final String? format;
  final String? mime;
  final String? transcode;

  Track toTrack() => Track(
        disc: disc,
        number: number,
        title: title,
        length: Duration(milliseconds: lengthMs),
        path: path,
        partId: partId,
        url: url,
        live: live,
        itemId: itemId,
        format: format,
        mime: mime,
        transcode: transcode,
      );

  Map<String, dynamic> toMap() => {
        'disc': disc,
        'number': number,
        'title': title,
        'length_ms': lengthMs,
        'path': path,
        'part_id': partId,
        'url': url,
        'live': live,
        'item_id': itemId,
        'format': format,
        'mime': mime,
        'transcode': transcode,
      };
}

/// The part of a [LibraryItem] that the engine reads: its id, kind, title and the tracks.
class LibraryItemData {
  LibraryItemData({required this.id, required this.title, required this.kind, required this.tracks, this.live});

  factory LibraryItemData.of(LibraryItem b) => LibraryItemData(id: b.id, title: b.title, kind: b.kind, tracks: [for (final t in b.tracks) TrackData.of(t)], live: b);

  factory LibraryItemData.fromMap(Map<String, dynamic> m) => LibraryItemData(
        id: m['id'] as String,
        title: m['title'] as String,
        kind: m['kind'] as String,
        tracks: [for (final t in (m['tracks'] as List)) TrackData.fromMap((t as Map).cast<String, dynamic>())],
      );

  final String id;
  final String title;
  final String kind;
  final List<TrackData> tracks;

  /// In-process only, never serialized: the live book of the UI.
  final LibraryItem? live;

  LibraryItem? _built;

  /// The live book if there is one, else a book rebuilt from the data (the isolate case; built once, so its tracks keep their identity).
  LibraryItem toItem() =>
      live ??
      (_built ??= LibraryItem(id: id, title: title, author: '', color: const Color(0xFF607D8B), kind: kind, tracks: [for (final t in tracks) t.toTrack()]));

  Map<String, dynamic> toMap() => {'id': id, 'title': title, 'kind': kind, 'tracks': [for (final t in tracks) t.toMap()]};
}

// ---- Commands: UI -> engine -----------------------------------------------------------------------

sealed class PlayerCommand {
  const PlayerCommand();

  Map<String, dynamic> toMap();

  static PlayerCommand fromMap(Map<String, dynamic> m) {
    switch (m['type'] as String) {
      case 'open':
        return OpenCommand(
          LibraryItemData.fromMap((m['book'] as Map).cast<String, dynamic>()),
          trackIndex: m['track'] as int?,
          startMs: m['start_ms'] as int?,
          forceReload: m['force_reload'] as bool? ?? false,
        );
      case 'toggle':
        return const ToggleCommand();
      case 'play':
        return const PlayCommand();
      case 'seek':
        return SeekCommand(m['to_ms'] as int);
      case 'select_track':
        return SelectTrackCommand(m['track'] as int, m['position_ms'] as int);
      case 'skip':
        return SkipCommand(m['delta_ms'] as int);
      case 'next':
        return const NextCommand();
      case 'previous':
        return const PreviousCommand();
      case 'set_speed':
        return SetSpeedCommand((m['speed'] as num).toDouble());
      case 'set_place':
        return SetPlaceCommand(
          book: m['book'] == null ? null : LibraryItemData.fromMap((m['book'] as Map).cast<String, dynamic>()),
          clearItem: m['clear_book'] as bool? ?? false,
          trackIndex: m['track'] as int?,
          positionMs: m['position_ms'] as int?,
        );
      case 'halt':
        return const HaltCommand();
      case 'clear_heard':
        return const ClearHeardCommand();
      case 'set_env':
        return SetEnvCommand(offlineActive: m['offline_active'] as bool, writesLast: m['writes_last'] as bool);
      case 'flush':
        return const FlushCommand();
      case 'dispose':
        return const DisposeCommand();
    }
    throw FormatException('unknown command ${m['type']}');
  }
}

/// Opens a book and plays it; [startMs] is where in the track it begins. The UI has resolved the resume place already.
class OpenCommand extends PlayerCommand {
  const OpenCommand(this.book, {this.trackIndex, this.startMs, this.forceReload = false});
  final LibraryItemData book;
  final int? trackIndex;
  final int? startMs;

  /// Load the track again even if it is the one that is loaded (a passage of the Bible).
  final bool forceReload;
  @override
  Map<String, dynamic> toMap() =>
      {'type': 'open', 'book': book.toMap(), 'track': trackIndex, 'start_ms': startMs, 'force_reload': forceReload};
}

class ToggleCommand extends PlayerCommand {
  const ToggleCommand();
  @override
  Map<String, dynamic> toMap() => {'type': 'toggle'};
}

/// Plays what is open (the voice "go on").
class PlayCommand extends PlayerCommand {
  const PlayCommand();
  @override
  Map<String, dynamic> toMap() => {'type': 'play'};
}

class SeekCommand extends PlayerCommand {
  const SeekCommand(this.toMs);
  final int toMs;
  @override
  Map<String, dynamic> toMap() => {'type': 'seek', 'to_ms': toMs};
}

/// Goes to another track of the open book at a place (a seek in the whole book).
class SelectTrackCommand extends PlayerCommand {
  const SelectTrackCommand(this.trackIndex, this.positionMs);
  final int trackIndex;
  final int positionMs;
  @override
  Map<String, dynamic> toMap() => {'type': 'select_track', 'track': trackIndex, 'position_ms': positionMs};
}

class SkipCommand extends PlayerCommand {
  const SkipCommand(this.deltaMs);
  final int deltaMs;
  @override
  Map<String, dynamic> toMap() => {'type': 'skip', 'delta_ms': deltaMs};
}

class NextCommand extends PlayerCommand {
  const NextCommand();
  @override
  Map<String, dynamic> toMap() => {'type': 'next'};
}

class PreviousCommand extends PlayerCommand {
  const PreviousCommand();
  @override
  Map<String, dynamic> toMap() => {'type': 'previous'};
}

class SetSpeedCommand extends PlayerCommand {
  const SetSpeedCommand(this.speed);
  final double speed;
  @override
  Map<String, dynamic> toMap() => {'type': 'set_speed', 'speed': speed};
}

/// Moves the place without touching the sound: the library changed, another device went on, the UI restores a place.
/// Only the fields that are set change.
class SetPlaceCommand extends PlayerCommand {
  const SetPlaceCommand({this.book, this.clearItem = false, this.trackIndex, this.positionMs});
  final LibraryItemData? book;
  final bool clearItem;
  final int? trackIndex;
  final int? positionMs;
  @override
  Map<String, dynamic> toMap() =>
      {'type': 'set_place', 'book': book?.toMap(), 'clear_book': clearItem, 'track': trackIndex, 'position_ms': positionMs};
}

/// Stops the sound and forgets the loaded track (the server or the library changed under it).
class HaltCommand extends PlayerCommand {
  const HaltCommand();
  @override
  Map<String, dynamic> toMap() => {'type': 'halt'};
}

class ClearHeardCommand extends PlayerCommand {
  const ClearHeardCommand();
  @override
  Map<String, dynamic> toMap() => {'type': 'clear_heard'};
}

/// What the engine needs to know of the UI side: whether the progress waits for the server (offline) and whether anybody
/// wants the place written down (a last-played store or the playlist hook).
class SetEnvCommand extends PlayerCommand {
  const SetEnvCommand({required this.offlineActive, required this.writesLast});
  final bool offlineActive;
  final bool writesLast;
  @override
  Map<String, dynamic> toMap() => {'type': 'set_env', 'offline_active': offlineActive, 'writes_last': writesLast};
}

/// Reports and writes the place now (the app goes to the background).
class FlushCommand extends PlayerCommand {
  const FlushCommand();
  @override
  Map<String, dynamic> toMap() => {'type': 'flush'};
}

class DisposeCommand extends PlayerCommand {
  const DisposeCommand();
  @override
  Map<String, dynamic> toMap() => {'type': 'dispose'};
}

// ---- Events: engine -> UI -------------------------------------------------------------------------

sealed class PlayerEvent {
  const PlayerEvent();

  Map<String, dynamic> toMap();

  static PlayerEvent fromMap(Map<String, dynamic> m) {
    switch (m['type'] as String) {
      case 'snapshot':
        return PlayerSnapshot.fromMap(m);
      case 'position':
        return PositionTick(m['seq'] as int, m['track'] as int, m['position_ms'] as int);
      case 'track_finished':
        return TrackFinished(m['book'] as String, m['track'] as int);
      case 'track_heard':
        return TrackHeard(m['book'] as String, m['track'] as int);
      case 'progress_queued':
        return ProgressQueued(
          bookId: m['book'] as String,
          title: m['title'] as String,
          positionMs: m['position_ms'] as int,
          finished: m['finished'] as bool,
          episodeId: m['episode'] as int?,
          item: m['item'] as int?,
          part: m['part'] as int?,
        );
      case 'last_written':
        return LastWritten(m['book'] as String, m['track'] as int, m['position_ms'] as int, m['at_ms'] as int);
      case 'playlist_position':
        return PlaylistPosition(m['book'] as String, m['track'] as int, m['position_ms'] as int);
      case 'podcast_state':
        return PodcastState(m['episode'] as int, m['position_ms'] as int, m['finished'] as bool);
    }
    throw FormatException('unknown event ${m['type']}');
  }
}

/// The coarse state of the engine. [seq] grows with every snapshot, so a mirror can drop a stale one.
class PlayerSnapshot extends PlayerEvent {
  const PlayerSnapshot({
    required this.seq,
    required this.bookId,
    required this.trackIndex,
    required this.positionMs,
    required this.playing,
    required this.speed,
    required this.loading,
    required this.error,
    this.errorKind,
    required this.unplayableIndex,
    required this.streamTitle,
    required this.streamDetails,
  });

  factory PlayerSnapshot.fromMap(Map<String, dynamic> m) => PlayerSnapshot(
        seq: m['seq'] as int,
        bookId: m['book'] as String?,
        trackIndex: m['track'] as int,
        positionMs: m['position_ms'] as int,
        playing: m['playing'] as bool,
        speed: (m['speed'] as num).toDouble(),
        loading: m['loading'] as bool,
        error: m['error'] as String?,
        errorKind: m['error_kind'] == null ? null : AudioErrorKind.values.byName(m['error_kind'] as String),
        unplayableIndex: m['unplayable'] as int?,
        streamTitle: m['stream_title'] as String?,
        streamDetails: m['stream_details'] == null ? null : _detailsFrom((m['stream_details'] as Map).cast<String, dynamic>()),
      );

  final int seq;
  final String? bookId;
  final int trackIndex;
  final int positionMs;
  final bool playing;
  final double speed;
  final bool loading;
  final String? error;

  /// For a radio stream: why it failed (unsupported, connection, server, certificate); null without an error.
  final AudioErrorKind? errorKind;

  /// The index (in the open book) of the track that cannot be played here, or null.
  final int? unplayableIndex;
  final String? streamTitle;
  final StreamDetails? streamDetails;

  @override
  Map<String, dynamic> toMap() => {
        'type': 'snapshot',
        'seq': seq,
        'book': bookId,
        'track': trackIndex,
        'position_ms': positionMs,
        'playing': playing,
        'speed': speed,
        'loading': loading,
        'error': error,
        'error_kind': errorKind?.name,
        'unplayable': unplayableIndex,
        'stream_title': streamTitle,
        'stream_details': streamDetails == null ? null : _detailsTo(streamDetails!),
      };
}

Map<String, dynamic> _detailsTo(StreamDetails d) =>
    {'name': d.name, 'genre': d.genre, 'description': d.description, 'website': d.website, 'bitrate': d.bitrateKbps, 'format': d.format};

StreamDetails _detailsFrom(Map<String, dynamic> m) => StreamDetails(
      name: m['name'] as String?,
      genre: m['genre'] as String?,
      description: m['description'] as String?,
      website: m['website'] as String?,
      bitrateKbps: m['bitrate'] as int?,
      format: m['format'] as String?,
    );

/// The cheap, frequent message: only the place in the track (twice a second while playing).
class PositionTick extends PlayerEvent {
  const PositionTick(this.seq, this.trackIndex, this.positionMs);
  final int seq;
  final int trackIndex;
  final int positionMs;
  @override
  Map<String, dynamic> toMap() => {'type': 'position', 'seq': seq, 'track': trackIndex, 'position_ms': positionMs};
}

/// A track played to its end.
class TrackFinished extends PlayerEvent {
  const TrackFinished(this.bookId, this.trackIndex);
  final String bookId;
  final int trackIndex;
  @override
  Map<String, dynamic> toMap() => {'type': 'track_finished', 'book': bookId, 'track': trackIndex};
}

/// A track is heard (within the tail of its end, or ended): the UI marks it.
class TrackHeard extends PlayerEvent {
  const TrackHeard(this.bookId, this.trackIndex);
  final String bookId;
  final int trackIndex;
  @override
  Map<String, dynamic> toMap() => {'type': 'track_heard', 'book': bookId, 'track': trackIndex};
}

/// The progress could not go to the server (offline, or no connection): the UI puts it in its outbox (one writer).
class ProgressQueued extends PlayerEvent {
  const ProgressQueued({required this.bookId, required this.title, required this.positionMs, required this.finished, this.episodeId, this.item, this.part});
  final String bookId;
  final String title;
  final int positionMs;
  final bool finished;

  /// Set for an episode of a podcast; else [item] and [part] name the title and its part.
  final int? episodeId;
  final int? item;
  final int? part;
  @override
  Map<String, dynamic> toMap() => {
        'type': 'progress_queued',
        'book': bookId,
        'title': title,
        'position_ms': positionMs,
        'finished': finished,
        'episode': episodeId,
        'item': item,
        'part': part,
      };
}

/// The place was written down (throttled): the UI stores it for "where I left off".
class LastWritten extends PlayerEvent {
  const LastWritten(this.bookId, this.trackIndex, this.positionMs, this.atMs);
  final String bookId;
  final int trackIndex;
  final int positionMs;
  final int atMs;
  @override
  Map<String, dynamic> toMap() => {'type': 'last_written', 'book': bookId, 'track': trackIndex, 'position_ms': positionMs, 'at_ms': atMs};
}

/// The place in a playlist (sent just before [LastWritten]).
class PlaylistPosition extends PlayerEvent {
  const PlaylistPosition(this.bookId, this.trackIndex, this.positionMs);
  final String bookId;
  final int trackIndex;
  final int positionMs;
  @override
  Map<String, dynamic> toMap() => {'type': 'playlist_position', 'book': bookId, 'track': trackIndex, 'position_ms': positionMs};
}

/// Where an episode of a podcast is, for the list of the downloaded episodes.
class PodcastState extends PlayerEvent {
  const PodcastState(this.episodeId, this.positionMs, this.finished);
  final int episodeId;
  final int positionMs;
  final bool finished;
  @override
  Map<String, dynamic> toMap() => {'type': 'podcast_state', 'episode': episodeId, 'position_ms': positionMs, 'finished': finished};
}

// ---- Transport ------------------------------------------------------------------------------------

/// The wire between the facade and the engine. [send] delivers a command; the engine's events come to [onEvent].
abstract class PlayerTransport {
  void send(PlayerCommand command);
  set onEvent(void Function(PlayerEvent event)? callback);
  void dispose();
}

/// Runs the engine in the same isolate. Delivery is SYNCHRONOUS in both directions: after [send] returns, the engine has acted and its
/// events have reached [onEvent] (this keeps `FakeAsync` tests deterministic). With [roundTrip] every message goes through
/// `toMap`, JSON and `fromMap` (the live book is dropped), which finds data that an isolate could not carry.
class InProcessTransport implements PlayerTransport {
  InProcessTransport(PlayerEngine Function(void Function(PlayerEvent) emit) build, {this.roundTrip = false}) {
    engine = build(_fromEngine);
  }

  final bool roundTrip;

  /// The engine on the other side. Service-side objects (backend, media source) are set on it directly; they do not travel.
  late final PlayerEngine engine;
  void Function(PlayerEvent event)? _listener;

  @override
  set onEvent(void Function(PlayerEvent event)? callback) => _listener = callback;

  @override
  void send(PlayerCommand command) {
    engine.handle(roundTrip ? PlayerCommand.fromMap(_wire(command.toMap())) : command);
  }

  void _fromEngine(PlayerEvent event) {
    _listener?.call(roundTrip ? PlayerEvent.fromMap(_wire(event.toMap())) : event);
  }

  static Map<String, dynamic> _wire(Map<String, dynamic> m) => (jsonDecode(jsonEncode(m)) as Map).cast<String, dynamic>();

  @override
  void dispose() => _listener = null;
}
