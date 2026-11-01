// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../audio/file_formats.dart';
import '../format.dart';
import '../l10n/app_localizations.dart';
import '../models.dart';
import '../player_controller.dart';
import '../widgets.dart';
import 'labels_ui.dart';
import 'hide_menu.dart';
import 'playlists_screen.dart';
import 'share_button.dart';

class ItemScreen extends StatelessWidget {
  const ItemScreen({super.key, required this.book});

  final LibraryItem book;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final cs = Theme.of(context).colorScheme;
    final text = Theme.of(context).textTheme;
    final isCurrent = p.book?.id == book.id;
    final progress = p.progressOf(book);
    final started = progress > 0;

    return Scaffold(
      appBar: AppBar(
        backgroundColor: Colors.transparent,
        actions: [
          // Books of the server only (an episode or a station is no item of the library).
          if (int.tryParse(book.id) != null) AddToPlaylistButton(bookId: int.parse(book.id)),
          ShareButton(book: book),
          HideMenuButton(book: book),
        ],
      ),
      bottomNavigationBar: const MiniPlayer(),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 720),
          child: ListView(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            children: [
              Center(
                child: SizedBox.square(dimension: 220, child: Cover(book: book)),
              ),
              const SizedBox(height: 20),
              Text(book.title, style: text.headlineMedium),
              const SizedBox(height: 4),
              Text(book.author, style: text.titleMedium?.copyWith(color: cs.primary)),
              if (book.narrator != null && book.narrator!.trim().isNotEmpty) ...[
                const SizedBox(height: 2),
                Text(
                  book.kind == 'music' ? l.artistLabel(book.narrator!) : l.readBy(book.narrator!),
                  style: text.bodyMedium?.copyWith(color: cs.onSurfaceVariant),
                ),
              ],
              // Whom the book belongs to (the nickname); not the same as who may see it.
              if (book.owner != null && book.owner!.trim().isNotEmpty) ...[
                const SizedBox(height: 2),
                Text(
                  l.ownerLabel(book.owner!),
                  key: const ValueKey('owner-line'),
                  style: text.bodyMedium?.copyWith(color: cs.onSurfaceVariant),
                ),
              ],
              if (book.year != null) ...[
                const SizedBox(height: 2),
                Text(
                  l.releasedLabel(book.year!),
                  key: const ValueKey('year-line'),
                  style: text.bodyMedium?.copyWith(color: cs.onSurfaceVariant),
                ),
              ],
              // The labels (classic tagging): everybody who sees the book sees them; the owner and an administrator may change them.
              LabelsLine(book: book),
              const SizedBox(height: 4),
              Text(
                l.bookMeta(book.discs, book.tracks.length, formatTotal(l, book.total)),
                style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
              ),
              if (_playsLine(context, p) case final line?) ...[
                const SizedBox(height: 4),
                Text(
                  line,
                  key: const ValueKey('plays-line'),
                  style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
                ),
              ],
              if (book.blurb != null && book.blurb!.trim().isNotEmpty) ...[
                const SizedBox(height: 12),
                Text(
                  book.blurb!,
                  style: text.bodyMedium?.copyWith(color: cs.onSurfaceVariant, height: 1.45),
                ),
              ],
              if (started) ...[
                const SizedBox(height: 12),
                ClipRRect(
                  borderRadius: BorderRadius.circular(2),
                  child: LinearProgressIndicator(
                    value: progress,
                    minHeight: 3,
                    color: cs.primary,
                    backgroundColor: cs.outlineVariant,
                  ),
                ),
                const SizedBox(height: 4),
                Text(
                  l.percentListened((progress * 100).round()),
                  style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
                ),
              ],
              const SizedBox(height: 16),
              Row(
                children: [
                  IconButton.filled(
                    tooltip: isCurrent && p.playing ? l.tooltipPause : l.tooltipPlay,
                    onPressed: () => isCurrent ? p.toggle() : p.open(book),
                    style: IconButton.styleFrom(
                      backgroundColor: cs.primary,
                      foregroundColor: cs.onPrimary,
                      fixedSize: const Size(56, 56),
                    ),
                    icon: Icon(isCurrent && p.playing ? Icons.pause : Icons.play_arrow),
                    iconSize: 32,
                  ),
                  const SizedBox(width: 8),
                  Text(started ? l.actionContinue : l.actionListen, style: text.titleSmall),
                  const Spacer(),
                  DownloadButton(book: book),
                ],
              ),
              for (final disc in book.discNumbers) ...[
                SectionHeader(l.discLabel(disc)),
                for (final t in book.tracks.where((t) => t.disc == disc))
                  _TrackTile(book: book, track: t),
              ],
              const SizedBox(height: 24),
            ],
          ),
        ),
      ),
    );
  }
}

extension on ItemScreen {
  /// "Played 3 times · last played on 27 Oct 2026"; null if there is nothing to say.
  String? _playsLine(BuildContext context, PlayerController p) {
    final l = AppLocalizations.of(context);
    final plays = p.playsOf(book);
    // A book heard just now has no date from the server yet: today.
    final at = book.lastPlayedAt ?? (plays > 0 ? DateTime.now() : null);
    final parts = [if (plays > 0) l.playsCount(plays), if (at != null) l.lastPlayedOn(at)];
    return parts.isEmpty ? null : parts.join(' · ');
  }
}

class _TrackTile extends StatelessWidget {
  const _TrackTile({required this.book, required this.track});

  final LibraryItem book;
  final Track track;

  @override
  Widget build(BuildContext context) {
    final p = PlayerScope.of(context);
    final cs = Theme.of(context).colorScheme;
    final index = book.tracks.indexOf(track);
    final active = p.book?.id == book.id && p.trackIndex == index;
    return ListTile(
      dense: true,
      selected: active,
      selectedColor: cs.primary,
      leading: SizedBox(
        width: 28,
        child: active
            ? Icon(Icons.graphic_eq, color: cs.primary)
            : Text('${track.number}', textAlign: TextAlign.end),
      ),
      title: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [Text(track.title), TrackLabels(book: book, track: track)],
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          TrackLabelsButton(book: book, track: track),
          if (p.isTrackPlayed(book, index)) ...const [PlayedMark(), SizedBox(width: 8)],
          Text(formatDuration(track.length)),
          if (track.partId != null && int.tryParse(book.id) != null)
            AddToPlaylistButton(partId: track.partId, asMenu: true),
        ],
      ),
      onTap: () {
        // A file this platform cannot play (Ogg on an iPhone) says so instead of failing silently.
        if (!trackPlayableHere(track.format, track.transcode)) {
          ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text(AppLocalizations.of(context).formatNotPlayableIos)));
          return;
        }
        p.open(book, track: index);
      },
    );
  }
}

/// Keeps the book on the device: download, progress with a way to stop, and removing it again.
class DownloadButton extends StatelessWidget {
  const DownloadButton({super.key, required this.book});

  final LibraryItem book;

  @override
  Widget build(BuildContext context) {
    final p = PlayerScope.of(context);
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    if (!p.canDownload) {
      return IconButton(
        tooltip: l.tooltipDownloadUnavailable,
        onPressed: null,
        icon: const Icon(Icons.download_for_offline_outlined),
      );
    }
    final running = p.downloads[book.id];
    if (running != null) {
      return Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            l.downloadedCount(running.done, running.total),
            style: Theme.of(context).textTheme.bodySmall,
          ),
          IconButton(
            tooltip: l.tooltipDownloadCancel,
            onPressed: () => p.cancelDownload(book),
            icon: SizedBox.square(
              dimension: 24,
              child: CircularProgressIndicator(
                value: running.total == 0 ? null : running.done / running.total,
                strokeWidth: 3,
              ),
            ),
          ),
        ],
      );
    }
    final have = p.downloadedCount(book);
    final all = have == book.tracks.length && have > 0;
    if (all) {
      return IconButton(
        tooltip: l.tooltipDownloaded,
        onPressed: () => _confirmRemove(context, p, l),
        icon: Icon(Icons.download_done, color: cs.primary),
      );
    }
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (have > 0)
          Text(
            l.downloadedCount(have, book.tracks.length),
            style: Theme.of(context).textTheme.bodySmall,
          ),
        IconButton(
          tooltip: l.tooltipDownload,
          onPressed: () => p.downloadItem(book),
          icon: const Icon(Icons.download_for_offline_outlined),
        ),
      ],
    );
  }

  Future<void> _confirmRemove(BuildContext context, PlayerController p, AppLocalizations l) async {
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.downloadRemoveTitle),
        content: Text(l.downloadRemoveText),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.cancel)),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(l.downloadRemove),
          ),
        ],
      ),
    );
    if (yes == true) await p.removeDownload(book);
  }
}
