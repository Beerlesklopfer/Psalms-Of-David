// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import '../l10n/app_localizations.dart';

/// Every error code the CD import can show: those of the server's `cd/*` interface
/// (`docs/spec-cd-import-api.md`) and those of reading the disc on this computer. A code that is
/// not in this list is shown with a general text and the code in brackets.
const List<String> kCdErrorCodes = [
  // Local: the drive and the disc.
  'no_disc', // no readable audio CD in the drive
  'drive_gone', // the drive stopped answering
  'read', // a sector could not be read
  'verify', // two reads of a track never agreed
  'cancelled', // the user stopped it
  'io', // other trouble with files on the server
  'encode', // the audio could not be converted
  'unsupported', // the server asks for a format this app cannot make
  'network', // the server did not answer
  // The server's drive.
  'no_drive',
  'drive_denied',
  'tray_open',
  'busy',
  'eject_failed',
  'internal',
  'certificate', // the server's certificate is not the paired one
  // The server, by its codes.
  'forbidden',
  'invalid_move', // the CD is not in the state the move needs (decided, or read again meanwhile)
  'decided', // somebody was faster with the decision
  'invalid_paging',
  'unauthorized',
  'invalid_toc',
  'invalid_query',
  'no_contact',
  'lookup_failed',
  'not_found',
  'invalid_release',
  'track_numbers',
  'invalid_meta',
  'invalid_selection',
  'empty_selection',
  'unreadable_file',
  'format_mismatch',
  'bad_extension',
  'unknown_file',
  'missing_file',
  'bad_cover',
  'bad_tags',
  'no_import_folder',
  'no_space',
  'disc_exists',
  'too_large',
  'bad_request',
  'invalid_job',
  'job_closed',
  'import_failed',
  'interrupted',
  'stale',
  'client',
  // A CD that is in the library already, resuming, and the reasons of the automatic import.
  'disc_present',
  'not_present',
  'nothing_to_heal',
  'unreadable_track',
  'unverified_track',
  'resume_mismatch',
  'heal_needed',
  'resume_pending',
  'ambiguous_match',
  'no_match',
  'no_target',
  'udev_off',
  'not_audio',
  'start_failed',
  'locked',
];

/// The words for [code] in the language of [l]; unknown codes get the general text with the code in brackets.
String cdErrorText(AppLocalizations l, String code) {
  final known = <String, String>{
    'no_disc': l.cdErrNoDisc,
    'drive_gone': l.cdErrDriveGone,
    'read': l.cdErrRead,
    'verify': l.cdErrVerify,
    'cancelled': l.cdErrCancelled,
    'io': l.cdErrIo,
    'encode': l.cdErrEncode,
    'unsupported': l.cdErrUnsupported,
    'network': l.cdErrNetwork,
    'certificate': l.cdErrCertificate,
    'no_drive': l.cdErrNoDrive,
    'drive_denied': l.cdErrDriveDenied,
    'tray_open': l.cdErrTrayOpen,
    'busy': l.cdErrBusy,
    'eject_failed': l.cdErrEjectFailed,
    'internal': l.cdErrInternal,
    'forbidden': l.cdErrForbidden,
    'invalid_move': l.cdErrInvalidMove,
    'decided': l.cdErrDecided,
    'invalid_paging': l.cdErrInvalidPaging,
    'unauthorized': l.cdErrUnauthorized,
    'invalid_toc': l.cdErrInvalidToc,
    'invalid_query': l.cdErrInvalidQuery,
    'no_contact': l.cdErrNoContact,
    'lookup_failed': l.cdErrLookupFailed,
    'not_found': l.cdErrNotFound,
    'invalid_release': l.cdErrInvalidRelease,
    'track_numbers': l.cdErrTrackNumbers,
    'invalid_meta': l.cdErrInvalidMeta,
    'invalid_selection': l.cdErrInvalidSelection,
    'empty_selection': l.cdErrEmptySelection,
    'unreadable_file': l.cdErrUnreadableFile,
    'format_mismatch': l.cdErrFormatMismatch,
    'bad_extension': l.cdErrBadExtension,
    'unknown_file': l.cdErrUnknownFile,
    'missing_file': l.cdErrMissingFile,
    'bad_cover': l.cdErrBadCover,
    'bad_tags': l.cdErrBadTags,
    'no_import_folder': l.cdErrNoImportFolder,
    'no_space': l.cdErrNoSpace,
    'disc_exists': l.cdErrDiscExists,
    'too_large': l.cdErrTooLarge,
    'bad_request': l.cdErrBadRequest,
    'invalid_job': l.cdErrInvalidJob,
    'job_closed': l.cdErrJobClosed,
    'import_failed': l.cdErrImportFailed,
    'interrupted': l.cdErrInterrupted,
    'stale': l.cdErrStale,
    'client': l.cdErrClient,
    'disc_present': l.cdErrDiscPresent,
    'not_present': l.cdErrNotPresent,
    'nothing_to_heal': l.cdErrNothingToHeal,
    'unreadable_track': l.cdErrUnreadableTrack,
    'unverified_track': l.cdErrUnverifiedTrack,
    'resume_mismatch': l.cdErrResumeMismatch,
    'heal_needed': l.cdErrHealNeeded,
    'resume_pending': l.cdErrResumePending,
    'ambiguous_match': l.cdErrAmbiguousMatch,
    'no_match': l.cdErrNoMatch,
    'no_target': l.cdErrNoTarget,
    'udev_off': l.cdErrUdevOff,
    'not_audio': l.cdErrNotAudio,
    'start_failed': l.cdErrStartFailed,
    'locked': l.cdErrLocked,
  };
  return known[code] ?? l.cdErrUnknown(code);
}

/// Texts of all known codes, for the test that checks both languages.
Map<String, String> cdErrorTexts(AppLocalizations l) => {
  for (final c in kCdErrorCodes) c: cdErrorText(l, c),
};
