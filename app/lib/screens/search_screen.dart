// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../library_groups.dart';
import '../models.dart';
import '../player_controller.dart';
import '../widgets.dart';
import 'media_category.dart';

class SearchScreen extends StatefulWidget {
  const SearchScreen({super.key});

  @override
  State<SearchScreen> createState() => _SearchScreenState();
}

class _SearchScreenState extends State<SearchScreen> {
  final _controller = TextEditingController();
  MediaCategory _category = MediaCategory.all;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  /// A name from the hits; a tap searches for exactly this name, which brings all their albums.
  Widget _names(List<String> names, IconData icon) => SliverList.builder(
        itemCount: names.length,
        itemBuilder: (_, i) => ListTile(
          leading: Icon(icon),
          title: Text(names[i]),
          onTap: () => setState(() => _controller.text = names[i]),
        ),
      );

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final cs = Theme.of(context).colorScheme;
    final query = _controller.text.trim().toLowerCase();
    // The books of the chosen media category; null for one David does not manage yet.
    final pool = _category.pick(p.library);
    final found = searchGroups(pool ?? const <Book>[], query);
    final authors = {for (final b in pool ?? const <Book>[]) b.author}.toList()..sort();

    return CustomScrollView(
      slivers: [
        SliverAppBar(
          floating: true,
          backgroundColor: cs.surface,
          title: Text(l.navSearch, style: Theme.of(context).textTheme.headlineSmall),
        ),
        SliverPadding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          sliver: SliverToBoxAdapter(
            child: TextField(
              controller: _controller,
              onChanged: (_) => setState(() {}),
              decoration: InputDecoration(
                hintText: l.searchHint,
                prefixIcon: const Icon(Icons.search),
                suffixIcon: query.isEmpty
                    ? null
                    : IconButton(
                        tooltip: l.tooltipClear,
                        icon: const Icon(Icons.close),
                        onPressed: () => setState(_controller.clear),
                      ),
                filled: true,
                fillColor: cs.surfaceContainerHigh,
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(6),
                  borderSide: BorderSide.none,
                ),
              ),
            ),
          ),
        ),
        SliverPadding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 0),
          sliver: SliverToBoxAdapter(child: CategoryChips(value: _category, forSearch: true, onChanged: (c) => setState(() => _category = c))),
        ),
        // With a server, the text may be a passage of an audio Bible ("Johannes 3,16").
        if (query.isNotEmpty && p.source != null)
          SliverToBoxAdapter(
            child: ListTile(
              leading: const Icon(Icons.menu_book_outlined),
              title: Text(l.openPassage(_controller.text.trim())),
              onTap: () async {
                final messenger = ScaffoldMessenger.of(context);
                final opened = await p.openPassage(_controller.text.trim());
                if (!opened) messenger.showSnackBar(SnackBar(content: Text(l.passageNotFound)));
              },
            ),
          ),
        if (pool == null)
          SliverFillRemaining(hasScrollBody: false, child: ComingSoon(category: _category))
        else if (query.isEmpty) ...[
          SliverToBoxAdapter(
            child: SectionHeader(switch (_category) {
              MediaCategory.music => l.sectionArtists,
              MediaCategory.audiobooks => l.sectionAuthors,
              _ => l.sectionAuthorsAndArtists,
            }),
          ),
          SliverPadding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            sliver: SliverGrid.extent(
              maxCrossAxisExtent: 260,
              mainAxisSpacing: 8,
              crossAxisSpacing: 8,
              childAspectRatio: 2.4,
              children: [
                for (final a in authors)
                  Material(
                    color: cs.secondary.withValues(alpha: 0.55),
                    borderRadius: BorderRadius.circular(6),
                    child: InkWell(
                      borderRadius: BorderRadius.circular(6),
                      onTap: () => setState(() => _controller.text = a),
                      child: Padding(
                        padding: const EdgeInsets.all(12),
                        child: Text(
                          a,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context)
                              .textTheme
                              .titleSmall
                              ?.copyWith(
                                fontWeight: FontWeight.w600,
                                color: cs.onSecondary,
                              ),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ] else if (found.isEmpty)
          SliverFillRemaining(
            hasScrollBody: false,
            child: Center(child: Text(l.nothingFound)),
          )
        else ...[
          // The hits in groups: the albums (books) by title, then the artists (authors) and the narrators.
          if (found.albums.isNotEmpty) ...[
            SliverToBoxAdapter(
              child: SectionHeader(switch (_category) {
                MediaCategory.music => l.sectionAlbums,
                MediaCategory.audiobooks => l.sectionBooks,
                _ => l.sectionAlbumsAndBooks,
              }),
            ),
            SliverList.builder(
              itemCount: found.albums.length,
              itemBuilder: (context, i) {
                final b = found.albums[i];
                return ListTile(
                  leading: SizedBox.square(dimension: 48, child: Cover(book: b)),
                  title: Text(b.title),
                  subtitle: Text(b.author),
                  onTap: () => openBook(context, b),
                );
              },
            ),
          ],
          if (found.creators.isNotEmpty) ...[
            SliverToBoxAdapter(
              child: SectionHeader(switch (_category) {
                MediaCategory.music => l.sectionArtists,
                MediaCategory.audiobooks => l.sectionAuthors,
                _ => l.sectionAuthorsAndArtists,
              }),
            ),
            _names(found.creators, Icons.person_outline),
          ],
          if (found.narrators.isNotEmpty) ...[
            SliverToBoxAdapter(child: SectionHeader(l.sectionNarrators)),
            _names(found.narrators, Icons.record_voice_over_outlined),
          ],
        ],
        const SliverToBoxAdapter(child: SizedBox(height: 32)),
      ],
    );
  }
}
