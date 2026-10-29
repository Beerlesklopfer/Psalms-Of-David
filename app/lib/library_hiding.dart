// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/widgets.dart';

import 'server/api_client.dart' show ApiException;

/// A book that this profile hid (`docs/spec-library-hiding.md`).
class HiddenBook {
  const HiddenBook({required this.id, required this.title, required this.author});

  final int id;
  final String title;
  final String author;

  factory HiddenBook.fromJson(Map<String, dynamic> j) =>
      HiddenBook(id: j['id'] as int, title: j['title'] as String, author: j['author'] as String);
}

/// An author or artist whose books of one kind this profile hid. [kind] is `audiobook`, `music`, or empty for both.
class HiddenAuthor {
  const HiddenAuthor({required this.id, required this.name, this.kind = ''});

  final int id;
  final String name;
  final String kind;

  factory HiddenAuthor.fromJson(Map<String, dynamic> j) => HiddenAuthor(
    id: j['id'] as int,
    name: j['name'] as String,
    kind: (j['kind'] as String?) ?? '',
  );
}

/// What the profile hid and its setting "show hidden".
class HiddenState {
  const HiddenState({this.showHidden = false, this.books = const [], this.authors = const []});

  final bool showHidden;
  final List<HiddenBook> books;
  final List<HiddenAuthor> authors;

  bool get isEmpty => books.isEmpty && authors.isEmpty;

  /// Is every book of [name] of [kind] hidden (by an entry for exactly this kind, or for both)?
  bool authorHidden(String name, String kind) => authors.any(
    (a) => a.name.toLowerCase() == name.toLowerCase() && (a.kind.isEmpty || a.kind == kind),
  );

  /// Is there an entry for exactly this author and this kind (`''` = both)?
  bool authorEntry(String name, String kind) =>
      authors.any((a) => a.name.toLowerCase() == name.toLowerCase() && a.kind == kind);

  factory HiddenState.fromJson(Map<String, dynamic> j) => HiddenState(
    showHidden: j['show_hidden'] == true,
    books: [
      for (final b in (j['books'] as List? ?? const []))
        HiddenBook.fromJson(b as Map<String, dynamic>),
    ],
    authors: [
      for (final a in (j['authors'] as List? ?? const []))
        HiddenAuthor.fromJson(a as Map<String, dynamic>),
    ],
  );
}

/// The server side of the own library: every answer is the whole state.
abstract class HidingApi {
  Future<HiddenState> state();

  Future<HiddenState> setShowHidden(bool show);

  Future<HiddenState> hideBook(int id, bool hidden);

  /// [kind] is `audiobook`, `music` or empty for both.
  Future<HiddenState> hideAuthor(String name, String kind, bool hidden);
}

/// Looks after what this profile hid. A change goes to the server; [onChanged] then reloads the lists (they leave out what is hidden).
class HidingController extends ChangeNotifier {
  HidingController({this.onChanged});

  /// Called after a change that the server took, to load the library again.
  VoidCallback? onChanged;

  HidingApi? api;
  HiddenState state = const HiddenState();

  /// The code of the last refusal or failure, or null.
  String? error;

  bool _disposed = false;

  /// True once there is a server to ask.
  bool get available => api != null;

  Future<void> load() async {
    final a = api;
    if (a == null) return;
    try {
      state = await a.state();
      error = null;
    } on ApiException catch (e) {
      error = e.code;
    }
    _notify();
  }

  Future<bool> setShowHidden(bool show) => _change((a) => a.setShowHidden(show));

  Future<bool> hideBook(String bookId, bool hidden) {
    final id = int.tryParse(bookId);
    if (id == null) return Future.value(false);
    return _change((a) => a.hideBook(id, hidden));
  }

  Future<bool> hideAuthor(String name, String kind, bool hidden) =>
      _change((a) => a.hideAuthor(name, kind, hidden));

  Future<bool> _change(Future<HiddenState> Function(HidingApi) call) async {
    final a = api;
    if (a == null) return false;
    try {
      state = await call(a);
      error = null;
      _notify();
      onChanged?.call();
      return true;
    } on ApiException catch (e) {
      error = e.code;
      _notify();
      return false;
    }
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }
}

/// Gives the screens the own library of the profile. Without it (demo, tests) nothing about hiding is shown.
class HidingScope extends InheritedNotifier<HidingController> {
  const HidingScope({super.key, required HidingController controller, required super.child})
    : super(notifier: controller);

  static HidingController? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<HidingScope>()?.notifier;
}
