// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/widgets.dart';

import 'audio/audio_backend.dart';
import 'models.dart';
import 'server/last_played.dart';
import 'server/server_source.dart';

/// Playback state. A track with a [Track.path] is played by the [backend] and the
/// position is read from it. Without a backend, or for demo tracks without a file,
/// the position just keeps advancing so the UI and controls can be judged.
class PlayerController extends ChangeNotifier {
  PlayerController(this.library, {this.backend, this.source, this.lastStore}) {
    // Demo: the first book has already been listened to.
    if (library.isEmpty) return;
    book = library.first;
    if (book!.tracks.length > 4) {
      trackIndex = 4;
      position = const Duration(minutes: 1, seconds: 20);
    }
  }

  List<Book> library;
  final AudioBackend? backend;

  /// Where the last listening is remembered on this device; null in tests and demos.
  final LastPlayedStore? lastStore;
  DateTime _lastWritten = DateTime.fromMillisecondsSinceEpoch(0);

  /// The server, if there is one: fetches the files of parts and takes the position.
  MediaSource? source;
  final Map<String, ({int track, Duration position})> _resume = {};

  Book? book;
  int trackIndex = 0;
  Duration position = Duration.zero;
  bool playing = false;
  double speed = 1.0;
  Timer? _timer;

  /// The last failure of the sound engine, or null. Playback is paused after it.
  String? error;

  /// Which track the backend has loaded, as `bookId:trackIndex`.
  String? _loadedKey;

  /// True while the file of the current track is being fetched.
  bool loading = false;
  DateTime _lastReport = DateTime.fromMillisecondsSinceEpoch(0);
  bool _disposed = false;

  Track? get track => book?.tracks[trackIndex];

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
  void seekBook(Duration to) {
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
    _report();
    trackIndex = index;
    position = rest;
    _syncBackend();
    notifyListeners();
  }

  double get trackProgress {
    final t = track;
    if (t == null || t.length == Duration.zero) return 0;
    return position.inMilliseconds / t.length.inMilliseconds;
  }

  /// Fraction of the whole book, 0 to 1.
  double progressOf(Book b) {
    final int index;
    final Duration pos;
    if (b.id == book?.id) {
      index = trackIndex;
      pos = position;
    } else {
      final r = _resume[b.id];
      if (r == null) return 0;
      index = r.track;
      pos = r.position;
    }
    var done = pos;
    for (var i = 0; i < index; i++) {
      done += b.tracks[i].length;
    }
    return done.inMilliseconds / b.total.inMilliseconds;
  }

  bool get _useBackend => backend != null && (track?.path != null || (track?.partId != null && source != null));

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

  int downloadedCount(Book b) => b.tracks.where(isTrackDownloaded).length;

  /// Fetches every part of [b] that is not on the device yet, one after the other.
  Future<void> downloadBook(Book b) async {
    final s = source;
    if (s == null || !s.canDownload || downloads.containsKey(b.id)) return;
    final todo = b.tracks.where((t) => t.partId != null && !s.isDownloaded(t)).toList();
    final total = b.tracks.length;
    var done = total - todo.length;
    downloadError = null;
    downloads[b.id] = (done: done, total: total);
    notifyListeners();
    try {
      for (final t in todo) {
        if (_cancelled.remove(b.id) || _disposed) break;
        await s.fetchPart(t);
        done++;
        downloads[b.id] = (done: done, total: total);
        if (!_disposed) notifyListeners();
      }
    } on Object catch (e) {
      downloadError = '$e';
    } finally {
      _cancelled.remove(b.id);
      downloads.remove(b.id);
      storageRevision++;
      if (!_disposed) notifyListeners();
    }
  }

  /// Stops a running download after the part that is coming in.
  void cancelDownload(Book b) {
    if (downloads.containsKey(b.id)) _cancelled.add(b.id);
  }

  /// Deletes the downloaded parts of [b] from the device.
  Future<void> removeDownload(Book b) async {
    final s = source;
    if (s == null) return;
    for (final t in b.tracks) {
      await s.removeDownload(t);
    }
    storageRevision++;
    if (!_disposed) notifyListeners();
  }

  /// The nickname of the signed-in user, for the greeting; null without a server.
  String? userName;

  /// The ids of the books listened to, the latest first. Comes from the server and is kept up to
  /// date here, so the home page shows a book at once when it is opened.
  final List<String> recentIds = [];

  void _touchRecent(Book b) {
    recentIds
      ..remove(b.id)
      ..insert(0, b.id);
  }

  /// The books listened to last, the latest first, at most [count].
  List<Book> recentBooks([int count = 6]) => [
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
    if (loaded.recent.isNotEmpty) {
      recentIds
        ..clear()
        ..addAll(loaded.recent);
    }
    final openId = book?.id;
    library = loaded.books;
    for (final e in loaded.resume.entries) {
      _resume.putIfAbsent(e.key, () => e.value);
    }
    if (openId != null) {
      final same = library.where((b) => b.id == openId).firstOrNull;
      if (same != null) {
        book = same;
        if (trackIndex >= same.tracks.length) trackIndex = same.tracks.length - 1;
      } else {
        _timer?.cancel();
        playing = false;
        backend?.stop();
        _loadedKey = null;
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
    _timer?.cancel();
    playing = false;
    backend?.stop();
    _loadedKey = null;
    library = loaded.books;
    _resume
      ..clear()
      ..addAll(loaded.resume);
    userName = loaded.userName;
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
    Book? find(String id) => library.where((b) => b.id == id).firstOrNull;
    final fromServer = loaded.lastBookId == null ? null : find(loaded.lastBookId!);
    final fromDevice = mine == null ? null : find(mine.bookId);
    if (fromDevice != null && (fromServer == null || mine!.at >= loaded.lastAt)) {
      book = fromDevice;
      trackIndex = mine!.track.clamp(0, fromDevice.tracks.length - 1);
      position = mine.position;
      return;
    }
    book = fromServer ?? (library.isEmpty ? null : library.first);
    final r = book == null ? null : _resume[book!.id];
    trackIndex = r?.track ?? 0;
    position = r?.position ?? Duration.zero;
  }

  /// Writes down where the listening is, now. For the moments when the app may be closed
  /// without further notice (it goes to the background, the window closes).
  void flush() {
    _report();
    _writeLast(force: true);
  }

  void _writeLast({bool force = false}) {
    final store = lastStore;
    final b = book;
    if (store == null || b == null || b.tracks.isEmpty) return;
    final now = DateTime.now();
    if (!force && now.difference(_lastWritten) < const Duration(seconds: 5)) return;
    _lastWritten = now;
    store.write(LastPlayed(bookId: b.id, track: trackIndex, position: position, at: now.millisecondsSinceEpoch));
  }

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
    _report();
    _saveResume();
    book = b;
    trackIndex = index;
    position = hit.position;
    _loadedKey = null;
    _setPlaying(true);
    return true;
  }

  /// Tells the server where the listening is, at most every few seconds unless
  /// [force]d (on pause, on leaving a track).
  void _report({bool finished = false, bool force = true}) {
    _writeLast(force: force);
    final b = book;
    final t = track;
    final s = source;
    if (b == null || t == null || s == null || t.partId == null) return;
    final now = DateTime.now();
    if (!force && now.difference(_lastReport) < const Duration(seconds: 10)) return;
    _lastReport = now;
    s.saveProgress(b, t, position, finished: finished).catchError((Object _) {});
  }

  /// Makes the backend follow the controller: loads the current track if it is
  /// not the loaded one, then plays or pauses.
  void _syncBackend() {
    if (!_useBackend) {
      if (_loadedKey != null) {
        backend!.stop();
        _loadedKey = null;
      }
      return;
    }
    final b = backend!;
    final key = '${book!.id}:$trackIndex';
    try {
      if (b.speed != speed) b.setSpeed(speed);
      if (key != _loadedKey) {
        final path = track!.path;
        if (path == null) {
          _loadRemote(key);
          return;
        }
        b.load(path, start: position, play: playing, title: track!.title);
        _loadedKey = key;
      } else if (playing) {
        b.play();
      } else {
        b.pause();
      }
      error = null;
    } on AudioException catch (e) {
      error = e.message;
      _loadedKey = null;
      playing = false;
    }
  }

  /// Fetches the file of the current track, then loads it, if the player is still
  /// on that track by then.
  Future<void> _loadRemote(String key) async {
    final t = track!;
    loading = true;
    notifyListeners();
    try {
      final path = await source!.fetchPart(t);
      if (_disposed || key != '${book?.id}:$trackIndex') return;
      backend!.load(path, start: position, play: playing, title: t.title);
      _loadedKey = key;
      error = null;
    } on Object catch (e) {
      if (_disposed || key != '${book?.id}:$trackIndex') return;
      error = e is AudioException ? e.message : '$e';
      playing = false;
      _timer?.cancel();
    } finally {
      if (!_disposed && key == '${book?.id}:$trackIndex') {
        loading = false;
        notifyListeners();
      }
    }
  }

  void open(Book b, {int? track}) {
    _report();
    _saveResume();
    book = b;
    _touchRecent(b);
    if (track != null) {
      trackIndex = track;
      position = Duration.zero;
    } else {
      final r = _resume[b.id];
      trackIndex = r?.track ?? 0;
      position = r?.position ?? Duration.zero;
    }
    _setPlaying(true);
  }

  void toggle() {
    if (book == null) return;
    _setPlaying(!playing);
  }

  void seek(Duration to) {
    final t = track;
    if (t == null) return;
    position = to < Duration.zero
        ? Duration.zero
        : (to > t.length ? t.length : to);
    if (_useBackend && _loadedKey == '${book!.id}:$trackIndex') {
      try {
        backend!.seek(position);
      } on AudioException catch (e) {
        error = e.message;
      }
    }
    notifyListeners();
  }

  /// Jumps relative to the current position, also across track boundaries.
  void skip(Duration delta) {
    if (book == null) return;
    final last = book!.tracks.length - 1;
    var p = position + delta;
    while (p >= track!.length && trackIndex < last) {
      p -= track!.length;
      trackIndex++;
    }
    while (p < Duration.zero && trackIndex > 0) {
      trackIndex--;
      p += track!.length;
    }
    position = p;
    _syncBackend();
    seek(p);
  }

  void next() {
    if (book == null || trackIndex >= book!.tracks.length - 1) return;
    _report();
    trackIndex++;
    position = Duration.zero;
    _syncBackend();
    notifyListeners();
  }

  /// Like Spotify: after the first seconds go to the start, otherwise go back.
  void previous() {
    if (book == null) return;
    _report();
    if (position > const Duration(seconds: 3) || trackIndex == 0) {
      position = Duration.zero;
    } else {
      trackIndex--;
      position = Duration.zero;
    }
    _syncBackend();
    notifyListeners();
  }

  /// The speed applies to everything that is played, until it is changed again.
  void setSpeed(double value) {
    speed = value;
    if (_useBackend) {
      try {
        backend!.setSpeed(value);
      } on AudioException catch (e) {
        error = e.message;
      }
    }
    notifyListeners();
  }

  void _setPlaying(bool value) {
    playing = value;
    _timer?.cancel();
    if (!value) _report();
    _syncBackend();
    if (playing) {
      _timer = Timer.periodic(const Duration(milliseconds: 500), _onTick);
    }
    notifyListeners();
  }

  void _onTick(Timer _) {
    final t = track;
    if (t == null) return;
    if (_useBackend && _loadedKey != '${book!.id}:$trackIndex') return; // still fetching
    if (_useBackend) {
      position = backend!.position;
      _report(force: false);
      if (backend!.state == BackendState.ended) {
        if (trackIndex < book!.tracks.length - 1) {
          trackIndex++;
          position = Duration.zero;
          _syncBackend();
        } else {
          position = t.length;
          playing = false;
          _report(finished: true);
          _timer?.cancel();
        }
      } else if (backend!.state == BackendState.failed) {
        error = 'playback failed';
        playing = false;
        _timer?.cancel();
      }
      notifyListeners();
      return;
    }
    position += Duration(milliseconds: (500 * speed).round());
    if (position >= t.length) {
      if (trackIndex < book!.tracks.length - 1) {
        trackIndex++;
        position = Duration.zero;
      } else {
        position = t.length;
        playing = false;
        _timer?.cancel();
      }
    }
    notifyListeners();
  }

  void _saveResume() {
    final b = book;
    if (b != null) _resume[b.id] = (track: trackIndex, position: position);
  }

  @override
  void dispose() {
    _disposed = true;
    _report();
    _timer?.cancel();
    backend?.dispose();
    super.dispose();
  }
}

class PlayerScope extends InheritedNotifier<PlayerController> {
  const PlayerScope({
    super.key,
    required PlayerController controller,
    required super.child,
  }) : super(notifier: controller);

  static PlayerController of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<PlayerScope>()!.notifier!;
}
