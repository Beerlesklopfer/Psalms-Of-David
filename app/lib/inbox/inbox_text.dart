// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import '../l10n/app_localizations.dart';
import 'inbox_models.dart';

/// Every `code` this app knows (`docs/spec-cd-inbox.md`, the table of messages). Another code is shown with a generic text.
const knownInboxCodes = <String>[
  'cd_present',
  'cd_imported',
  'cd_decided',
  'cd_expired',
  'cd_read_error',
  'cd_no_match',
  'cd_ambiguous',
  'cd_heal_needed',
  'cd_no_space',
  'cd_no_target',
  'cd_awaiting_approval',
];

String _str(Object? v) => v is String ? v.trim() : (v is num ? '$v' : '');

int? _int(Object? v) => v is num ? v.toInt() : null;

/// "Author – Title (CD 2 of 12)" from the `args` of an item; "a CD" if the message knows nothing of it.
String inboxAlbum(AppLocalizations l, Map<String, dynamic> args) {
  final author = _str(args['author']);
  final title = _str(args['title']);
  final name = [author, title].where((e) => e.isNotEmpty).join(' – ');
  if (name.isEmpty) return l.inboxThisCd;
  final disc = _int(args['disc']);
  final total = _int(args['discs_total']);
  if (disc == null || disc < 1) return name;
  if (total != null && total > 1) {
    return '$name (${l.inboxDiscOf(disc, total)})';
  }
  return disc > 1 ? '$name (${l.inboxDiscNumber(disc)})' : name;
}

/// The numbers of the tracks that failed, "3, 7".
String _trackList(Object? v) =>
    v is List ? v.map(_str).where((e) => e.isNotEmpty).join(', ') : _str(v);

/// The sentence for an item, in the language of [l]. Never throws: missing or odd `args` give a shorter sentence, an unknown `code` the generic one.
String inboxText(AppLocalizations l, InboxItem item) {
  final a = item.args;
  final album = inboxAlbum(l, a);
  switch (item.code) {
    case 'cd_awaiting_approval':
      if (!item.canDecide && !item.resolved) return l.inboxCdWaiting(album);
      final by = _str(a['by']);
      return by.isEmpty || by == 'auto'
          ? l.inboxCdAwaitingApprovalAuto(album)
          : l.inboxCdAwaitingApproval(by, album);
    case 'cd_decided':
      final by = _str(a['by']);
      return a['approved'] == false
          ? l.inboxCdDecidedRejected(by, album)
          : l.inboxCdDecidedApproved(by, album);
    case 'cd_expired':
      return l.inboxCdExpired(album);
    case 'cd_imported':
      final tracks = _int(a['tracks']);
      return tracks == null
          ? l.inboxCdImportedNoCount(album)
          : l.inboxCdImported(album, tracks);
    case 'cd_present':
      return l.inboxCdPresent(album);
    case 'cd_read_error':
      final tracks = _trackList(a['tracks']);
      return tracks.isEmpty
          ? l.inboxCdReadErrorNoTracks(album)
          : l.inboxCdReadError(album, tracks);
    case 'cd_no_match':
      return l.inboxCdNoMatch;
    case 'cd_ambiguous':
      final n = _int(a['candidates']);
      return n == null ? l.inboxCdAmbiguousNoCount : l.inboxCdAmbiguous(n);
    case 'cd_heal_needed':
      return l.inboxCdHealNeeded(album);
    case 'cd_no_space':
      return l.inboxCdNoSpace(album);
    case 'cd_no_target':
      return l.inboxCdNoTarget(album);
  }
  return l.inboxUnknown;
}

/// The title of a system notification.
String inboxTitle(AppLocalizations l, InboxItem item) =>
    l.inboxNotificationTitle;

/// What a decided task says under its text: "Imported by Bea", "Rejected", "Expired"; null for an open item.
String? inboxResolution(AppLocalizations l, InboxItem item) {
  if (!item.resolved) return null;
  final by = item.by ?? '';
  return switch (item.resolution) {
    'approved' =>
      by.isEmpty ? l.inboxResolvedApprovedYou : l.inboxResolvedApproved(by),
    'rejected' =>
      by.isEmpty ? l.inboxResolvedRejectedYou : l.inboxResolvedRejected(by),
    'expired' => l.inboxResolvedExpired,
    _ => null,
  };
}
