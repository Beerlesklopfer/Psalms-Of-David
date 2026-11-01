// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../models.dart';

/// The kinds of media, as the library and the search offer them. Videos and e-books are shown so
/// that it is clear they are coming; David does not manage them yet.
enum MediaCategory { all, music, audiobooks, podcasts, radio, playlists, videos, ebooks, hidden }

extension MediaCategoryItems on MediaCategory {
  /// The books of this category, or null for a category David does not manage yet.
  List<LibraryItem>? pick(List<LibraryItem> library) => switch (this) {
        MediaCategory.all => library,
        MediaCategory.music => library.where((b) => b.kind == 'music').toList(),
        MediaCategory.audiobooks => library.where((b) => b.kind == 'audiobook').toList(),
        MediaCategory.podcasts ||
        MediaCategory.radio ||
        MediaCategory.playlists ||
        MediaCategory.videos ||
        MediaCategory.ebooks ||
        MediaCategory.hidden => null,
      };
}

/// The row of chips that chooses a category. For the search ([forSearch]) there is "All" in front
/// and no podcasts and radio (they are lists of feeds and stations, not found by the search); the
/// library has them and no "All".
class CategoryChips extends StatelessWidget {
  const CategoryChips({
    super.key,
    required this.value,
    required this.onChanged,
    this.forSearch = false,
    this.showHidden = false,
  });

  final MediaCategory value;
  final ValueChanged<MediaCategory> onChanged;
  final bool forSearch;

  /// The library offers the rubric of what the person hid (their switch "show what I hid" is on).
  final bool showHidden;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final entries = [
      if (forSearch) (MediaCategory.all, l.filterAll, Icons.apps_outlined),
      (MediaCategory.music, l.filterMusic, Icons.music_note_outlined),
      (MediaCategory.audiobooks, l.filterBooks, Icons.menu_book_outlined),
      if (!forSearch) (MediaCategory.podcasts, l.navPodcasts, Icons.podcasts),
      if (!forSearch) (MediaCategory.radio, l.navRadio, Icons.radio),
      if (!forSearch) (MediaCategory.playlists, l.filterPlaylists, Icons.queue_music),
      (MediaCategory.videos, l.filterVideos, Icons.movie_outlined),
      (MediaCategory.ebooks, l.filterEbooks, Icons.auto_stories_outlined),
      if (!forSearch && showHidden)
        (MediaCategory.hidden, l.settingsHiddenTitle, Icons.visibility_off_outlined),
    ];
    return Wrap(
      // Justified: the buttons spread over the whole width of the page.
      alignment: WrapAlignment.spaceBetween,
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
