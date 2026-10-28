// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../player_controller.dart';
import '../widgets.dart';

enum _Filter { books, music }

class LibraryScreen extends StatefulWidget {
  const LibraryScreen({super.key});

  @override
  State<LibraryScreen> createState() => _LibraryScreenState();
}

class _LibraryScreenState extends State<LibraryScreen> {
  _Filter _filter = _Filter.books;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final cs = Theme.of(context).colorScheme;

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
            child: Wrap(
              spacing: 8,
              children: [
                ChoiceChip(
                  label: Text(l.filterBooks),
                  selected: _filter == _Filter.books,
                  onSelected: (_) => setState(() => _filter = _Filter.books),
                ),
                ChoiceChip(
                  label: Text(l.filterMusic),
                  selected: _filter == _Filter.music,
                  onSelected: (_) => setState(() => _filter = _Filter.music),
                ),
              ],
            ),
          ),
        ),
        if (_filter == _Filter.music)
          SliverFillRemaining(
            hasScrollBody: false,
            child: Center(child: Text(l.noMusicYet)),
          )
        else
          SliverPadding(
            padding: const EdgeInsets.all(12),
            sliver: SliverGrid.builder(
              gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
                maxCrossAxisExtent: 200,
                childAspectRatio: 0.74,
              ),
              itemCount: p.library.length,
              itemBuilder: (_, i) => BookCard(book: p.library[i]),
            ),
          ),
      ],
    );
  }
}
