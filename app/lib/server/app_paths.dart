// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:path_provider/path_provider.dart';

/// Where the app keeps its own files on this device: the pairing with the server and the
/// downloaded parts.
///
/// The place is different on every system, so it comes from the system and not from a
/// guess: `path_provider` gives the private folder of the app (Linux
/// `~/.local/share/io.onesimus.psalm`, macOS `~/Library/Application Support/<bundle id>`,
/// Windows `%APPDATA%\<company>\<app>`, Android the internal files folder of the app, iOS
/// `Library/Application Support` of the app). It is the folder for data the app must not lose,
/// not the cache folder that a cleaner or the system may empty: a download is something the user
/// asked for.
///
/// `PSALM_DATA_DIR` replaces it (development and tests), `PSALM_CONFIG_DIR` and
/// `PSALM_CACHE_DIR` replace the pairing and the downloads one by one.
class AppPaths {
  AppPaths._();

  static Directory? _base;

  /// Looks the folder up once, at the start of the app, and moves what an older version kept
  /// in the XDG folders of Linux.
  static Future<void> init() async {
    final env = Platform.environment['PSALM_DATA_DIR'];
    _base = env != null && env.isNotEmpty ? Directory(env) : await getApplicationSupportDirectory();
    await _base!.create(recursive: true);
    if (Platform.isLinux) _moveLegacyLinux(_base!);
  }

  /// The folder of the app. Before [init] (tests without it) the old Linux places are used.
  static Directory get base => _base ?? _legacyBase();

  static File get credentialsFile {
    final env = Platform.environment['PSALM_CONFIG_DIR'];
    return File('${env != null && env.isNotEmpty ? env : base.path}/server.json');
  }

  static Directory get parts {
    final env = Platform.environment['PSALM_CACHE_DIR'];
    return Directory(env != null && env.isNotEmpty ? '$env/parts' : '${base.path}/parts');
  }

  static Directory _legacyBase() {
    final env = Platform.environment;
    return Directory('${env['XDG_DATA_HOME'] ?? '${env['HOME'] ?? Directory.systemTemp.path}/.local/share'}/psalm');
  }

  /// Version 1.0 kept the pairing in `~/.config/psalm` and the parts in `~/.cache/psalm/parts`.
  static void _moveLegacyLinux(Directory base) {
    final env = Platform.environment;
    final home = env['HOME'];
    if (home == null) return;
    try {
      final oldCredentials = File('${env['XDG_CONFIG_HOME'] ?? '$home/.config'}/psalm/server.json');
      final newCredentials = File('${base.path}/server.json');
      if (oldCredentials.existsSync() && !newCredentials.existsSync()) {
        oldCredentials.copySync(newCredentials.path);
        oldCredentials.deleteSync();
      }
      final oldParts = Directory('${env['XDG_CACHE_HOME'] ?? '$home/.cache'}/psalm/parts');
      final newParts = Directory('${base.path}/parts');
      if (oldParts.existsSync() && !newParts.existsSync()) oldParts.renameSync(newParts.path);
    } on FileSystemException {
      // Left where they are; the app pairs or downloads again.
    }
  }
}
