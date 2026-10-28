// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../server/credentials.dart';
import '../widgets.dart';

/// What this device is connected to, and the way to disconnect it.
class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key, this.credentials, this.onDisconnect});

  /// The stored pairing; null in demo mode.
  final Credentials? credentials;

  /// Forgets the pairing and goes back to the connect screen; null in demo mode.
  final VoidCallback? onDisconnect;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    final creds = credentials;

    return CenteredPage(
      title: l.navSettings,
      children: [
        const SizedBox(height: 8),
        Text(l.settingsServer, style: text.titleMedium?.copyWith(color: cs.primary)),
        const SizedBox(height: 8),
        if (creds == null)
          Text(l.settingsDemo, style: text.bodyMedium?.copyWith(color: cs.onSurfaceVariant))
        else ...[
          Card(
            margin: EdgeInsets.zero,
            clipBehavior: Clip.antiAlias,
            child: Column(
              children: [
                ListTile(
                  leading: Icon(Icons.dns_outlined, color: cs.primary),
                  title: Text(l.settingsAddress),
                  subtitle: SelectableText(creds.address),
                ),
                const Divider(height: 1),
                ListTile(
                  leading: Icon(Icons.devices_outlined, color: cs.primary),
                  title: Text(l.settingsDevice),
                  subtitle: Text(creds.deviceName.isEmpty ? '-' : creds.deviceName),
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          OutlinedButton.icon(
            icon: const Icon(Icons.link_off),
            label: Text(l.settingsDisconnect),
            onPressed: onDisconnect == null ? null : () => _confirm(context, l),
          ),
          const SizedBox(height: 8),
          Text(l.settingsDisconnectHint, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
        ],
      ],
    );
  }

  Future<void> _confirm(BuildContext context, AppLocalizations l) async {
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.settingsDisconnect),
        content: Text(l.settingsDisconnectHint),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.cancel)),
          FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(l.settingsDisconnect)),
        ],
      ),
    );
    if (yes == true) onDisconnect?.call();
  }
}
