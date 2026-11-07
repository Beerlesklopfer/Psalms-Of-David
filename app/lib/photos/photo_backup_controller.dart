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

  /// The pictures in the trash of the server, while the trash screen shows them.
  List<TrashedPhoto>? trash;

  /// The last answer about the trash went wrong (an [ApiException] code), or a run held the state.
  String? trashError;

  /// Changes the state file outside of a run: only while no run holds it.
  bool _withState(void Function(PhotoDeviceState s) change) {
    if (!store.tryLock()) {
      trashError = 'busy';
      notifyListeners();
      return false;
    }
    try {
      final s = store.load();
      change(s);
      store.save(s);
      state = s;
    } finally {
      store.unlock();
    }
    trashError = null;
    notifyListeners();
    return true;
  }

  /// "Delete here too": the pictures another device deleted go from this gallery (Android asks once more); what Android did not delete
  /// stays on the list.
  Future<void> deleteHereToo() async {
    final ids = [for (final g in state.toDelete) g.assetId];
    final deleted = (await library.delete(ids)).toSet();
    _withState((s) {
      for (final g in s.toDelete.where((g) => deleted.contains(g.assetId)).toList()) {
        s.entries.remove(g.key);
        s.toDelete.remove(g);
      }
    });
  }

  /// "Keep them here": the pictures stay on this device (and in the trash of the server until it is emptied).
  void keepHere() => _withState((s) => s.toDelete.clear());

  /// Many pictures were gone at once and the person says they were deleted: into the trash of the server.
  Future<void> trashGone() async {
    final gone = List.of(state.toTrash);
    try {
      await api.trash([for (final g in gone) g.sha256]);
    } on ApiException catch (e) {
      trashError = e.code;
      notifyListeners();
      return;
    }
    _withState((s) {
      for (final g in gone) {
        s.entries.remove(g.key);
      }
      s.toTrash.removeWhere((g) => gone.any((x) => x.key == g.key));
    });
  }

  /// They were not deleted (moved to a card, say): the server keeps them, this device forgets them.
  void forgetGone() => _withState((s) {
        for (final g in s.toTrash) {
          s.entries.remove(g.key);
        }
        s.toTrash.clear();
      });

  /// A received picture into the trash of the server, from the view of the app; the other devices are told.
  Future<bool> trashReceived(ReceivedPhoto photo) async {
    try {
      await api.trash([photo.sha256]);
    } on ApiException catch (e) {
      trashError = e.code;
      notifyListeners();
      return false;
    }
    return _withState((s) {
      s.received.removeWhere((r) => r.id == photo.id);
      final f = store.thumbFile(photo.id);
      if (f.existsSync()) f.deleteSync();
    });
  }

  Future<void> loadTrash() async {
    try {
      trash = await api.trashList();
      trashError = null;
    } on ApiException catch (e) {
      trashError = e.code;
    }
    notifyListeners();
  }

  /// Takes a picture out of the trash; the other devices are told (a device that has not deleted it yet keeps it).
  Future<void> restore(TrashedPhoto photo) async {
    try {
      await api.restore(photo.id);
      trash?.removeWhere((t) => t.id == photo.id);
      trashError = null;
      server = await api.state();
    } on ApiException catch (e) {
      trashError = e.code;
    }
    notifyListeners();
  }
}
