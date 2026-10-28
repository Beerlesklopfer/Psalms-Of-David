// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../models.dart';
import '../player_controller.dart';
import '../widgets.dart';

String _greeting(AppLocalizations l) {
  final h = DateTime.now().hour;
  if (h < 11) return l.greetingMorning;
  if (h < 18) return l.greetingDay;
  return l.greetingEvening;
}

class HomeScreen extends StatelessWidget {
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final wide = MediaQuery.sizeOf(context).width >= 800;
    final books = p.library;

    return CustomScrollView(
      slivers: [
        SliverAppBar(
          floating: true,
          backgroundColor: Theme.of(context).colorScheme.surface,
          title: Text(
            _greeting(l),
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          actions: [RefreshButton()],
        ),
        SliverPadding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          sliver: SliverGrid.count(
            crossAxisCount: wide ? 3 : 2,
            mainAxisSpacing: 8,
            crossAxisSpacing: 8,
            childAspectRatio: wide ? 5 : 3.2,
            children: [for (final b in books.take(6)) _Shortcut(book: b)],
          ),
        ),
        if (p.book != null) ...[
          SliverToBoxAdapter(child: SectionHeader(l.sectionContinue)),
          SliverToBoxAdapter(child: _Resume(book: p.book!)),
        ],
        SliverToBoxAdapter(child: SectionHeader(l.sectionYourBooks)),
        SliverToBoxAdapter(
          child: SizedBox(
            height: 230,
            child: ListView.separated(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: 10),
              itemCount: books.length,
              separatorBuilder: (_, _) => const SizedBox(width: 4),
              itemBuilder: (_, i) =>
                  SizedBox(width: 156, child: BookCard(book: books[i])),
            ),
          ),
        ),
        const SliverToBoxAdapter(child: SizedBox(height: 32)),
      ],
    );
  }
}

/// Tile with a small cover, like the quick-access shortcuts on Spotify.
class _Shortcut extends StatelessWidget {
  const _Shortcut({required this.book});

  final Book book;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Material(
      color: cs.surfaceContainerHigh,
      borderRadius: BorderRadius.circular(6),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: () => openBook(context, book),
        child: Row(
          children: [
            AspectRatio(aspectRatio: 1, child: Cover(book: book)),
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                book.title,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context)
                    .textTheme
                    .titleSmall
                    ?.copyWith(fontWeight: FontWeight.w600),
              ),
            ),
            const SizedBox(width: 8),
          ],
        ),
      ),
    );
  }
}

class _Resume extends StatelessWidget {
  const _Resume({required this.book});

  final Book book;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final cs = Theme.of(context).colorScheme;
    final text = Theme.of(context).textTheme;
    final track = p.track!;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16),
      child: Material(
        color: cs.surfaceContainer,
        borderRadius: BorderRadius.circular(8),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: () => openBook(context, book),
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Row(
              children: [
                SizedBox.square(dimension: 84, child: Cover(book: book)),
                const SizedBox(width: 16),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        book.title,
                        style: text.titleMedium
                            ?.copyWith(fontWeight: FontWeight.w600),
                      ),
                      Text(
                        '${book.author} · ${l.discTrack(track.disc, track.title)}',
                        style: text.bodySmall
                            ?.copyWith(color: cs.onSurfaceVariant),
                      ),
                      const SizedBox(height: 10),
                      ClipRRect(
                        borderRadius: BorderRadius.circular(2),
                        child: LinearProgressIndicator(
                          value: p.progressOf(book),
                          minHeight: 3,
                          color: cs.primary,
                          backgroundColor: cs.outlineVariant,
                        ),
                      ),
                    ],
                  ),
                ),
                IconButton.filled(
                  tooltip: p.playing ? l.tooltipPause : l.tooltipPlay,
                  onPressed: p.toggle,
                  style: IconButton.styleFrom(
                    backgroundColor: cs.primary,
                    foregroundColor: cs.onPrimary,
                  ),
                  icon: Icon(p.playing ? Icons.pause : Icons.play_arrow),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
