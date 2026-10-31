// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import 'announcer_scope.dart';

/// The switch "CD announcements" with its one-line explanation and a Test button, in the card of the CD inbox in the settings. Nothing
/// where the app has no announcer; the Test button is off, with the reason, where the device has no voice.
class AnnounceSection extends StatelessWidget {
  const AnnounceSection({super.key});

  @override
  Widget build(BuildContext context) {
    final a = AnnouncerScope.maybeOf(context);
    if (a == null) return const SizedBox.shrink();
    final l = AppLocalizations.of(context);
    final reason = a.available
        ? null
        : (a.engine.unavailableReason == 'no_spd_say' ? l.cdAnnounceNoSpdSay : l.cdAnnounceNoVoice);
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const Divider(height: 1),
        SwitchListTile(
          key: const ValueKey('announce-switch'),
          secondary: const Icon(Icons.record_voice_over_outlined),
          title: Text(l.cdAnnounceTitle),
          subtitle: Text(reason ?? l.cdAnnounceHint, key: const ValueKey('announce-hint')),
          value: a.enabled,
          onChanged: a.setEnabled,
        ),
        Align(
          alignment: Alignment.centerRight,
          child: Padding(
            padding: const EdgeInsets.only(right: 16, bottom: 8),
            child: TextButton.icon(
              key: const ValueKey('announce-test'),
              onPressed: a.available ? () => a.test(l) : null,
              icon: const Icon(Icons.volume_up_outlined),
              label: Text(l.cdAnnounceTestButton),
            ),
          ),
        ),
      ],
    );
  }
}
