// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../server/api_client.dart';
import '../server/app_paths.dart';
import 'ebook.dart';
import 'ebook_api.dart';

/// The e-books on this device: `<app folder>/ebooks/`, the last list of the server (`list.json`, for reading offline), the places read
/// (`progress.json`, with the ones the server has not heard yet), and one folder per e-book loaded onto the device: `meta.json` (which
/// version), `text.json` and `pictures/` of an EPUB, `book.pdf` of a PDF.
class EbookShelf {
  EbookShelf({Directory? root}) : root = root ?? Directory('${AppPaths.base.path}/ebooks');

  final Directory root;

  Directory dirOf(int id) => Directory('${root.path}/$id');
  File _meta(int id) => File('${dirOf(id).path}/meta.json');
  File textOf(int id) => File('${dirOf(id).path}/text.json');
  File pdfOf(int id) => File('${dirOf(id).path}/book.pdf');
  File coverOf(int id) => File('${dirOf(id).path}/cover');

  /// The file of a picture of the text: its name inside the EPUB, made safe as a file name.
  File pictureOf(int id, String name) => File('${dirOf(id).path}/pictures/${base64Url.encode(utf8.encode(name))}');

  /// The version of the copy of [id] on this device (`mtime_ms` of the server), or null when there is none.
  int? keptVersion(int id) {
    final f = _meta(id);
    if (!f.existsSync()) return null;
    try {
      final j = jsonDecode(f.readAsStringSync());
      return j is Map<String, dynamic> && j['mtime_ms'] is int ? j['mtime_ms'] as int : 0;
    } on Object {
      return null;
    }
  }

  /// Marks a copy as whole: written last, after every file of it.
  Future<void> markKept(Ebook e) async => _meta(e.id).writeAsString(jsonEncode(e.toJson()));

  Future<void> remove(int id) async {
    final d = dirOf(id);
    if (d.existsSync()) await d.delete(recursive: true);
  }

  Future<List<Ebook>?> readList() async {
    final f = File('${root.path}/list.json');
    if (!f.existsSync()) return null;
    try {
      final j = jsonDecode(await f.readAsString());
      return j is List ? [for (final e in j) ?Ebook.fromJson(e)] : null;
    } on Object {
      return null;
    }
  }

  Future<void> writeList(List<Ebook> list) async {
    await root.create(recursive: true);
    await File('${root.path}/list.json').writeAsString(jsonEncode([for (final e in list) e.toJson()]));
  }

  /// The places read on this device, and which of them still have to go to the server.
  Future<(Map<int, EbookProgress>, Set<int>)> readProgress() async {
    final f = File('${root.path}/progress.json');
    if (!f.existsSync()) return (<int, EbookProgress>{}, <int>{});
    try {
      final j = jsonDecode(await f.readAsString()) as Map<String, dynamic>;
      final places = <int, EbookProgress>{};
      for (final MapEntry(:key, :value) in (j['places'] as Map<String, dynamic>? ?? const {}).entries) {
        final p = EbookProgress.fromJson(value);
        if (int.tryParse(key) case final id? when p != null) places[id] = p;
      }
      final pending = {for (final v in (j['pending'] as List? ?? const [])) if (v is int) v};
      return (places, pending);
    } on Object {
      return (<int, EbookProgress>{}, <int>{});
    }
  }

  Future<void> writeProgress(Map<int, EbookProgress> places, Set<int> pending) async {
    await root.create(recursive: true);
    await File('${root.path}/progress.json').writeAsString(jsonEncode({
      'places': {for (final e in places.entries) '${e.key}': e.value.toJson()},
      'pending': pending.toList(),
    }));
  }
}

/// The e-books for the reader: the list of the server (or the last one, without it), the copies on this device and the places read.
class EbookController extends ChangeNotifier {
  EbookController({this.api, EbookShelf? shelf}) : shelf = shelf ?? EbookShelf();

  /// The server; null without one (what is on this device still opens).
  final EbookApi? api;
  final EbookShelf shelf;

  List<Ebook> books = const [];
  bool loaded = false;

  /// The list is the last one kept on this device: the server did not answer, or the app is offline.
  bool stale = false;

  /// While an e-book is loaded onto the device: its id and the bytes so far.
  final Map<int, int> loading = {};

  /// What could not be loaded the last time, for the screen to say.
  String? error;

  Map<int, EbookProgress> _places = {};
  Set<int> _pending = {};
  final Map<int, EbookText> _texts = {};
  final Map<String, Uint8List> _pictures = {};

  /// Loads the list; [offline]: only the one kept on this device.
  Future<void> load({bool offline = false}) async {
    final (places, pending) = await shelf.readProgress();
    _places = places;
    _pending = pending;
    List<Ebook>? list;
    if (!offline && api != null) {
      try {
        list = await api!.list();
        await shelf.writeList(list);
        stale = false;
        await _flush();
      } on Object {
        list = null;
      }
    }
    if (list == null) {
      list = await shelf.readList() ?? const [];
      stale = true;
    }
    books = list;
    loaded = true;
    notifyListeners();
  }

  /// Whether a copy of [e] is on this device (of any version).
  bool isKept(Ebook e) => shelf.keptVersion(e.id) != null;

  /// Whether the copy on this device is of an older version than the server's.
  bool outdated(Ebook e) {
    final v = shelf.keptVersion(e.id);
    return v != null && v != e.mtimeMs;
  }

  /// Where the person is: the newer of this device and the server.
  EbookProgress? progressOf(Ebook e) {
    final here = _places[e.id];
    final there = e.progress;
    if (here == null) return there;
    if (there == null) return here;
    return here.readAt >= there.readAt ? here : there;
  }

  /// Notes where the person is, on this device at once and on the server when it answers.
  Future<void> saveProgress(Ebook e, EbookProgress p) async {
    _places[e.id] = p;
    _pending.add(e.id);
    await shelf.writeProgress(_places, _pending);
    await _flush(only: e.id);
    notifyListeners();
  }

  Future<void> _flush({int? only}) async {
    final api = this.api;
    if (api == null || _pending.isEmpty) return;
    var changed = false;
    for (final id in [..._pending]) {
      if (only != null && id != only) continue;
      final p = _places[id];
      try {
        if (p != null) await api.saveProgress(id, p);
        _pending.remove(id);
        changed = true;
      } on ApiException catch (e) {
        // The book is gone for the person: nothing to send any more.
        if (e.status == 404) {
          _pending.remove(id);
          changed = true;
        }
      } on Object {
        // Offline: later.
      }
    }
    if (changed) await shelf.writeProgress(_places, _pending);
  }

  /// Loads [e] onto this device: the text and its pictures, or the PDF. A copy of an older version is replaced.
  Future<bool> download(Ebook e) async {
    final api = this.api;
    if (api == null || loading.containsKey(e.id)) return false;
    error = null;
    loading[e.id] = 0;
    notifyListeners();
    try {
      await shelf.remove(e.id);
      await shelf.dirOf(e.id).create(recursive: true);
      if (e.isPdf) {
        await api.file(e.id, shelf.pdfOf(e.id), progress: (n) {
          loading[e.id] = n;
          notifyListeners();
        });
      } else {
        final json = await api.text(e.id);
        final text = EbookText.parse(json);
        var n = json.length;
        for (final name in text.pictures) {
          try {
            final bytes = await api.picture(e.id, name);
            final f = shelf.pictureOf(e.id, name);
            await f.parent.create(recursive: true);
            await f.writeAsBytes(bytes);
            n += bytes.length;
            loading[e.id] = n;
            notifyListeners();
          } on ApiException {
            // A picture the server does not give: the text without it.
          }
        }
        await shelf.textOf(e.id).writeAsString(json);
        _texts[e.id] = text;
      }
      try {
        await shelf.coverOf(e.id).writeAsBytes(await api.cover(e.id));
      } on ApiException {
        // No cover: the list shows its title.
      }
      await shelf.markKept(e);
      return true;
    } on Object catch (x) {
      await shelf.remove(e.id);
      error = x is FileSystemException && x.osError?.errorCode == 28 ? 'space' : 'failed';
      return false;
    } finally {
      loading.remove(e.id);
      notifyListeners();
    }
  }

  Future<void> remove(Ebook e) async {
    await shelf.remove(e.id);
    _texts.remove(e.id);
    notifyListeners();
  }

  /// The text of an EPUB: from the copy on this device, else from the server.
  Future<EbookText> text(Ebook e) async {
    final cached = _texts[e.id];
    if (cached != null && !outdated(e)) return cached;
    final f = shelf.textOf(e.id);
    final String json;
    if (isKept(e) && f.existsSync()) {
      json = await f.readAsString();
    } else {
      final api = this.api;
      if (api == null) throw ApiException(0, 'network');
      json = await api.text(e.id);
    }
    return _texts[e.id] = EbookText.parse(json);
  }

  final Map<int, Future<Uint8List?>> _covers = {};

  /// The cover of [e], or null: from the copy on this device, else from the server (once per start).
  Future<Uint8List?> cover(Ebook e) => _covers.putIfAbsent(e.id, () async {
        final f = shelf.coverOf(e.id);
        if (f.existsSync()) return f.readAsBytes();
        try {
          return await api?.cover(e.id);
        } on Object {
          return null;
        }
      });

  /// A picture of the text of an EPUB, or null.
  Future<Uint8List?> picture(Ebook e, String name) async {
    final f = shelf.pictureOf(e.id, name);
    if (f.existsSync()) return f.readAsBytes();
    final key = '${e.id}/$name';
    final cached = _pictures[key];
    if (cached != null) return cached;
    try {
      final b = await api?.picture(e.id, name);
      if (b != null) _pictures[key] = b;
      return b;
    } on Object {
      return null;
    }
  }

  /// The file of a PDF on this device: the copy, else fetched into a folder that is cleared at the next start.
  Future<String> pdf(Ebook e) async {
    final kept = shelf.pdfOf(e.id);
    if (isKept(e) && kept.existsSync()) return kept.path;
    final api = this.api;
    if (api == null) throw ApiException(0, 'network');
    final temp = File('${shelf.root.path}/.read/${e.id}-${e.mtimeMs}.pdf');
    if (!temp.existsSync()) await api.file(e.id, temp);
    return temp.path;
  }

  /// Clears what was fetched only for reading (not loaded onto the device).
  Future<void> tidy() async {
    final d = Directory('${shelf.root.path}/.read');
    if (d.existsSync()) await d.delete(recursive: true);
  }
}
