// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show rootBundle;

import '../about_info.dart';
import '../format.dart';
import '../l10n/app_localizations.dart';
import '../player_controller.dart';
import '../widgets.dart';

class AboutScreen extends StatefulWidget {
  const AboutScreen({super.key});

  @override
  State<AboutScreen> createState() => _AboutScreenState();
}

class _AboutScreenState extends State<AboutScreen> {
  Future<({int files, int bytes})>? _usage;
  int _revision = -1;

  @override
  Widget build(BuildContext context) {
    final p = PlayerScope.of(context);
    // The size is measured again only when the downloads changed, not on every redraw.
    if (_usage == null || _revision != p.storageRevision) {
      _revision = p.storageRevision;
      _usage = p.storageUsed();
    }
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;

    return CenteredPage(
      title: l.navAbout,
      children: [
        const SizedBox(height: 16),
        Center(
          child: Container(
            width: 132,
            height: 132,
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              gradient: RadialGradient(colors: [cs.primary.withValues(alpha: 0.35), cs.primary.withValues(alpha: 0.05)]),
              border: Border.all(color: cs.primary.withValues(alpha: 0.6), width: 2),
              boxShadow: [BoxShadow(color: cs.primary.withValues(alpha: 0.25), blurRadius: 36, spreadRadius: 2)],
            ),
            alignment: Alignment.center,
            child: Lyre(size: 80, color: cs.primary),
          ),
        ),
        const SizedBox(height: 20),
        Text(l.appTitle, textAlign: TextAlign.center, style: text.displayMedium?.copyWith(color: cs.primary, letterSpacing: 1)),
        const SizedBox(height: 4),
        Text(l.aboutVersion(appVersion), textAlign: TextAlign.center, style: text.bodyMedium?.copyWith(color: cs.onSurfaceVariant)),
        const SizedBox(height: 20),
        Text(l.aboutDescription, textAlign: TextAlign.center, style: text.bodyLarge),
        const SizedBox(height: 20),
        Wrap(
          alignment: WrapAlignment.center,
          spacing: 8,
          runSpacing: 8,
          children: [
            Chip(avatar: const Icon(Icons.menu_book_outlined, size: 18), label: Text(l.filterBooks)),
            Chip(avatar: const Icon(Icons.music_note_outlined, size: 18), label: Text(l.filterMusic)),
            Chip(avatar: const Icon(Icons.auto_stories_outlined, size: 18), label: Text(l.aboutScripture)),
            Chip(avatar: const Icon(Icons.family_restroom_outlined, size: 18), label: Text(l.aboutFamily)),
          ],
        ),
        const SizedBox(height: 28),
        Card(
          margin: EdgeInsets.zero,
          color: cs.primaryContainer.withValues(alpha: 0.25),
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(children: [
                  Icon(Icons.dns_outlined, color: cs.primary),
                  const SizedBox(width: 10),
                  Expanded(child: Text(l.aboutDavidTitle, style: text.titleMedium?.copyWith(color: cs.primary))),
                ]),
                const SizedBox(height: 10),
                Text(l.aboutDavidText),
                const SizedBox(height: 10),
                Text(l.aboutDavidSteps, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
              ],
            ),
          ),
        ),
        if (p.canDownload) ...[
          const SizedBox(height: 12),
          Card(
            margin: EdgeInsets.zero,
            child: FutureBuilder<({int files, int bytes})>(
              future: _usage,
              builder: (context, snap) {
                final usage = snap.data;
                return ListTile(
                  leading: Icon(Icons.sd_storage_outlined, color: cs.primary),
                  title: Text(l.aboutStorage),
                  subtitle: Text(usage == null
                      ? '…'
                      : usage.files == 0
                          ? l.aboutStorageNone
                          : l.aboutStorageUsed(formatBytes(l.localeName, usage.bytes), usage.files)),
                  trailing: usage == null || usage.files == 0
                      ? null
                      : TextButton(onPressed: () => _confirmRemove(context, p, l), child: Text(l.downloadRemove)),
                );
              },
            ),
          ),
        ],
        const SizedBox(height: 12),
        Card(
          margin: EdgeInsets.zero,
          clipBehavior: Clip.antiAlias,
          child: Column(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 14, 16, 4),
                child: Row(children: [
                  Icon(Icons.verified_user_outlined, color: cs.primary),
                  const SizedBox(width: 10),
                  Expanded(child: Text(l.craTitle, style: text.titleMedium?.copyWith(color: cs.primary))),
                ]),
              ),
              ListTile(dense: true, title: Text(l.craManufacturer), subtitle: const Text(manufacturerName)),
              ListTile(dense: true, title: Text(l.craVersion), subtitle: Text('$appVersion ($licenseName)')),
              ListTile(dense: true, title: Text(l.aboutSource), subtitle: const SelectableText(sourceUrl)),
              ListTile(dense: true, title: Text(l.craContact), subtitle: const SelectableText(securityContact)),
              ListTile(dense: true, title: Text(l.craSupport), subtitle: Text(supportUntil == null ? l.craSupportOpen : l.craSupportUntil(supportUntil!))),
              ListTile(dense: true, title: Text(l.craData), subtitle: Text(l.craDataText)),
              ListTile(
                dense: true,
                title: Text(l.craComponents),
                subtitle: Text(l.craComponentsText),
                trailing: const Icon(Icons.chevron_right),
                onTap: () => showLicensePage(context: context, applicationName: l.appTitle, applicationVersion: appVersion),
              ),
            ],
          ),
        ),
        const SizedBox(height: 12),
        Card(
          margin: EdgeInsets.zero,
          clipBehavior: Clip.antiAlias,
          child: Column(
            children: [
              ListTile(
                leading: Icon(Icons.balance, color: cs.primary),
                title: Text(l.aboutLicenseTitle),
                subtitle: Text(l.aboutLicense),
                trailing: const Icon(Icons.chevron_right),
                onTap: () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => const LicenseTextScreen())),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

Future<void> _confirmRemove(BuildContext context, PlayerController p, AppLocalizations l) async {
  final yes = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(l.aboutStorageRemoveTitle),
      content: Text(l.downloadRemoveText),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.cancel)),
        FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(l.downloadRemove)),
      ],
    ),
  );
  if (yes == true) await p.removeAllDownloads();
}

/// The full text of the AGPL, bundled with the app (the licence asks for it to be given).
class LicenseTextScreen extends StatelessWidget {
  const LicenseTextScreen({super.key, this.load});

  /// For tests; defaults to the bundled asset.
  final Future<String> Function()? load;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l.aboutLicenseTitle)),
      body: FutureBuilder<String>(
        future: (load ?? () => rootBundle.loadString('assets/LICENSE-AGPL-3.0.txt'))(),
        builder: (context, snap) {
          if (!snap.hasData) return const Center(child: CircularProgressIndicator());
          return Scrollbar(
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(16),
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 760),
                  child: SelectableText(snap.data!, style: Theme.of(context).textTheme.bodySmall?.copyWith(fontFamily: 'monospace')),
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}
