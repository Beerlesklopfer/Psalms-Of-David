// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';

import 'audio/file_formats.dart';
import 'podcasts.dart';
import 'podcasts/download_machine.dart' as machine;
import 'server/app_paths.dart';

/// The file of an episode in the folder of the downloads, named by its address: an episode that was played from the publisher is kept there
/// too (the player fetches it once), so "played" and "downloaded" are the same thing for the files.
File episodeCacheFile(Directory cache, String url) => File('${cache.path}/ep-${sha1.convert(utf8.encode(url))}.${episodeFileEnding(url)}');

/// Only http and https addresses are fetched: an episode address comes from a feed of the internet.
bool isWebAddress(String url) {
  final u = Uri.tryParse(url.trim());
  return u != null && u.hasAuthority && u.host.isNotEmpty && (u.scheme == 'http' || u.scheme == 'https');
}

/// Why a download stopped: `scheme` (not an http or https address), `http` (the publisher answered with an error), `too_large` (more than the
/// limit of the settings), `network` (no connection, a certificate that does not count), `cancelled`, `io` (the file could not be written).
class EpisodeDownloadException implements Exception {
  const EpisodeDownloadException(this.code, [this.message = '']);

  final String code;
  final String message;

  @override
  String toString() => 'EpisodeDownloadException($code${message.isEmpty ? '' : ': $message'})';
}

/// Stops a running download.
class CancelToken {
  bool _cancelled = false;
  bool get isCancelled => _cancelled;
  void cancel() => _cancelled = true;
}

/// Fetches a file from the internet; [HttpEpisodeFetcher] is the real one, tests use a fake.
abstract class EpisodeFetcher {
  /// Loads [url] into [dest] (through a `.part` file, so that a half-loaded file is never played) and returns the size. Stops with
  /// [EpisodeDownloadException] and leaves no file behind when the limit [maxBytes] is passed, the token is cancelled or the transfer fails.
  Future<int> fetch(Uri url, File dest, {required int maxBytes, required CancelToken cancel, void Function(int received, int? total)? onProgress});
}

/// The plain HTTPS client: the publisher's own certificate rules apply (this is no connection to David), redirects are followed (publishers
/// send episodes through trackers and content networks).
class HttpEpisodeFetcher implements EpisodeFetcher {
  HttpEpisodeFetcher({HttpClient Function()? client}) : _client = client ?? (() => HttpClient()..connectionTimeout = const Duration(seconds: 20));

  final HttpClient Function() _client;

  @override
  Future<int> fetch(Uri url, File dest, {required int maxBytes, required CancelToken cancel, void Function(int received, int? total)? onProgress}) async {
    if (url.scheme != 'http' && url.scheme != 'https') throw const EpisodeDownloadException('scheme');
    final client = _client();
    final part = File('${dest.path}.part');
    try {
      final req = await client.getUrl(url);
      final res = await req.close();
      if (res.statusCode >= 400) throw EpisodeDownloadException('http', 'HTTP ${res.statusCode}');
      final total = res.contentLength >= 0 ? res.contentLength : null;
      if (total != null && total > maxBytes) throw const EpisodeDownloadException('too_large');
      await dest.parent.create(recursive: true);
      final sink = part.openWrite();
      var received = 0;
      try {
        await for (final chunk in res) {
          if (cancel.isCancelled) throw const EpisodeDownloadException('cancelled');
          received += chunk.length;
          if (received > maxBytes) throw const EpisodeDownloadException('too_large');
          sink.add(chunk);
          onProgress?.call(received, total);
        }
        await sink.close();
      } on Object {
        try {
          await sink.close();
        } on Object {
          // closing a sink that failed
        }
        rethrow;
      }
      await part.rename(dest.path);
      return received;
    } on EpisodeDownloadException {
      _dropPart(part);
      rethrow;
    } on FileSystemException catch (e) {
      _dropPart(part);
      throw EpisodeDownloadException('io', e.message);
    } on Object catch (e) {
      // No connection, a certificate that does not count, a timeout.
      _dropPart(part);
      throw EpisodeDownloadException('network', '$e');
    } finally {
      client.close(force: true);
    }
  }

  void _dropPart(File part) {
    try {
      if (part.existsSync()) part.deleteSync();
    } on FileSystemException {
      // left over; the next start cleans it up
    }
  }
}

/// An episode that is on the device, with what the lists need to show it without the server.
class DownloadedEpisode {
  const DownloadedEpisode({
    required this.episodeId,
    required this.podcastId,
    required this.podcastTitle,
    required this.title,
    required this.url,
    required this.durationS,
    required this.bytes,
    this.publishedMs,
    this.positionMs = 0,
    this.finished = false,
  });

  factory DownloadedEpisode.fromJson(Map<String, dynamic> j) => DownloadedEpisode(
        episodeId: j['episode_id'] as int,
        podcastId: j['podcast_id'] as int,
        podcastTitle: j['podcast_title'] as String? ?? '',
        title: j['title'] as String? ?? '',
        url: j['url'] as String,
        durationS: j['duration_s'] as int? ?? 0,
        bytes: j['bytes'] as int? ?? 0,
        publishedMs: j['published_ms'] as int?,
        positionMs: j['position_ms'] as int? ?? 0,
        finished: j['finished'] as bool? ?? false,
      );

  final int episodeId;
  final int podcastId;
  final String podcastTitle;
  final String title;
  final String url;
  final int durationS;
  final int bytes;
  final int? publishedMs;

  /// Where this device is in the episode (the account's own state, kept up to date by the player; the server has the same).
  final int positionMs;
  final bool finished;

  DownloadedEpisode copyWith({String? podcastTitle, String? title, int? durationS, int? bytes, int? publishedMs, int? positionMs, bool? finished}) => DownloadedEpisode(
        episodeId: episodeId,
        podcastId: podcastId,
        podcastTitle: podcastTitle ?? this.podcastTitle,
        title: title ?? this.title,
        url: url,
        durationS: durationS ?? this.durationS,
        bytes: bytes ?? this.bytes,
        publishedMs: publishedMs ?? this.publishedMs,
        positionMs: positionMs ?? this.positionMs,
        finished: finished ?? this.finished,
      );

  Map<String, dynamic> toJson() => {
        'episode_id': episodeId,
        'podcast_id': podcastId,
        'podcast_title': podcastTitle,
        'title': title,
        'url': url,
        'duration_s': durationS,
        'bytes': bytes,
        'published_ms': publishedMs,
        'position_ms': positionMs,
        'finished': finished,
      };

  /// The episode as the list shows it.
  Episode toEpisode() => Episode(
        id: episodeId,
        title: title,
        audioUrl: url,
        published: publishedMs == null ? null : DateTime.fromMillisecondsSinceEpoch(publishedMs!),
        duration: Duration(seconds: durationS),
        position: Duration(milliseconds: positionMs),
        finished: finished,
      );
}

/// A podcast with episodes on the device.
class DownloadedPodcast {
  const DownloadedPodcast({required this.id, required this.title, required this.episodes, required this.bytes});

  final int id;
  final String title;
  final int episodes;
  final int bytes;
}

/// The size limits of one download that the settings offer, in megabytes.
const kEpisodeSizeChoices = [100, 250, 500, 1000, 2000];

/// How many of the newest unheard episodes "download the newest" fetches.
const kNewestEpisodes = 5;

/// The podcast episodes that are kept on the device for listening without the server (`docs/spec-app-api.md`, "podcast downloads"). The episodes
/// are fetched from the publisher (the server does not carry them) into the folder of the other downloads; a small file next to the pairing says
/// which episodes they are, so the offline view can list them. The files are the truth: an entry whose file is gone is dropped.
class PodcastDownloads extends ChangeNotifier {
  PodcastDownloads({required this.cache, this.file, EpisodeFetcher? fetcher}) : fetcher = fetcher ?? HttpEpisodeFetcher() {
    _load();
    _dropPartials();
  }

  factory PodcastDownloads.standard() => PodcastDownloads(cache: AppPaths.parts, file: File('${AppPaths.base.path}/podcast-downloads.json'));

  final Directory cache;

  /// Where the list and the settings are kept; null for tests and the demo.
  final File? file;
  final EpisodeFetcher fetcher;

  final Map<int, DownloadedEpisode> _entries = {};

  /// The state of each episode that is not [machine.NotDownloaded] (the machine in podcasts/download_machine.dart); an episode that is not
  /// here is not downloaded. `_entries` holds the data of the [machine.Downloaded] ones.
  final Map<int, machine.DownloadState> _states = {};

  /// The token of each running transfer (a resource of the shell, not a state).
  final Map<int, CancelToken> _tokens = {};
  final Map<int, DownloadedEpisode> _recordFor = {};
  DateTime _lastNotified = DateTime.fromMillisecondsSinceEpoch(0);
  bool _disposed = false;

  /// Deletes an episode once it was heard to the end. Off by default: nothing is deleted without the person asking for it.
  bool deleteAfterListening = false;

  /// The most one download may take, in megabytes.
  int maxMegabytes = 500;

  File _fileOfUrl(String url) => episodeCacheFile(cache, url);

  void _load() {
    final f = file;
    if (f == null) return;
    try {
      final j = jsonDecode(f.readAsStringSync()) as Map<String, dynamic>;
      final s = j['settings'];
      if (s is Map) {
        deleteAfterListening = s['delete_after_listening'] == true;
        final mb = s['max_megabytes'];
        if (mb is int && mb >= 10) maxMegabytes = mb;
      }
      for (final e in (j['episodes'] as List? ?? const [])) {
        try {
          final d = DownloadedEpisode.fromJson(Map<String, dynamic>.from(e as Map));
          _entries[d.episodeId] = d;
          _states[d.episodeId] = const machine.Downloaded();
        } on Object {
          // a damaged entry is left out
        }
      }
    } on Object {
      // Missing or damaged: nothing is listed; the files are still there and come back with the next look at the episodes.
    }
  }

  void _save() {
    final f = file;
    if (f == null) return;
    try {
      f.parent.createSync(recursive: true);
      final temp = File('${f.path}.tmp');
      temp.writeAsStringSync(jsonEncode({
        'settings': {'delete_after_listening': deleteAfterListening, 'max_megabytes': maxMegabytes},
        'episodes': [for (final e in _entries.values) e.toJson()],
      }));
      temp.renameSync(f.path);
    } on FileSystemException {
      // The list is in memory; the next change tries again.
    }
  }

  /// A half-loaded file of an earlier run (the app was closed during a download) is of no use.
  void _dropPartials() {
    try {
      if (!cache.existsSync()) return;
      for (final e in cache.listSync()) {
        if (e is File && e.path.endsWith('.part') && e.uri.pathSegments.last.startsWith('ep-')) e.deleteSync();
      }
    } on FileSystemException {
      // left over
    }
  }

  void _notify({bool force = true}) {
    if (_disposed) return;
    final now = DateTime.now();
    if (!force && now.difference(_lastNotified) < const Duration(milliseconds: 250)) return;
    _lastNotified = now;
    notifyListeners();
  }

  /// Drops the entries whose file is gone (deleted by hand or with "delete all downloads").
  void _validate() {
    final gone = [for (final e in _entries.values) if (!_fileOfUrl(e.url).existsSync()) e.episodeId];
    if (gone.isEmpty) return;
    for (final id in gone) {
      _entries.remove(id);
      _send(id, const machine.FileGone());
    }
    _save();
  }

  // ---- What is on the device ------------------------------------------------------------------

  /// Is the file of this episode on the device?
  bool isDownloaded(String url) => _fileOfUrl(url).existsSync();

  DownloadedEpisode? entryOf(int episodeId) {
    _validate();
    return _entries[episodeId];
  }

  /// The size of a downloaded episode in bytes, or null.
  int? sizeOf(int episodeId) => entryOf(episodeId)?.bytes;

  /// Every downloaded episode, by podcast and then the newest first.
  List<DownloadedEpisode> get entries {
    _validate();
    final list = _entries.values.toList();
    list.sort((a, b) {
      final p = a.podcastTitle.toLowerCase().compareTo(b.podcastTitle.toLowerCase());
      return p != 0 ? p : (b.publishedMs ?? 0).compareTo(a.publishedMs ?? 0);
    });
    return list;
  }

  List<DownloadedEpisode> episodesOf(int podcastId) => [for (final e in entries) if (e.podcastId == podcastId) e];

  /// The podcasts with at least one episode on the device.
  List<DownloadedPodcast> get podcasts {
    final byId = <int, DownloadedPodcast>{};
    for (final e in entries) {
      final old = byId[e.podcastId];
      byId[e.podcastId] = DownloadedPodcast(id: e.podcastId, title: e.podcastTitle, episodes: (old?.episodes ?? 0) + 1, bytes: (old?.bytes ?? 0) + e.bytes);
    }
    final out = byId.values.toList()..sort((a, b) => a.title.toLowerCase().compareTo(b.title.toLowerCase()));
    return out;
  }

  /// How many episodes are on the device and what they take.
  ({int files, int bytes}) usage() {
    final all = entries;
    return (files: all.length, bytes: all.fold(0, (n, e) => n + e.bytes));
  }

  // ---- Download -------------------------------------------------------------------------------

  /// The state of the life cycle of an episode.
  machine.DownloadState stateOf(int episodeId) => _states[episodeId] ?? const machine.NotDownloaded();

  ({int received, int? total})? progressOf(int episodeId) => switch (stateOf(episodeId)) {
        machine.Queued() => (received: 0, total: null),
        machine.Downloading(:final received, :final total) || machine.Cancelling(:final received, :final total) => (received: received, total: total),
        _ => null,
      };

  bool isDownloading(int episodeId) => machine.isActive(stateOf(episodeId));

  /// Feeds [event] to the machine for [episodeId] and carries out the effects that finish at once; returns the ones that take time.
  List<machine.DownloadEffect> _send(int episodeId, machine.DownloadEvent event) {
    final (next, effects) = machine.transition(stateOf(episodeId), event);
    if (next is machine.NotDownloaded) {
      _states.remove(episodeId);
    } else {
      _states[episodeId] = next;
    }
    final slow = <machine.DownloadEffect>[];
    for (final fx in effects) {
      switch (fx) {
        case machine.StartFetch():
          slow.add(fx);
        case machine.CancelFetch():
          _tokens[episodeId]?.cancel();
        case machine.Register():
          final record = _recordFor[episodeId];
          if (record != null) _entries[episodeId] = record;
          _save();
        case machine.DeleteFile():
          slow.add(fx);
        case machine.Notify(:final force):
          _notify(force: force);
      }
    }
    return slow;
  }

  /// Fetches [e] of [podcast]. Returns null when the episode is on the device afterwards, or the code of why not (see [EpisodeDownloadException]).
  Future<String?> download(Episode e, Podcast podcast) async {
    if (isDownloading(e.id)) return null;
    final url = e.audioUrl.trim();
    if (!isWebAddress(url)) return 'scheme';
    final dest = _fileOfUrl(url);
    final onDisk = dest.existsSync();
    if (!onDisk && stateOf(e.id) is machine.Downloaded) {
      // The entry outlived its file.
      _entries.remove(e.id);
      _send(e.id, const machine.FileGone());
    }
    _recordFor[e.id] = _record(e, podcast, onDisk ? dest.lengthSync() : 0);
    final token = CancelToken();
    _tokens[e.id] = token;
    final todo = _send(e.id, machine.Download(webAddress: true, onDisk: onDisk));
    if (!todo.any((fx) => fx is machine.StartFetch)) {
      _tokens.remove(e.id);
      _recordFor.remove(e.id);
      return null;
    }
    _send(e.id, const machine.FetchStarted());
    try {
      final bytes = await fetcher.fetch(
        Uri.parse(url),
        dest,
        maxBytes: maxMegabytes * 1024 * 1024,
        cancel: token,
        onProgress: (received, total) => _send(e.id, machine.Progress(received, total)),
      );
      _recordFor[e.id] = _record(e, podcast, bytes);
      _send(e.id, machine.Done(bytes));
      return null;
    } on EpisodeDownloadException catch (err) {
      _send(e.id, machine.IoError(err.code));
      return err.code;
    } on FileSystemException {
      _send(e.id, const machine.IoError('io'));
      return 'io';
    } finally {
      _tokens.remove(e.id);
      _recordFor.remove(e.id);
    }
  }

  DownloadedEpisode _record(Episode e, Podcast podcast, int bytes) => DownloadedEpisode(
        episodeId: e.id,
        podcastId: podcast.id,
        podcastTitle: podcast.title,
        title: e.title,
        url: e.audioUrl.trim(),
        durationS: e.duration.inSeconds,
        bytes: bytes,
        publishedMs: e.published?.millisecondsSinceEpoch,
        positionMs: e.position.inMilliseconds,
        finished: e.finished,
      );

  /// Stops a running download; the half-loaded file goes.
  void cancel(int episodeId) => _send(episodeId, const machine.Cancel());

  /// The unheard episodes of [list] that are not on the device and not on their way, the newest first, at most [limit].
  List<Episode> newestToFetch(List<Episode> list, {int limit = kNewestEpisodes}) {
    final todo = [for (final e in list) if (!e.finished && !isDownloading(e.id) && !isDownloaded(e.audioUrl)) e];
    todo.sort((a, b) => (b.published ?? DateTime(1970)).compareTo(a.published ?? DateTime(1970)));
    return todo.take(limit).toList();
  }

  // ---- Removing -------------------------------------------------------------------------------

  /// Deletes an episode from the device. The podcast keeps it; it can be fetched again.
  Future<void> remove(int episodeId) async {
    _carryOut(episodeId, _send(episodeId, const machine.Delete()));
    _notify();
  }

  /// The slow effects of [_send] that finish at once: removes the file and the entry, and answers with [machine.Deleted].
  void _carryOut(int episodeId, List<machine.DownloadEffect> todo) {
    if (!todo.any((fx) => fx is machine.DeleteFile)) return;
    final e = _entries.remove(episodeId);
    if (e != null) {
      final f = _fileOfUrl(e.url);
      try {
        if (f.existsSync()) f.deleteSync();
      } on FileSystemException {
        // already gone
      }
      _save();
    }
    _send(episodeId, const machine.Deleted());
  }

  /// Forgets which episodes these are (the pairing with the server was forgotten: the list belongs to the account that is gone). The files stay,
  /// like the other downloads; they come back into the list when the episodes are listed again, and "delete all" removes them.
  void forgetList() {
    for (final id in _entries.keys.toList()) {
      _send(id, const machine.FileGone());
    }
    _entries.clear();
    _save();
    _notify();
  }

  /// Deletes every episode on the device, also those that were only kept because they were played (the files `ep-*`).
  Future<void> removeAll() async {
    for (final id in _tokens.keys.toList()) {
      _send(id, const machine.Cancel());
    }
    for (final id in _entries.keys.toList()) {
      _send(id, const machine.FileGone());
    }
    _entries.clear();
    try {
      if (cache.existsSync()) {
        for (final e in cache.listSync()) {
          if (e is File && e.uri.pathSegments.last.startsWith('ep-')) e.deleteSync();
        }
      }
    } on FileSystemException {
      // what is left is dropped by the next look
    }
    _save();
    _notify();
  }

  // ---- State and the lists ----------------------------------------------------------------------

  /// The player reports where the listening is: the offline list shows it, and with "delete after listening" a heard episode goes.
  void noteState(int episodeId, Duration position, bool finished) {
    final e = _entries[episodeId];
    if (e == null) return;
    if (finished && deleteAfterListening) {
      _carryOut(episodeId, _send(episodeId, machine.DeleteAfterListening(enabled: deleteAfterListening)));
      _notify();
      return;
    }
    _entries[episodeId] = e.copyWith(positionMs: position.inMilliseconds, finished: finished);
    _save();
    _notify();
  }

  /// The episodes of [podcast] as the server lists them. What is on the device from playing gets its entry (so the offline view lists it),
  /// the state of the account is taken over, and with "delete after listening" the heard episodes go, and those that the podcast does not list
  /// any more (an episode that is newer than the oldest one of the list but missing: the publisher took it out).
  Future<void> noteListed(Podcast podcast, List<Episode> list) async {
    var changed = false;
    final listed = {for (final e in list) e.id};
    for (final e in list) {
      final f = _fileOfUrl(e.audioUrl.trim());
      if (!f.existsSync()) continue;
      final old = _entries[e.id];
      final now = DownloadedEpisode(
        episodeId: e.id,
        podcastId: podcast.id,
        podcastTitle: podcast.title,
        title: e.title,
        url: e.audioUrl.trim(),
        durationS: e.duration.inSeconds,
        bytes: old?.bytes ?? f.lengthSync(),
        publishedMs: e.published?.millisecondsSinceEpoch,
        positionMs: e.position.inMilliseconds,
        finished: e.finished,
      );
      if (old == null || old.toJson().toString() != now.toJson().toString()) {
        _entries[e.id] = now;
        _send(e.id, const machine.FileFound());
        changed = true;
      }
    }
    if (deleteAfterListening) {
      final dates = [for (final e in list) if (e.published != null) e.published!.millisecondsSinceEpoch];
      final oldestListed = dates.isEmpty ? null : dates.reduce((a, b) => a < b ? a : b);
      for (final d in _entries.values.where((d) => d.podcastId == podcast.id).toList()) {
        final heard = d.finished;
        final taken = !listed.contains(d.episodeId) && oldestListed != null && (d.publishedMs ?? 0) >= oldestListed;
        if (heard || taken) {
          await remove(d.episodeId);
          changed = false;
        }
      }
    }
    if (changed) {
      _save();
      _notify();
    }
  }

  // ---- Settings -------------------------------------------------------------------------------

  void setDeleteAfterListening(bool value) {
    if (deleteAfterListening == value) return;
    deleteAfterListening = value;
    _save();
    _notify();
  }

  void setMaxMegabytes(int value) {
    if (maxMegabytes == value || value < 10) return;
    maxMegabytes = value;
    _save();
    _notify();
  }

  @override
  void dispose() {
    _disposed = true;
    for (final id in _tokens.keys.toList()) {
      _send(id, const machine.Cancel());
    }
    super.dispose();
  }
}
