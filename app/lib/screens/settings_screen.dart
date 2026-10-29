// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../audio/audio_backend.dart';
import '../l10n/app_localizations.dart';
import '../library_hiding.dart';
import '../player_controller.dart';
import '../server/credentials.dart';
import '../server/device_pairing.dart';
import 'connect_device.dart';
import '../widgets.dart';

/// What this device is connected to, and the way to disconnect it.
class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key, this.credentials, this.onDisconnect, this.devicePairing});

  /// The stored pairing; null in demo mode.
  final Credentials? credentials;

  /// Connects a further device with a QR code (parents only; the server decides); null in demo mode.
  final DevicePairingApi? devicePairing;

  /// Forgets the pairing and goes back to the connect screen; null in demo mode.
  final VoidCallback? onDisconnect;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    final creds = credentials;
    final player = PlayerScope.maybeOf(context);

    return CenteredPage(
      title: l.navSettings,
      children: [
        const SizedBox(height: 8),
        if (player != null && player.bufferAdjustable) ...[
          Text(l.settingsPlayback, style: text.titleMedium?.copyWith(color: cs.primary)),
          const SizedBox(height: 8),
          _BufferSlider(player: player),
          const SizedBox(height: 24),
        ],
        // The own library: what this profile hid, and whether it is still listed.
        if (HidingScope.maybeOf(context) case final hiding? when hiding.available) ...[
          _HidingCard(controller: hiding),
          const SizedBox(height: 24),
        ],
        // Connecting a further device: a parent only, with a server (the card is empty for anybody else).
        if (devicePairing != null && creds != null)
          ConnectDeviceCard(api: devicePairing!, credentials: creds),
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
          Text(
            l.settingsDisconnectHint,
            style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
          ),
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
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(l.settingsDisconnect),
          ),
        ],
      ),
    );
    if (yes == true) onDisconnect?.call();
  }
}

/// The size of the playback buffer in seconds. The value is shown while the slider moves and set when it is let go.
class _BufferSlider extends StatefulWidget {
  const _BufferSlider({required this.player});

  final PlayerController player;

  @override
  State<_BufferSlider> createState() => _BufferSliderState();
}

class _BufferSliderState extends State<_BufferSlider> {
  late double _value = widget.player.bufferSeconds.toDouble();

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    return Card(
      margin: EdgeInsets.zero,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(child: Text(l.settingsBuffer, style: text.bodyLarge)),
                Text(
                  l.settingsBufferSeconds(_value.round()),
                  key: const Key('buffer-value'),
                  style: text.bodyLarge,
                ),
              ],
            ),
            Slider(
              key: const Key('buffer-slider'),
              min: kBufferMinSeconds.toDouble(),
              max: kBufferMaxSeconds.toDouble(),
              divisions: kBufferMaxSeconds - kBufferMinSeconds,
              value: _value,
              label: l.settingsBufferSeconds(_value.round()),
              onChanged: (v) => setState(() => _value = v),
              onChangeEnd: (v) => widget.player.setBufferSeconds(v.round()),
            ),
            Text(l.settingsBufferHint, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
          ],
        ),
      ),
    );
  }
}

/// The own library: the switch "show what I hid" and the list of what was hidden, each with "show again".
class _HidingCard extends StatelessWidget {
  const _HidingCard({required this.controller});

  final HidingController controller;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final l = AppLocalizations.of(context);
        final text = Theme.of(context).textTheme;
        final cs = Theme.of(context).colorScheme;
        final s = controller.state;
        Future<void> run(Future<bool> Function() change) async {
          final messenger = ScaffoldMessenger.maybeOf(context);
          final failed = l.hidingFailed;
          if (!await change()) messenger?.showSnackBar(SnackBar(content: Text(failed)));
        }

        String authorLine(HiddenAuthor a) => switch (a.kind) {
          'music' => l.settingsHiddenAuthorMusic(a.name),
          'audiobook' => l.settingsHiddenAuthorAudiobooks(a.name),
          _ => l.settingsHiddenAuthorAll(a.name),
        };

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.settingsMyLibrary, style: text.titleMedium?.copyWith(color: cs.primary)),
            const SizedBox(height: 8),
            Card(
              margin: EdgeInsets.zero,
              clipBehavior: Clip.antiAlias,
              child: Column(
                children: [
                  SwitchListTile(
                    key: const Key('show-hidden'),
                    title: Text(l.settingsShowHidden),
                    subtitle: Text(l.settingsShowHiddenHint),
                    value: s.showHidden,
                    onChanged: (v) => run(() => controller.setShowHidden(v)),
                  ),
                  const Divider(height: 1),
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
                    child: Align(
                      alignment: Alignment.centerLeft,
                      child: Text(l.settingsHiddenTitle, style: text.titleSmall),
                    ),
                  ),
                  if (s.isEmpty)
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 4, 16, 16),
                      child: Text(
                        l.settingsHiddenNone,
                        key: const Key('hidden-none'),
                        style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
                      ),
                    ),
                  for (final a in s.authors)
                    ListTile(
                      key: Key('hidden-author-${a.id}-${a.kind}'),
                      dense: true,
                      leading: const Icon(Icons.person_off_outlined),
                      title: Text(authorLine(a)),
                      trailing: TextButton(
                        onPressed: () => run(() => controller.hideAuthor(a.name, a.kind, false)),
                        child: Text(l.settingsShowAgain),
                      ),
                    ),
                  for (final b in s.books)
                    ListTile(
                      key: Key('hidden-book-${b.id}'),
                      dense: true,
                      leading: const Icon(Icons.visibility_off_outlined),
                      title: Text(b.title),
                      subtitle: Text(b.author),
                      trailing: TextButton(
                        onPressed: () => run(() => controller.hideBook('${b.id}', false)),
                        child: Text(l.settingsShowAgain),
                      ),
                    ),
                ],
              ),
            ),
          ],
        );
      },
    );
  }
}
