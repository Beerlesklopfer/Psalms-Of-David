// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import '../server/api_client.dart' show ApiException;

/// What the device did while the server was out of reach, to be sent when it is back (`docs/spec-app-api.md`, "offline mode and synchronisation").
/// The server stays the one who decides: a change that it refuses is dropped and reported.
class OutboxChange {
  const OutboxChange({required this.seq, required this.kind, required this.data, required this.at});

  /// Labels of a book, a title, a podcast or a station, as a delta (`scope`, `id`, `part`, `add`, `remove`).
  static const labels = 'labels';

  /// A book hidden or shown again (`id`, `hidden`).
  static const hideBook = 'hide_book';

  /// An author hidden or shown again (`name`, `kind`, `hidden`).
  static const hideAuthor = 'hide_author';

  /// Where the listening to a book is (`item`, `part`, `position_ms`, `finished`, `at`).
  static const progress = 'progress';

  /// The order in which the changes were made; they are sent in this order.
  final int seq;
  final String kind;
  final Map<String, Object?> data;

  /// When the change was made (Unix milliseconds on this device).
  final int at;

  /// A name for the person: the title of the book or the author, for the report of a refused change.
  String get title => (data['title'] as String?) ?? '';

  List<String> get add => [for (final a in (data['add'] as List? ?? const [])) a as String];
  List<String> get remove => [for (final a in (data['remove'] as List? ?? const [])) a as String];

  /// What a change is about: a second change of the same thing is merged into this one.
  String get key => switch (kind) {
    labels => 'labels:${data['scope']}:${data['id']}:${data['part'] ?? ''}',
    hideBook => 'hide_book:${data['id']}',
    hideAuthor => 'hide_author:${(data['name'] as String).toLowerCase()}:${data['kind'] ?? ''}',
    progress => 'progress:${data['item']}',
    _ => '$kind:$seq',
  };

  Map<String, Object?> toJson() => {'seq': seq, 'kind': kind, 'data': data, 'at': at};

  static OutboxChange? fromJson(Object? j) {
    if (j is! Map || j['seq'] is! int || j['kind'] is! String || j['data'] is! Map || j['at'] is! int) return null;
    return OutboxChange(seq: j['seq'] as int, kind: j['kind'] as String, data: Map<String, Object?>.from(j['data'] as Map), at: j['at'] as int);
  }
}

/// A change that the server did not take (403, 404, 409, 422): it is dropped, and the person is told.
class RejectedChange {
  const RejectedChange(this.change, this.status, this.code);

  final OutboxChange change;
  final int status;
  final String code;
}

/// What a round of sending came to.
class SyncResult {
  const SyncResult({required this.sent, required this.rejected, required this.complete});

  /// How many changes the server took.
  final int sent;
  final List<RejectedChange> rejected;

  /// False if the sending stopped because the server could not be reached (the rest stays).
  final bool complete;
}

/// What the server offers for the replay of the changes. A change may be sent twice without harm (the server's state is set, not counted).
abstract class OutboxApi {
  /// `scope` is `book`, `part`, `podcast` or `station`.
  Future<void> labels(String scope, int id, {int? part, required List<String> add, required List<String> remove});

  Future<void> hideBook(int id, bool hidden);

  Future<void> hideAuthor(String name, String kind, bool hidden);

  /// [at] is when the listening happened: the server keeps the newer state, so an old report cannot overwrite a newer one.
  Future<void> progress({required int item, required int part, required int positionMs, required bool finished, required int at});
}

/// The changes waiting for the server, kept in a file so that they survive a restart of the app. Changes of the same thing are merged:
/// two label changes become one delta, the last state of a book hidden or shown wins, and of the listening only the newest position stays.
class Outbox {
  Outbox(this.file) {
    _load();
  }

  /// Not kept anywhere (tests, the demo).
  Outbox.memory() : file = null;

  final File? file;
  final List<OutboxChange> _entries = [];
  int _seq = 1;

  List<OutboxChange> get entries => List.unmodifiable(_entries);
  bool get isEmpty => _entries.isEmpty;
  int get length => _entries.length;

  /// The changes that the person made and wants to hear about when they are refused: everything but the listening position.
  int get visibleLength => _entries.where((e) => e.kind != OutboxChange.progress).length;

  void _load() {
    final f = file;
    if (f == null) return;
    try {
      final j = jsonDecode(f.readAsStringSync());
      if (j is List) {
        for (final e in j) {
          final c = OutboxChange.fromJson(e);
          if (c != null) _entries.add(c);
        }
        _entries.sort((a, b) => a.seq.compareTo(b.seq));
        _seq = _entries.isEmpty ? 1 : _entries.last.seq + 1;
      }
    } on Object {
      // Missing or damaged: nothing waits.
    }
  }

  void _save() {
    final f = file;
    if (f == null) return;
    try {
      f.parent.createSync(recursive: true);
      final temp = File('${f.path}.tmp');
      temp.writeAsStringSync(jsonEncode([for (final e in _entries) e.toJson()]));
      temp.renameSync(f.path);
    } on FileSystemException {
      // The change is still in memory; the next one tries again.
    }
  }

  /// Forgets every change (the pairing with the server was forgotten: they belong to the account that is gone).
  void clear() {
    _entries.clear();
    _save();
  }

  OutboxChange? _find(String key) => _entries.where((e) => e.key == key).firstOrNull;

  /// Puts [change] at the end; an earlier change of the same thing goes.
  void _replace(String key, String kind, Map<String, Object?> data, int at) {
    _entries.removeWhere((e) => e.key == key);
    _entries.add(OutboxChange(seq: _seq++, kind: kind, data: data, at: at));
    _save();
  }

  static String _fold(String s) => s.toLowerCase();

  static List<String> _unique(Iterable<String> words) {
    final seen = <String>{};
    return [for (final w in words) if (seen.add(_fold(w))) w];
  }

  /// A label change. With an earlier one of the same thing the two are merged into one delta: for each word the last thing that was done to it
  /// counts (added and then taken off again is "taken off"; the server takes both as a state, so sending a word that is there or gone already is harmless).
  void addLabels(String scope, int id, {int? part, List<String> add = const [], List<String> remove = const [], String title = '', int? at}) {
    final probe = OutboxChange(seq: 0, kind: OutboxChange.labels, data: {'scope': scope, 'id': id, 'part': part}, at: 0);
    final old = _find(probe.key);
    final newAdd = _unique(add), newRemove = _unique(remove);
    var nextAdd = newAdd, nextRemove = newRemove;
    if (old != null) {
      final goneNow = {for (final r in newRemove) _fold(r)};
      final addedNow = {for (final a in newAdd) _fold(a)};
      nextAdd = _unique([...old.add.where((a) => !goneNow.contains(_fold(a))), ...newAdd]);
      nextRemove = _unique([...old.remove.where((r) => !addedNow.contains(_fold(r))), ...newRemove]);
    }
    if (nextAdd.isEmpty && nextRemove.isEmpty) {
      _entries.removeWhere((e) => e.key == probe.key);
      _save();
      return;
    }
    // The place in the order stays with the first change of this thing: the later ones are only more of it.
    final data = {'scope': scope, 'id': id, 'part': ?part, 'add': nextAdd, 'remove': nextRemove, 'title': title.isNotEmpty ? title : (old?.title ?? '')};
    if (old == null) {
      _entries.add(OutboxChange(seq: _seq++, kind: OutboxChange.labels, data: data, at: at ?? DateTime.now().millisecondsSinceEpoch));
    } else {
      final i = _entries.indexOf(old);
      _entries[i] = OutboxChange(seq: old.seq, kind: OutboxChange.labels, data: data, at: at ?? DateTime.now().millisecondsSinceEpoch);
    }
    _save();
  }

  /// A book hidden or shown again: the last state counts.
  void addHideBook(int id, bool hidden, {String title = '', int? at}) {
    _replace('hide_book:$id', OutboxChange.hideBook, {'id': id, 'hidden': hidden, 'title': title}, at ?? DateTime.now().millisecondsSinceEpoch);
  }

  /// An author hidden or shown again ([kind] is `audiobook`, `music` or empty for both).
  void addHideAuthor(String name, String kind, bool hidden, {int? at}) {
    _replace('hide_author:${_fold(name)}:$kind', OutboxChange.hideAuthor, {'name': name, 'kind': kind, 'hidden': hidden, 'title': name}, at ?? DateTime.now().millisecondsSinceEpoch);
  }

  /// Where the listening to a book is: only the newest position of a book is kept.
  void addProgress({required int item, required int part, required int positionMs, required bool finished, int? at}) {
    final when = at ?? DateTime.now().millisecondsSinceEpoch;
    final old = _find('progress:$item');
    if (old != null && old.at > when) return;
    _replace('progress:$item', OutboxChange.progress, {'item': item, 'part': part, 'position_ms': positionMs, 'finished': finished, 'at': when}, when);
  }

  /// Sends the changes in the order in which they were made. What the server takes (or refuses with a final answer) leaves the list; when the
  /// server cannot be reached the rest stays and the next round goes on from there.
  Future<SyncResult> replay(OutboxApi api) async {
    var sent = 0;
    final rejected = <RejectedChange>[];
    for (final change in List.of(_entries)) {
      try {
        await _send(api, change);
        sent++;
      } on ApiException catch (e) {
        // No answer, an answer of the server that is not up (5xx), too many requests, or a token that does not count (401): try again later.
        if (e.status == 0 || e.status >= 500 || e.status == 429 || e.status == 401) {
          return SyncResult(sent: sent, rejected: rejected, complete: false);
        }
        rejected.add(RejectedChange(change, e.status, e.code));
      }
      _entries.removeWhere((e) => e.seq == change.seq && e.at == change.at && e.key == change.key);
      _save();
    }
    return SyncResult(sent: sent, rejected: rejected, complete: true);
  }

  Future<void> _send(OutboxApi api, OutboxChange c) {
    final d = c.data;
    return switch (c.kind) {
      OutboxChange.labels => api.labels(d['scope'] as String, d['id'] as int, part: d['part'] as int?, add: c.add, remove: c.remove),
      OutboxChange.hideBook => api.hideBook(d['id'] as int, d['hidden'] == true),
      OutboxChange.hideAuthor => api.hideAuthor(d['name'] as String, (d['kind'] as String?) ?? '', d['hidden'] == true),
      OutboxChange.progress => api.progress(item: d['item'] as int, part: d['part'] as int, positionMs: d['position_ms'] as int, finished: d['finished'] == true, at: d['at'] as int),
      _ => Future.value(),
    };
  }
}
