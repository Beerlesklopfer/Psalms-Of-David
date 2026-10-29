// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/cd/cd_models.dart';
import 'package:psalm/cd/cd_ports.dart';

const audioDrive = CdDriveInfo(
  device: 'drive0',
  tray: CdTray.closed,
  medium: CdMedium.audio,
  tracks: 3,
  trackList: [
    CdTrackInfo(number: 1, seconds: 60),
    CdTrackInfo(number: 2, seconds: 120),
    CdTrackInfo(number: 3, seconds: 185),
  ],
  seconds: 365,
);

/// The server's CD interface without a server.
class FakeCdServer implements CdServerApi {
  CdSettings settingsValue = const CdSettings();
  Object? settingsError;
  CdDrivesState drivesValue = const CdDrivesState([audioDrive]);
  Object? drivesError;
  List<CdCandidate> candidates = const [];
  Object? lookupError;
  Object? startError;
  String startedId = '12';
  String? resumeAnswer;
  final allStarted = <CdRipRequest>[];
  final dismissed = <String?>[];
  CdRipRequest? started;
  final cancelled = <String>[];
  Object? cancelError;
  final ejects = <String?>[];
  bool ejectResult = true;
  Object? ejectError;
  String? lookedUpDevice;

  /// Every lookup with what was asked beyond the disc.
  final lookups = <({String? query, String? releaseId})>[];

  /// What a release chosen from a search answers with, by its id (else the candidate itself).
  final fetched = <String, CdCandidate>{};

  /// What a search by hand finds (appended to [candidates]).
  List<CdCandidate> searchResults = const [];

  @override
  Future<CdSettings> settings() async {
    if (settingsError != null) throw settingsError!;
    return settingsValue;
  }

  @override
  Future<CdDrivesState> drives() async {
    if (drivesError != null) throw drivesError!;
    return drivesValue;
  }

  @override
  Future<List<CdCandidate>> lookup({String? device, String? query, String? releaseId}) async {
    lookedUpDevice = device;
    lookups.add((query: query, releaseId: releaseId));
    if (releaseId != null) {
      return [
        for (final c in [...candidates, ...searchResults])
          if (c.mbid == releaseId) fetched[releaseId] ?? c,
      ];
    }
    if (query != null) return [...candidates, ...searchResults];
    if (lookupError != null) throw lookupError!;
    return candidates;
  }

  @override
  Future<CdRipStart> startRip(CdRipRequest request) async {
    if (startError != null) throw startError!;
    started = request;
    allStarted.add(request);
    return CdRipStart(startedId, resume: resumeAnswer);
  }

  @override
  Future<void> dismissRip({String? device}) async {
    dismissed.add(device);
  }

  @override
  Future<void> cancelRip(String jobId) async {
    if (cancelError != null) throw cancelError!;
    cancelled.add(jobId);
  }

  @override
  Future<bool> eject({String? device}) async {
    ejects.add(device);
    if (ejectError != null) throw ejectError!;
    return ejectResult;
  }
}

/// The event channel without a network: the test sends the messages.
class FakeEvents implements CdEventsSource {
  final connections = <StreamController<CdJobState>>[];
  List<CdJobState> open = [];
  final jobs = <String, CdJobState>{};
  final driveStream = StreamController<CdDrivesState>.broadcast();
  CdDrivesState drivesValue = CdDrivesState.none;
  Object? drivesError;
  int driveReads = 0;

  @override
  Stream<CdJobState> connect() {
    final c = StreamController<CdJobState>();
    connections.add(c);
    return c.stream;
  }

  @override
  Future<CdJobState?> job(String id) async => jobs[id];

  @override
  Future<List<CdJobState>> openJobs() async => open;

  @override
  Stream<CdDrivesState> get driveChanged => driveStream.stream;

  @override
  Future<CdDrivesState> drives() async {
    driveReads++;
    if (drivesError != null) throw drivesError!;
    return drivesValue;
  }
}
