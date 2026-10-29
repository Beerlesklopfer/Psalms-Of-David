// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import '../server/app_paths.dart';
import 'audio_backend.dart';

/// What the listener set for playback on this device. Small, so it is written in one go and replaced as a whole.
class PlaybackSettingsStore {
  PlaybackSettingsStore(this.file);

  factory PlaybackSettingsStore.standard() =>
      PlaybackSettingsStore(File('${AppPaths.base.path}/playback.json'));

  final File file;

  /// Seconds of audio the engine keeps ready; the default when nothing (or nothing usable) is stored.
  int readBufferSeconds() {
    try {
      final j = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
      final n = j['buffer_seconds'];
      if (n is int) return n.clamp(kBufferMinSeconds, kBufferMaxSeconds);
    } on Object {
      // Missing or damaged: the default.
    }
    return kBufferDefaultSeconds;
  }

  void writeBufferSeconds(int seconds) {
    try {
      file.parent.createSync(recursive: true);
      final temp = File('${file.path}.tmp');
      temp.writeAsStringSync(jsonEncode({'buffer_seconds': seconds}));
      temp.renameSync(file.path);
    } on FileSystemException {
      // Not being able to remember is not worth a failure; the setting then holds until the app ends.
    }
  }
}
