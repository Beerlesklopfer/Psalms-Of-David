// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/foundation.dart' show ValueListenable, ValueNotifier;
import 'package:flutter/widgets.dart';

import 'audio/audio_backend.dart';
import 'audio/playback_settings.dart';
import 'm3u.dart';
import 'player/engine.dart';
import 'player/transport.dart';
import 'voice/voice_match.dart';
import 'models.dart';
import 'playlists.dart' show kPlaylistBookPrefix, kPlaylistKind;
import 'podcast_downloads.dart';
import 'offline/offline_controller.dart';
import 'offline/offline_view.dart';
import 'server/last_played.dart';
import 'server/server_source.dart';

/// Playback state. A track with a [Track.path] is played by the [backend] and the
/// position is read from it. Without a backend, or for demo tracks without a file,
/// the position just keeps advancing so the UI and controls can be judged.
class PlayerController extends ChangeNotifier {
  PlayerController(this.library, {this.backend, MediaSource? source, this.lastStore, this.playbackStore, PlayerTransport? transport}) {
    // The playback state machine runs behind a transport (in this isolate for now, synchronously); this class mirrors what it says.
    _transport = transport ?? InProcessTransport((emit) => PlayerEngine(backend: backend, source: source, emit: emit));
    _transport.onEvent = _onEvent;
    final b = backend;
    if (b != null && b.bufferAdjustable) b.setBufferSeconds(playbackStore?.readBufferSeconds() ?? kBufferDefaultSeconds);
    showItemTime = playbackStore?.readShowItemTime() ?? false;
    // Demo: the first book has already been listened to.
    if (library.isEmpty) return;
    book = library.first;
    if (book!.tracks.length > 4) {
      trackIndex = 4;
      position = const Duration(minutes: 1, seconds: 20);
    }
  }

  late final PlayerTransport _transport;

  /// The engine behind an in-process transport (null behind another one).
  PlayerEngine? get _engine => switch (_transport) { InProcessTransport(:final engine) => engine, _ => null };

  /// What the lists show: the library of the server, or in the offline view the downloaded titles of it.
  List<LibraryItem> library;

  List<LibraryItem>? _full;
  bool _offlineView = false;

  /// The whole library of the server (what [library] is when the app is online).
  List<LibraryItem> get fullLibrary => _full ?? library;

  /// Offline mode: set by the app; the progress of the listening then waits in its outbox for the server.
  OfflineController? get offline => _offline;
  OfflineController? _offline;
  set offline(OfflineController? value) {
    _offline?.removeListener(_syncEnv);
    _offline = value;
    value?.addListener(_syncEnv);
    _syncEnv();
  }

  /// Shows only the downloaded titles (the offline view), or everything again. What plays is not interrupted.
  void setOfflineView(bool value) {
    if (_offlineView == value) return;
    _offlineView = value;
    _full ??= library;
    final openId = book?.id;
    final partId = track?.partId;
    library = _visible(fullLibrary);
    final same = openId == null ? null : library.where((b) => b.id == openId).firstOrNull;
    if (same != null) {
      final i = partId == null ? -1 : same.tracks.indexWhere((t) => t.partId == partId);
      if (i >= 0) {
        book = same;
        trackIndex = i;
      }
    } else if (book == null && library.isNotEmpty) {
      book = library.first;
    }
    notifyListeners();
  }

  List<LibraryItem> _visible(List<LibraryItem> all) => _offlineView && source != null ? downloadedOnly(all, source!.isDownloaded) : all;

  /// Changes a book of the library on this device (labels set without the server, a title with new labels): the change shows at once.
  void patchItem(String id, LibraryItem Function(LibraryItem) change) {
    List<LibraryItem> apply(List<LibraryItem> list) => [for (final b in list) b.id == id ? change(b) : b];
    _full = apply(fullLibrary);
    library = apply(library);
    if (book?.id == id) book = library.where((b) => b.id == id).firstOrNull ?? change(book!);
    notifyListeners();
  }

  /// Takes a book out of the lists (hidden without the server; the server decides when it is back).
  void removeItemLocally(String id) {
    _full = [for (final b in fullLibrary) if (b.id != id) b];
    library = [for (final b in library) if (b.id != id) b];
    notifyListeners();
  }

  /// The genres in the order of the administration; music is shown by them.
  List<String> genres = const [];
  final AudioBackend? backend;

  /// Where the last listening is remembered on this device; null in tests and demos.
  final LastPlayedStore? lastStore;

  /// Where the playback settings of this device are kept; null in tests and demos.
  final PlaybackSettingsStore? playbackStore;

  /// The time of the mini player (and its bar) is the time of the whole book (CD), not of the track that plays. The listener switches it by a tap.
  bool showItemTime = false;

  void toggleTimeScope() {
    showItemTime = !showItemTime;
    playbackStore?.writeShowItemTime(showItemTime);
    notifyListeners();
  }

  /// Whether the engine has a buffer whose size the listener can set.
  bool get bufferAdjustable => backend?.bufferAdjustable ?? false;

  /// Seconds of audio the engine keeps ready.
  int get bufferSeconds => backend?.bufferSeconds ?? kBufferDefaultSeconds;

  /// Sets the buffer and remembers it. It applies from the next track that is loaded.
  void setBufferSeconds(int seconds) {
    final b = backend;
    if (b == null || !b.bufferAdjustable) return;
    final n = seconds.clamp(kBufferMinSeconds, kBufferMaxSeconds);
    b.setBufferSeconds(n);
    playbackStore?.writeBufferSeconds(n);
    notifyListeners();
  }

  /// The server, if there is one: fetches the files of parts and takes the position. It lives on the engine side.
  MediaSource? get source => _engine?.source;
  set source(MediaSource? value) => _engine?.source = value;

  final Map<String, ({int track, Duration position})> _resume = {};

  // ---- The mirror of the engine: set by its snapshots, changed by the commands below -------------------

  LibraryItem? _book;
  int _trackIndex = 0;
  bool _playing = false;
  double _speed = 1.0;
  bool _loading = false;
  String? _error;
  AudioErrorKind? _errorKind;
  Track? _unplayable;
  int _seq = 0;
  int _depth = 0;
  bool _gone = false;
  bool _sentOffline = false;
  bool _sentWrites = false;

  /// The books handed to the engine, by id, so that an event about a book that is no longer open finds it.
  final Map<String, LibraryItem> _known = {};

  LibraryItem? get book => _book;

  /// Setting the book (tests, library changes) moves the place of the engine without touching the sound.
  set book(LibraryItem? value) {
    _book = value;
    if (value == null) {
      _send(const SetPlaceCommand(clearItem: true));
    } else {
      _known[value.id] = value;
      _send(SetPlaceCommand(book: LibraryItemData.of(value)));
    }
  }

  int get trackIndex => _trackIndex;
  set trackIndex(int value) {
    _trackIndex = value;
    _send(SetPlaceCommand(trackIndex: value));
  }

  final ValueNotifier<Duration> _position = ValueNotifier<Duration>(Duration.zero);

  /// The position in the current track, updated on every tick without a `notifyListeners` of the controller (which only notifies on
  /// coarse changes: state, track, book, speed, ...). Widgets that show the place listen to this.
  ValueListenable<Duration> get positionListenable => _position;

  Duration get position => _position.value;
  set position(Duration value) {
    _position.value = value;
    _send(SetPlaceCommand(positionMs: value.inMilliseconds));
  }

  bool get playing => _playing;
  double get speed => _speed;

  /// True while the file of the current track is being fetched.
  bool get loading => _loading;

  /// The last failure of the sound engine, or null. Playback is paused after it.
  String? get error => _error;

  /// For a radio stream: why it failed (unsupported format, connection, server, certificate).
  AudioErrorKind? get errorKind => _errorKind;

  /// The track that was asked for but cannot be played on this platform (its file format), or null. The mini player says so.
  Track? get unplayable => _unplayable;

  /// What a radio station announces now ("Artist - Song"), or null. Only set for live
  /// streams on a backend that knows titles (`StreamInfo`).
  String? streamTitle;

  /// What the station says about itself (name, genre, bitrate, format); null for a book or before the answer of the station.
  StreamDetails? streamDetails;

  /// What the station sends about what it plays now (the ICY title), or null: not a live stream, or nothing sent yet.
  String? get nowPlaying {
    final t = streamTitle?.trim();
    return track?.live == true && t != null && t.isNotEmpty ? t : null;
  }

  bool _disposed = false;

  /// The podcast episodes that are on this device (null in the demo); set by the app. The player reports the state of an episode to it.
  PodcastDownloads? podcastDownloads;

  Track? get track => book?.tracks[trackIndex];

  void Function(LibraryItem book, Track track, Duration position)? _onPlaylistPosition;

  /// Called when the place in a playlist is written down (for the playlists to remember it); null elsewhere.
  void Function(LibraryItem book, Track track, Duration position)? get onPlaylistPosition => _onPlaylistPosition;
  set onPlaylistPosition(void Function(LibraryItem book, Track track, Duration position)? value) {
    _onPlaylistPosition = value;
    _syncEnv();
  }

  /// The book to show for what plays: for a playlist the book of the track that plays now (its cover, its
  /// author), so a change across the border of two books shows the right one; else [book].
  LibraryItem? get shownItem {
    final b = book, t = track;
    if (b == null || b.kind != kPlaylistKind || t == null) return b;
    return library.where((x) => x.id == t.itemId).firstOrNull ?? b;
  }

  /// Where the listening is in the whole book: the tracks before this one and the
  /// position in this one.
  Duration get bookPosition {
    final b = book;
    if (b == null) return Duration.zero;
    var done = Duration.zero;
    for (var i = 0; i < trackIndex && i < b.tracks.length; i++) {
      done += b.tracks[i].length;
    }
    return done + position;
  }

  /// The running time of the whole book.
  Duration get bookTotal => book?.total ?? Duration.zero;

  /// Jumps to a place in the whole book: to the track that holds it, and to the place in it.
  void seekItem(Duration to) {
    final b = book;
    if (b == null || b.tracks.isEmpty) return;
    var rest = to < Duration.zero ? Duration.zero : to;
    var index = 0;
    while (index < b.tracks.length - 1 && rest >= b.tracks[index].length) {
      rest -= b.tracks[index].length;
      index++;
    }
    if (rest > b.tracks[index].length) rest = b.tracks[index].length;
    if (index == trackIndex) {
      seek(rest);
      return;
    }
    _send(SelectTrackCommand(index, rest.inMilliseconds));
    notifyListeners();
  }

  double get trackProgress {
    final t = track;
    if (t == null || t.length == Duration.zero) return 0;
    return position.inMilliseconds / t.length.inMilliseconds;
  }

  /// The full book of [shown] (the offline view may have cut it down to the downloaded tracks); [shown] itself if there is no other.
  LibraryItem _fullOf(LibraryItem shown) => fullLibrary.where((b) => b.id == shown.id).firstOrNull ?? shown;

  /// The place of the track [index] of [shown] in the full book: what is remembered counts in the full book, so that it still fits when the
  /// offline view shows fewer tracks.
  int _fullIndex(LibraryItem shown, int index) {
    final full = _fullOf(shown);
    if (identical(full, shown) || index < 0 || index >= shown.tracks.length) return index;
    final at = full.tracks.indexWhere((t) => t.partId != null && t.partId == shown.tracks[index].partId);
    return at < 0 ? index : at;
  }

  /// The place in [shown] of the track that is remembered at [stored] of the full book; null if that track is not in [shown] (not downloaded).
  int? _shownIndex(LibraryItem shown, int stored) {
    if (shown.tracks.isEmpty || stored < 0) return null;
    final full = _fullOf(shown);
    // Nothing was cut: the index counts as it is, and one past the end (the book got shorter) is the last part.
    if (full.tracks.length == shown.tracks.length) return stored.clamp(0, shown.tracks.length - 1);
    if (stored >= full.tracks.length) return null;
    final at = shown.tracks.indexWhere((t) => t.partId != null && t.partId == full.tracks[stored].partId);
    return at < 0 ? null : at;
  }

  /// Fraction of the whole book, 0 to 1.
  double progressOf(LibraryItem b) {
    final int index;
    final Duration pos;
    if (b.id == book?.id) {
      index = trackIndex;
      pos = position;
    } else {
      final r = _resume[b.id];
      if (r == null) return 0;
      final shown = _shownIndex(b, r.track);
      if (shown == null) return 0;
      index = shown;
      pos = r.position;
    }
    var done = pos;
    for (var i = 0; i < index; i++) {
      done += b.tracks[i].length;
    }
    return done.inMilliseconds / b.total.inMilliseconds;
  }

  // ---- Downloads: keeping a book on the device ---------------------------------------

  /// Books being downloaded right now, with the parts done and the parts in all.
  final Map<String, ({int done, int total})> downloads = {};
  final Set<String> _cancelled = {};

  /// The reason the last download stopped, or null.
  String? downloadError;

  bool get canDownload => source?.canDownload ?? false;

  /// Counts up whenever the downloads on the device change, so a page can show the size again.
  int storageRevision = 0;

  Future<({int files, int bytes})> storageUsed() async => await source?.storageUsed() ?? (files: 0, bytes: 0);

  /// Deletes every downloaded part of every book.
  Future<void> removeAllDownloads() async {
    await source?.removeAllDownloads();
    storageRevision++;
    if (!_disposed) notifyListeners();
  }

  bool isTrackDownloaded(Track t) => source?.isDownloaded(t) ?? false;

  int downloadedCount(LibraryItem b) => b.tracks.where(isTrackDownloaded).length;

  /// Fetches every part of [b] that is not on the device yet, one after the other.
  Future<void> downloadItem(LibraryItem b) async {
    final s = source;
    if (s == null || !s.canDownload || downloads.containsKey(b.id)) return;
    downloadError = null;
    final total = b.tracks.length;
    downloads[b.id] = (done: 0, total: total);
    notifyListeners();
    try {
      // What is on the device is compared with the server first (online only): a corrupt file is dropped and fetched again.
      if (_offline?.active != true) {
        for (final t in b.tracks) {
          if (t.partId != null && s.isDownloaded(t)) await s.verifyDownloaded(t);
        }
      }
      final todo = b.tracks.where((t) => t.partId != null && !s.isDownloaded(t)).toList();
      var done = total - todo.length;
      downloads[b.id] = (done: done, total: total);
      if (!_disposed) notifyListeners();
      for (final t in todo) {
        if (_cancelled.remove(b.id) || _disposed) break;
        await s.fetchPart(t);
        done++;
        downloads[b.id] = (done: done, total: total);
        await _writePlaylistFile(b);
        if (!_disposed) notifyListeners();
      }
    } on Object catch (e) {
      downloadError = '$e';
    } finally {
      // Whatever happened, the file says what is on the device now (also when nothing was left to fetch).
      await _writePlaylistFile(b);
      _cancelled.remove(b.id);
      downloads.remove(b.id);
      storageRevision++;
      if (!_disposed) notifyListeners();
    }
  }

  /// The name of the playlist file (without the ending) next to the downloads of [b]: one per book, one per
  /// playlist. Null for what is not kept as parts (episodes, stations).
  String? m3uNameOf(LibraryItem b) {
    if (b.kind == kPlaylistKind) return 'playlist-${b.id.substring(kPlaylistBookPrefix.length)}';
    if (b.kind == 'podcast' || b.isStream) return null;
    return 'book-${b.id}';
  }

  /// Writes the extended M3U of [b] again: the parts that are on the device, in order, with paths relative to
  /// the folder. With no part left the file is removed. Called when parts come in and when the download goes.
  Future<void> _writePlaylistFile(LibraryItem b) async {
    final s = source;
    final name = m3uNameOf(b);
    if (s == null || name == null) return;
    final entries = <M3uEntry>[];
    for (final t in b.tracks) {
      final path = s.relativePath(t);
      if (path == null) continue;
      entries.add(M3uEntry(duration: t.live ? null : t.length, artist: t.itemAuthor ?? b.author, title: t.title, path: path));
    }
    try {
      await s.writeM3u(name, entries);
    } on Object {
      // The playlist file is a convenience; the download stands without it.
    }
  }

  /// Stops a running download after the part that is coming in.
  void cancelDownload(LibraryItem b) {
    if (downloads.containsKey(b.id)) _cancelled.add(b.id);
  }

  /// Deletes the downloaded parts of [b] from the device.
  Future<void> removeDownload(LibraryItem b) async {
    final s = source;
    if (s == null) return;
    for (final t in b.tracks) {
      await s.removeDownload(t);
    }
    await _writePlaylistFile(b);
    storageRevision++;
    if (!_disposed) notifyListeners();
  }

  /// The nickname of the signed-in user, for the greeting; null without a server.
  String? userName;

  /// The ids of the books that are new for this user (from the server, newest first).
  List<String> newIds = const [];

  /// What the other members of the family listened to lately (the rows "recently heard by" of the home page).
  List<FamilyMember> heard = const [];

  /// The books of the library that [m] listened to lately, the latest first (unknown ids are skipped).
  List<LibraryItem> itemsOf(FamilyMember m) => [
        for (final id in m.itemIds)
          for (final b in library.where((b) => b.id == id).take(1)) b,
      ];

  /// The media time of the family, the own first (the charts of the home page).
  List<FamilyTime> family = const [];
  bool shareListening = true;
  bool mayHideListening = true;

  /// Remembers the choice after the server took it (the settings call the server first).
  void setShareListening(bool share) {
    shareListening = share;
    notifyListeners();
  }

  /// The books of [newIds] that are in the library, in the order of the server. Unknown ids are skipped.
  List<LibraryItem> newBooks() => [
        for (final id in newIds)
          for (final b in library.where((b) => b.id == id).take(1)) b,
      ];

  /// Tracks heard to the end in this session, as `bookId:trackIndex`. It shows the check mark at once;
  /// a reload from the server replaces it.
  final Set<String> _heard = {};

  /// Has this user heard track [index] of [b] to the end: the server says so, or it happened just now.
  bool isTrackPlayed(LibraryItem b, int index) =>
      (index >= 0 && index < b.tracks.length && b.tracks[index].plays > 0) || _heard.contains('${b.id}:$index');

  /// How often [b] was heard as a whole. The server knows; a book finished just now counts once at least.
  int playsOf(LibraryItem b) {
    if (b.plays > 0 || b.tracks.isEmpty) return b.plays;
    return [for (var i = 0; i < b.tracks.length; i++) i].every((i) => isTrackPlayed(b, i)) ? 1 : 0;
  }

  /// The ids of the books listened to, the latest first. Comes from the server and is kept up to
  /// date here, so the home page shows a book at once when it is opened.
  final List<String> recentIds = [];

  void _touchRecent(LibraryItem b) {
    recentIds
      ..remove(b.id)
      ..insert(0, b.id);
  }

  /// The books listened to last, the latest first, at most [count].
  List<LibraryItem> recentItems([int count = 6]) => [
        for (final id in recentIds)
          for (final b in library.where((b) => b.id == id).take(1)) b,
      ].take(count).toList();

  /// Set by the app when there is a server: loads the library again. Null without a server.
  Future<void> Function()? refresher;

  /// True while the library is being loaded again.
  bool refreshing = false;

  bool get canRefresh => refresher != null;

  /// Asks the server for the library again (new CDs, releases, blurbs).
  Future<void> refreshLibrary() async {
    final r = refresher;
    if (r == null || refreshing) return;
    refreshing = true;
    notifyListeners();
    try {
      await r();
    } finally {
      refreshing = false;
      if (!_disposed) notifyListeners();
    }
  }

  /// Puts a reloaded library in place without interrupting what plays: the open book stays,
  /// with its track and position, if it is still there; otherwise the player stops.
  void updateLibrary(LoadedLibrary loaded) {
    userName = loaded.userName ?? userName;
    newIds = loaded.newIds;
    family = loaded.family;
    heard = loaded.heard;
    shareListening = loaded.shareListening;
    mayHideListening = loaded.mayHideListening;
    _heard.clear();
    _send(const ClearHeardCommand());
    if (loaded.recent.isNotEmpty) {
      recentIds
        ..clear()
        ..addAll(loaded.recent);
    }
    final openId = book?.id;
    _full = loaded.books;
    library = _visible(_full!);
    genres = loaded.genres;
    for (final e in loaded.resume.entries) {
      _resume.putIfAbsent(e.key, () => e.value);
    }
    if (openId != null) {
      final same = library.where((b) => b.id == openId).firstOrNull;
      if (same != null) {
        book = same;
        if (trackIndex >= same.tracks.length) trackIndex = same.tracks.isEmpty ? 0 : same.tracks.length - 1;
      } else {
        _send(const HaltCommand());
        book = library.isEmpty ? null : library.first;
        trackIndex = 0;
        position = Duration.zero;
      }
    } else if (library.isNotEmpty) {
      book = library.first;
    }
    notifyListeners();
  }

  /// Replaces the demo data with the library of the server.
  void attachServer(MediaSource source, LoadedLibrary loaded) {
    this.source = source;
    _send(const HaltCommand());
    _full = loaded.books;
    library = _visible(_full!);
    genres = loaded.genres;
    _resume
      ..clear()
      ..addAll(loaded.resume);
    userName = loaded.userName;
    newIds = loaded.newIds;
    family = loaded.family;
    heard = loaded.heard;
    shareListening = loaded.shareListening;
    mayHideListening = loaded.mayHideListening;
    _heard.clear();
    _send(const ClearHeardCommand());
    recentIds
      ..clear()
      ..addAll(loaded.recent);
    _openWhereLeftOff(loaded);
    notifyListeners();
  }

  /// Opens the book that was listened to last, at the place where it was left: what this device
  /// remembers, or what the server says about another device, whichever is newer. If neither is
  /// known (or the book is gone) the first book of the library is shown.
  void _openWhereLeftOff(LoadedLibrary loaded) {
    final mine = lastStore?.read();
    LibraryItem? find(String id) => library.where((b) => b.id == id).firstOrNull;
    final fromServer = loaded.lastItemId == null ? null : find(loaded.lastItemId!);
    final fromDevice = mine == null ? null : find(mine.bookId);
    if (fromDevice != null && (fromServer == null || mine!.at >= loaded.lastAt)) {
      book = fromDevice;
      final at = _shownIndex(fromDevice, mine!.track);
      trackIndex = at ?? 0;
      position = at == null ? Duration.zero : mine.position;
      return;
    }
    book = fromServer ?? (library.isEmpty ? null : library.first);
    final r = book == null ? null : _resume[book!.id];
    final at = r == null || book == null ? null : _shownIndex(book!, r.track);
    trackIndex = at ?? 0;
    position = at == null ? Duration.zero : r!.position;
  }

  /// When this device last listened or opened something (ms since 1970), to tell whether another device went on after it.
  int _localAt = 0;

  /// The listening of the account as the server has it now (`continueListening`: the latest first). Another device (the iPad, the phone) may have gone on
  /// with another book or another track: when it did so after this device and nothing plays here, the player shows that place (book, track, position)
  /// instead of the old one. While something plays here only the places of the other books are taken over.
  void followServerListening(List<Map<String, dynamic>> entries) {
    if (entries.isEmpty || library.isEmpty) return;
    var changed = false;
    for (final e in entries) {
      final b = library.where((b) => b.id == '${e['item_id']}').firstOrNull;
      if (b == null || b.id == book?.id && (playing || loading)) continue;
      final at = e['updated_at'];
      final index = _fullOf(b).tracks.indexWhere((t) => t.partId == e['part_id']);
      if (index < 0 || at is! int) continue;
      final r = (
        track: index,
        position: Duration(milliseconds: (e['position_ms'] as int?) ?? 0),
      );
      final old = _resume[b.id];
      if (old == null || old.track != r.track || old.position != r.position) {
        _resume[b.id] = r;
        changed = true;
      }
    }
    final first = entries.first;
    final newest = library
        .where((b) => b.id == '${first['item_id']}')
        .firstOrNull;
    final at = first['updated_at'];
    if (newest != null &&
        at is int &&
        at > _localAt + 1000 &&
        !playing &&
        !loading) {
      final r = _resume[newest.id];
      if (r != null &&
          (book?.id != newest.id ||
              trackIndex != r.track ||
              (position - r.position).abs() > const Duration(seconds: 2))) {
        book = newest;
        trackIndex = _shownIndex(newest, r.track) ?? 0;
        position = r.position;
        _touchRecent(newest);
        _localAt = at;
        changed = true;
      }
    }
    if (changed) notifyListeners();
  }

  /// Writes down where the listening is, now. For the moments when the app may be closed
  /// without further notice (it goes to the background, the window closes).
  void flush() => _send(const FlushCommand());

  /// Opens a passage of an audio Bible by its text ("Johannes 3,16"). False if
  /// there is no server, nothing matches, or the planner forbids listening now.
  Future<bool> openPassage(String text) async {
    final s = source;
    if (s == null) return false;
    final hit = await s.resolvePassage(text);
    if (hit == null || !hit.allowed) return false;
    final b = library.where((b) => b.id == hit.itemId).firstOrNull;
    final index = b?.tracks.indexWhere((t) => t.partId == hit.partId) ?? -1;
    if (b == null || index < 0) return false;
    _saveResume();
    _openItem(b, index, hit.position, forceReload: true);
    return true;
  }

  /// Hands a book to the engine and plays it from [index] at [start].
  void _openItem(LibraryItem b, int index, Duration start, {bool forceReload = false}) {
    _book = b;
    _known[b.id] = b;
    _send(OpenCommand(LibraryItemData.of(b), trackIndex: index, startMs: start.inMilliseconds, forceReload: forceReload));
    notifyListeners();
  }

  /// Opens a book and plays it; [start] is where in the track it begins (an episode goes on where the account stopped).
  void open(LibraryItem b, {int? track, Duration? start}) {
    _localAt = DateTime.now().millisecondsSinceEpoch;
    _saveResume();
    _touchRecent(b);
    final int index;
    Duration at;
    if (track != null) {
      index = track;
      at = Duration.zero;
    } else {
      final r = _resume[b.id];
      final shown = r == null ? null : _shownIndex(b, r.track);
      index = shown ?? 0;
      at = shown == null ? Duration.zero : r!.position;
    }
    if (start != null) at = start;
    _openItem(b, index, at);
  }

  void toggle() {
    if (book == null) return;
    _send(const ToggleCommand());
    notifyListeners();
  }

  /// Voice: go on where the listening stopped. Plays the open book if it is paused, else the book listened to last.
  void resumeListening() {
    if (playing) return;
    if (book != null) {
      _send(const PlayCommand());
      notifyListeners();
      return;
    }
    final last = recentItems(1).firstOrNull;
    if (last != null) open(last);
  }

  /// Voice: opens the book that fits [query] best (title, author, labels) and plays it. False if nothing fits.
  bool playByVoice(String query) {
    final found = bestMatch(library, query);
    if (found == null) return false;
    open(found);
    return true;
  }

  void seek(Duration to) {
    if (track == null) return;
    _send(SeekCommand(to.inMilliseconds));
    notifyListeners();
  }

  /// Jumps relative to the current position, also across track boundaries.
  void skip(Duration delta) {
    if (book == null) return;
    _send(SkipCommand(delta.inMilliseconds));
    notifyListeners();
  }

  void next() {
    if (book == null || trackIndex >= book!.tracks.length - 1) return;
    _send(const NextCommand());
    notifyListeners();
  }

  /// Like Spotify: after the first seconds go to the start, otherwise go back.
  void previous() {
    if (book == null) return;
    _send(const PreviousCommand());
    notifyListeners();
  }

  /// The speed applies to everything that is played, until it is changed again.
  void setSpeed(double value) {
    _send(SetSpeedCommand(value));
    notifyListeners();
  }

  // ---- The wire to the engine -------------------------------------------------------------------------

  /// Tells the engine what it needs of this side: whether the progress waits for the server and whether the place is wanted.
  void _syncEnv() {
    final offline = _offline?.active == true;
    final writes = lastStore != null || _onPlaylistPosition != null;
    if (offline == _sentOffline && writes == _sentWrites) return;
    _sentOffline = offline;
    _sentWrites = writes;
    _transport.send(SetEnvCommand(offlineActive: offline, writesLast: writes));
  }

  void _send(PlayerCommand c) {
    if (_gone) return;
    _syncEnv();
    _depth++;
    try {
      _transport.send(c);
    } finally {
      _depth--;
    }
  }

  List<Object?> _coarse() => [_book, _trackIndex, _playing, _loading, _error, _speed, streamTitle, streamDetails, _unplayable, _heard.length];

  /// What the engine says. A snapshot that changes something coarse notifies, unless a command of this class is on its way (it notifies itself).
  void _onEvent(PlayerEvent e) {
    if (_gone) return;
    switch (e) {
      case PlayerSnapshot():
        if (e.seq <= _seq) return; // stale
        _seq = e.seq;
        final before = _coarse();
        _trackIndex = e.trackIndex;
        _position.value = Duration(milliseconds: e.positionMs);
        _playing = e.playing;
        _speed = e.speed;
        _loading = e.loading;
        _error = e.error;
        _errorKind = e.errorKind;
        final i = e.unplayableIndex;
        _unplayable = i != null && _book != null && i >= 0 && i < _book!.tracks.length ? _book!.tracks[i] : null;
        streamTitle = e.streamTitle;
        streamDetails = e.streamDetails;
        _notifyIfCoarse(before);
      case PositionTick():
        if (e.seq <= _seq) return;
        _seq = e.seq;
        _trackIndex = e.trackIndex;
        _position.value = Duration(milliseconds: e.positionMs);
      case TrackHeard():
        final before = _coarse();
        _heard.add('${e.bookId}:${e.trackIndex}');
        _notifyIfCoarse(before);
      case TrackFinished():
        break;
      case ProgressQueued():
        _queueProgress(e);
      case LastWritten():
        _localAt = e.atMs;
        lastStore?.write(LastPlayed(bookId: e.bookId, track: _known[e.bookId] == null ? e.trackIndex : _fullIndex(_known[e.bookId]!, e.trackIndex), position: Duration(milliseconds: e.positionMs), at: e.atMs));
      case PlaylistPosition():
        final b = _known[e.bookId];
        if (b != null && e.trackIndex < b.tracks.length) {
          _onPlaylistPosition?.call(b, b.tracks[e.trackIndex], Duration(milliseconds: e.positionMs));
        }
      case PodcastState():
        podcastDownloads?.noteState(e.episodeId, Duration(milliseconds: e.positionMs), e.finished);
    }
  }

  void _notifyIfCoarse(List<Object?> before) {
    if (_depth > 0 || _disposed) return;
    final after = _coarse();
    for (var i = 0; i < before.length; i++) {
      if (!identical(before[i], after[i]) && before[i] != after[i]) {
        notifyListeners();
        return;
      }
    }
  }

  /// Keeps where the listening is for the server (a book of the library or a podcast episode; a station has no position).
  void _queueProgress(ProgressQueued e) {
    final off = _offline;
    if (off == null) return;
    final episode = e.episodeId;
    if (episode != null) {
      off.outbox.addEpisode(id: episode, positionMs: e.positionMs, finished: e.finished, title: e.title);
      off.touch();
      return;
    }
    off.outbox.addProgress(item: e.item!, part: e.part!, positionMs: e.positionMs, finished: e.finished);
    off.touch();
  }

  void _saveResume() {
    final b = book;
    if (b != null) _resume[b.id] = (track: _fullIndex(b, trackIndex), position: position);
  }

  @override
  void dispose() {
    _disposed = true;
    _send(const DisposeCommand()); // reports, stops the clock, disposes the backend
    _gone = true;
    _transport.dispose();
    _offline?.removeListener(_syncEnv);
    podcastDownloads?.dispose();
    _position.dispose();
    super.dispose();
  }
}

class PlayerScope extends InheritedNotifier<PlayerController> {
  const PlayerScope({
    super.key,
    required PlayerController controller,
    required super.child,
  }) : super(notifier: controller);

  /// The player, or null where there is none above (a page shown on its own).
  static PlayerController? maybeOf(BuildContext context) => context.dependOnInheritedWidgetOfExactType<PlayerScope>()?.notifier;

  static PlayerController of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<PlayerScope>()!.notifier!;
}
