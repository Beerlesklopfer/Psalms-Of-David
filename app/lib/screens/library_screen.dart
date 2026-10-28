// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import 'media_category.dart';
import '../player_controller.dart';
import '../widgets.dart';


class LibraryScreen extends StatefulWidget {
  const LibraryScreen({super.key});

  @override
  State<LibraryScreen> createState() => _LibraryScreenState();
}

class _LibraryScreenState extends State<LibraryScreen> {
  MediaCategory _category = MediaCategory.audiobooks;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final cs = Theme.of(context).colorScheme;
    // The books of the chosen category; null for a category David does not manage yet.
    final shown = _category.pick(p.library);

    return CustomScrollView(
      slivers: [
        SliverAppBar(
          floating: true,
          backgroundColor: cs.surface,
          title: Text(
            l.navLibrary,
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          actions: [RefreshButton()],
        ),
        SliverToBoxAdapter(
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: CategoryChips(value: _category, onChanged: (c) => setState(() => _category = c)),
          ),
        ),
        if (shown == null)
          SliverFillRemaining(hasScrollBody: false, child: ComingSoon(category: _category))
        else if (shown.isEmpty)
          SliverFillRemaining(
            hasScrollBody: false,
            child: Center(child: Text(_category == MediaCategory.music ? l.noMusicYet : l.noAudiobooksYet)),
          )
        else
          SliverPadding(
            padding: const EdgeInsets.all(12),
            sliver: SliverGrid.builder(
              gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
                maxCrossAxisExtent: 200,
                childAspectRatio: 0.74,
              ),
              itemCount: shown.length,
              itemBuilder: (_, i) => BookCard(book: shown[i]),
            ),
          ),
      ],
    );
  }
}
