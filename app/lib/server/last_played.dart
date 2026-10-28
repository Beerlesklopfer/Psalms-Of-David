// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'app_paths.dart';

/// What was playing last on this device: which book, which part, where. Kept in a small file so
/// that the app opens on it after it was closed, also without the server.
class LastPlayed {
  LastPlayed({required this.bookId, required this.track, required this.position, required this.at});

  final String bookId;

  /// Index of the part in the book.
  final int track;
  final Duration position;

  /// When it was written, in milliseconds since 1970: compared with what the server says.
  final int at;

  Map<String, Object> toJson() => {'book': bookId, 'track': track, 'position_ms': position.inMilliseconds, 'at': at};
}

class LastPlayedStore {
  LastPlayedStore(this.file);

  factory LastPlayedStore.standard() => LastPlayedStore(File('${AppPaths.base.path}/last_played.json'));

  final File file;

  LastPlayed? read() {
    try {
      final j = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
      return LastPlayed(
        bookId: j['book'] as String,
        track: j['track'] as int,
        position: Duration(milliseconds: j['position_ms'] as int),
        at: j['at'] as int,
      );
    } on Object {
      // Missing or damaged: nothing remembered.
      return null;
    }
  }

  /// Small and written often, so it is written in one go and replaced as a whole.
  void write(LastPlayed last) {
    try {
      file.parent.createSync(recursive: true);
      final temp = File('${file.path}.tmp');
      temp.writeAsStringSync(jsonEncode(last.toJson()));
      temp.renameSync(file.path);
    } on FileSystemException {
      // Not being able to remember is not worth stopping the player for.
    }
  }
}
