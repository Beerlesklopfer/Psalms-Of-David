// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'cd_models.dart';

/// State of a CD import job as the server holds it (`docs/spec-cd-import-api.md`, "Jobs").
/// Where a job stands, as the app shows it: [running] (the server or an app works on it, a bar), [waitingApproval] and [waitingRetry]
/// (a person must act, no bar) or [ended] (anything else, also a state this app does not know: it is never shown as running).
enum CdJobKind { running, waitingApproval, waitingRetry, ended }

class CdJobState {
  const CdJobState({
    required this.id,
    required this.state,
    this.phase,
    this.track,
    this.tracksTotal,
    this.fraction,
    this.message,
    this.title,
    this.disc,
    this.discsTotal,
    this.trackStates = const [],
    this.buffer,
    this.error,
    this.device,
    this.active = true,
    this.cdState,
  });

  final String id;

  /// The old word: `working`, `done` or `failed`; a newer server also sends `present`, `skipped`, `retry_wait`, `awaiting_approval`,
  /// `rejected`.
  final String state;

  /// The stored state of the server (`ripping`, `retry_wait`, `ripped`, `awaiting_approval`, `imported`, `rejected`, `present`,
  /// `cancelled`, `failed`, `skipped`); null for an older server, which sends only [state].
  final String? cdState;

  /// `ripping`, `encoding`, `uploading`, `placing`, `importing`, `done`, `failed`.
  final String? phase;
  final double? fraction;
  final int? track;
  final int? tracksTotal;
  final String? message;
  final String? title;

  /// The number of the CD and how many there are (`disc`, `discs_total`), when the job knows them.
  final int? disc;
  final int? discsTotal;

  /// The chosen tracks of a rip with their state; empty for other jobs.
  final List<CdTrackState> trackStates;

  /// The ring buffer of the reader, when the job reports it.
  final CdBuffer? buffer;

  /// The short error code of a failed job (`disc_exists`, `import_failed`, ...).
  final String? error;

  /// Which device of the account runs the job (a piece of the token's hash).
  final String? device;
  final bool active;

  /// The state that counts: `cd_state` when the server sends it, else the old `state`.
  String get stateName => cdState ?? state;

  /// The one place that says what a job is doing now. Only `ripping`/`working` (and the short `ripped`) with `active` not false is running;
  /// `awaiting_approval` and `retry_wait` wait for a person; everything else, an unknown state too, has ended.
  CdJobKind get kind {
    switch (stateName) {
      case 'ripping':
      case 'working':
      case 'ripped':
        return active ? CdJobKind.running : CdJobKind.ended;
      // The files are placed, but the library import still runs (the old word `working` says so).
      case 'imported':
        return state == 'working' && active ? CdJobKind.running : CdJobKind.ended;
      case 'awaiting_approval':
        return CdJobKind.waitingApproval;
      case 'retry_wait':
        return CdJobKind.waitingRetry;
      default:
        return CdJobKind.ended;
    }
  }

  bool get running => kind == CdJobKind.running;
  bool get waiting => kind == CdJobKind.waitingApproval || kind == CdJobKind.waitingRetry;
  bool get finished => kind == CdJobKind.ended;

  /// The job ended well: `done`, or `imported` (the old server says `done`).
  bool get succeeded => finished && (stateName == 'done' || stateName == 'imported');

  /// The track being worked on, counted from 1: the server's `track` is the number of tracks done.
  int? get currentTrack {
    // With the list of the tracks the place is where the rip is (the track that is being read or checked), not how many are done: with
    // the second reading none is done while the first reading goes through the disc.
    final at = trackStates.indexWhere((s) => s.state == 'reading' || s.state == 'verifying');
    if (running && at >= 0) return at + 1;
    final t = track, n = tracksTotal;
    if (t == null || n == null || n <= 0) return null;
    return finished ? (succeeded ? n : null) : (t + 1).clamp(1, n);
  }

  /// How far the whole job is, 0 to 1: the server's `fraction`, else the tracks done of all; null if neither is known.
  double? get overall {
    final f = fraction;
    if (f != null) return f.clamp(0.0, 1.0);
    final t = track, n = tracksTotal;
    return t == null || n == null || n <= 0 ? null : (t / n).clamp(0.0, 1.0);
  }

  factory CdJobState.fromJson(Map<String, dynamic> j) => CdJobState(
    id: '${j['id']}',
    state: (j['state'] as String?) ?? 'working',
    cdState: j['cd_state'] as String?,
    phase: j['phase'] as String?,
    fraction: (j['fraction'] as num?)?.toDouble(),
    track: j['track'] as int?,
    tracksTotal: j['tracks_total'] as int?,
    message: j['message'] as String?,
    title: (j['album'] as String?) ?? (j['title'] as String?),
    disc: j['disc'] as int?,
    discsTotal: j['discs_total'] as int?,
    trackStates: CdTrackState.list(j['track_states']),
    buffer: CdBuffer.from(j),
    error: (j['error'] as String?)?.isEmpty ?? true ? null : j['error'] as String?,
    device: j['device'] as String?,
    active: (j['active'] as bool?) ?? true,
  );

  @override
  bool operator ==(Object other) =>
      other is CdJobState &&
      other.id == id &&
      other.state == state &&
      other.cdState == cdState &&
      other.active == active &&
      other.phase == phase &&
      other.fraction == fraction &&
      other.track == track &&
      other.tracksTotal == tracksTotal &&
      other.trackStates.length == trackStates.length &&
      _sameStates(other.trackStates, trackStates) &&
      other.buffer?.seconds == buffer?.seconds &&
      other.message == message &&
      other.error == error;

  @override
  int get hashCode => Object.hash(
    id,
    state,
    cdState,
    active,
    phase,
    fraction,
    track,
    tracksTotal,
    trackStates.length,
    buffer?.seconds,
    message,
    error,
  );
}

bool _sameStates(List<CdTrackState> a, List<CdTrackState> b) {
  for (var i = 0; i < a.length && i < b.length; i++) {
    if (a[i].state != b[i].state || a[i].fraction != b[i].fraction || a[i].error != b[i].error) {
      return false;
    }
  }
  return true;
}

/// The server's push channel for import jobs, and the fallback to ask for one job.
abstract class CdEventsSource {
  /// Job states as they change. The stream ends (with an error or normally) when the connection
  /// breaks; [CdJobTracker] reconnects.
  Stream<CdJobState> connect();

  /// One job by id (`GET cd/import/{id}`), or null if unknown.
  Future<CdJobState?> job(String id);

  /// The state of the drives, pushed by the server (`drive` message,
  /// `{"type":"drive","mode":..,"drives":[...]}`, see `docs/spec-cd-import-api.md`).
  Stream<CdDrivesState> get driveChanged;

  /// The drives now (`GET cd/drive`).
  Future<CdDrivesState> drives();

  /// The jobs that are still open, for a freshly started app or another device.
  Future<List<CdJobState>> openJobs();
}

/// What one WebSocket message says: job states (`job`, `progress`, `done`, `failed` carry one,
/// `snapshot` carries the open jobs) or the state of the drives (`drive`).
class CdMessage {
  const CdMessage(this.jobs, {this.drives});
  final List<CdJobState> jobs;
  final CdDrivesState? drives;
}

/// Parses one text message of `GET cd/events`; unknown types and broken text give an empty message.
CdMessage parseCdMessage(String text) {
  try {
    final j = jsonDecode(text);
    if (j is! Map<String, dynamic>) return const CdMessage([]);
    switch (j['type']) {
      case 'job' || 'progress' || 'done' || 'failed':
        return CdMessage([CdJobState.fromJson(j['job'] as Map<String, dynamic>)]);
      case 'snapshot':
        return CdMessage([
          for (final e in (j['jobs'] as List? ?? const []).cast<Map<String, dynamic>>())
            CdJobState.fromJson(e),
        ]);
      case 'drive':
        return CdMessage(const [], drives: CdDrivesState.fromJson(j));
    }
  } on Object {
    // not an event
  }
  return const CdMessage([]);
}

/// Opens a WebSocket; replaceable in tests. [url] uses `ws`/`wss`.
typedef WebSocketOpener = Future<WebSocket> Function(String url, Map<String, String> headers);

/// Keeps the picture of the server's import jobs: asks once for the open jobs, then follows the
/// push channel, and reconnects after [retryDelay] when it breaks. Nothing runs before [start];
/// [dispose] closes the connection and cancels a pending retry.
class CdJobTracker extends ValueNotifier<Map<String, CdJobState>> {
  CdJobTracker(this._source, {this.retryDelay = const Duration(seconds: 5), this.onReconnected})
    : super(const {});

  /// Called when the channel is connected again after a break (settings may have changed meanwhile).
  final void Function()? onReconnected;

  final CdEventsSource _source;
  final Duration retryDelay;
  StreamSubscription<CdJobState>? _sub;
  Timer? _retry;
  bool _disposed = false;
  bool _started = false;

  /// Jobs not yet finished.
  List<CdJobState> get open {
    final list = [
      for (final j in value.values)
        if (!j.finished) j,
    ];
    // Running jobs first, then the ones that wait for a person; the newest (highest id) first in each group.
    list.sort((a, b) {
      final k = (a.running ? 0 : 1).compareTo(b.running ? 0 : 1);
      if (k != 0) return k;
      final x = int.tryParse(a.id), y = int.tryParse(b.id);
      return x != null && y != null ? y.compareTo(x) : b.id.compareTo(a.id);
    });
    return list;
  }

  /// The jobs that really run now (not the ones that wait for a person).
  List<CdJobState> get running => [for (final j in open) if (j.running) j];

  void start() {
    if (_started || _disposed) return;
    _started = true;
    unawaited(_loadOpen());
    _connect();
  }

  Future<void> _loadOpen() async {
    try {
      for (final j in await _source.openJobs()) {
        _apply(j);
      }
    } on Object {
      // The push channel fills it in; no list is not an error.
    }
  }

  void _apply(CdJobState j) {
    if (_disposed) return;
    value = {...value, j.id: j};
  }

  void _connect() {
    if (_disposed) return;
    _sub = _source.connect().listen(
      _apply,
      onError: (_) => _scheduleRetry(),
      onDone: _scheduleRetry,
      cancelOnError: true,
    );
  }

  void _scheduleRetry() {
    if (_disposed) return;
    _sub?.cancel();
    _sub = null;
    _retry?.cancel();
    _retry = Timer(retryDelay, () {
      // Events may have been missed meanwhile: ask again for the open jobs.
      unawaited(_loadOpen());
      onReconnected?.call();
      _connect();
    });
  }

  /// Asks the server for job [id] and takes the answer (the fallback when the channel is down).
  Future<void> refresh(String id) async {
    try {
      final j = await _source.job(id);
      if (j != null) _apply(j);
    } on Object {
      // try again later
    }
  }

  /// Waits for [id] to finish, from the push channel; asks `job(id)` if the channel is silent for [poll].
  Future<CdJobState> waitFor(String id, {Duration poll = const Duration(seconds: 10)}) async {
    final done = Completer<CdJobState>();
    void check() {
      final j = value[id];
      if (j != null && j.finished && !done.isCompleted) done.complete(j);
    }

    void listener() => check();
    addListener(listener);
    final timer = Timer.periodic(poll, (_) async {
      try {
        final j = await _source.job(id);
        if (j != null) _apply(j);
      } on Object {
        // try again at the next tick
      }
    });
    check();
    try {
      return await done.future;
    } finally {
      timer.cancel();
      removeListener(listener);
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _retry?.cancel();
    _sub?.cancel();
    super.dispose();
  }
}

/// The real source: WebSocket to `/api/v1/cd/events`, REST for the rest. [socket] opens the
/// connection with the app's own HttpClient so the pinned fingerprint or CA mode applies.
class SocketCdEventsSource implements CdEventsSource {
  SocketCdEventsSource({
    required this.url,
    required this.headers,
    required this.socket,
    required this.getJson,
  });

  /// `wss://host:port/api/v1/cd/events`
  final String url;
  final Map<String, String> headers;
  final WebSocketOpener socket;

  /// GET returning decoded JSON (the app's authenticated client).
  final Future<Object?> Function(String path) getJson;

  final _drives = StreamController<CdDrivesState>.broadcast();

  @override
  Stream<CdDrivesState> get driveChanged => _drives.stream;

  @override
  Future<CdDrivesState> drives() async =>
      CdDrivesState.fromJson(await getJson('/api/v1/cd/drive') as Map<String, dynamic>);

  @override
  Stream<CdJobState> connect() async* {
    final ws = await socket(url, headers);
    try {
      await for (final m in ws) {
        if (m is! String) continue;
        final msg = parseCdMessage(m);
        if (msg.drives != null) _drives.add(msg.drives!);
        for (final j in msg.jobs) {
          yield j;
        }
      }
    } finally {
      await ws.close();
    }
  }

  @override
  Future<CdJobState?> job(String id) async {
    try {
      final j = await getJson('/api/v1/cd/import/$id');
      final job = j is Map<String, dynamic> ? j['job'] : null;
      return job is Map<String, dynamic> ? CdJobState.fromJson(job) : null;
    } on Object {
      return null;
    }
  }

  @override
  Future<List<CdJobState>> openJobs() async {
    final j = await getJson('/api/v1/cd/import');
    final list = j is Map<String, dynamic> ? (j['jobs'] as List? ?? const []) : const [];
    return [for (final e in list.cast<Map<String, dynamic>>()) CdJobState.fromJson(e)]
        .where((s) => !s.finished)
        .toList();
  }
}
