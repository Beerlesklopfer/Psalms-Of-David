// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../server/api_client.dart';
import 'photo_backup_api.dart';
import 'photo_device.dart';
import 'photo_sync.dart';

/// Registers the background job with the conditions of the profile (replaced in tests).
typedef PhotoScheduler = Future<void> Function(PhotoSettings settings, PhotoDeviceSettings device, PhotoDeviceStore store);

/// The state of the screen "Photo backup": the choices of the profile (from the server), the settings of this device, what the server
/// keeps, and a run started by hand.
class PhotoBackupController extends ChangeNotifier {
  PhotoBackupController({required this.api, required this.library, required this.store, required this.schedule, DateTime Function()? now})
      : now = now ?? DateTime.now,
        device = store.loadSettings(),
        state = store.load();

  final PhotoBackupApi api;
  final PhotoLibrary library;
  final PhotoDeviceStore store;
  final PhotoScheduler schedule;
  final DateTime Function() now;

  /// Null until loaded, and when the server has the backup off (or the profile is a guest): then nothing is shown.
  PhotoSettings? settings;
  PhotoServerState? server;
  PhotoDeviceSettings device;
  PhotoDeviceState state;

  bool loaded = false;

  /// The server could not be asked (the code of [ApiException]).
  String? loadError;

  /// The choice the server refused at the last change, by key, with the reason.
  Map<String, String> refused = const {};

  /// The app may read the pictures (asked when the device is switched on).
  bool? access;

  /// The person said no to the question of the system when switching the device on.
  bool accessRefused = false;

  bool running = false;
  int done = 0;
  int total = 0;
  PhotoRunResult? last;
  PhotoSync? _sync;

  bool get available => settings != null;

  Future<void> load() async {
    try {
      settings = await api.settings();
      loadError = null;
      if (settings != null) {
        server = await api.state();
        access = await library.hasAccess();
        await schedule(settings!, device, store);
      }
    } on ApiException catch (e) {
      loadError = e.code;
    }
    state = store.load();
    loaded = true;
    notifyListeners();
  }

  /// Changes a choice of the profile; a refusal of the server is kept in [refused] and nothing changes.
  Future<void> choose(String key, String option) async {
    try {
      settings = await api.putSettings({key: option});
      refused = const {};
      await schedule(settings!, device, store);
    } on PhotoChoiceRefused catch (e) {
      refused = e.refused;
    } on ApiException catch (e) {
      refused = {key: e.code};
    }
    notifyListeners();
  }

  /// Switches this device on (asks the system for the pictures first; refused → stays off) or off. The first time, only the pictures from
  /// this day on count.
  Future<void> setDeviceEnabled(bool on) async {
    if (on) {
      access = await library.requestAccess();
      accessRefused = access != true;
      if (access != true) {
        notifyListeners();
        return;
      }
      final t = now();
      device.since ??= DateTime(t.year, t.month, t.day);
    }
    device.enabled = on;
    store.saveSettings(device);
    if (settings case final s?) await schedule(s, device, store);
    notifyListeners();
  }

  void setSources(String sources) {
    device.sources = sources;
    store.saveSettings(device);
    notifyListeners();
  }

  /// Bisync: what this device does with the pictures of the other devices (`app`, `gallery`, `off`).
  void setReceive(String receive) {
    device.receive = receive;
    store.saveSettings(device);
    notifyListeners();
  }

  /// The original of a received picture, loaded for the view. Only the one being looked at stays on the device: the one before is removed.
  Future<File> original(ReceivedPhoto photo) async {
    final dir = store.receivedDir;
    final file = File('${dir.path}/original-${photo.id}');
    if (file.existsSync()) return file;
    if (dir.existsSync()) {
      for (final f in dir.listSync().whereType<File>()) {
        if (f.uri.pathSegments.last.startsWith('original-')) f.deleteSync();
      }
    }
    await api.download(photo.id, file, photo.sha256);
    return file;
  }

  /// Also the pictures from before the day the device was switched on (the explicit button).
  void includeOlder() {
    device.olderToo = true;
    store.saveSettings(device);
    notifyListeners();
  }

  /// A run now, in the app, with its progress on the screen.
  Future<void> runNow() async {
    if (running) return;
    running = true;
    done = 0;
    total = 0;
    notifyListeners();
    final sync = _sync = PhotoSync(
      api: api,
      library: library,
      store: store,
      now: now,
      onProgress: (d, t) {
        done = d;
        total = t;
        notifyListeners();
      },
    );
    try {
      last = await sync.run();
      if (last?.settings case final s?) settings = s;
      try {
        server = await api.state();
      } on ApiException {
        // the numbers stay as they were
      }
    } finally {
      _sync = null;
      running = false;
      state = store.load();
      notifyListeners();
    }
  }

  /// Ends the run after the current piece; the next one goes on there.
  void pause() => _sync?.stop();
}
