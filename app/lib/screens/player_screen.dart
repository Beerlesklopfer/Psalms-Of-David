// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../format.dart';
import '../l10n/app_localizations.dart';
import '../player_controller.dart';
import '../stream_info_card.dart';
import '../widgets.dart';


class PlayerScreen extends StatelessWidget {
  const PlayerScreen({super.key});

  static Route<void> route() => MaterialPageRoute<void>(
        fullscreenDialog: true,
        builder: (_) => const PlayerScreen(),
      );

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final book = p.book;
    final track = p.track;
    if (book == null || track == null) {
      return Scaffold(body: Center(child: Text(l.nothingSelected)));
    }
    // In a playlist the book of the track that plays now (its cover, author and title).
    final shown = p.shownBook ?? book;
    final cs = Theme.of(context).colorScheme;
    final text = Theme.of(context).textTheme;

    return Scaffold(
      appBar: AppBar(
        backgroundColor: Colors.transparent,
        leading: IconButton(
          tooltip: l.tooltipClose,
          icon: const Icon(Icons.expand_more, size: 30),
          onPressed: () => Navigator.of(context).pop(),
        ),
        title: Text(
          shown.author,
          style: text.titleSmall?.copyWith(color: cs.onSurfaceVariant),
        ),
        centerTitle: true,
      ),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 520),
          child: ListView(
            padding: const EdgeInsets.symmetric(horizontal: 24),
            children: [
              const SizedBox(height: 8),
              AspectRatio(aspectRatio: 1, child: Cover(book: shown)),
              const SizedBox(height: 28),
              Text(shown.title, style: text.headlineSmall),
              const SizedBox(height: 4),
              Text(
                // A station says what it plays now; a book says its disc and track.
                p.nowPlaying ?? (track.live ? track.title : l.discTrack(track.disc, track.title)),
                style: text.bodyMedium?.copyWith(color: cs.onSurfaceVariant),
              ),
              const SizedBox(height: 12),
              if (track.live) ...[
                Padding(padding: const EdgeInsets.symmetric(vertical: 12), child: Text(l.radioLive, style: text.titleMedium?.copyWith(color: cs.primary))),
                StreamInfoCard(player: p),
              ]
              else ...[
              const SeekBar(),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(formatDuration(p.position), style: text.bodySmall),
                    Text(
                      '−${formatDuration(track.length - p.position)}',
                      style: text.bodySmall,
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 4),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(l.bookPlayed(formatDuration(p.bookPosition)), style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
                    Text(l.bookTotal(formatDuration(p.bookTotal)), style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
                  ],
                ),
              ),
              const SeekBar(wholeBook: true),
              ],
              const SizedBox(height: 8),
              Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  IconButton(
                    tooltip: l.tooltipPrevious,
                    onPressed: p.previous,
                    icon: const Icon(Icons.skip_previous),
                    iconSize: 34,
                  ),
                  IconButton(
                    tooltip: l.tooltipBack10,
                    onPressed: () => p.skip(const Duration(seconds: -10)),
                    icon: const Icon(Icons.replay_10),
                    iconSize: 34,
                  ),
                  IconButton.filled(
                    tooltip: p.playing ? l.tooltipPause : l.tooltipPlay,
                    onPressed: p.toggle,
                    style: IconButton.styleFrom(
                      backgroundColor: cs.primary,
                      foregroundColor: cs.onPrimary,
                      fixedSize: const Size(68, 68),
                    ),
                    icon: Icon(p.playing ? Icons.pause : Icons.play_arrow),
                    iconSize: 40,
                  ),
                  IconButton(
                    tooltip: l.tooltipForward30,
                    onPressed: () => p.skip(const Duration(seconds: 30)),
                    icon: const Icon(Icons.forward_30),
                    iconSize: 34,
                  ),
                  IconButton(
                    tooltip: l.tooltipNext,
                    onPressed: p.next,
                    icon: const Icon(Icons.skip_next),
                    iconSize: 34,
                  ),
                ],
              ),
              const SizedBox(height: 20),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                alignment: WrapAlignment.center,
                children: [
                  for (final s in playbackSpeeds)
                    ChoiceChip(
                      label: Text(formatSpeed(l.localeName, s)),
                      selected: p.speed == s,
                      onSelected: (_) => p.setSpeed(s),
                    ),
                ],
              ),
              SectionHeader(l.sectionChapters),
              for (final (i, t) in book.tracks.indexed)
                ListTile(
                  dense: true,
                  selected: i == p.trackIndex,
                  selectedColor: cs.primary,
                  leading: SizedBox(
                    width: 28,
                    child: i == p.trackIndex
                        ? Icon(Icons.graphic_eq, color: cs.primary)
                        : Text('${i + 1}', textAlign: TextAlign.end),
                  ),
                  title: Text(l.discTrack(t.disc, t.title)),
                  trailing: Row(mainAxisSize: MainAxisSize.min, children: [
                    if (p.isTrackPlayed(book, i)) ...const [PlayedMark(), SizedBox(width: 8)],
                    Text(formatDuration(t.length)),
                  ]),
                  onTap: () => p.open(book, track: i),
                ),
              const SizedBox(height: 24),
            ],
          ),
        ),
      ),
    );
  }
}
