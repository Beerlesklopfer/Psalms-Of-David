// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../audio/audio_backend.dart';
import '../l10n/app_localizations.dart';
import '../labels.dart';
import '../profile.dart';
import 'profile_screen.dart';
import '../library_hiding.dart';
import '../offline/offline_controller.dart';
import '../offline/offline_view.dart';
import '../player_controller.dart';
import '../server/credentials.dart';
import '../theme_mode.dart';
import '../server/device_pairing.dart';
import 'connect_device.dart';
import 'about_screen.dart';
import 'labels_ui.dart';
import 'podcast_downloads_ui.dart';
import '../widgets.dart';

/// What this device is connected to, and the way to disconnect it.
class SettingsScreen extends StatelessWidget {
  const SettingsScreen({
    super.key,
    this.credentials,
    this.onDisconnect,
    this.devicePairing,
    this.onSync,
    this.onShareListening,
    this.openUrl = _launch,
  });

  /// Tells the server whether the family sees what this user listens to (throws when it refuses); null in demo mode.
  final Future<void> Function(bool share)? onShareListening;

  /// Looks for the server and sends the changes that wait (the offline mode); null in demo mode.
  final Future<void> Function()? onSync;

  /// Opens an address in the browser of the system; replaced in tests.
  final Future<bool> Function(Uri url) openUrl;

  static Future<bool> _launch(Uri url) =>
      launchUrl(url, mode: LaunchMode.externalApplication);

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
        // The own profile: name, picture, language, PIN.
        if (ProfileScope.maybeOf(context) case final profile? when creds != null) ...[
          ListenableBuilder(
            listenable: profile,
            builder: (context, _) => Card(
              margin: EdgeInsets.zero,
              clipBehavior: Clip.antiAlias,
              child: ListTile(
                key: const ValueKey('open-profile'),
                leading: const OwnAvatar(),
                title: Text(profile.profile?.name ?? l.profileTitle),
                subtitle: Text(l.profileSettingsHint),
                trailing: const Icon(Icons.chevron_right),
                onTap: () => openProfile(context),
              ),
            ),
          ),
          const SizedBox(height: 24),
        ],
        // The groups: each has a title with an icon (as the cards of the page "About"), and shows only when it has something.
        _GroupTitle(icon: Icons.tune, title: l.settingsGroupGeneral),
        const _ThemeCard(),
        if (player != null && onShareListening != null && creds != null) ...[
          const SizedBox(height: 16),
          _ShareListeningCard(player: player, onChanged: onShareListening!),
        ],
        if (player != null && player.bufferAdjustable) ...[
          const SizedBox(height: 16),
          Text(l.settingsPlayback, style: text.titleSmall),
          const SizedBox(height: 8),
          _BufferSlider(player: player),
        ],
        const SizedBox(height: 24),
        if (OfflineScope.maybeOf(context) case final offline? when creds != null || player?.podcastDownloads != null) ...[
          _GroupTitle(icon: Icons.cloud_download_outlined, title: l.settingsGroupOffline),
          // The offline mode: the switch, what waits for the server, what the server did not take.
          if (creds != null) OfflineCard(controller: offline, onSync: onSync == null ? null : () => onSync!()),
          // The podcast episodes on this device: what they take, delete all, delete after listening, the largest download.
          if (player?.podcastDownloads case final downloads?) ...[
            if (creds != null) const SizedBox(height: 16),
            PodcastDownloadsCard(downloads: downloads),
          ],
          const SizedBox(height: 24),
        ] else if (player?.podcastDownloads case final downloads?) ...[
          _GroupTitle(icon: Icons.cloud_download_outlined, title: l.settingsGroupOffline),
          PodcastDownloadsCard(downloads: downloads),
          const SizedBox(height: 24),
        ],
        if ((HidingScope.maybeOf(context)?.available ?? false) || (LabelsScope.maybeOf(context)?.available ?? false)) ...[
          _GroupTitle(icon: Icons.library_music_outlined, title: l.settingsGroupLibrary),
          // The own library: what this profile hid, and whether it is still listed.
          if (HidingScope.maybeOf(context) case final hiding? when hiding.available) _HidingCard(controller: hiding),
          if ((HidingScope.maybeOf(context)?.available ?? false) && (LabelsScope.maybeOf(context)?.available ?? false))
            const SizedBox(height: 16),
          // The labels of the library: everybody sees the list; an administrator renames and deletes.
          if (LabelsScope.maybeOf(context) case final labels? when labels.available) LabelsCard(controller: labels),
          const SizedBox(height: 24),
        ],
        // Connecting a further device: a parent only, with a server (the card is empty for anybody else).
        if (devicePairing != null && creds != null) ...[
          _GroupTitle(icon: Icons.devices_outlined, title: l.settingsGroupDevices),
          ConnectDeviceCard(api: devicePairing!, credentials: creds),
          const SizedBox(height: 24),
        ],
        _GroupTitle(icon: Icons.dns_outlined, title: l.settingsGroupServer),
        if (creds == null)
          Text(
            l.settingsDemo,
            style: text.bodyMedium?.copyWith(color: cs.onSurfaceVariant),
          )
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
                  subtitle: Text(
                    creds.deviceName.isEmpty ? '-' : creds.deviceName,
                  ),
                ),
                const Divider(height: 1),
                // The website of the server: its start page, which leads on to the administration.
                ListTile(
                  key: const Key('open-website'),
                  leading: Icon(Icons.language, color: cs.primary),
                  title: Text(l.settingsWebsite),
                  subtitle: Text(l.settingsWebsiteHint),
                  trailing: const Icon(Icons.open_in_new),
                  onTap: () => _openWebsite(context, l, creds),
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
        // The page "About": on a narrow screen it has no place in the bar below (seven entries are too narrow there), so it is here.
        const SizedBox(height: 24),
        Card(
          margin: EdgeInsets.zero,
          clipBehavior: Clip.antiAlias,
          child: ListTile(
            key: const Key('open-about'),
            leading: Icon(Icons.info_outline, color: cs.primary),
            title: Text(l.navAbout),
            trailing: const Icon(Icons.chevron_right),
            onTap: () => Navigator.of(context).push(
              MaterialPageRoute<void>(builder: (_) => Scaffold(appBar: AppBar(title: Text(l.navAbout)), body: const AboutScreen())),
            ),
          ),
        ),
      ],
    );
  }

  /// The start page of the server in the browser (`https://<address>/`, the address is `host:port`).
  Future<void> _openWebsite(
    BuildContext context,
    AppLocalizations l,
    Credentials creds,
  ) async {
    final messenger = ScaffoldMessenger.maybeOf(context);
    final failed = l.settingsWebsiteFailed;
    final url = Uri.tryParse('https://${creds.address}/');
    final ok = url != null && await openUrl(url);
    if (!ok) messenger?.showSnackBar(SnackBar(content: Text(failed)));
  }

  Future<void> _confirm(BuildContext context, AppLocalizations l) async {
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.settingsDisconnect),
        content: Text(l.settingsDisconnectHint),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(l.cancel),
          ),
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

/// The title of a group of settings: an icon and the name, in the colour of the titles of the page "About".
class _GroupTitle extends StatelessWidget {
  const _GroupTitle({required this.icon, required this.title});

  final IconData icon;
  final String title;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Row(
        children: [
          Icon(icon, color: cs.primary),
          const SizedBox(width: 12),
          Expanded(child: Text(title, style: Theme.of(context).textTheme.titleMedium?.copyWith(color: cs.primary))),
        ],
      ),
    );
  }
}

/// Whether the family sees what this user listened to (the rows "recently heard by" of their start page). An administrator can forbid hiding it.
class _ShareListeningCard extends StatelessWidget {
  const _ShareListeningCard({required this.player, required this.onChanged});

  final PlayerController player;
  final Future<void> Function(bool share) onChanged;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return ListenableBuilder(
      listenable: player,
      builder: (context, _) {
        final locked = !player.mayHideListening;
        return Card(
          margin: EdgeInsets.zero,
          child: SwitchListTile(
            key: const Key('share-listening'),
            secondary: const Icon(Icons.groups_outlined),
            title: Text(l.settingsShareListening),
            subtitle: Text(locked ? l.settingsShareListeningLocked : l.settingsShareListeningHint),
            value: player.shareListening || locked,
            onChanged: locked
                ? null
                : (v) async {
                    final messenger = ScaffoldMessenger.maybeOf(context);
                    final failed = l.settingsShareListeningFailed;
                    try {
                      await onChanged(v);
                      player.setShareListening(v);
                    } on Object {
                      messenger?.showSnackBar(SnackBar(content: Text(failed)));
                    }
                  },
          ),
        );
      },
    );
  }
}

/// The choice of the colour scheme: like the system, light or dark.
class _ThemeCard extends StatelessWidget {
  const _ThemeCard();

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return Card(
      margin: EdgeInsets.zero,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.settingsTheme, style: Theme.of(context).textTheme.titleSmall),
            const SizedBox(height: 12),
            ValueListenableBuilder<ThemeMode>(
              valueListenable: themeModeNotifier,
              builder: (context, mode, _) => SegmentedButton<ThemeMode>(
                key: const Key('theme-mode'),
                showSelectedIcon: false,
                segments: [
                  ButtonSegment(value: ThemeMode.system, icon: const Icon(Icons.brightness_auto), label: Text(l.themeSystem)),
                  ButtonSegment(value: ThemeMode.light, icon: const Icon(Icons.light_mode), label: Text(l.themeLight)),
                  ButtonSegment(value: ThemeMode.dark, icon: const Icon(Icons.dark_mode), label: Text(l.themeDark)),
                ],
                selected: {mode},
                onSelectionChanged: (s) => setThemeMode(s.first),
              ),
            ),
          ],
        ),
      ),
    );
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
            Text(
              l.settingsBufferHint,
              style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
            ),
          ],
        ),
      ),
    );
  }
}

/// The own library: the switch "show what I hid". The list of what was hidden is a rubric of the library (Library -> Hidden).
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
          if (!await change()) {
            messenger?.showSnackBar(SnackBar(content: Text(failed)));
          }
        }

        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              l.settingsMyLibrary,
              style: text.titleMedium?.copyWith(color: cs.primary),
            ),
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
                ],
              ),
            ),
          ],
        );
      },
    );
  }
}
