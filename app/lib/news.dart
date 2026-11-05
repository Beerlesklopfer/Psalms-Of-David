// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'inbox/inbox_notifier.dart';
import 'l10n/app_localizations.dart';

/// One new medium: an album or audiobook (`id` is the item), an episode of a podcast it follows, a station of its list.
class NewsEntry {
  const NewsEntry({required this.kind, required this.id, required this.title, this.from = ''});

  /// `music`, `audiobook`, `episode` or `station`.
  final String kind;
  final int id;
  final String title;

  /// The artist or author of an album, the podcast of an episode; empty for a station.
  final String from;
}

/// The answer of `GET /api/v1/news`: what became new since the last look, and the time to come back with.
class NewsAnswer {
  const NewsAnswer(this.now, this.entries);

  final int now;
  final List<NewsEntry> entries;

  factory NewsAnswer.fromJson(Map<String, dynamic> j) {
    List<Map<String, dynamic>> list(String key) => [for (final e in (j[key] as List? ?? const [])) if (e is Map<String, dynamic>) e];
    return NewsAnswer((j['now'] as num?)?.toInt() ?? 0, [
      for (final i in list('items'))
        NewsEntry(kind: i['kind'] == 'audiobook' ? 'audiobook' : 'music', id: (i['id'] as num).toInt(), title: '${i['title'] ?? ''}', from: '${i['creator'] ?? ''}'),
      for (final e in list('episodes'))
        NewsEntry(kind: 'episode', id: (e['id'] as num).toInt(), title: '${e['title'] ?? ''}', from: '${e['podcast'] ?? ''}'),
      for (final s in list('stations')) NewsEntry(kind: 'station', id: (s['id'] as num).toInt(), title: '${s['name'] ?? ''}'),
    ]);
  }
}

/// The server (`GET /api/v1/news?since=`); [since] null is the first look of this device.
abstract class NewsApi {
  Future<NewsAnswer> news(int? since);
}

/// Tells the family about new media with a notification of the device: albums and audiobooks, episodes of the podcasts the profile follows,
/// stations of its list, only what the profile may see (the server decides). It asks David while the app runs (now, every few minutes, when
/// the app comes back to the front): only in the home network, no push service (owner, 2026-11-05). The first look of a device tells nothing
/// old; the time of the last look is kept in [cursorFile]. A tap on the notification of an album asks to open it ([openItem]).
class NewsWatcher {
  NewsWatcher({required this.api, required this.notifier, required this.cursorFile, this.interval = const Duration(minutes: 5)}) {
    _sub = notifier.events.listen((e) {
      final item = _albums[e.id];
      if (item != null) openItem.value = item;
    });
  }

  final NewsApi api;
  final InboxNotifier notifier;
  final File cursorFile;
  final Duration interval;

  /// The words of the notifications, in the language of the app (set by the shell, as for the inbox).
  AppLocalizations? l10n;

  /// The item whose notification was tapped; the shell opens it.
  final ValueNotifier<int?> openItem = ValueNotifier(null);

  /// Notifications of albums by their (negative) id: the inbox has the positive ones.
  final Map<int, int> _albums = {};
  int _nextId = -1;
  Timer? _timer;
  StreamSubscription<InboxNotifierEvent>? _sub;
  bool _busy = false;

  /// At most this many single notifications at once; more become one that counts them.
  static const maxSingle = 4;

  void start() {
    unawaited(check());
    _timer = Timer.periodic(interval, (_) => unawaited(check()));
  }

  int? _readCursor() {
    try {
      return int.tryParse(cursorFile.readAsStringSync().trim());
    } on Object {
      return null;
    }
  }

  /// Asks the server and tells what is new. Fails quietly: away from home, or without the server, there is simply no news.
  Future<void> check() async {
    if (_busy) return;
    _busy = true;
    try {
      final since = _readCursor();
      final answer = await api.news(since);
      if (answer.now > 0) {
        try {
          cursorFile.parent.createSync(recursive: true);
          cursorFile.writeAsStringSync('${answer.now}');
        } on Object {
          // the next look tells it again
        }
      }
      if (since != null) await _tell(answer.entries);
    } on Object {
      // no server, no news
    } finally {
      _busy = false;
    }
  }

  Future<void> _tell(List<NewsEntry> entries) async {
    final l = l10n;
    if (l == null || entries.isEmpty) return;
    String head(NewsEntry e) => switch (e.kind) {
      'audiobook' => l.newsAudiobook,
      'episode' => l.newsEpisode(e.from),
      'station' => l.newsStation,
      _ => l.newsAlbum,
    };
    String body(NewsEntry e) => e.kind == 'episode' || e.from.isEmpty ? e.title : '${e.title} · ${e.from}';
    if (entries.length > maxSingle) {
      await notifier.show(InboxNotice(id: _nextId--, title: l.newsMany(entries.length), body: entries.take(5).map((e) => e.title).join(', ')));
      return;
    }
    for (final e in entries) {
      final id = _nextId--;
      if (e.kind == 'music' || e.kind == 'audiobook') _albums[id] = e.id;
      await notifier.show(InboxNotice(id: id, title: head(e), body: body(e)));
    }
  }

  void dispose() {
    _timer?.cancel();
    unawaited(_sub?.cancel());
    openItem.dispose();
  }
}
