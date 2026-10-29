// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'server/app_paths.dart';

/// Which groups of the library (by owner) are open. The group of the signed-in person is open
/// and all others are closed until the person opens them; what the person chose is kept on this
/// device, so that ten siblings do not mean ten open groups every time.
class LibraryFolds extends ChangeNotifier {
  /// Kept in [file]; without a file (tests) the choice holds until the app ends.
  LibraryFolds([this.file]) {
    _load();
  }

  factory LibraryFolds.standard() => LibraryFolds(File('${AppPaths.base.path}/library_folds.json'));

  final File? file;
  final Map<String, bool> _chosen = {};

  static String _key(String? me, String category, String? owner) =>
      '${me ?? ''}|$category|${owner ?? ''}';

  /// Is the group of [owner] (null: what belongs to nobody) open in [category]? Without a choice
  /// only the group of [me] is.
  bool isOpen(String? me, String category, String? owner) {
    final chosen = _chosen[_key(me, category, owner)];
    if (chosen != null) return chosen;
    return owner != null && me != null && owner == me;
  }

  void toggle(String? me, String category, String? owner) {
    _chosen[_key(me, category, owner)] = !isOpen(me, category, owner);
    notifyListeners();
    _save();
  }

  void _load() {
    try {
      final j = jsonDecode(file!.readAsStringSync()) as Map<String, dynamic>;
      for (final e in j.entries) {
        if (e.value is bool) _chosen[e.key] = e.value as bool;
      }
    } on Object {
      // Missing or damaged: the defaults.
    }
  }

  void _save() {
    final f = file;
    if (f == null) return;
    try {
      f.parent.createSync(recursive: true);
      final temp = File('${f.path}.tmp');
      temp.writeAsStringSync(jsonEncode(_chosen));
      temp.renameSync(f.path);
    } on FileSystemException {
      // Not being able to remember is not worth a failure; the choice then holds until the app ends.
    }
  }
}
