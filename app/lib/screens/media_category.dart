// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../models.dart';

/// The kinds of media, as the library and the search offer them. Videos and e-books are shown so
/// that it is clear they are coming; David does not manage them yet.
enum MediaCategory { all, music, audiobooks, videos, ebooks }

extension MediaCategoryBooks on MediaCategory {
  /// The books of this category, or null for a category David does not manage yet.
  List<Book>? pick(List<Book> library) => switch (this) {
        MediaCategory.all => library,
        MediaCategory.music => library.where((b) => b.kind == 'music').toList(),
        MediaCategory.audiobooks => library.where((b) => b.kind == 'audiobook').toList(),
        MediaCategory.videos || MediaCategory.ebooks => null,
      };
}

/// The row of chips that chooses a category. [withAll] adds "All" in front (the search has it,
/// the library does not).
class CategoryChips extends StatelessWidget {
  const CategoryChips({super.key, required this.value, required this.onChanged, this.withAll = false});

  final MediaCategory value;
  final ValueChanged<MediaCategory> onChanged;
  final bool withAll;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final entries = [
      if (withAll) (MediaCategory.all, l.filterAll, Icons.apps_outlined),
      (MediaCategory.music, l.filterMusic, Icons.music_note_outlined),
      (MediaCategory.audiobooks, l.filterBooks, Icons.menu_book_outlined),
      (MediaCategory.videos, l.filterVideos, Icons.movie_outlined),
      (MediaCategory.ebooks, l.filterEbooks, Icons.auto_stories_outlined),
    ];
    return Wrap(
      spacing: 8,
      runSpacing: 4,
      children: [
        for (final (category, label, icon) in entries)
          ChoiceChip(
            avatar: Icon(icon, size: 18),
            label: Text(label),
            selected: value == category,
            onSelected: (_) => onChanged(category),
          ),
      ],
    );
  }
}

/// What a category without any support shows: a note that it is planned.
class ComingSoon extends StatelessWidget {
  const ComingSoon({super.key, required this.category});

  final MediaCategory category;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    final videos = category == MediaCategory.videos;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(videos ? Icons.movie_outlined : Icons.auto_stories_outlined, size: 48, color: cs.onSurfaceVariant),
            const SizedBox(height: 12),
            Text(l.comingSoonTitle, style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            Text(videos ? l.comingSoonVideos : l.comingSoonEbooks, textAlign: TextAlign.center, style: TextStyle(color: cs.onSurfaceVariant)),
          ],
        ),
      ),
    );
  }
}
