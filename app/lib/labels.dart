// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/widgets.dart';

import 'models.dart';
import 'server/api_client.dart';

/// Classic tagging: free words on a book (prayer, school, holidays together). Everybody who sees the book sees them; the owner of the book and an
/// administrator may set them. The server decides ([Book.canLabel]).
abstract class LabelApi {
  /// Sets all labels of a book; returns the labels it has now.
  Future<List<String>> setLabels(int id, List<String> labels);

  /// Sets all labels of one title (part) of a book; returns the labels of its own it has now.
  Future<List<String>> setTrackLabels(int id, int partId, List<String> labels);

  /// The labels of the library with the number of books (of those this user sees), and whether this user manages the list.
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
List<(String, int)> labelCounts(Iterable<Book> books) {
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

/// The books that carry **all** the [chosen] labels (case ignored); all books when nothing is chosen.
List<Book> filterByLabels(List<Book> books, Set<String> chosen) {
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
  Iterable<Book> books,
  String typed,
  Iterable<String> chosen, {
  int limit = 8,
}) {
  final needle = typed.trim().toLowerCase();
  final taken = {for (final c in chosen) c.toLowerCase()};
  final fits = [
    for (final (name, count) in labelCounts(books))
      if (!taken.contains(name.toLowerCase()) &&
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

/// A label as the server keeps it: white space made one space, 1 to 40 characters, no comma, semicolon or bar; null if it is not usable.
String? cleanLabel(String raw) {
  final name = raw.trim().replaceAll(RegExp(r'\s+'), ' ');
  if (name.isEmpty || name.length > 40) return null;
  if (RegExp(r'[,;|\u0000-\u001f]').hasMatch(name)) return null;
  return name;
}

/// Sets the labels on the server and makes the library load again.
class LabelsController extends ChangeNotifier {
  LabelsController({this.onChanged});

  /// Called after a change the server took, to load the library again.
  VoidCallback? onChanged;

  LabelApi? api;

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
    final a = api;
    final id = int.tryParse(bookId);
    if (a == null || id == null) return false;
    try {
      await a.setTrackLabels(id, partId, labels);
      error = null;
      onChanged?.call();
      notifyListeners();
      return true;
    } on ApiException catch (e) {
      error = e.code;
      notifyListeners();
      return false;
    }
  }

  Future<bool> save(String bookId, List<String> labels) async {
    final a = api;
    final id = int.tryParse(bookId);
    if (a == null || id == null) return false;
    try {
      await a.setLabels(id, labels);
      error = null;
      onChanged?.call();
      notifyListeners();
      return true;
    } on ApiException catch (e) {
      error = e.code;
      notifyListeners();
      return false;
    }
  }
}

class LabelsScope extends InheritedNotifier<LabelsController> {
  const LabelsScope({super.key, required LabelsController controller, required super.child})
    : super(notifier: controller);

  static LabelsController? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<LabelsScope>()?.notifier;
}
