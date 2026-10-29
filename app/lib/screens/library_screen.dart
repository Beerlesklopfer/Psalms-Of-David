// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../cd/cd_import_ui.dart';
import '../cd/cd_kit.dart';
import '../cd/cd_progress_bar.dart';
import '../l10n/app_localizations.dart';
import '../library_groups.dart';
import '../models.dart';
import '../podcasts.dart';
import 'media_category.dart';
import 'playlists_screen.dart';
import 'podcasts_screen.dart';
import 'radio_screen.dart';
import '../player_controller.dart';
import '../widgets.dart';

class LibraryScreen extends StatefulWidget {
  const LibraryScreen({super.key, this.cd, this.api});

  /// The server, for the podcast and radio lists of the library; null without a server.
  final FeedApi? api;

  /// The CD import (drive, job tracker); null where there is none.
  final CdKit? cd;

  @override
  State<LibraryScreen> createState() => _LibraryScreenState();
}

class _LibraryScreenState extends State<LibraryScreen> {
  Widget _grid(List<Book> books) => SliverPadding(
    padding: const EdgeInsets.all(12),
    sliver: SliverGrid.builder(
      gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
        maxCrossAxisExtent: 200,
        childAspectRatio: 0.74,
      ),
      itemCount: books.length,
      itemBuilder: (_, i) => BookCard(book: books[i]),
    ),
  );

  MediaCategory _category = MediaCategory.audiobooks;

  // The lists of podcasts and stations are the same as on the pages of their own; the library
  // makes them when the category is first chosen and keeps them.
  PodcastsController? _podcasts;
  RadioController? _radio;

  @override
  void dispose() {
    _podcasts?.dispose();
    _radio?.dispose();
    super.dispose();
  }

  /// The chosen genre of the music (null with [_other] false: all genres).
  String? _genre;

  /// The albums without a genre are chosen.
  bool _other = false;

  /// Does the album fit the chosen genre (or are all genres chosen)?
  bool _matchesGenre(Book b) {
    final g = (b.genre ?? '').trim();
    if (_other) return g.isEmpty;
    return _genre == null || g == _genre;
  }

  void _pick(String? genre, bool other) {
    _genre = genre;
    _other = other;
  }

  /// The second row of chips under the categories, only for music: All, the genres in the order of
  /// the administration with the number of albums, and Other for albums without a genre.
  Widget _genreChips(AppLocalizations l, List<(String?, List<Book>)> groups) {
    Widget chip(String label, bool selected, VoidCallback onTap, {Key? key}) => Padding(
      padding: const EdgeInsets.only(right: 8),
      child: FilterChip(
        key: key,
        label: Text(label),
        selected: selected,
        onSelected: (_) => onTap(),
        visualDensity: VisualDensity.compact,
        materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
        labelStyle: Theme.of(context).textTheme.labelMedium,
      ),
    );
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.symmetric(horizontal: 16),
      child: Row(
        children: [
          chip(
            l.filterAll,
            _genre == null && !_other,
            () => setState(() => _pick(null, false)),
            key: const Key('genre-all'),
          ),
          for (final (genre, books) in groups)
            if (genre != null)
              chip(
                '$genre (${books.length})',
                _genre == genre,
                () => setState(() => _pick(genre, false)),
                key: Key('genre-$genre'),
              ),
          for (final (genre, books) in groups)
            if (genre == null)
              chip(
                '${l.genreOther} (${books.length})',
                _other,
                () => setState(() => _pick(null, true)),
                key: const Key('genre-other'),
              ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final cs = Theme.of(context).colorScheme;
    // The books of the chosen category; null for a category David does not manage yet.
    final shown = _category.pick(p.library);
    if (_category == MediaCategory.podcasts) _podcasts ??= PodcastsController(widget.api);
    if (_category == MediaCategory.radio) _radio ??= RadioController(widget.api);
    final playlists = PlaylistScope.maybeOf(context);
    final isMusic = _category == MediaCategory.music && shown != null && shown.isNotEmpty;
    final groups = isMusic ? groupByGenre(shown, p.genres) : const <(String?, List<Book>)>[];
    // A chosen genre that is gone (a reload took the last album) falls back to All.
    if (isMusic &&
        (_other
            ? !groups.any((g) => g.$1 == null)
            : (_genre != null && !groups.any((g) => g.$1 == _genre)))) {
      _genre = null;
      _other = false;
    }

    return CustomScrollView(
      slivers: [
        SliverAppBar(
          floating: true,
          backgroundColor: cs.surface,
          title: Text(l.navLibrary, style: Theme.of(context).textTheme.headlineSmall),
          actions: [
            if (_category == MediaCategory.podcasts)
              PodcastsActions(controller: _podcasts!, withRefresh: false),
            if (_category == MediaCategory.radio)
              RadioActions(controller: _radio!, withRefresh: false),
            if (_category == MediaCategory.playlists && playlists != null)
              PlaylistsActions(controller: playlists),
            if (widget.cd != null) CdActions(kit: widget.cd!),
            // One refresh button: it reloads the library and, in the categories of podcasts and radio, both lists.
            RefreshButton(
              also: widget.api == null
                  ? null
                  : () {
                      _podcasts?.load();
                      _radio?.load();
                      playlists?.load(force: true);
                    },
            ),
          ],
        ),
        if (widget.cd != null) SliverToBoxAdapter(child: CdProgressStrip(kit: widget.cd!)),
        SliverToBoxAdapter(
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: CategoryChips(value: _category, onChanged: (c) => setState(() => _category = c)),
          ),
        ),
        if (isMusic)
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.only(bottom: 4),
              child: _genreChips(l, groups),
            ),
          ),
        if (_category == MediaCategory.podcasts)
          PodcastsBody(controller: _podcasts!)
        else if (_category == MediaCategory.radio)
          RadioBody(controller: _radio!)
        else if (_category == MediaCategory.playlists)
          playlists == null
              ? SliverFillRemaining(
                  hasScrollBody: false,
                  child: Center(child: Text(l.playlistsEmpty)),
                )
              : PlaylistsBody(controller: playlists)
        else if (shown == null)
          SliverFillRemaining(hasScrollBody: false, child: ComingSoon(category: _category))
        else if (shown.isEmpty)
          SliverFillRemaining(
            hasScrollBody: false,
            child: Center(
              child: Text(_category == MediaCategory.music ? l.noMusicYet : l.noAudiobooksYet),
            ),
          )
        else if (_category == MediaCategory.music && shown.any((b) => (b.owner ?? '').isNotEmpty))
          // Music of the family members: "Music of Anna" ... and what belongs to nobody. The chosen genre narrows every group.
          for (final (owner, books) in groupByOwner(
            shown.where((b) => _matchesGenre(b)).toList(),
          )) ...[
            SliverToBoxAdapter(
              child: SectionHeader(owner == null ? l.musicOfNobody : l.musicOf(owner)),
            ),
            _grid(books),
          ]
        else if (_category == MediaCategory.music)
          // Music is shown by genre, in the order of the administration.
          for (final (genre, books) in groups.where(
            (g) => _other ? g.$1 == null : (_genre == null || g.$1 == _genre),
          )) ...[SliverToBoxAdapter(child: SectionHeader(genre ?? l.genreOther)), _grid(books)]
        else if (_category == MediaCategory.audiobooks &&
            shown.any((b) => (b.owner ?? '').isNotEmpty))
          // Audiobooks of the family members: "Audiobooks of Anna" ... and what belongs to nobody.
          for (final (owner, books) in groupByOwner(shown)) ...[
            SliverToBoxAdapter(
              child: SectionHeader(owner == null ? l.audiobooksOfNobody : l.audiobooksOf(owner)),
            ),
            _grid(books),
          ]
        else
          _grid(shown),
      ],
    );
  }
}
