// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';

/// Tray of a drive as the server reports it.
enum CdTray {
  open,
  closed,
  unknown;

  static CdTray parse(Object? v) => switch (v) {
    'open' => CdTray.open,
    'closed' => CdTray.closed,
    _ => CdTray.unknown,
  };
}

/// What is in a drive: nothing, an audio CD, another disc (data, blank) or not known.
enum CdMedium {
  none,
  audio,
  other,
  unknown;

  static CdMedium parse(Object? v) => switch (v) {
    'none' => CdMedium.none,
    'audio' => CdMedium.audio,
    'other' => CdMedium.other,
    _ => CdMedium.unknown,
  };
}

/// An earlier rip of this disc that was interrupted and can be continued (`resume` of a drive).
class CdResume {
  const CdResume({
    required this.discId,
    this.tracksDone = const [],
    this.tracksTotal = 0,
    this.only = const [],
    this.interruptedAt,
    this.keepUntil,
    this.settingsMatch = true,
  });

  final String discId;
  final List<int> tracksDone;
  final int tracksTotal;

  /// The choice of tracks of that rip; empty is all.
  final List<int> only;
  final DateTime? interruptedAt;
  final DateTime? keepUntil;

  /// The settings that decide the audio are still the same; if not, the disc is read again.
  final bool settingsMatch;

  static List<int> _ints(Object? v) => [for (final n in (v as List? ?? const [])) n as int];
  static DateTime? _ms(Object? v) => v is int ? DateTime.fromMillisecondsSinceEpoch(v) : null;

  factory CdResume.fromJson(Map<String, dynamic> j) => CdResume(
    discId: (j['disc_id'] as String?) ?? '',
    tracksDone: _ints(j['tracks_done']),
    tracksTotal: (j['tracks_total'] as int?) ?? 0,
    only: _ints(j['only']),
    interruptedAt: _ms(j['interrupted_at']),
    keepUntil: _ms(j['keep_until']),
    settingsMatch: (j['settings_match'] as bool?) ?? true,
  );
}

/// A file of a CD in the library that does not match the disc.
class CdDamaged {
  const CdDamaged({required this.number, this.reason = '', this.haveSeconds, this.wantSeconds});

  final int number;

  /// `empty`, `unreadable` or `length`.
  final String reason;
  final int? haveSeconds;
  final int? wantSeconds;

  factory CdDamaged.fromJson(Map<String, dynamic> j) => CdDamaged(
    number: j['number'] as int,
    reason: (j['reason'] as String?) ?? '',
    haveSeconds: j['have_s'] as int?,
    wantSeconds: j['want_s'] as int?,
  );
}

/// The CD in the drive is in the library already (`present` of a drive, or of a `disc_present` refusal).
class CdPresent {
  const CdPresent({
    required this.book,
    this.author = '',
    this.disc = 1,
    this.via = '',
    this.tracksInLibrary = 0,
    this.tracksOnDisc = 0,
    this.missing = const [],
    this.damaged = const [],
    this.complete = false,
  });

  /// The title of the book or album in the library.
  final String book;
  final String author;
  final int disc;

  /// `disc_id` or `book`: how the server knows it.
  final String via;
  final int tracksInLibrary;
  final int tracksOnDisc;
  final List<int> missing;
  final List<CdDamaged> damaged;

  /// Every track is there and whole.
  final bool complete;

  factory CdPresent.fromJson(Map<String, dynamic> j) => CdPresent(
    book: (j['book'] as String?) ?? '',
    author: (j['author'] as String?) ?? '',
    disc: (j['disc'] as int?) ?? 1,
    via: (j['via'] as String?) ?? '',
    tracksInLibrary: (j['tracks_in_library'] as int?) ?? 0,
    tracksOnDisc: (j['tracks_on_disc'] as int?) ?? 0,
    missing: CdResume._ints(j['missing']),
    damaged: [
      for (final d in (j['damaged'] as List? ?? const []).cast<Map<String, dynamic>>())
        CdDamaged.fromJson(d),
    ],
    complete: (j['complete'] as bool?) ?? false,
  );
}

/// The state of one chosen track while a rip runs.
class CdTrackState {
  const CdTrackState({
    required this.number,
    this.title = '',
    this.seconds = 0,
    this.state = 'waiting',
    this.fraction,
    this.error = '',
  });

  final int number;
  final String title;
  final int seconds;

  /// `waiting`, `reading`, `verifying`, `encoding`, `done` or `failed`; anything else counts as waiting.
  final String state;

  /// How far the track is, in `reading`.
  final double? fraction;

  /// The code of the failure, in `failed`.
  final String error;

  factory CdTrackState.fromJson(Map<String, dynamic> j) => CdTrackState(
    number: j['number'] as int,
    title: (j['title'] as String?) ?? '',
    seconds: (j['seconds'] as num?)?.round() ?? 0,
    state: (j['state'] as String?) ?? 'waiting',
    fraction: (j['fraction'] as num?)?.toDouble(),
    error: (j['error'] as String?) ?? '',
  );

  static List<CdTrackState> list(Object? v) => [
    for (final t in (v as List? ?? const []).cast<Map<String, dynamic>>()) CdTrackState.fromJson(t),
  ];
}

/// How full the ring buffer of the reader is (`buffer_*` of a rip).
class CdBuffer {
  const CdBuffer({required this.seconds, required this.max, this.percent, this.speedX});

  final int seconds;
  final int max;
  final int? percent;

  /// The speed the drive reads at, as a multiple of 1x, as the server measured it; null while it is not known.
  final double? speedX;

  static CdBuffer? from(Map<String, dynamic> j) {
    final max = (j['buffer_max'] as num?)?.round();
    if (max == null || max <= 0) return null;
    return CdBuffer(
      seconds: (j['buffer_seconds'] as num?)?.round() ?? 0,
      max: max,
      percent: (j['buffer_percent'] as num?)?.round(),
      speedX: (j['speed_x'] as num?)?.toDouble(),
    );
  }
}

/// The rip that runs on a drive, or ran last until it is closed (`rip` of a drive).
class CdRipInfo {
  const CdRipInfo({
    required this.jobId,
    this.finished = false,
    this.error = '',
    this.fraction,
    this.buffer,
    this.trackStates = const [],
  });

  final String jobId;
  final bool finished;
  final String error;
  final double? fraction;
  final CdBuffer? buffer;
  final List<CdTrackState> trackStates;

  factory CdRipInfo.fromJson(Map<String, dynamic> j) => CdRipInfo(
    jobId: '${j['job_id']}',
    finished: (j['finished'] as bool?) ?? false,
    error: (j['error'] as String?) ?? '',
    fraction: (j['fraction'] as num?)?.toDouble(),
    buffer: CdBuffer.from(j),
    trackStates: CdTrackState.list(j['track_states']),
  );
}

/// One drive of the server (`GET /api/v1/cd/drive` and the `drive` event).
///
/// The shape is `drives[]` of `docs/spec-cd-import-api.md`, section "The drive of the server".
class CdDriveInfo {
  const CdDriveInfo({
    required this.device,
    this.tray = CdTray.unknown,
    this.medium = CdMedium.unknown,
    this.tracks = 0,
    this.trackList = const [],
    this.seconds = 0,
    this.denied = false,
    this.ripping = false,
    this.resume,
    this.present,
    this.rip,
    this.signature = '',
  });

  /// The drive's name on the server, as the server gives it (shown to nobody, sent back).
  final String device;
  final CdTray tray;
  final CdMedium medium;

  /// Number of audio tracks.
  final int tracks;

  /// The tracks with number and length (`track_list`); empty when the server sends no list.
  final List<CdTrackInfo> trackList;
  final int seconds;

  /// The server may not use the drive (rights).
  final bool denied;

  /// A rip is running on it.
  final bool ripping;

  /// An interrupted rip of the disc in the drive that can be continued, or null.
  final CdResume? resume;

  /// The disc is in the library already, or null (not, or not looked at).
  final CdPresent? present;

  /// The rip that runs, or ran last until it is closed, with the state of each track; or null.
  final CdRipInfo? rip;

  /// The JSON the server sent, to tell a changed state from the same one.
  final String signature;

  factory CdDriveInfo.fromJson(Map<String, dynamic> j) => CdDriveInfo(
    device: (j['device'] as String?) ?? '',
    tray: CdTray.parse(j['tray']),
    medium: CdMedium.parse(j['medium']),
    tracks: (j['tracks'] as int?) ?? 0,
    trackList: [
      for (final t in (j['track_list'] as List? ?? const []).cast<Map<String, dynamic>>())
        CdTrackInfo(number: t['number'] as int, seconds: (t['seconds'] as num?)?.toDouble() ?? 0),
    ],
    seconds: (j['seconds'] as int?) ?? 0,
    denied: (j['denied'] as bool?) ?? false,
    ripping: (j['ripping'] as bool?) ?? false,
    resume: j['resume'] is Map<String, dynamic>
        ? CdResume.fromJson(j['resume'] as Map<String, dynamic>)
        : null,
    present: j['present'] is Map<String, dynamic>
        ? CdPresent.fromJson(j['present'] as Map<String, dynamic>)
        : null,
    rip: j['rip'] is Map<String, dynamic>
        ? CdRipInfo.fromJson(j['rip'] as Map<String, dynamic>)
        : null,
    signature: jsonEncode(j),
  );

  @override
  bool operator ==(Object other) =>
      other is CdDriveInfo &&
      other.device == device &&
      other.tray == tray &&
      other.medium == medium &&
      other.tracks == tracks &&
      other.trackList.length == trackList.length &&
      other.seconds == seconds &&
      other.denied == denied &&
      other.ripping == ripping &&
      other.signature == signature;

  @override
  int get hashCode => Object.hash(
    device,
    tray,
    medium,
    tracks,
    trackList.length,
    seconds,
    denied,
    ripping,
    signature,
  );
}

/// One audio track of the disc in the drive.
class CdTrackInfo {
  const CdTrackInfo({required this.number, this.seconds = 0});

  /// The number on the disc.
  final int number;
  final double seconds;
}

/// The drives of the server and what decides the two symbols of the library.
class CdDrivesState {
  const CdDrivesState(this.drives);

  /// No server, no drive: both symbols stay hidden.
  static const none = CdDrivesState([]);

  final List<CdDriveInfo> drives;

  bool get hasDrive => drives.isNotEmpty;

  /// A drive with a shut tray and an audio CD that nothing is using: the disc to import.
  CdDriveInfo? get importable {
    for (final d in drives) {
      if (d.tray == CdTray.closed && d.medium == CdMedium.audio && !d.denied && !d.ripping) {
        return d;
      }
    }
    return null;
  }

  /// A drive with a shut tray and some disc in it: a tray that can be opened. An open tray
  /// shows nothing, because nothing here ever closes it.
  CdDriveInfo? get ejectable {
    for (final d in drives) {
      if (d.tray == CdTray.closed &&
          (d.medium == CdMedium.audio || d.medium == CdMedium.other) &&
          !d.denied) {
        return d;
      }
    }
    return null;
  }

  /// A rip runs on some drive.
  bool get ripping => drives.any((d) => d.ripping);

  factory CdDrivesState.fromJson(Map<String, dynamic> j) => CdDrivesState([
    for (final d in (j['drives'] as List? ?? const []).cast<Map<String, dynamic>>())
      CdDriveInfo.fromJson(d),
  ]);

  @override
  bool operator ==(Object other) =>
      other is CdDrivesState &&
      other.drives.length == drives.length &&
      [for (var i = 0; i < drives.length; i++) drives[i] == other.drives[i]].every((e) => e);

  @override
  int get hashCode => Object.hashAll(drives);
}

/// A guess for what the disc is: from the server's lookup, or typed by hand.
class CdCandidate {
  const CdCandidate({
    this.author = '',
    this.title = '',
    this.disc = 1,
    this.discsTotal,
    this.trackTitles = const [],
    this.mbid,
    this.narrator,
    this.year,
    this.audiobook = false,
  });

  final String author;
  final String title;
  final int disc;
  final int? discsTotal;
  final List<String> trackTitles;

  /// The MusicBrainz release id (`release_id`), handed back when the rip starts (the server fetches the cover).
  final String? mbid;
  final String? narrator;
  final String? year;

  /// The server's lookup says the release is an audiobook (`audiobook`).
  final bool audiobook;

  factory CdCandidate.fromJson(Map<String, dynamic> j) => CdCandidate(
    author: (j['author'] as String?) ?? '',
    title: (j['title'] as String?) ?? '',
    disc: (j['disc'] as int?) ?? 1,
    discsTotal: j['discs_total'] as int?,
    trackTitles: [for (final t in (j['tracks'] as List? ?? const [])) t as String],
    mbid: j['release_id'] as String?,
    narrator: (j['narrators'] as List?)?.map((e) => '$e').join(', '),
    year: j['year'] as String?,
    audiobook: (j['audiobook'] as bool?) ?? false,
  );
}

/// Neutral fallbacks for when the server's settings cannot be read. The server's "CD import"
/// settings win; these only fill the dialog.
class CdDefaults {
  CdDefaults._();

  static const kind = 'audiobook';
  static const eject = false;
}

/// The part of the server's CD settings that the dialog uses (`GET /api/v1/cd/settings`).
class CdSettings {
  const CdSettings({
    this.eject = CdDefaults.eject,
    this.kind = CdDefaults.kind,
    this.lookupReady = true,
  });

  /// Eject after a successful import (the preset of the checkbox).
  final bool eject;

  /// `audiobook` or `music`: the preset of the choice.
  final String kind;

  /// The server has a MusicBrainz contact address; without one a lookup answers `no_contact`.
  final bool lookupReady;

  factory CdSettings.fromJson(Map<String, dynamic> j) => CdSettings(
    eject: (j['eject'] as bool?) ?? CdDefaults.eject,
    kind: (j['kind'] as String?) ?? CdDefaults.kind,
    lookupReady: (j['lookup_ready'] as bool?) ?? true,
  );
}
