// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart' show ThemeMode;

import '../server/app_paths.dart';
import 'audio_backend.dart';

/// What the listener set for playback on this device. Small, so it is written in one go and replaced as a whole.
class PlaybackSettingsStore {
  PlaybackSettingsStore(this.file);

  factory PlaybackSettingsStore.standard() =>
      PlaybackSettingsStore(File('${AppPaths.base.path}/playback.json'));

  final File file;

  Map<String, dynamic> _read() {
    try {
      final j = jsonDecode(file.readAsStringSync());
      if (j is Map<String, dynamic>) return j;
    } on Object {
      // Missing or damaged: nothing is stored.
    }
    return {};
  }

  void _write(Map<String, dynamic> values) {
    try {
      file.parent.createSync(recursive: true);
      final temp = File('${file.path}.tmp');
      temp.writeAsStringSync(jsonEncode(values));
      temp.renameSync(file.path);
    } on FileSystemException {
      // Not being able to remember is not worth a failure; the setting then holds until the app ends.
    }
  }

  /// Seconds of audio the engine keeps ready; the default when nothing (or nothing usable) is stored.
  int readBufferSeconds() {
    final n = _read()['buffer_seconds'];
    return n is int ? n.clamp(kBufferMinSeconds, kBufferMaxSeconds) : kBufferDefaultSeconds;
  }

  void writeBufferSeconds(int seconds) => _write({..._read(), 'buffer_seconds': seconds});

  /// The time in the mini player: of the whole book (CD), or of the track that plays; the track when nothing is stored.
  bool readShowItemTime() => _read()['show_book_time'] == true;

  void writeShowItemTime(bool value) => _write({..._read(), 'show_book_time': value});

  /// The colour scheme: "system", "light" or "dark"; dark when nothing (or nothing usable) is stored.
  ThemeMode readThemeMode() => switch (_read()['theme_mode']) {
        'system' => ThemeMode.system,
        'light' => ThemeMode.light,
        _ => ThemeMode.dark,
      };

  void writeThemeMode(ThemeMode mode) => _write({..._read(), 'theme_mode': mode.name});

  /// The switch "CD announcements": on when nothing is stored (it shows only for a profile with a CD right).
  bool readAnnounceCds() => _read()['announce_cds'] != false;

  void writeAnnounceCds(bool value) => _write({..._read(), 'announce_cds': value});

  /// The share of the volume the music keeps while a sentence is spoken; null when nothing usable is stored (the default applies).
  double? readDuckLevel() {
    final n = _read()['duck_level'];
    return n is num ? n.toDouble().clamp(0.0, 1.0) : null;
  }

  void writeDuckLevel(double value) => _write({..._read(), 'duck_level': value});
}
