// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../format.dart';
import '../l10n/app_localizations.dart';
import '../labels.dart';
import '../player_controller.dart';
import '../podcasts.dart';
import '../server/api_client.dart';
import '../widgets.dart';
import 'labels_ui.dart';

/// The state of the podcast list: loading, adding. Owned by the page or the library, shown by
/// [PodcastsBody], operated by [PodcastsActions].
class PodcastsController extends ChangeNotifier {
  PodcastsController(this.api) {
    load();
  }

  /// Null without a server: podcasts live on the server, so there is nothing to show.
  final FeedApi? api;
  List<Podcast>? podcasts;
  String? error;
  bool _disposed = false;

  /// The labels the list is narrowed to (a podcast must carry all of them).
  Set<String> chosen = {};

  void choose(Set<String> labels) {
    chosen = labels;
    notifyListeners();
  }

  Future<void> load() async {
    final api = this.api;
    if (api == null) return;
    try {
      final rows = await api.podcasts();
      podcasts = rows.map(Podcast.fromJson).toList();
      // A chosen label that no podcast carries any more is dropped.
      final rest = labelCountsOf([for (final p in podcasts!) p.labels]);
      chosen.removeWhere((c) => !rest.any((r) => r.$1.toLowerCase() == c.toLowerCase()));
      error = null;
    } on ApiException catch (e) {
      error = e.code;
    }
    if (!_disposed) notifyListeners();
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

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
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
    if (controller.api == null) return const SizedBox.shrink();
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
        final list = controller.podcasts;
        if (controller.api == null) return SliverFillRemaining(hasScrollBody: false, child: Center(child: Text(l.needsServer)));
        if (controller.error != null) return SliverFillRemaining(hasScrollBody: false, child: Center(child: Text(l.serverUnreachableShort)));
        if (list == null) return const SliverFillRemaining(hasScrollBody: false, child: Center(child: CircularProgressIndicator()));
        if (list.isEmpty) return SliverFillRemaining(hasScrollBody: false, child: Center(child: Text(l.noPodcastsYet)));
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
      if (mounted) setState(() { _episodes = rows.map(Episode.fromJson).toList(); _failed = false; });
    } on ApiException {
      if (mounted) setState(() => _failed = true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    final list = _episodes;
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.podcast.title),
        actions: [
          IconButton(
            tooltip: l.podcastUnfollow,
            icon: const Icon(Icons.bookmark_remove_outlined),
            onPressed: () async {
              final nav = Navigator.of(context);
              await widget.api.unfollowPodcast(widget.podcast.id);
              await widget.onUnfollowed();
              nav.pop();
            },
          ),
        ],
      ),
      body: _failed
          ? Center(child: Text(l.serverUnreachableShort))
          : list == null
              ? const Center(child: CircularProgressIndicator())
              : list.isEmpty
                  ? Center(child: Text(l.noEpisodesYet))
                  : ListView.builder(
                      itemCount: list.length,
                      itemBuilder: (context, i) {
                        final e = list[i];
                        final date = e.published;
                        return ListTile(
                          leading: Icon(e.finished ? Icons.check_circle_outline : Icons.play_circle_outline),
                          title: Text(e.title, style: e.finished ? TextStyle(color: Theme.of(context).colorScheme.onSurfaceVariant) : null),
                          subtitle: Text([
                            if (date != null) '${date.year}-${date.month.toString().padLeft(2, '0')}-${date.day.toString().padLeft(2, '0')}',
                            if (e.duration > Duration.zero) formatDuration(e.duration),
                            if (!e.finished && e.position > Duration.zero) l.episodeAt(formatDuration(e.position)),
                          ].join(' · ')),
                          onTap: () {
                            p.open(e.asBook(widget.podcast.title), start: e.finished ? Duration.zero : e.position);
                          },
                        );
                      },
                    ),
    );
  }
}
