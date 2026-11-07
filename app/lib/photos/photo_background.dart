// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:flutter/foundation.dart';
import 'package:workmanager/workmanager.dart';

import '../server/api_client.dart';
import '../server/app_paths.dart';
import '../server/credentials.dart';
import 'photo_backup_api.dart';
import 'photo_device.dart';
import 'photo_library_device.dart';
import 'photo_sync.dart';

/// The unique name of the job of WorkManager.
const kPhotoJob = 'david-photo-backup';

/// How often the system starts the job, at most (it waits for the conditions: Wi-Fi, charging).
const kPhotoJobEvery = Duration(hours: 1);

/// The background job: Android and iOS (the desktop has no camera roll).
bool get photoBackupSupported => !kIsWeb && (Platform.isAndroid || Platform.isIOS);

/// iOS runs the job when it decides (a "processing" task, mostly at night while charging), and once each time: it is asked for again after
/// every run. It knows "any network" only, so "Wi-Fi only" is checked here.
bool get _ios => !kIsWeb && Platform.isIOS;

/// The device is on Wi-Fi or a cable (iOS; Android has the condition in WorkManager).
Future<bool> _onWifi() async {
  try {
    final kinds = await Connectivity().checkConnectivity();
    return kinds.contains(ConnectivityResult.wifi) || kinds.contains(ConnectivityResult.ethernet);
  } on Object {
    return false;
  }
}

/// The entry point of the job, in an isolate of its own without the app: it reads the pairing and runs the backup once.
@pragma('vm:entry-point')
void photoBackupDispatcher() {
  PhotoSync? running;
  Workmanager().executeTask((task, input) async {
    await AppPaths.init();
    final creds = CredentialStore.standard().load();
    if (creds == null) return true;
    final client = ApiClient(address: creds.address, fingerprint: creds.fingerprint, ca: creds.ca, token: creds.token);
    final store = PhotoDeviceStore.standard();
    final api = ServerPhotoBackupApi(client);
    final sync = running = PhotoSync(api: api, library: DevicePhotoLibrary(background: true), store: store);
    try {
      if (_ios) {
        // Not on Wi-Fi while the profile wants it: only ask for the next time.
        final settings = await api.settings().catchError((Object _) => null);
        if (settings != null && settings.wifiOnly && !await _onWifi()) {
          await schedulePhotoBackup(settings, store.loadSettings(), store);
          return true;
        }
      }
      final result = await sync.run();
      if (result.settings case final s?) await schedulePhotoBackup(s, store.loadSettings(), store);
      // A broken connection is tried again by WorkManager (with its backoff); everything else waits for the next period.
      return result.stop != PhotoRunStop.network;
    } finally {
      client.close();
    }
  }, onTaskStopped: (task, reason) async => running?.stop());
}

/// Starts WorkManager in the app (once, at the start).
Future<void> initPhotoBackup() async {
  if (!photoBackupSupported) return;
  await Workmanager().initialize(photoBackupDispatcher);
}

/// Registers the job with the conditions of the profile ([settings]: Wi-Fi only, charging only), or removes it when the backup does not run
/// on this device. Registers again only when the conditions change, so the period of the system is not started over.
Future<void> schedulePhotoBackup(PhotoSettings settings, PhotoDeviceSettings device, PhotoDeviceStore store) async {
  if (!photoBackupSupported) return;
  final on = settings.active && device.enabled;
  final wanted = on ? 'wifi=${settings.wifiOnly},charging=${settings.chargingOnly}' : 'off';
  final state = store.load();
  // On iOS the task runs once per request: it is asked for again every time.
  if (state.constraints == wanted && !(on && _ios)) return;
  if (on && _ios) {
    await Workmanager().registerProcessingTask(
      kPhotoJob,
      kPhotoJob,
      initialDelay: kPhotoJobEvery,
      constraints: Constraints(networkType: NetworkType.connected, requiresCharging: settings.chargingOnly),
    );
  } else if (on) {
    await Workmanager().registerPeriodicTask(
      kPhotoJob,
      kPhotoJob,
      frequency: kPhotoJobEvery,
      existingWorkPolicy: ExistingPeriodicWorkPolicy.update,
      backoffPolicy: BackoffPolicy.exponential,
      backoffPolicyDelay: const Duration(minutes: 10),
      constraints: Constraints(
        networkType: settings.wifiOnly ? NetworkType.unmetered : NetworkType.connected,
        requiresCharging: settings.chargingOnly,
        requiresBatteryNotLow: true,
        requiresStorageNotLow: false,
        requiresDeviceIdle: false,
      ),
    );
  } else {
    await Workmanager().cancelByUniqueName(kPhotoJob);
  }
  // Only the run writes the state file; outside of a run the lock keeps the two apart.
  if (store.tryLock()) {
    try {
      final fresh = store.load()..constraints = wanted;
      store.save(fresh);
    } finally {
      store.unlock();
    }
  }
}
