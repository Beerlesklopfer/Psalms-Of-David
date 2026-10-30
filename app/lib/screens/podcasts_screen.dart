// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../lists/list_controller.dart';
import '../lists/list_machine.dart';
import 'confirm_dialog.dart';
import '../labels.dart';
import '../offline/offline_controller.dart';
import '../podcast_downloads.dart';
import '../player_controller.dart';
import '../podcasts.dart';
import '../server/api_client.dart';
import '../widgets.dart';
import 'labels_ui.dart';
import 'podcast_downloads_ui.dart';

/// The state of the podcast list: loading, adding. Owned by the page or the library, shown by
/// [PodcastsBody], operated by [PodcastsActions].
class PodcastsController extends ListController<Podcast> {
  PodcastsController(this.api) : super(hasServer: api != null) {
    load();
  }

  /// Null without a server: podcasts live on the server, so there is nothing to show.
  final FeedApi? api;

  /// The labels the list is narrowed to (a podcast must carry all of them).
  Set<String> chosen = {};

  void choose(Set<String> labels) {
    chosen = labels;
    notifyListeners();
  }

  @override
  Future<List<Podcast>> fetchItems() async => (await api!.podcasts()).map(Podcast.fromJson).toList();

  @override
  void itemsArrived(List<Podcast> items) {
    // A chosen label that no podcast carries any more is dropped.
    final rest = labelCountsOf([for (final p in items) p.labels]);
    chosen.removeWhere((c) => !rest.any((r) => r.$1.toLowerCase() == c.toLowerCase()));
  }

  Future<void> add(BuildContext context) async {
    final l = AppLocalizations.of(context);
    final url = await askText(context, title: l.podcastAdd, label: l.podcastAddress, hint: 'https://example.org/feed.xml', action: l.podcastFollow);
    if (url == null || !context.mounted) return;
    final messenger = ScaffoldMessenger.of(context);
    try {
      final r = await api!.followPodcast(url);
      messenger.showSnackBar(SnackBar(content: Text(r['approved'] == true ? l.podcastFollowed(r['title'] as String) : l.askToRelease)));
      await load();
    } on ApiException catch (e) {
      messenger.showSnackBar(SnackBar(content: Text(e.code == 'invalid_feed' ? l.podcastInvalid : e.code == 'forbidden' ? l.notAllowed : l.serverUnreachableShort)));
    }
  }
}

/// The add and refresh buttons of the podcast list (for an app bar).
class PodcastsActions extends StatelessWidget {
  const PodcastsActions({super.key, required this.controller, this.withRefresh = true});

  final PodcastsController controller;

  /// The page of its own has a refresh button of its own; in the library the one of the library reloads the lists too.
  final bool withRefresh;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    // Adding needs the server.
    controller.noteOffline(active: OfflineScope.maybeOf(context)?.active == true);
    final state = controller.state;
    if (state is ListNeedsServer<Podcast> || state is ListOfflineView<Podcast>) return const SizedBox.shrink();
    return Row(mainAxisSize: MainAxisSize.min, children: [
      IconButton(tooltip: l.podcastAdd, icon: const Icon(Icons.add), onPressed: () => controller.add(context)),
      if (withRefresh) IconButton(tooltip: l.tooltipRefresh, icon: const Icon(Icons.refresh), onPressed: controller.load),
    ]);
  }
}

/// The podcast list as a sliver, for the page of its own and for the library.
class PodcastsBody extends StatelessWidget {
  const PodcastsBody({super.key, required this.controller});

  final PodcastsController controller;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        controller.noteOffline(active: OfflineScope.maybeOf(context)?.active == true);
        final state = controller.state;
        if (state is! ListLoaded<Podcast>) {
          return switch (state) {
            // The offline view: the podcasts have episodes on the device, nothing is asked of the server.
            ListOfflineView() => OfflinePodcastsSliver(downloads: PlayerScope.maybeOf(context)?.podcastDownloads),
            ListNeedsServer() => SliverFillRemaining(hasScrollBody: false, child: Center(child: Text(l.needsServer))),
            ListFailed() => SliverFillRemaining(hasScrollBody: false, child: Center(child: Text(l.serverUnreachableShort))),
            ListIdle() || ListLoading() => const SliverFillRemaining(hasScrollBody: false, child: Center(child: CircularProgressIndicator())),
            ListEmpty() => SliverFillRemaining(hasScrollBody: false, child: Center(child: Text(l.noPodcastsYet))),
            ListLoaded() => throw StateError('unreachable'),
          };
        }
        final list = state.items;
        final labels = LabelsScope.maybeOf(context);
        final shown = [for (final p in list) if (carriesAll(p.labels, controller.chosen)) p];
        return SliverMainAxisGroup(slivers: [
          SliverToBoxAdapter(
            child: LabelFilter(
              counts: labelCountsOf([for (final p in list) p.labels]),
              chosen: controller.chosen,
              onChanged: controller.choose,
            ),
          ),
          SliverList.builder(
          itemCount: shown.length,
          itemBuilder: (context, i) {
            final p = shown[i];
            return ListTile(
              leading: const Icon(Icons.podcasts),
              title: Text(p.title),
              subtitle: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(!p.approved ? l.waitingForRelease : (p.error != null ? l.feedProblem : p.description), maxLines: 2, overflow: TextOverflow.ellipsis),
                  FeedLabelsLine(
                    idKey: 'podcast-${p.id}',
                    title: p.title,
                    labels: p.labels,
                    canLabel: p.canLabel,
                    onSave: (words) async {
                      final ok = labels != null && await labels.savePodcast(p.id, words, current: p.labels, title: p.title);
                      if (ok) await controller.load();
                      return ok;
                    },
                  ),
                ],
              ),
              trailing: p.approved && p.unheard > 0 ? Badge(label: Text('${p.unheard}')) : null,
              enabled: p.approved,
              onTap: p.approved
                  ? () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => EpisodesScreen(api: controller.api!, podcast: p, onUnfollowed: controller.load)))
                  : null,
            );
          },
        ),
        ]);
      },
    );
  }
}

/// The podcasts of the account (the same on every device of it): follow one by the address of its feed, open
/// it to see the episodes, play an episode where the account stopped.
class PodcastsScreen extends StatefulWidget {
  const PodcastsScreen({super.key, required this.api});

  final FeedApi? api;

  @override
  State<PodcastsScreen> createState() => _PodcastsScreenState();
}

class _PodcastsScreenState extends State<PodcastsScreen> {
  late final PodcastsController _controller = PodcastsController(widget.api);

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return CustomScrollView(
      slivers: [
        SliverAppBar(
          floating: true,
          backgroundColor: Theme.of(context).colorScheme.surface,
          title: Text(l.navPodcasts, style: Theme.of(context).textTheme.headlineSmall),
          actions: [PodcastsActions(controller: _controller)],
        ),
        PodcastsBody(controller: _controller),
      ],
    );
  }
}

/// The episodes of one podcast, newest first.
class EpisodesScreen extends StatefulWidget {
  const EpisodesScreen({super.key, required this.api, required this.podcast, required this.onUnfollowed});

  final FeedApi api;
  final Podcast podcast;
  final Future<void> Function() onUnfollowed;

  @override
  State<EpisodesScreen> createState() => _EpisodesScreenState();
}

class _EpisodesScreenState extends State<EpisodesScreen> {
  List<Episode>? _episodes;
  bool _failed = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final rows = await widget.api.episodes(widget.podcast.id);
      final list = rows.map(Episode.fromJson).toList();
      if (mounted) setState(() { _episodes = list; _failed = false; });
      // What is on the device from playing gets its entry, the state of the account is taken over, and (if the person asked) heard episodes go.
      if (mounted) await PlayerScope.maybeOf(context)?.podcastDownloads?.noteListed(widget.podcast, list);
    } on ApiException {
      if (mounted) setState(() => _failed = true);
    }
  }

  /// Fetches the newest unheard episodes that are not on the device yet, one after the other.
  Future<void> _downloadNewest(PodcastDownloads downloads) async {
    final l = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    final todo = downloads.newestToFetch(_episodes ?? const []);
    if (todo.isEmpty) {
      messenger.showSnackBar(SnackBar(content: Text(l.podcastDownloadNewestNone)));
      return;
    }
    for (final e in todo) {
      final code = await downloads.download(e, widget.podcast);
      if (code != null) {
        if (code != 'cancelled') messenger.showSnackBar(SnackBar(content: Text(episodeDownloadMessage(l, code, downloads.maxMegabytes))));
        break;
      }
    }
  }

  /// The server does not answer: the episodes that are on the device are still listed (and play).
  Widget _unreachable(BuildContext context, PodcastDownloads? downloads) {
    final l = AppLocalizations.of(context);
    final rows = downloads?.episodesOf(widget.podcast.id) ?? const <DownloadedEpisode>[];
    if (rows.isEmpty) return Center(child: Text(l.serverUnreachableShort));
    return ListView(children: [for (final r in rows) EpisodeTile(episode: r.toEpisode(), podcast: widget.podcast)]);
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final downloads = PlayerScope.of(context).podcastDownloads;
    final list = _episodes;
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.podcast.title),
        actions: [
          if (downloads != null && list != null && list.isNotEmpty)
            IconButton(
              key: const ValueKey('download-newest'),
              tooltip: l.podcastDownloadNewest(kNewestEpisodes),
              icon: const Icon(Icons.download_for_offline_outlined),
              onPressed: () => _downloadNewest(downloads),
            ),
          IconButton(
            tooltip: l.podcastUnfollow,
            icon: const Icon(Icons.bookmark_remove_outlined),
            onPressed: () async {
              final nav = Navigator.of(context);
              final sure = await confirmRemoval(context, title: l.podcastUnfollowTitle, text: l.podcastUnfollowAsk(widget.podcast.title), action: l.podcastUnfollow);
              if (!sure) return;
              await widget.api.unfollowPodcast(widget.podcast.id);
              await widget.onUnfollowed();
              nav.pop();
            },
          ),
        ],
      ),
      body: _failed
          ? _unreachable(context, downloads)
          : list == null
              ? const Center(child: CircularProgressIndicator())
              : list.isEmpty
                  ? Center(child: Text(l.noEpisodesYet))
                  : ListView.builder(
                      itemCount: list.length,
                      itemBuilder: (context, i) => EpisodeTile(episode: list[i], podcast: widget.podcast),
                    ),
    );
  }
}
