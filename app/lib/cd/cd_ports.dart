// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'cd_models.dart';

/// What the user confirmed in the dialog; sent with `POST /api/v1/cd/rip`.
class CdRipRequest {
  const CdRipRequest({
    required this.kind,
    required this.author,
    required this.title,
    required this.disc,
    this.discsTotal,
    this.narrator,
    this.year,
    this.mbid,
    this.tracks = const [],
    this.eject,
    this.device,
    this.only,
    this.mode,
    this.force = false,
    this.resume,
  });

  final String kind;
  final String author;
  final String title;
  final int disc;
  final int? discsTotal;
  final String? narrator;
  final String? year;
  final String? mbid;

  /// Titles of all tracks of the disc, in order; an empty text where none is known.
  final List<String> tracks;

  /// Eject when done; null lets the server's setting decide.
  final bool? eject;

  /// The drive, as the server named it; null lets the server choose.
  final String? device;

  /// The numbers on the disc of the tracks to import; null is all of them. An empty list is refused by the
  /// server (`empty_selection`).
  final List<int>? only;

  /// `heal` repairs the missing and damaged tracks of a CD that is in the library; null is a normal import.
  final String? mode;

  /// Import a CD that is in the library already, as a new copy. Only on purpose.
  final bool force;

  /// Continue an interrupted rip (the server's default) or, with false, read the disc again.
  final bool? resume;

  Map<String, Object?> toJson() => {
    'kind': kind,
    'author': author,
    'title': title,
    'disc': disc,
    'discs_total': ?discsTotal,
    'narrator': ?narrator,
    'year': ?year,
    'mbid': ?mbid,
    'tracks': tracks,
    'eject': ?eject,
    'device': ?device,
    'only': ?only,
    'mode': ?mode,
    if (force) 'force': true,
    'resume': ?resume,
  };
}

/// The answer to a started rip.
class CdRipStart {
  const CdRipStart(this.jobId, {this.resume});

  final String jobId;

  /// `resumed`, `fresh` or `mismatch` (the settings or the choice changed, so the disc is read again);
  /// null when the server says nothing.
  final String? resume;
}

/// The server side of the CD import (`docs/spec-cd-import-api.md`). The drive is on the server:
/// the app asks for its state, starts and stops the rip and ejects.
abstract class CdServerApi {
  Future<CdSettings> settings();

  /// The drives and what is in them.
  Future<CdDrivesState> drives();

  /// Looks the disc in the server's drive up. The server reads the table of contents itself. [device]
  /// names the drive, or null.
  ///
  /// [query] adds the releases MusicBrainz finds for words typed by hand (they come without track titles); [releaseId] returns that one release
  /// with the track titles of this disc.
  Future<List<CdCandidate>> lookup({String? device, String? query, String? releaseId});

  /// Starts the rip; returns the id of the job and what became of an earlier rip of the disc. Throws an `ApiException` whose code says why not
  /// (`busy`, `tray_open`, `no_disc`, ...).
  Future<CdRipStart> startRip(CdRipRequest request);

  /// Closes the result of the last rip of [device] (the list of tracks stays until then).
  Future<void> dismissRip({String? device});

  Future<void> cancelRip(String jobId);

  /// Opens the tray. True if it was opened, false if there was nothing to eject. The server never
  /// closes a tray. Throws an `ApiException` on a refusal.
  Future<bool> eject({String? device});
}
