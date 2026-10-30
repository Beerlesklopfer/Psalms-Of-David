// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter/widgets.dart';

import '../server/app_paths.dart';
import 'outbox.dart';

/// The setting "offline mode" of this device, in a small file.
class OfflineSettingsStore {
  OfflineSettingsStore(this.file);

  factory OfflineSettingsStore.standard() => OfflineSettingsStore(File('${AppPaths.base.path}/offline.json'));

  final File file;

  bool read() {
    try {
      return (jsonDecode(file.readAsStringSync()) as Map<String, dynamic>)['offline'] == true;
    } on Object {
      return false;
    }
  }

  void write(bool offline) {
    try {
      file.parent.createSync(recursive: true);
      file.writeAsStringSync(jsonEncode({'offline': offline}));
    } on FileSystemException {
      // The setting is lost at the next start; not worth a failure.
    }
  }
}

/// Offline mode: the app shows only what is on the device and keeps what the person changes until the server can take it
/// (`docs/spec-app-api.md`, "offline mode and synchronisation"). Offline is the person's switch ([manual]) or the fact that the server
/// cannot be reached ([unreachable]); either way the lists show the downloaded titles only.
class OfflineController extends ChangeNotifier {
  OfflineController({Outbox? outbox, this.store, bool manual = false}) : outbox = outbox ?? Outbox.memory(), manual = store?.read() ?? manual;

  final Outbox outbox;
  final OfflineSettingsStore? store;

  /// The person switched the offline mode on: the app does not ask the server at all.
  bool manual;

  /// The server did not answer (found by the look at the library that the app makes now and then).
  bool unreachable = false;

  /// True while the changes are being sent.
  bool syncing = false;

  /// What the server did not take at the last sending; stays until the person has seen it.
  List<RejectedChange> rejected = const [];

  /// The lists show downloaded titles only, and nothing is asked of the server.
  bool get active => manual || unreachable;

  /// How many changes wait for the server.
  int get pending => outbox.length;

  void setManual(bool value) {
    if (manual == value) return;
    manual = value;
    store?.write(value);
    notifyListeners();
  }

  void setUnreachable(bool value) {
    if (unreachable == value) return;
    unreachable = value;
    notifyListeners();
  }

  /// Sends the changes that wait. Null if there is nothing to send or the sending is going on already.
  Future<SyncResult?> sync(OutboxApi api) async {
    if (syncing || outbox.isEmpty) return null;
    syncing = true;
    notifyListeners();
    try {
      final result = await outbox.replay(api);
      // Only what the person did themselves is told; a position of the listening that the server does not know any more is of no interest.
      final shown = [for (final r in result.rejected) if (r.change.kind != OutboxChange.progress && r.change.kind != OutboxChange.episode) r];
      if (shown.isNotEmpty) rejected = [...rejected, ...shown];
      return result;
    } finally {
      syncing = false;
      notifyListeners();
    }
  }

  /// The outbox changed (the number of waiting changes).
  void touch() => notifyListeners();

  void dismissRejected() {
    if (rejected.isEmpty) return;
    rejected = const [];
    notifyListeners();
  }
}

class OfflineScope extends InheritedNotifier<OfflineController> {
  const OfflineScope({super.key, required OfflineController controller, required super.child}) : super(notifier: controller);

  static OfflineController? maybeOf(BuildContext context) => context.dependOnInheritedWidgetOfExactType<OfflineScope>()?.notifier;
}
