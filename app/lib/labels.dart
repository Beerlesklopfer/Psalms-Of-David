// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/widgets.dart';

import 'models.dart';
import 'offline/offline_controller.dart';
import 'server/api_client.dart';

/// Classic tagging: free words on a book (prayer, school, holidays together). Everybody who sees the book sees them; the owner of the book and an
/// administrator may set them. The server decides ([LibraryItem.canLabel]).
abstract class LabelApi {
  /// Sets all labels of a book; returns the labels it has now.
  Future<List<String>> setLabels(int id, List<String> labels);

  /// Sets all labels of one title (part) of a book; returns the labels of its own it has now.
  Future<List<String>> setTrackLabels(int id, int partId, List<String> labels);

  /// Sets all labels of a podcast (by its id); returns the labels it has now.
  Future<List<String>> setPodcastLabels(int id, List<String> labels);

  /// Sets all labels of a radio station; returns the labels it has now.
  Future<List<String>> setStationLabels(int id, List<String> labels);

  /// The labels of the library with the number of items (books, podcasts, stations; of those this user sees) (of those this user sees), and whether this user manages the list.
  Future<LabelList> list();

  /// Renames a label for every book; an existing name takes the books over. Administrators only (the server decides).
  Future<void> rename(String name, String to);

  /// Takes a label off every book. Administrators only.
  Future<void> delete(String name);
}

/// The answer of `GET /api/v1/labels`.
class LabelList {
  const LabelList(this.labels, this.canManage);

  final List<(String, int)> labels;
  final bool canManage;
}

/// The labels of the books in [books] (and of their titles) with the number of books that carry each, the most used first (then by name).
List<(String, int)> labelCounts(Iterable<LibraryItem> books) {
  final counts = <String, (String, int)>{};
  for (final b in books) {
    for (final l in b.allLabels) {
      final key = l.toLowerCase();
      counts[key] = (counts[key]?.$1 ?? l, (counts[key]?.$2 ?? 0) + 1);
    }
  }
  final list = counts.values.toList()
    ..sort((a, b) {
      final byCount = b.$2.compareTo(a.$2);
      return byCount != 0 ? byCount : a.$1.toLowerCase().compareTo(b.$1.toLowerCase());
    });
  return list;
}

/// The labels of podcasts or stations (one list of labels for each) with the number of them that carry each, in the order of [labelCounts].
List<(String, int)> labelCountsOf(Iterable<List<String>> labelLists) {
  final counts = <String, (String, int)>{};
  for (final labels in labelLists) {
    for (final l in labels) {
      final key = l.toLowerCase();
      counts[key] = (counts[key]?.$1 ?? l, (counts[key]?.$2 ?? 0) + 1);
    }
  }
  return _sorted(counts.values);
}

/// Two lists of label counts as one (case ignored, the counts added), the most used first.
List<(String, int)> mergeLabelCounts(List<(String, int)> a, List<(String, int)> b) {
  final counts = <String, (String, int)>{};
  for (final (name, n) in [...a, ...b]) {
    final key = name.toLowerCase();
    counts[key] = (counts[key]?.$1 ?? name, (counts[key]?.$2 ?? 0) + n);
  }
  return _sorted(counts.values);
}

List<(String, int)> _sorted(Iterable<(String, int)> counts) => counts.toList()
  ..sort((a, b) {
    final byCount = b.$2.compareTo(a.$2);
    return byCount != 0 ? byCount : a.$1.toLowerCase().compareTo(b.$1.toLowerCase());
  });

/// Does an item with [labels] carry **all** the [chosen] labels (case ignored)? Everything fits when nothing is chosen.
bool carriesAll(List<String> labels, Set<String> chosen) =>
    chosen.every((c) => labels.any((l) => l.toLowerCase() == c.toLowerCase()));

/// The books that carry **all** the [chosen] labels (case ignored); all books when nothing is chosen.
List<LibraryItem> filterByLabels(List<LibraryItem> books, Set<String> chosen) {
  if (chosen.isEmpty) return books;
  final wanted = {for (final c in chosen) c.toLowerCase()};
  return [
    for (final b in books)
      if (wanted.every((w) => b.allLabels.any((l) => l.toLowerCase() == w))) b,
  ];
}

/// What to suggest while a label is typed: the labels in use that fit [typed] (the ones that begin with it first), without the ones already
/// chosen, the most used first; the most used ones when nothing is typed. At most [limit].
List<String> labelSuggestions(
  Iterable<LibraryItem> books,
  String typed,
  Iterable<String> chosen, {
  int limit = 8,
}) {
  final needle = typed.trim().toLowerCase();
  final taken = {for (final c in chosen) c.toLowerCase()};
  final fits = [
    for (final (name, count) in labelCounts(books))
      if (!taken.contains(name.toLowerCase()) &&
          !isReservedLabel(name) &&
          (needle.isEmpty || name.toLowerCase().contains(needle)))
        (name, count),
  ];
  fits.sort((a, b) {
    final pa = a.$1.toLowerCase().startsWith(needle), pb = b.$1.toLowerCase().startsWith(needle);
    if (pa != pb) return pa ? -1 : 1;
    return b.$2.compareTo(a.$2);
  });
  return [for (final f in fits.take(limit)) f.$1];
}

/// The words a change adds and takes off to get from [before] to [after] (case ignored), as the delta that the server takes.
(List<String> add, List<String> remove) labelDelta(List<String> before, List<String> after) {
  final had = {for (final b in before) b.toLowerCase()};
  final has = {for (final a in after) a.toLowerCase()};
  return ([for (final a in after) if (!had.contains(a.toLowerCase())) a], [for (final b in before) if (!has.contains(b.toLowerCase())) b]);
}

/// The system label of the blacklist: only administrators receive it from the server, and nobody can type or rename it (the server decides what is
/// marked). An administrator may take it off a medium (which approves the medium).
const kBlacklistLabel = 'Blacklist';

bool isReservedLabel(String name) => name.trim().toLowerCase() == kBlacklistLabel.toLowerCase();

/// A label as the server keeps it: white space made one space, 1 to 40 characters, no comma, semicolon or bar; null if it is not usable.
String? cleanLabel(String raw) {
  final name = raw.trim().replaceAll(RegExp(r'\s+'), ' ');
  if (name.isEmpty || name.length > 40) return null;
  if (RegExp(r'[,;|\u0000-\u001f]').hasMatch(name)) return null;
  if (isReservedLabel(name)) return null;
  return name;
}

/// Sets the labels on the server and makes the library load again.
class LabelsController extends ChangeNotifier {
  LabelsController({this.onChanged});

  /// Called after a change the server took, to load the library again.
  VoidCallback? onChanged;

  LabelApi? api;

  /// Offline mode: a change of the labels of a book or a title is then kept for the server (and shows at once), and so is one that fails for want
  /// of a connection. The app sets the three; without them every change goes to the server at once.
  OfflineController? offline;

  /// The book of this id as the app has it now (for the words that were there before a change).
  LibraryItem? Function(String id)? itemOf;

  /// Changes the book of this id in the lists of the app.
  void Function(String id, LibraryItem Function(LibraryItem) change)? patchItem;

  /// The code of the last refusal or failure, or null.
  String? error;

  /// True once there is a server to ask.
  bool get available => api != null;

  /// The list of the labels (loaded when the page of the labels is opened).
  LabelList list = const LabelList([], false);

  Future<void> load() async {
    final a = api;
    if (a == null) return;
    try {
      list = await a.list();
      error = null;
    } on ApiException catch (e) {
      error = e.code;
    }
    notifyListeners();
  }

  /// Renames a label everywhere; the library is loaded again (the books carry the new name).
  Future<bool> rename(String name, String to) => _manage(() => api!.rename(name, to));

  /// Takes a label off every book.
  Future<bool> remove(String name) => _manage(() => api!.delete(name));

  Future<bool> _manage(Future<void> Function() change) async {
    if (api == null) return false;
    try {
      await change();
      error = null;
      onChanged?.call();
      await load();
      return true;
    } on ApiException catch (e) {
      error = e.code;
      notifyListeners();
      return false;
    }
  }

  /// Sets the labels of one title of a book (the owner of the book and an administrator may).
  Future<bool> saveTrack(String bookId, int partId, List<String> labels) async {
    final id = int.tryParse(bookId);
    if (id == null) return false;
    if (offline?.active == true) return _keepTrack(bookId, id, partId, labels);
    final a = api;
    if (a == null) return false;
    try {
      await a.setTrackLabels(id, partId, labels);
      error = null;
      onChanged?.call();
      notifyListeners();
      return true;
    } on ApiException catch (e) {
      if (e.code == 'network' && _keepTrack(bookId, id, partId, labels)) return true;
      error = e.code;
      notifyListeners();
      return false;
    }
  }

  /// Sets the labels of a podcast (an administrator, or a member who follows it).
  /// [current] and [title] let a change that fails for want of a connection wait for the server (as a delta).
  Future<bool> savePodcast(int id, List<String> labels, {List<String>? current, String title = ''}) =>
      _saveWith(() => api!.setPodcastLabels(id, labels), () => _keepFeed('podcast', id, labels, current, title));

  /// Sets the labels of a radio station (an administrator, or a member who has it among the favourites).
  Future<bool> saveStation(int id, List<String> labels, {List<String>? current, String title = ''}) =>
      _saveWith(() => api!.setStationLabels(id, labels), () => _keepFeed('station', id, labels, current, title));

  bool _keepFeed(String scope, int id, List<String> labels, List<String>? current, String title) {
    final off = offline;
    if (off == null || current == null) return false;
    final (add, remove) = labelDelta(current, labels);
    off.outbox.addLabels(scope, id, add: add, remove: remove, title: title);
    error = null;
    off.touch();
    notifyListeners();
    return true;
  }

  Future<bool> _saveWith(Future<List<String>> Function() change, [bool Function()? keep]) async {
    if (api == null) return false;
    try {
      await change();
      error = null;
      onChanged?.call();
      notifyListeners();
      return true;
    } on ApiException catch (e) {
      if (e.code == 'network' && keep != null && keep()) return true;
      error = e.code;
      notifyListeners();
      return false;
    }
  }

  Future<bool> save(String bookId, List<String> labels) async {
    final id = int.tryParse(bookId);
    if (id == null) return false;
    if (offline?.active == true) return _keepItem(bookId, id, labels);
    final a = api;
    if (a == null) return false;
    try {
      await a.setLabels(id, labels);
      error = null;
      onChanged?.call();
      notifyListeners();
      return true;
    } on ApiException catch (e) {
      if (e.code == 'network' && _keepItem(bookId, id, labels)) return true;
      error = e.code;
      notifyListeners();
      return false;
    }
  }

  /// Keeps the new labels of a book for the server, as a delta against what the app showed, and shows them now.
  bool _keepItem(String bookId, int id, List<String> labels) {
    final off = offline;
    if (off == null) return false;
    final book = itemOf?.call(bookId);
    final (add, remove) = labelDelta(book?.labels ?? const [], labels);
    off.outbox.addLabels('book', id, add: add, remove: remove, title: book?.title ?? '');
    patchItem?.call(bookId, (b) => b.copyWith(labels: labels));
    error = null;
    off.touch();
    notifyListeners();
    return true;
  }

  bool _keepTrack(String bookId, int id, int partId, List<String> labels) {
    final off = offline;
    if (off == null) return false;
    final book = itemOf?.call(bookId);
    final before = book?.tracks.where((t) => t.partId == partId).firstOrNull?.labels ?? const <String>[];
    final (add, remove) = labelDelta(before, labels);
    off.outbox.addLabels('part', id, part: partId, add: add, remove: remove, title: book?.title ?? '');
    patchItem?.call(bookId, (b) {
      final tracks = [for (final t in b.tracks) t.partId == partId ? t.withLabels(labels) : t];
      final seen = <String>{};
      final union = [for (final t in tracks) for (final l in t.labels) if (seen.add(l.toLowerCase())) l];
      return b.copyWith(tracks: tracks, trackLabels: union);
    });
    error = null;
    off.touch();
    notifyListeners();
    return true;
  }
}

class LabelsScope extends InheritedNotifier<LabelsController> {
  const LabelsScope({super.key, required LabelsController controller, required super.child})
    : super(notifier: controller);

  static LabelsController? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<LabelsScope>()?.notifier;
}
