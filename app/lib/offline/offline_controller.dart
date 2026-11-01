// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter/widgets.dart';

import '../server/app_paths.dart';
import 'outbox.dart';

/// How often the app looks for the server while "stay offline" waits for it, unless the person chose another period.
const kDefaultProbeSeconds = 60;

/// The periods to choose from, in seconds.
const kProbePeriods = [30, 60, 300, 900];

/// The setting "offline mode" of this device, in a small file.
class OfflineSettingsStore {
  OfflineSettingsStore(this.file);

  factory OfflineSettingsStore.standard() => OfflineSettingsStore(File('${AppPaths.base.path}/offline.json'));

  final File file;

  Map<String, dynamic> _all() {
    try {
      return jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
    } on Object {
      return {};
    }
  }

  bool read() => _all()['offline'] == true;

  /// Whether the offline mode ends by itself when the server can be reached, not over mobile data (see [OfflineController.stayUntilWifi]).
  bool readUntilWifi() => _all()['until_wifi'] == true;

  /// Seconds between two looks for the server while the offline mode waits for it; null if never set.
  int? readProbeSeconds() => _all()['probe_s'] is int ? _all()['probe_s'] as int : null;

  void write(bool offline, {bool untilWifi = false, int? probeSeconds}) {
    try {
      file.parent.createSync(recursive: true);
      file.writeAsStringSync(jsonEncode({'offline': offline, 'until_wifi': offline && untilWifi, 'probe_s': probeSeconds ?? readProbeSeconds()}));
    } on FileSystemException {
      // The setting is lost at the next start; not worth a failure.
    }
  }
}

/// Offline mode: the app shows only what is on the device and keeps what the person changes until the server can take it
/// (`docs/spec-app-api.md`, "offline mode and synchronisation"). Offline is the person's switch ([manual]) or the fact that the server
/// cannot be reached ([unreachable]); either way the lists show the downloaded titles only.
class OfflineController extends ChangeNotifier {
  OfflineController({Outbox? outbox, this.store, bool manual = false}) : outbox = outbox ?? Outbox.memory(), manual = store?.read() ?? manual,
        untilWifi = (store?.read() ?? manual) && (store?.readUntilWifi() ?? false),
        probeSeconds = store?.readProbeSeconds() ?? kDefaultProbeSeconds;

  final Outbox outbox;
  final OfflineSettingsStore? store;

  /// The person switched the offline mode on: the app does not ask the server at all.
  bool manual;

  /// The offline mode was chosen away from home and ends by itself at the next WLAN (or cable); the person's own switch stays until switched off.
  bool untilWifi = false;

  /// Seconds between two looks for the server while [untilWifi] waits (set in the settings).
  int probeSeconds;

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
    if (manual == value && !untilWifi) return;
    manual = value;
    untilWifi = false;
    store?.write(value);
    notifyListeners();
  }

  void setProbeSeconds(int seconds) {
    if (probeSeconds == seconds) return;
    probeSeconds = seconds;
    store?.write(manual, untilWifi: untilWifi, probeSeconds: seconds);
    notifyListeners();
  }

  /// "Stay offline" of the bar: offline now, and back online when the server answers (not over mobile data).
  void stayUntilWifi() {
    manual = true;
    untilWifi = true;
    store?.write(true, untilWifi: true);
    notifyListeners();
  }

  /// The server answered on a WLAN (or cable): ends an offline mode that was chosen "stay offline". Returns true if it did.
  bool leaveOnWifi() {
    if (!untilWifi) return false;
    setManual(false);
    return true;
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
