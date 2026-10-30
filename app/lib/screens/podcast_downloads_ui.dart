// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../format.dart';
import '../l10n/app_localizations.dart';
import '../player_controller.dart';
import '../podcast_downloads.dart';
import '../podcasts.dart';
import '../podcasts/download_gate.dart';
import '../podcasts/download_machine.dart' as machine;

/// The words for why a download did not work (the codes of [EpisodeDownloadException]).
String episodeDownloadMessage(AppLocalizations l, String code, int maxMegabytes) => switch (code) {
  'scheme' => l.episodeErrorScheme,
  'http' => l.episodeErrorHttp,
  'too_large' => l.episodeErrorTooLarge(l.podcastMegabytes(maxMegabytes)),
  'io' => l.episodeErrorIo,
  _ => l.episodeErrorNetwork,
};

/// The button of an episode: download, the progress with a way to stop, and (when it is on the device) the check mark that removes it again.
class EpisodeDownloadButton extends StatelessWidget {
  const EpisodeDownloadButton({super.key, required this.episode, required this.podcast});

  final Episode episode;
  final Podcast podcast;

  Future<void> _download(BuildContext context, PodcastDownloads downloads) async {
    final l = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.maybeOf(context);
    final code = await downloads.download(episode, podcast);
    if (code != null && code != 'cancelled') {
      messenger?.showSnackBar(SnackBar(content: Text(episodeDownloadMessage(l, code, downloads.maxMegabytes))));
    }
  }

  Future<void> _confirmRemove(BuildContext context, PodcastDownloads downloads) async {
    final l = AppLocalizations.of(context);
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.episodeRemoveTitle),
        content: Text(l.episodeRemoveText),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.cancel)),
          FilledButton(key: const ValueKey('episode-remove-yes'), onPressed: () => Navigator.pop(context, true), child: Text(l.downloadRemove)),
        ],
      ),
    );
    if (yes == true) await downloads.remove(episode.id);
  }

  @override
  Widget build(BuildContext context) {
    final downloads = PlayerScope.maybeOf(context)?.podcastDownloads;
    if (downloads == null) return const SizedBox.shrink();
    return ListenableBuilder(
      listenable: downloads,
      builder: (context, _) {
        final l = AppLocalizations.of(context);
        final cs = Theme.of(context).colorScheme;
        // The life cycle of the episode (podcasts/download_machine.dart) decides what the button shows.
        final running = switch (downloads.stateOf(episode.id)) {
          machine.Queued() => (received: 0, total: null as int?),
          machine.Downloading(:final received, :final total) || machine.Cancelling(:final received, :final total) => (received: received, total: total),
          _ => null,
        };
        if (running != null) {
          final total = running.total;
          return Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                total == null ? formatBytes(l.localeName, running.received) : l.episodeProgress(formatBytes(l.localeName, running.received), formatBytes(l.localeName, total)),
                style: Theme.of(context).textTheme.bodySmall,
              ),
              IconButton(
                key: ValueKey('episode-download-cancel-${episode.id}'),
                tooltip: l.episodeDownloadCancel,
                onPressed: () => downloads.cancel(episode.id),
                icon: SizedBox.square(dimension: 24, child: CircularProgressIndicator(value: total == null || total == 0 ? null : running.received / total, strokeWidth: 3)),
              ),
            ],
          );
        }
        if (downloads.isDownloaded(episode.audioUrl)) {
          return IconButton(
            key: ValueKey('episode-downloaded-${episode.id}'),
            tooltip: l.episodeDownloaded,
            onPressed: () => _confirmRemove(context, downloads),
            icon: Icon(Icons.download_done, color: cs.primary),
          );
        }
        return IconButton(
          key: ValueKey('episode-download-${episode.id}'),
          tooltip: l.episodeDownload,
          onPressed: () => _download(context, downloads),
          icon: const Icon(Icons.download_for_offline_outlined),
        );
      },
    );
  }
}

/// One episode in a list: the same for the list of the server and for the offline list of what is on the device.
class EpisodeTile extends StatelessWidget {
  const EpisodeTile({super.key, required this.episode, required this.podcast});

  final Episode episode;
  final Podcast podcast;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final downloads = p.podcastDownloads;
    // The size appears when the episode has come in.
    return ListenableBuilder(
      listenable: downloads ?? const _Never(),
      builder: (context, _) => _tile(context, l, p, downloads?.sizeOf(episode.id)),
    );
  }

  Widget _tile(BuildContext context, AppLocalizations l, PlayerController p, int? size) {
    final e = episode;
    final date = e.published;
    return ListTile(
      key: ValueKey('episode-${e.id}'),
      leading: Icon(e.finished ? Icons.check_circle_outline : Icons.play_circle_outline),
      title: Text(e.title, style: e.finished ? TextStyle(color: Theme.of(context).colorScheme.onSurfaceVariant) : null),
      subtitle: Text([
        if (date != null) '${date.year}-${date.month.toString().padLeft(2, '0')}-${date.day.toString().padLeft(2, '0')}',
        if (e.duration > Duration.zero) formatDuration(e.duration),
        if (!e.finished && e.position > Duration.zero) l.episodeAt(formatDuration(e.position)),
        if (size != null && size > 0) formatBytes(l.localeName, size),
      ].join(' · ')),
      trailing: EpisodeDownloadButton(episode: e, podcast: podcast),
      onTap: () => p.open(e.asBook(podcast.title), start: e.finished ? Duration.zero : e.position),
    );
  }
}

/// A [Listenable] that never changes: for a place that listens to something that may not be there.
class _Never implements Listenable {
  const _Never();

  @override
  void addListener(VoidCallback listener) {}

  @override
  void removeListener(VoidCallback listener) {}
}

/// The podcasts that have episodes on the device, for the offline view (as a sliver of the list of podcasts).
class OfflinePodcastsSliver extends StatelessWidget {
  const OfflinePodcastsSliver({super.key, required this.downloads});

  final PodcastDownloads? downloads;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final d = downloads;
    if (d == null) return SliverFillRemaining(hasScrollBody: false, child: Center(child: Text(l.offlinePodcastsNone, key: const ValueKey('offline-podcasts-none'), textAlign: TextAlign.center)));
    return ListenableBuilder(
      listenable: d,
      builder: (context, _) {
        final list = d.podcasts;
        if (list.isEmpty) {
          return SliverFillRemaining(
            hasScrollBody: false,
            child: Center(
              child: Padding(
                padding: const EdgeInsets.all(32),
                child: Text(l.offlinePodcastsNone, key: const ValueKey('offline-podcasts-none'), textAlign: TextAlign.center),
              ),
            ),
          );
        }
        return SliverList.builder(
          itemCount: list.length,
          itemBuilder: (context, i) {
            final p = list[i];
            return ListTile(
              key: ValueKey('offline-podcast-${p.id}'),
              leading: const Icon(Icons.podcasts),
              title: Text(p.title),
              subtitle: Text(l.podcastDownloadsUsed(formatBytes(l.localeName, p.bytes), p.episodes)),
              onTap: () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => OfflineEpisodesScreen(podcast: p))),
            );
          },
        );
      },
    );
  }
}

/// The episodes of one podcast that are on the device: they play without the server.
class OfflineEpisodesScreen extends StatelessWidget {
  const OfflineEpisodesScreen({super.key, required this.podcast});

  final DownloadedPodcast podcast;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final downloads = PlayerScope.of(context).podcastDownloads;
    final asPodcast = Podcast(id: podcast.id, title: podcast.title, description: '', approved: true, unheard: 0);
    return Scaffold(
      appBar: AppBar(title: Text(podcast.title)),
      body: downloads == null
          ? Center(child: Text(l.offlinePodcastsNone))
          : ListenableBuilder(
              listenable: downloads,
              builder: (context, _) {
                final rows = downloads.episodesOf(podcast.id);
                if (rows.isEmpty) return Center(child: Text(l.offlinePodcastsNone));
                return ListView.builder(
                  itemCount: rows.length,
                  itemBuilder: (context, i) => EpisodeTile(episode: rows[i].toEpisode(), podcast: asPodcast),
                );
              },
            ),
    );
  }
}

/// The settings of the podcast downloads: what they take, delete all, delete after listening, the largest download.
class PodcastDownloadsCard extends StatelessWidget {
  const PodcastDownloadsCard({super.key, required this.downloads});

  final PodcastDownloads downloads;

  Future<void> _confirmDeleteAll(BuildContext context) async {
    final l = AppLocalizations.of(context);
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.podcastDownloadsDeleteAllTitle),
        content: Text(l.podcastDownloadsDeleteAllText),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.cancel)),
          FilledButton(key: const ValueKey('podcast-downloads-delete-yes'), onPressed: () => Navigator.pop(context, true), child: Text(l.downloadRemove)),
        ],
      ),
    );
    if (yes == true) await downloads.removeAll();
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: downloads,
      builder: (context, _) {
        final l = AppLocalizations.of(context);
        final text = Theme.of(context).textTheme;
        final cs = Theme.of(context).colorScheme;
        final usage = downloads.usage();
        final choices = {...kEpisodeSizeChoices, downloads.maxMegabytes}.toList()..sort();
        final bulkChoices = {...kBulkSizeChoices, downloads.bulkMegabytes}.toList()..sort();
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.podcastDownloadsTitle, style: text.titleMedium?.copyWith(color: cs.primary)),
            const SizedBox(height: 8),
            Card(
              margin: EdgeInsets.zero,
              clipBehavior: Clip.antiAlias,
              child: Column(
                children: [
                  ListTile(
                    key: const ValueKey('podcast-downloads-usage'),
                    leading: Icon(Icons.podcasts, color: cs.primary),
                    title: Text(usage.files == 0 ? l.podcastDownloadsNone : l.podcastDownloadsUsed(formatBytes(l.localeName, usage.bytes), usage.files)),
                    trailing: usage.files == 0
                        ? null
                        : TextButton(key: const ValueKey('podcast-downloads-delete'), onPressed: () => _confirmDeleteAll(context), child: Text(l.podcastDownloadsDeleteAll)),
                  ),
                  const Divider(height: 1),
                  SwitchListTile(
                    key: const ValueKey('podcast-delete-after'),
                    title: Text(l.podcastDeleteAfter),
                    subtitle: Text(l.podcastDeleteAfterHint),
                    value: downloads.deleteAfterListening,
                    onChanged: downloads.setDeleteAfterListening,
                  ),
                  const Divider(height: 1),
                  ListTile(
                    title: Text(l.podcastBulkLimit),
                    subtitle: Text(l.podcastBulkLimitHint),
                    trailing: DropdownButton<int>(
                      key: const ValueKey('podcast-bulk-limit'),
                      value: downloads.bulkMegabytes,
                      items: [for (final mb in bulkChoices) DropdownMenuItem(value: mb, child: Text(formatBytes(l.localeName, mb * 1000 * 1000)))],
                      onChanged: (mb) {
                        if (mb != null) downloads.setBulkMegabytes(mb);
                      },
                    ),
                  ),
                  const Divider(height: 1),
                  ListTile(
                    title: Text(l.podcastMaxSize),
                    trailing: DropdownButton<int>(
                      key: const ValueKey('podcast-max-size'),
                      value: downloads.maxMegabytes,
                      items: [for (final mb in choices) DropdownMenuItem(value: mb, child: Text(l.podcastMegabytes(mb)))],
                      onChanged: (mb) {
                        if (mb != null) downloads.setMaxMegabytes(mb);
                      },
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

/// The question in front of "download the whole collection": how many episodes, about how large, and, when more is missing than the limit allows, what
/// would be loaded instead. Returns the episodes to fetch, or null for "no". Nothing is fetched without this question.
Future<List<Episode>?> askBulkDownload(BuildContext context, BulkPlan plan) {
  final l = AppLocalizations.of(context);
  String size(int bytes) => formatBytes(l.localeName, bytes);
  final limit = size(plan.limitBytes);
  final String text;
  if (plan.blocked) {
    text = l.podcastDownloadAllBlocked(limit);
  } else if (plan.exceedsLimit) {
    text = l.podcastDownloadAllLimited(plan.count, size(plan.totalBytes), limit, plan.fittingCount, size(plan.fittingBytes));
  } else {
    text = l.podcastDownloadAllText(plan.count, size(plan.totalBytes));
  }
  return showDialog<List<Episode>>(
    context: context,
    builder: (context) => AlertDialog(
      key: const ValueKey('bulk-download-dialog'),
      title: Text(l.podcastDownloadAllTitle),
      content: Text(text),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(l.cancel)),
        if (!plan.blocked)
          FilledButton(
            key: const ValueKey('bulk-download-start'),
            onPressed: () => Navigator.pop(context, plan.fitting),
            child: Text(l.podcastDownloadAllStart),
          ),
      ],
    ),
  );
}
