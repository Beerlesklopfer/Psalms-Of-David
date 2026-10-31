// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/foundation.dart';

import '../inbox/inbox_api.dart';
import '../server/api_client.dart';
import 'cd_events.dart';
import 'cd_models.dart';
import 'cd_ports.dart';

/// `present`: the disc is in the library already; the dialog offers a check and repair, not an import. `ended`: the job we follow ended
/// in a state that is neither a success nor an error (`present`, `rejected`); the dialog says which, from [CdImportController.job].
enum CdPhase { identifying, present, confirm, starting, running, done, failed, cancelled, ended }

/// The tray in the `present` view: `idle` (the CD is in the drive), `ejecting` (the call runs), `ejected` (the CD is out, by the
/// app or by the server), `failed` (the tray did not open; the CD stays and the person may try again or import anyway).
enum CdEjectPhase { idle, ejecting, ejected, failed }

/// A move of a job that waits after a read error: `retrying` and `givingUp` run a call; the button is off meanwhile.
enum CdMovePhase { idle, retrying, givingUp }

/// Time between two direct questions about the running job, as a safety net for the event channel.
const Duration kCdJobPoll = Duration(seconds: 10);

/// The kinds the user can choose between (the server's names). The default comes from the server's settings.
const List<String> kCdKinds = ['audiobook', 'music'];

/// One CD import as the user sees it: look the disc up, ask, start the rip on the server, follow the
/// job and show its result. The reading, encoding and filing happen on the server; this class only
/// talks to it, through [CdServerApi] and the job tracker, so it runs on fakes in tests.
class CdImportController extends ChangeNotifier {
  CdImportController({
    required this.server,
    required this.tracker,
    required this.drive,
    this.resumeJobId,
    this.inboxApi,
    this.pollInterval = kCdJobPoll,
  }) : phase = resumeJobId == null ? CdPhase.identifying : CdPhase.running {
    tracker.addListener(_onJobs);
  }

  final CdServerApi server;
  final CdJobTracker tracker;

  /// The client of the job moves (`retry`, `give_up`) that the inbox uses too; null where there is no inbox (the buttons are then not shown).
  final InboxApi? inboxApi;

  /// The drive to import from (null: the server chooses). Only the name the server gave it.
  final CdDriveInfo drive;

  /// Set when the dialog opens on an import that is already running (also one of another device).
  final String? resumeJobId;

  /// How often the job is asked for when no news comes over the channel.
  final Duration pollInterval;
  Timer? _poll;

  CdPhase phase;

  /// The code of the failure (see `kCdErrorCodes` in cd_errors.dart); the dialog turns it into words.
  String? failureCode;
  String? failureDetail;

  /// For `disc_exists`: the numbers of the tracks that are in the library already.
  List<int> failureTaken = const [];

  /// The code of a failed lookup (for instance `no_contact`); the form is then filled by hand.
  String? lookupError;
  CdSettings settings = const CdSettings();
  List<CdCandidate> candidates = const [];
  int candidateIndex = 0;
  bool lookupFound = false;

  // The form.
  String author = '';
  String title = '';
  int disc = 1;
  int? discsTotal;
  String kind = CdDefaults.kind;
  bool eject = false;

  /// Titles by position on the disc; an empty text where none is known (the dialog then says "Track 01").
  List<String> trackTitles = [];

  /// The numbers on the disc of the tracks to import. All of them at first.
  final Set<int> selected = {};

  /// The tracks of the disc with number and length. The server's list when it gives one, else 1..n.
  late final List<CdTrackInfo> tracks = drive.trackList.isNotEmpty
      ? drive.trackList
      : [for (var i = 1; i <= drive.tracks; i++) CdTrackInfo(number: i)];

  String? jobId;

  /// The disc is in the library already (from the drive's state, or from a refusal of the server).
  CdPresent? present;

  /// The state of the tray in the `present` view. A disc that is whole in the library is ejected at once; one with gaps stays in the
  /// drive (the server may repair it) until the person ejects it.
  CdEjectPhase ejectPhase = CdEjectPhase.idle;

  /// The code of a failed eject (`eject_failed`, or the server's own code), for the dialog to turn into words.
  String? ejectError;

  /// The move of the job in `retry_wait` that is under way, and the code of the last one that failed (`forbidden`, `invalid_move`, ...).
  CdMovePhase movePhase = CdMovePhase.idle;
  String? moveError;

  /// The job shown waits after a read error and the app can offer to try again or give up.
  bool get canMoveJob =>
      inboxApi != null && job?.kind == CdJobKind.waitingRetry && int.tryParse(jobId ?? '') != null;

  /// A hint that comes with a good end (`unreadable_track`, `locked`), and the job's own message.
  String? doneNote;
  String? doneDetail;

  /// The title of the track [number] of the disc, from the lookup; empty if not known.
  String titleOf(int number) {
    final c = candidates.isEmpty ? null : candidates.first;
    final i = tracks.indexWhere((t) => t.number == number);
    if (c == null || i < 0 || i >= c.trackTitles.length) return '';
    return c.trackTitles[i].trim();
  }

  /// The user chose to import a disc that is in the library as a new copy. Only after a confirmation.
  bool force = false;

  /// Read the disc again from the start, even if an interrupted rip could be continued.
  bool rereadFromStart = false;

  /// What the server did with an earlier rip of the disc: `resumed`, `fresh` or `mismatch`.
  String? resumeAnswer;

  /// The latest state of the job on the server.
  CdJobState? job;
  bool _disposed = false;

  int get trackCount => tracks.length;

  /// An interrupted rip of this disc that can be continued with the choice made now; null if none. The choice may differ from the one of that
  /// rip: what was read and is chosen again is taken over, so the offer stands as long as the settings fit and at least one chosen track was
  /// read then (server since 2026-10-29).
  CdResume? get resumeOffer {
    final r = drive.resume;
    if (r == null || !r.settingsMatch) return null;
    return r.tracksDone.any(selected.contains) ? r : null;
  }

  /// An interrupted rip exists but cannot be continued (the settings changed): the disc is read again.
  bool get resumeLost => drive.resume != null && !drive.resume!.settingsMatch;

  /// The chosen tracks with their state while a rip runs: from the job, else from the drive.
  List<CdTrackState> get trackStates {
    final fromJob = job?.trackStates ?? const [];
    if (fromJob.isNotEmpty) return fromJob;
    final rip = drive.rip;
    return rip != null && rip.jobId == jobId ? rip.trackStates : const [];
  }

  /// The ring buffer of the reader, when the job reports it.
  CdBuffer? get buffer => job?.buffer;

  bool get allSelected => selected.length == tracks.length;
  int get selectedCount => selected.length;

  /// Playing time of the chosen tracks.
  Duration get selectedDuration => Duration(
    seconds: tracks
        .where((t) => selected.contains(t.number))
        .fold<double>(0, (a, t) => a + t.seconds)
        .round(),
  );

  void selectAll() {
    selected
      ..clear()
      ..addAll(tracks.map((t) => t.number));
    _notify();
  }

  void selectNone() {
    selected.clear();
    _notify();
  }

  void invertSelection() {
    final now = tracks.map((t) => t.number).where((n) => !selected.contains(n)).toList();
    selected
      ..clear()
      ..addAll(now);
    _notify();
  }

  void toggle(int number) {
    if (!selected.remove(number)) selected.add(number);
    _notify();
  }

  bool get canSave =>
      phase == CdPhase.confirm &&
      selected.isNotEmpty &&
      author.trim().isNotEmpty &&
      title.trim().isNotEmpty &&
      disc >= 1;
  bool get busy => phase == CdPhase.starting || phase == CdPhase.running;

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  CdCandidate? get _candidate =>
      lookupFound && candidates.isNotEmpty ? candidates[candidateIndex] : null;

  /// Reads the server's settings and looks the disc up.
  Future<void> identify() async {
    if (resumeJobId != null) {
      jobId = resumeJobId;
      _startPolling();
      _onJobs();
      return;
    }
    if (drive.present != null && !force) {
      present = drive.present;
      // The titles of the tracks make the list of what is missing readable; without them the numbers show.
      try {
        candidates = await server.lookup(device: drive.device);
      } on Object {
        candidates = const [];
      }
      phase = CdPhase.present;
      _notify();
      if (present!.complete) await ejectNow();
      return;
    }
    await _loadForm();
  }

  /// The user knows the disc is in the library and wants a new copy of it anyway.
  Future<void> importAnyway() async {
    force = true;
    present = null;
    ejectPhase = CdEjectPhase.idle;
    ejectError = null;
    phase = CdPhase.identifying;
    _notify();
    await _loadForm();
  }

  /// Opens the tray of the drive. Used by the automatic eject of a complete disc and by the Eject button; does nothing while a call
  /// runs or when the disc is out already.
  Future<void> ejectNow() async {
    if (_disposed || ejectPhase == CdEjectPhase.ejecting || ejectPhase == CdEjectPhase.ejected) return;
    ejectPhase = CdEjectPhase.ejecting;
    ejectError = null;
    _notify();
    try {
      final out = await server.eject(device: drive.device);
      ejectPhase = out ? CdEjectPhase.ejected : CdEjectPhase.failed;
      if (!out) ejectError = 'nothing_to_eject';
    } on Object catch (e) {
      ejectPhase = CdEjectPhase.failed;
      ejectError = e is ApiException && e.code.isNotEmpty ? e.code : 'eject_failed';
    }
    _notify();
  }

  Future<void> _loadForm() async {
    try {
      settings = await server.settings();
    } on Object {
      // The fallbacks in CdDefaults stand in; starting the rip will say if the server is not there.
    }
    kind = kCdKinds.contains(settings.kind) ? settings.kind : CdDefaults.kind;
    eject = settings.eject;
    try {
      candidates = await server.lookup(device: drive.device);
    } on Object catch (e) {
      candidates = const [];
      lookupError = e is ApiException ? e.code : 'lookup_failed';
    }
    lookupFound = candidates.isNotEmpty;
    trackTitles = List.filled(trackCount, '');
    selectAll();
    if (lookupFound) {
      selectCandidate(0);
    } else {
      phase = CdPhase.confirm;
      _notify();
    }
  }

  /// True while a search by hand or the fetching of a chosen release runs.
  bool searching = false;

  /// The code of a failed search (for instance `invalid_query`); null when it went well.
  String? searchError;

  /// Searches MusicBrainz for words typed by hand (title, author) and adds what is found to the matches. The form is not touched: the
  /// person chooses from the matches.
  Future<void> search(String words) async {
    final q = words.trim();
    if (q.isEmpty || searching) return;
    searching = true;
    searchError = null;
    _notify();
    try {
      final found = await server.lookup(device: drive.device, query: q);
      if (found.isNotEmpty) {
        candidates = found;
        lookupFound = true;
        lookupError = null;
        if (candidateIndex >= candidates.length) candidateIndex = 0;
      } else {
        searchError = 'not_found';
      }
    } on Object catch (e) {
      searchError = e is ApiException ? e.code : 'lookup_failed';
    }
    searching = false;
    _notify();
  }

  void selectCandidate(int i) {
    candidateIndex = i;
    final c = candidates[i];
    // A release from a search has no track titles yet: the server gives them for the disc that has as many tracks as this CD.
    if (c.trackTitles.isEmpty && (c.mbid ?? '').isNotEmpty) {
      unawaited(_fetchRelease(i));
    }
    author = c.author;
    title = c.title;
    disc = c.disc;
    discsTotal = c.discsTotal;
    trackTitles = [
      for (var k = 0; k < trackCount; k++) k < c.trackTitles.length ? c.trackTitles[k].trim() : '',
    ];
    // A release that the server's lookup calls an audiobook presets the kind.
    if (c.audiobook) kind = 'audiobook';
    phase = CdPhase.confirm;
    _notify();
  }

  Future<void> _fetchRelease(int i) async {
    final id = candidates[i].mbid!;
    searching = true;
    _notify();
    try {
      final full = await server.lookup(device: drive.device, releaseId: id);
      if (full.isNotEmpty && !_disposed && i < candidates.length && candidates[i].mbid == id) {
        candidates = [...candidates]..[i] = full.first;
        if (candidateIndex == i && full.first.trackTitles.isNotEmpty) selectCandidate(i);
      }
    } on Object catch (e) {
      searchError = e is ApiException ? e.code : 'lookup_failed';
    }
    searching = false;
    _notify();
  }

  void setReread(bool v) {
    rereadFromStart = v;
    _notify();
  }

  void update({
    String? author,
    String? title,
    int? disc,
    int? discsTotal,
    String? kind,
    bool? eject,
  }) {
    this.author = author ?? this.author;
    this.title = title ?? this.title;
    this.disc = disc ?? this.disc;
    if (discsTotal != null) this.discsTotal = discsTotal <= 0 ? null : discsTotal;
    this.kind = kind ?? this.kind;
    this.eject = eject ?? this.eject;
    _notify();
  }

  void _fail(String code, [String? detail]) {
    phase = CdPhase.failed;
    failureCode = code;
    failureDetail = detail;
    _notify();
  }

  /// The one Save action of the form: asks the server to read the disc and file it.
  Future<void> save() async {
    if (!canSave) return;
    phase = CdPhase.starting;
    _notify();
    final c = _candidate;
    try {
      final started = await server.startRip(
        CdRipRequest(
          kind: kind,
          author: author.trim(),
          title: title.trim(),
          disc: disc,
          discsTotal: discsTotal,
          narrator: c?.narrator,
          year: c?.year,
          mbid: c?.mbid,
          tracks: trackTitles,
          eject: eject,
          device: drive.device,
          // Only when not all tracks are wanted; the numbers are those on the disc.
          only: allSelected ? null : (selected.toList()..sort()),
          force: force,
          resume: rereadFromStart ? false : null,
        ),
      );
      jobId = started.jobId;
      resumeAnswer = started.resume;
    } on Object catch (e) {
      // The server's own code (`busy`, `tray_open`, `no_disc`, ...); 0 is no answer at all.
      if (e is ApiException) {
        if (_takeRefusal(e)) return;
        failureTaken = [for (final n in (e.body?['taken'] as List? ?? const [])) n as int];
        return _fail(e.code, 'HTTP ${e.status}');
      }
      return _fail('network', '$e');
    }
    phase = CdPhase.running;
    _startPolling();
    _notify();
    _onJobs();
  }

  /// A `disc_present` refusal: the disc is in the library after all. True if it was taken.
  bool _takeRefusal(ApiException e) {
    if (e.code != 'disc_present') return false;
    final b = e.body;
    final p = b?['present'];
    present = p is Map<String, dynamic>
        ? CdPresent.fromJson(p)
        : CdPresent(
            book: (b?['book'] as String?) ?? title,
            author: (b?['author'] as String?) ?? author,
            disc: (b?['disc'] as int?) ?? disc,
          );
    phase = CdPhase.present;
    ejectPhase = CdEjectPhase.idle;
    ejectError = null;
    // The server ejected the disc already when the request said so: no second eject, just the message.
    if (b?['ejected'] == true) {
      ejectPhase = CdEjectPhase.ejected;
    } else if (present!.complete) {
      unawaited(ejectNow());
    }
    _notify();
    return true;
  }

  /// Checks the CD against the library and reads only what is missing or damaged. The details come from
  /// the library; nothing is typed.
  Future<void> heal() async {
    final p = present;
    if (p == null || phase != CdPhase.present) return;
    phase = CdPhase.starting;
    _notify();
    try {
      final started = await server.startRip(
        CdRipRequest(
          kind: kind,
          author: p.author,
          title: p.book,
          disc: p.disc,
          device: drive.device,
          mode: 'heal',
        ),
      );
      jobId = started.jobId;
    } on Object catch (e) {
      if (e is ApiException) return _fail(e.code, 'HTTP ${e.status}');
      return _fail('network', '$e');
    }
    phase = CdPhase.running;
    _startPolling();
    _notify();
    _onJobs();
  }

  /// Reads the CD again (`POST /cd/jobs/{id}/retry`). The job goes back to running with the server's next message; an error is kept in
  /// [moveError] and the button is on again.
  Future<void> retryRead() => _move(CdMovePhase.retrying, (api, id) => api.retry(id));

  /// Ends the read error (`POST /cd/jobs/{id}/give_up`); the job then ends as cancelled. The dialog asks first.
  Future<void> giveUp() => _move(CdMovePhase.givingUp, (api, id) => api.giveUp(id));

  Future<void> _move(CdMovePhase what, Future<void> Function(InboxApi api, int id) call) async {
    final api = inboxApi, id = int.tryParse(jobId ?? '');
    if (api == null || id == null || movePhase != CdMovePhase.idle || !canMoveJob) return;
    movePhase = what;
    moveError = null;
    _notify();
    try {
      await call(api, id);
      // The phase stays until the job's next message says it moved (see _onJobs).
      _pollOnce();
    } on Object catch (e) {
      movePhase = CdMovePhase.idle;
      moveError = e is ApiException && e.code.isNotEmpty ? e.code : 'network';
      // `invalid_move`: the job moved on meanwhile; ask for its state so the dialog follows it.
      _pollOnce();
    }
    _notify();
  }

  void _pollOnce() {
    final id = jobId;
    if (id != null) unawaited(tracker.refresh(id));
  }

  /// Closes the result of the rip on the server (the list of tracks stays there until then).
  Future<void> dismiss() async {
    if (jobId == null) return;
    try {
      await server.dismissRip(device: drive.device);
    } on Object {
      // A result that stays a little longer does no harm.
    }
  }

  void _startPolling() {
    _poll?.cancel();
    _poll = Timer.periodic(pollInterval, (_) {
      final id = jobId;
      if (id != null) unawaited(tracker.refresh(id));
    });
  }

  /// Stops the rip on the server. The result arrives as the job's end.
  Future<void> cancel() async {
    if (phase == CdPhase.confirm || phase == CdPhase.identifying || phase == CdPhase.present) {
      phase = CdPhase.cancelled;
      _notify();
      return;
    }
    final id = jobId;
    if (id == null || !busy) return;
    try {
      await server.cancelRip(id);
    } on Object catch (e) {
      if (e is ApiException && e.code == 'job_closed') return;
      _fail(e is ApiException ? e.code : 'network');
    }
  }

  void _onJobs() {
    final id = jobId;
    if (id == null || _disposed || phase != CdPhase.running) return;
    final j = tracker.value[id];
    if (j == null) return;
    job = j;
    // The server moved the job (running again, ended, ...): the move is over.
    if (j.kind != CdJobKind.waitingRetry) movePhase = CdMovePhase.idle;
    if (j.finished) {
      _poll?.cancel();
      if (j.stateName == 'present' || j.stateName == 'rejected') {
        phase = CdPhase.ended;
      } else if (j.succeeded) {
        phase = CdPhase.done;
        // A job can end well and still carry a hint: `unreadable_track` (some tracks were left out), `locked`.
        doneNote = j.error;
        doneDetail = j.message;
      } else if (j.stateName == 'cancelled' || j.error == 'cancelled' || j.error == 'client') {
        phase = CdPhase.cancelled;
      } else {
        phase = CdPhase.failed;
        failureCode = j.error ?? 'import_failed';
        failureDetail = j.message;
      }
    }
    _notify();
  }

  @override
  void dispose() {
    _disposed = true;
    _poll?.cancel();
    tracker.removeListener(_onJobs);
    super.dispose();
  }
}
