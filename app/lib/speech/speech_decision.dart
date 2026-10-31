// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import '../inbox/inbox_models.dart';
import '../l10n/app_localizations.dart';

/// The codes that may be spoken (`docs/spec-cd-inbox.md`, "Spoken messages"): not the notices of decisions, not an expiry.
const spokenInboxCodes = <String>{
  'cd_imported',
  'cd_awaiting_approval',
  'cd_read_error',
  'cd_present',
};

/// Is this device on the home network of the server? The WireGuard tunnel is not built, so today it always is. Replace this function
/// (never a hard-coded address) when the tunnel exists.
bool atHomeNetwork() => true;

/// Why a message is not spoken; null (from [whyNotSpoken]) means it is.
enum SpeechSkip {
  notSpoken,
  wrongCode,
  read,
  alreadySpoken,
  switchOff,
  noRight,
  notPlaying,
  notForThisDevice,
  notAtHome,
}

/// What the decision needs to know about this device right now.
class SpeechContext {
  const SpeechContext({
    required this.playing,
    required this.ownDevice,
    required this.rights,
    required this.role,
    required this.enabled,
    this.atHome = true,
  });

  final bool playing;

  /// This device's name in the server's words (`hello.device`); null if unknown.
  final String? ownDevice;
  final CdRights rights;
  final String role;
  final bool enabled;
  final bool atHome;
}

/// The one decision: should [item] be spoken on this device now? Pure; [spoken] holds the ids already spoken. Snapshot items never get
/// here (the inbox controller hands over only messages that are new), and a read or decided item is not spoken.
SpeechSkip? whyNotSpoken(
  InboxItem item,
  String? speak,
  String? device,
  SpeechContext c,
  Set<int> spoken,
) {
  if (!spokenInboxCodes.contains(item.code)) return SpeechSkip.wrongCode;
  if (speak != 'starter' && speak != 'home') return SpeechSkip.notSpoken;
  if (item.read || item.resolved) return SpeechSkip.read;
  if (spoken.contains(item.id)) return SpeechSkip.alreadySpoken;
  // A child and a guest never speak, whatever the rights say.
  if (c.role == 'child' || c.role == 'guest' || !c.rights.hasInbox) {
    return SpeechSkip.noRight;
  }
  if (!c.enabled) return SpeechSkip.switchOff;
  if (!c.playing) return SpeechSkip.notPlaying;
  if (speak == 'starter') {
    final own = c.ownDevice;
    if (own == null || own.isEmpty || device != own) {
      return SpeechSkip.notForThisDevice;
    }
  } else {
    if (!c.rights.mayApprove) return SpeechSkip.noRight;
    if (!c.atHome) return SpeechSkip.notAtHome;
  }
  return null;
}

/// The sentence for [item] in the language of [l]; the title comes from `args`. Empty for a code that is not spoken.
String spokenText(AppLocalizations l, InboxItem item) {
  final t = item.args['title'];
  final title = t is String && t.trim().isNotEmpty ? t.trim() : null;
  return switch (item.code) {
    'cd_imported' =>
      title == null ? l.cdSpeakImportedNoTitle : l.cdSpeakImported(title),
    'cd_awaiting_approval' =>
      title == null ? l.cdSpeakAwaitingNoTitle : l.cdSpeakAwaiting(title),
    'cd_read_error' =>
      title == null ? l.cdSpeakReadErrorNoTitle : l.cdSpeakReadError(title),
    'cd_present' =>
      title == null ? l.cdSpeakPresentNoTitle : l.cdSpeakPresent(title),
    _ => '',
  };
}
