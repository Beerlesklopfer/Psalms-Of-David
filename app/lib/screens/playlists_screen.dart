// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../format.dart';
import '../l10n/app_localizations.dart';
import '../models.dart';
import '../player_controller.dart';
import '../playlists.dart';
import '../widgets.dart';
import 'book_screen.dart';

/// Gives the screens the playlists of the profile. Without it (demo, tests) nothing about playlists is shown.
class PlaylistScope extends InheritedNotifier<PlaylistController> {
  const PlaylistScope({super.key, required PlaylistController controller, required super.child})
    : super(notifier: controller);

  static PlaylistController? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<PlaylistScope>()?.notifier;
}

/// The words for the code of a playlist error; a code the app does not know shows with the code in brackets.
String playlistErrorText(AppLocalizations l, String code) => switch (code) {
  'name_taken' => l.plErrNameTaken,
  'invalid_name' => l.plErrInvalidName,
  'unknown_part' => l.plErrUnknownPart,
  'unknown_book' => l.plErrUnknownBook,
  'invalid_order' => l.plErrInvalidOrder,
  'invalid_position' => l.plErrInvalidPosition,
  'invalid_request' => l.plErrInvalidRequest,
  'playlist_full' => l.plErrPlaylistFull,
  'too_many_playlists' => l.plErrTooManyPlaylists,
  'not_found' => l.plErrNotFound,
  'network' => l.plErrNetwork,
  _ => l.plErrUnknown(code),
};

/// Runs a change of a playlist and tells the user in words if the server refuses it. True if it worked.
Future<bool> _try(BuildContext context, Future<void> Function() change) async {
  final l = AppLocalizations.of(context);
  final messenger = ScaffoldMessenger.maybeOf(context);
  try {
    await change();
    return true;
  } on Object catch (e) {
    messenger?.showSnackBar(
      SnackBar(content: Text(playlistErrorText(l, PlaylistController.codeOf(e)))),
    );
    return false;
  }
}

/// Makes a playlist: one field, Save / Cancel. Returns its id, or null if cancelled or refused.
Future<int?> _askNewPlaylist(BuildContext context, PlaylistController c) async {
  final l = AppLocalizations.of(context);
  final name = await askText(
    context,
    title: l.playlistNew,
    label: l.playlistName,
    action: l.playlistSave,
  );
  if (name == null || !context.mounted) return null;
  int? id;
  await _try(context, () async => id = await c.create(name));
  return id;
}

/// "Add to playlist" for parts or a whole book: a sheet with the playlists and "New playlist …".
Future<void> showAddToPlaylist(
  BuildContext context,
  PlaylistController c, {
  List<int>? partIds,
  int? bookId,
}) async {
  assert((partIds == null) != (bookId == null));
  final l = AppLocalizations.of(context);
  final chosen = await showModalBottomSheet<int>(
    context: context,
    showDragHandle: true,
    builder: (context) => SafeArea(
      child: ListView(
        shrinkWrap: true,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: Text(l.playlistChoose, style: Theme.of(context).textTheme.titleMedium),
          ),
          for (final p in c.lists)
            ListTile(
              key: Key('add-to-${p.id}'),
              leading: const Icon(Icons.queue_music),
              title: Text(p.name),
              subtitle: Text(l.playlistMeta(p.items, formatTotal(l, p.total))),
              onTap: () => Navigator.pop(context, p.id),
            ),
          ListTile(
            key: const Key('add-to-new'),
            leading: const Icon(Icons.add),
            title: Text(l.playlistNewItem),
            onTap: () => Navigator.pop(context, -1),
          ),
        ],
      ),
    ),
  );
  if (chosen == null || !context.mounted) return;
  var id = chosen;
  if (id == -1) {
    final made = await _askNewPlaylist(context, c);
    if (made == null || !context.mounted) return;
    id = made;
  }
  final name = c.lists.where((p) => p.id == id).map((p) => p.name).firstOrNull ?? '';
  final done = await _try(context, () async {
    if (partIds != null) {
      await c.addParts(id, partIds);
    } else {
      await c.addBook(id, bookId!);
    }
  });
  if (done && context.mounted) {
    ScaffoldMessenger.maybeOf(context)
        ?.showSnackBar(SnackBar(content: Text(l.playlistAddedTo(name))));
  }
}

/// The add button of a track or a book: shown only where there is a server to keep the playlists.
class AddToPlaylistButton extends StatelessWidget {
  const AddToPlaylistButton({super.key, this.partId, this.bookId, this.asMenu = false});

  final int? partId;
  final int? bookId;

  /// A track shows it in a menu (the row has little room), a book as a button in the app bar.
  final bool asMenu;

  @override
  Widget build(BuildContext context) {
    final c = PlaylistScope.maybeOf(context);
    if (c == null || c.api == null) return const SizedBox.shrink();
    final l = AppLocalizations.of(context);
    void add() =>
        showAddToPlaylist(context, c, partIds: partId == null ? null : [partId!], bookId: bookId);
    if (asMenu) {
      return PopupMenuButton<int>(
        key: Key('track-menu-$partId'),
        icon: const Icon(Icons.more_vert, size: 20),
        padding: EdgeInsets.zero,
        onSelected: (_) => add(),
        itemBuilder: (_) => [PopupMenuItem(value: 0, child: Text(l.playlistAddTo))],
      );
    }
    return IconButton(
      key: Key('book-add-to-playlist-$bookId'),
      tooltip: l.playlistAddTo,
      icon: const Icon(Icons.playlist_add),
      onPressed: add,
    );
  }
}

/// The new-playlist button of the library's app bar.
class PlaylistsActions extends StatelessWidget {
  const PlaylistsActions({super.key, required this.controller});

  final PlaylistController controller;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    if (controller.api == null) return const SizedBox.shrink();
    return IconButton(
      key: const Key('playlist-new'),
      tooltip: l.playlistNew,
      icon: const Icon(Icons.add),
      onPressed: () => _askNewPlaylist(context, controller),
    );
  }
}

/// The list of playlists as a sliver, for the library: name, number of titles, running time.
class PlaylistsBody extends StatelessWidget {
  const PlaylistsBody({super.key, required this.controller});

  final PlaylistController controller;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final lists = controller.lists;
        if (lists.isEmpty) {
          if (controller.loading) {
            return const SliverFillRemaining(
              hasScrollBody: false,
              child: Center(child: CircularProgressIndicator()),
            );
          }
          return SliverFillRemaining(
            hasScrollBody: false,
            child: Center(child: Text(l.playlistsEmpty)),
          );
        }
        return SliverList.builder(
          itemCount: lists.length,
          itemBuilder: (context, i) {
            final p = lists[i];
            return ListTile(
              key: Key('playlist-${p.id}'),
              leading: const Icon(Icons.queue_music),
              title: Text(p.name),
              subtitle: Text(l.playlistMeta(p.items, formatTotal(l, p.total))),
              onTap: () => Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (_) => PlaylistScreen(controller: controller, id: p.id),
                ),
              ),
            );
          },
        );
      },
    );
  }
}

/// One playlist: play, download, rename, delete; the entries in order, to be dragged, with a button to remove each.
class PlaylistScreen extends StatefulWidget {
  const PlaylistScreen({super.key, required this.controller, required this.id});

  final PlaylistController controller;
  final int id;

  @override
  State<PlaylistScreen> createState() => _PlaylistScreenState();
}

class _PlaylistScreenState extends State<PlaylistScreen> {
  PlaylistController get c => widget.controller;

  @override
  void initState() {
    super.initState();
    c.open(widget.id);
  }

  Future<void> _play(Playlist list, {int? entryIndex}) async {
    final p = PlayerScope.of(context);
    final book = c.bookOf(list);
    if (book.tracks.isEmpty) return;
    if (entryIndex != null) {
      p.open(book, track: entryIndex);
    } else {
      final start = c.startOf(list, book);
      p.open(book, track: start.track, start: start.position);
    }
  }

  Future<void> _rename(Playlist list) async {
    final l = AppLocalizations.of(context);
    final name = await askText(
      context,
      title: l.playlistRenameTitle,
      label: l.playlistName,
      action: l.playlistSave,
      initial: list.name,
    );
    if (name == null || name == list.name || !mounted) return;
    await _try(context, () => c.rename(list.id, name));
  }

  Future<void> _delete(Playlist list) async {
    final l = AppLocalizations.of(context);
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.playlistDeleteTitle),
        content: Text(l.playlistDeleteText(list.name)),
        actions: [
          TextButton(
            key: const Key('playlist-delete-no'),
            onPressed: () => Navigator.pop(context, false),
            child: Text(l.cancel),
          ),
          FilledButton(
            key: const Key('playlist-delete-yes'),
            onPressed: () => Navigator.pop(context, true),
            child: Text(l.playlistDelete),
          ),
        ],
      ),
    );
    if (yes != true || !mounted) return;
    final nav = Navigator.of(context);
    if (await _try(context, () => c.delete(list.id))) nav.pop();
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    return ListenableBuilder(
      listenable: Listenable.merge([c, p]),
      builder: (context, _) {
        final list = c.details[widget.id];
        final book = list == null ? null : c.bookOf(list);
        return Scaffold(
          appBar: AppBar(
            title: Text(list?.name ?? ''),
            actions: [
              if (list != null) ...[
                IconButton(
                  key: const Key('playlist-play'),
                  tooltip: l.playlistPlay,
                  icon: const Icon(Icons.play_arrow),
                  onPressed: book!.tracks.isEmpty ? null : () => _play(list),
                ),
                DownloadButton(book: book),
                PopupMenuButton<String>(
                  key: const Key('playlist-menu'),
                  onSelected: (v) => v == 'rename' ? _rename(list) : _delete(list),
                  itemBuilder: (_) => [
                    PopupMenuItem(value: 'rename', child: Text(l.playlistRename)),
                    PopupMenuItem(value: 'delete', child: Text(l.playlistDelete)),
                  ],
                ),
              ],
            ],
          ),
          bottomNavigationBar: const MiniPlayer(),
          body: list == null
              ? const Center(child: CircularProgressIndicator())
              : list.entries.isEmpty
              ? Center(
                  child: Padding(
                    padding: const EdgeInsets.all(24),
                    child: Text(l.playlistEmpty, textAlign: TextAlign.center),
                  ),
                )
              : ReorderableListView.builder(
                  key: const Key('playlist-entries'),
                  buildDefaultDragHandles: false,
                  itemCount: list.entries.length,
                  onReorderItem: (from, to) => _try(context, () => c.reorder(list.id, from, to)),
                  itemBuilder: (context, i) => _entry(context, l, list, book!, i),
                ),
        );
      },
    );
  }

  Widget _entry(BuildContext context, AppLocalizations l, Playlist list, Book book, int i) {
    final e = list.entries[i];
    final cs = Theme.of(context).colorScheme;
    final index = e.available
        ? book.tracks.indexWhere((t) => t.partId == e.partId && t.number == e.position)
        : -1;
    final grey = TextStyle(color: cs.onSurface.withValues(alpha: 0.4));
    return ListTile(
      key: Key('entry-${e.entryId}'),
      dense: true,
      enabled: e.available,
      leading: ReorderableDragStartListener(index: i, child: const Icon(Icons.drag_handle)),
      title: Text(e.available ? e.title : l.playlistUnavailable, style: e.available ? null : grey),
      subtitle: e.available
          ? Text([e.bookTitle, e.bookAuthor].where((s) => s.isNotEmpty).join(' · '))
          : null,
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (e.available) Text(formatDuration(e.duration)),
          IconButton(
            key: Key('entry-remove-${e.entryId}'),
            tooltip: l.playlistRemoveEntry,
            icon: const Icon(Icons.close, size: 18),
            onPressed: () => _try(context, () => c.removeEntry(list.id, e.entryId)),
          ),
        ],
      ),
      onTap: e.available && index >= 0 ? () => _play(list, entryIndex: index) : null,
    );
  }
}
