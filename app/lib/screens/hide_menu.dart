// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../library_hiding.dart';
import '../models.dart';

/// One thing the menu of a book can do about hiding.
class HideAction {
  const HideAction({required this.key, required this.label, required this.icon, required this.run});

  final String key;
  final String label;
  final IconData icon;
  final Future<bool> Function() run;
}

/// What can be done with [book] about hiding: the book, the author's books of its kind, everything by the author. Empty for a book that is
/// no item of the server (an episode, a station, a playlist) and where there is no server.
List<HideAction> hideActions(BuildContext context, LibraryItem book) {
  final c = HidingScope.maybeOf(context);
  final id = int.tryParse(book.id);
  if (c == null || !c.available || id == null || book.author.trim().isEmpty) return const [];
  final l = AppLocalizations.of(context);
  final name = book.author;
  final kind = book.kind == 'music' ? 'music' : 'audiobook';
  final itemHidden = book.hidden;
  final kindEntry = c.state.authorEntry(name, kind);
  final allEntry = c.state.authorEntry(name, '');
  return [
    HideAction(
      key: 'hide-book',
      label: itemHidden ? l.showBook : l.hideBook,
      icon: itemHidden ? Icons.visibility_outlined : Icons.visibility_off_outlined,
      run: () => c.hideBook(book.id, !itemHidden),
    ),
    HideAction(
      key: 'hide-kind',
      label: kind == 'music'
          ? (kindEntry ? l.showAuthorMusic(name) : l.hideAuthorMusic(name))
          : (kindEntry ? l.showAuthorAudiobooks(name) : l.hideAuthorAudiobooks(name)),
      icon: kindEntry ? Icons.visibility_outlined : Icons.person_off_outlined,
      run: () => c.hideAuthor(name, kind, !kindEntry),
    ),
    HideAction(
      key: 'hide-all',
      label: allEntry ? l.showAuthorAll(name) : l.hideAuthorAll(name),
      icon: allEntry ? Icons.visibility_outlined : Icons.person_off_outlined,
      run: () => c.hideAuthor(name, '', !allEntry),
    ),
  ];
}

Future<void> _run(BuildContext context, HideAction a) async {
  final messenger = ScaffoldMessenger.maybeOf(context);
  final failed = AppLocalizations.of(context).hidingFailed;
  final ok = await a.run();
  if (!ok) messenger?.showSnackBar(SnackBar(content: Text(failed)));
}

/// The menu in the bar of the book page: hide or show.
class HideMenuButton extends StatelessWidget {
  const HideMenuButton({super.key, required this.book});

  final LibraryItem book;

  @override
  Widget build(BuildContext context) {
    final actions = hideActions(context, book);
    if (actions.isEmpty) return const SizedBox.shrink();
    return PopupMenuButton<HideAction>(
      key: const Key('hide-menu'),
      tooltip: AppLocalizations.of(context).hideMenuTooltip,
      icon: Icon(book.hidden ? Icons.visibility_off_outlined : Icons.more_vert),
      onSelected: (a) => _run(context, a),
      itemBuilder: (_) => [
        for (final a in actions)
          PopupMenuItem(
            value: a,
            key: Key('hide-item-${a.key}'),
            child: ListTile(leading: Icon(a.icon), title: Text(a.label), dense: true),
          ),
      ],
    );
  }
}

/// The same choices as a sheet at the bottom (a long press on a book in a list).
Future<void> showHideSheet(BuildContext context, LibraryItem book) async {
  final actions = hideActions(context, book);
  if (actions.isEmpty) return;
  final choice = await showModalBottomSheet<HideAction>(
    context: context,
    builder: (context) => SafeArea(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final a in actions)
            ListTile(
              key: Key('hide-sheet-${a.key}'),
              leading: Icon(a.icon),
              title: Text(a.label),
              onTap: () => Navigator.pop(context, a),
            ),
        ],
      ),
    ),
  );
  if (choice != null && context.mounted) await _run(context, choice);
}
