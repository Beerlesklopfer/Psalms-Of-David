// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../labels.dart';
import '../library_groups.dart';
import '../models.dart';
import '../player_controller.dart';
import '../podcasts.dart';
import '../offline/offline_controller.dart';
import '../server/api_client.dart';
import 'barcode_scan_screen.dart';
import '../station_logo.dart';
import '../widgets.dart';
import 'labels_ui.dart';
import 'media_category.dart';

class SearchScreen extends StatefulWidget {
  /// [api] is the server, for the podcasts and radio stations that the search finds besides the books (by name and by label); null without one.
  const SearchScreen({super.key, this.api});

  final FeedApi? api;

  @override
  State<SearchScreen> createState() => _SearchScreenState();
}

class _SearchScreenState extends State<SearchScreen> {
  final _controller = TextEditingController();
  MediaCategory _category = MediaCategory.all;

  /// The labels the hits are narrowed to (a book must carry all of them).
  Set<String> _labels = {};

  /// The podcasts and stations of the account, loaded once; empty without a server.
  List<Podcast> _podcasts = const [];
  List<Station> _stations = const [];

  @override
  void initState() {
    super.initState();
    _loadFeeds();
  }

  Future<void> _loadFeeds() async {
    final api = widget.api;
    if (api == null) return;
    try {
      final podcasts = (await api.podcasts()).map(Podcast.fromJson).where((p) => p.approved).toList();
      final stations = (await api.stations()).map(Station.fromJson).where((s) => s.approved).toList();
      if (mounted) setState(() { _podcasts = podcasts; _stations = stations; });
    } on ApiException {
      // the books are searched anyway
    }
  }

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
    final base = _category.pick(p.library);
    // Podcasts and stations are found in "All" (by name and by label); their labels belong to the filter there.
    // Offline there is no server to play them from: only the books that are downloaded are found.
    final withFeeds = _category == MediaCategory.all && OfflineScope.maybeOf(context)?.active != true;
    final feedRows = withFeeds ? labelCountsOf([for (final p in _podcasts) p.labels, for (final s in _stations) s.labels]) : const <(String, int)>[];
    final labelRows = base == null ? const <(String, int)>[] : mergeLabelCounts(labelCounts(base), feedRows);
    _labels.removeWhere((c) => !labelRows.any((r) => r.$1.toLowerCase() == c.toLowerCase()));
    final pool = base == null ? null : filterByLabels(base, _labels);
    // Labels chosen and nothing typed: the books with those labels are the hits.
    final byLabels = query.isEmpty && _labels.isNotEmpty;
    final found = byLabels
        ? SearchGroups(pool ?? const <LibraryItem>[], const [], const [])
        : searchGroups(pool ?? const <LibraryItem>[], query);
    final authors = {for (final b in pool ?? const <LibraryItem>[]) b.author}.toList()..sort();
    // The podcasts and stations that fit: the name contains the text and they carry all chosen labels; with nothing typed and no label none are listed.
    final feedsOn = withFeeds && (query.isNotEmpty || _labels.isNotEmpty);
    bool fits(String name, List<String> labels) =>
        carriesAll(labels, _labels) && (query.isEmpty || name.toLowerCase().contains(query) || labels.any((x) => x.toLowerCase().contains(query)));
    final podcastHits = feedsOn ? [for (final p in _podcasts) if (fits(p.title, p.labels)) p] : const <Podcast>[];
    final stationHits = feedsOn ? [for (final s in _stations) if (fits(s.name, s.labels)) s] : const <Station>[];
    final feedHits = podcastHits.length + stationHits.length;

    return CustomScrollView(
      slivers: [
        SliverAppBar(
          floating: true,
          backgroundColor: cs.surface,
          title: Text(l.navSearch, style: Theme.of(context).textTheme.headlineSmall),
          actions: [BarcodeButton(api: widget.api)],
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
        if (labelRows.isNotEmpty)
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.only(top: 8),
              child: LabelFilter(
                counts: labelRows,
                chosen: _labels,
                onChanged: (c) => setState(() => _labels = c),
              ),
            ),
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
        else if (query.isEmpty && !byLabels) ...[
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
        ] else if (found.isEmpty && feedHits == 0)
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
                  onTap: () => openItem(context, b),
                );
              },
            ),
          ],
          if (found.tracks.isNotEmpty) ...[
            SliverToBoxAdapter(child: SectionHeader(l.sectionTracks)),
            SliverList.builder(
              itemCount: found.tracks.length,
              itemBuilder: (context, i) {
                final hit = found.tracks[i];
                return ListTile(
                  key: ValueKey('search-track-${hit.book.id}-$i'),
                  leading: SizedBox.square(dimension: 48, child: Cover(book: hit.book)),
                  title: Text(hit.track.title),
                  subtitle: Text('${hit.book.title} · ${hit.book.author}'),
                  onTap: () => p.open(hit.book, track: hit.index),
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
        // Podcasts and radio stations by name or label: a station plays when tapped (a podcast is opened from its list).
        if (pool != null && podcastHits.isNotEmpty) ...[
          SliverToBoxAdapter(child: SectionHeader(l.navPodcasts)),
          SliverList.builder(
            itemCount: podcastHits.length,
            itemBuilder: (context, i) {
              final pod = podcastHits[i];
              return ListTile(
                key: ValueKey('search-podcast-${pod.id}'),
                leading: const Icon(Icons.podcasts),
                title: Text(pod.title),
                subtitle: pod.labels.isEmpty ? null : Text(pod.labels.join(' · ')),
              );
            },
          ),
        ],
        if (pool != null && stationHits.isNotEmpty) ...[
          SliverToBoxAdapter(child: SectionHeader(l.navRadio)),
          SliverList.builder(
            itemCount: stationHits.length,
            itemBuilder: (context, i) {
              final s = stationHits[i];
              return ListTile(
                key: ValueKey('search-station-${s.id}'),
                leading: const Icon(Icons.radio),
                title: Text(s.name),
                subtitle: s.labels.isEmpty ? null : Text(s.labels.join(' · ')),
                onTap: () async {
                  final p = PlayerScope.of(context);
                  final logo = await StationLogos.load(s.logoUrl);
                  p.open(s.asBook(logo: logo));
                },
              );
            },
          ),
        ],
        const SliverToBoxAdapter(child: SizedBox(height: 32)),
      ],
    );
  }
}
