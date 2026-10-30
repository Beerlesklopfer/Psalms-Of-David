// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../models.dart';
import 'offline_controller.dart';
import 'outbox.dart';

/// What the offline view shows of [books]: the titles that are on the device. A book of which only some titles are downloaded keeps those;
/// a book of which none is downloaded is left out. Radio stations and the like (no parts of their own) have nothing to download and go.
List<Book> downloadedOnly(List<Book> books, bool Function(Track) isDownloaded) {
  final out = <Book>[];
  for (final b in books) {
    if (b.isStream) continue;
    final kept = [for (final t in b.tracks) if (t.path != null || isDownloaded(t)) t];
    if (kept.isEmpty) continue;
    out.add(kept.length == b.tracks.length ? b : b.copyWith(tracks: kept));
  }
  return out;
}

/// The bar at the top of the app while it is offline: what the person sees, and how many changes wait for the server.
class OfflineBanner extends StatelessWidget {
  const OfflineBanner({super.key, this.onSync});

  /// Looks for the server and sends the changes; null where there is no server (demo).
  final VoidCallback? onSync;

  @override
  Widget build(BuildContext context) {
    final offline = OfflineScope.maybeOf(context);
    if (offline == null || !offline.active) return const SizedBox.shrink();
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    final pending = offline.pending;
    return Material(
      key: const ValueKey('offline-banner'),
      color: cs.secondaryContainer,
      child: SafeArea(
        bottom: false,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
          child: Row(
            children: [
              Icon(Icons.cloud_off_outlined, size: 18, color: cs.onSecondaryContainer),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  [
                    offline.manual ? l.offlineBannerManual : l.offlineBannerUnreachable,
                    if (pending > 0) l.offlinePending(pending),
                  ].join(' · '),
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(color: cs.onSecondaryContainer),
                ),
              ),
              if (onSync != null && !offline.manual)
                TextButton(
                  key: const ValueKey('offline-retry'),
                  onPressed: offline.syncing ? null : onSync,
                  child: Text(l.offlineRetry),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// A page that needs the server (podcasts, radio): offline it says so instead of its content.
class OnlineOnly extends StatelessWidget {
  const OnlineOnly({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    final offline = OfflineScope.maybeOf(context);
    if (offline == null || !offline.active) return child;
    final l = AppLocalizations.of(context);
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.cloud_off_outlined, size: 48),
            const SizedBox(height: 12),
            Text(l.offlineNeedsNetwork, key: const ValueKey('offline-needs-network'), textAlign: TextAlign.center, style: Theme.of(context).textTheme.bodyLarge),
          ],
        ),
      ),
    );
  }
}

/// The settings of the offline mode: the switch, the changes that wait for the server (and the button to send them), and what the server did
/// not take.
class OfflineCard extends StatelessWidget {
  const OfflineCard({super.key, required this.controller, this.onSync});

  final OfflineController controller;

  /// Looks for the server and sends what waits.
  final VoidCallback? onSync;

  String _reason(AppLocalizations l, int status) => switch (status) {
    403 => l.offlineReasonForbidden,
    404 => l.offlineReasonGone,
    _ => l.offlineReasonOther,
  };

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final l = AppLocalizations.of(context);
        final text = Theme.of(context).textTheme;
        final cs = Theme.of(context).colorScheme;
        // The listening positions wait too, but the person is told about what they did themselves.
        final waiting = controller.pending;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.offlineSettingTitle, style: text.titleMedium?.copyWith(color: cs.primary)),
            const SizedBox(height: 8),
            Card(
              margin: EdgeInsets.zero,
              clipBehavior: Clip.antiAlias,
              child: Column(
                children: [
                  SwitchListTile(
                    key: const ValueKey('offline-switch'),
                    secondary: Icon(Icons.cloud_off_outlined, color: cs.primary),
                    title: Text(l.offlineSettingTitle),
                    subtitle: Text(l.offlineSettingHint),
                    value: controller.manual,
                    onChanged: controller.setManual,
                  ),
                  const Divider(height: 1),
                  ListTile(
                    key: const ValueKey('offline-pending'),
                    leading: Icon(controller.syncing ? Icons.sync : Icons.sync_alt, color: cs.primary),
                    title: Text(controller.syncing ? l.offlineSyncing : (waiting == 0 ? l.offlineNothingWaits : l.offlinePending(waiting))),
                    trailing: waiting > 0 && onSync != null && !controller.manual
                        ? TextButton(
                            key: const ValueKey('offline-sync'),
                            onPressed: controller.syncing ? null : onSync,
                            child: Text(l.offlineSyncNow),
                          )
                        : null,
                  ),
                  if (controller.rejected.isNotEmpty) ...[
                    const Divider(height: 1),
                    ListTile(
                      key: const ValueKey('offline-rejected'),
                      leading: Icon(Icons.error_outline, color: cs.error),
                      title: Text(l.offlineRejectedTitle),
                      subtitle: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          for (final r in controller.rejected)
                            Text(
                              r.change.kind == OutboxChange.labels
                                  ? l.offlineRejectedLabels(r.change.title, _reason(l, r.status))
                                  : l.offlineRejectedHide(r.change.title, _reason(l, r.status)),
                            ),
                        ],
                      ),
                      trailing: TextButton(
                        key: const ValueKey('offline-dismiss'),
                        onPressed: controller.dismissRejected,
                        child: Text(l.offlineDismiss),
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ],
        );
      },
    );
  }
}
