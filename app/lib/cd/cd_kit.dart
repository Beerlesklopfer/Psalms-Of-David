// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import '../server/api_client.dart';
import 'cd_drive_watcher.dart';
import 'cd_events.dart';
import 'cd_import_controller.dart';
import 'cd_models.dart';
import 'cd_ports.dart';
import 'server_cd_api.dart';

/// What the library needs for the CD symbols: the server's drive state, its jobs, a way to make the
/// import controller and to eject. Null in the screen where there is no server (demo mode).
class CdKit {
  CdKit({required this.server, required this.drives, required this.tracker});

  final CdServerApi server;
  final CdDriveWatcher drives;
  final CdJobTracker tracker;

  CdImportController controllerFor(CdDriveInfo drive, {String? resumeJobId}) =>
      CdImportController(server: server, tracker: tracker, drive: drive, resumeJobId: resumeJobId);

  void dispose() {
    drives.dispose();
    tracker.dispose();
  }
}

/// Builds the CD kit for a paired server and starts following it. Works on every platform: the
/// drive is the server's, the app only shows its state.
CdKit createCdKit(ApiClient api) {
  final events = serverCdEvents(api);
  final watcher = CdDriveWatcher(events);
  final tracker = CdJobTracker(events, onReconnected: watcher.reload);
  tracker.start();
  watcher.start();
  return CdKit(server: ServerCdApi(api), drives: watcher, tracker: tracker);
}
