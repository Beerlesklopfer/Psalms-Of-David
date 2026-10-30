// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../library_hiding.dart';

/// The rubric "Hidden" of the library: what the person hid (books and authors), each with "show again".
/// It is offered while their switch "show what I hid" is on.
class HiddenBody extends StatelessWidget {
  const HiddenBody({super.key, required this.controller});

  final HidingController controller;

  @override
  Widget build(BuildContext context) {
    return SliverToBoxAdapter(
      child: ListenableBuilder(
        listenable: controller,
        builder: (context, _) {
          final l = AppLocalizations.of(context);
          final text = Theme.of(context).textTheme;
          final cs = Theme.of(context).colorScheme;
          final s = controller.state;
          Future<void> run(Future<bool> Function() change) async {
            final messenger = ScaffoldMessenger.maybeOf(context);
            final failed = l.hidingFailed;
            if (!await change()) {
              messenger?.showSnackBar(SnackBar(content: Text(failed)));
            }
          }

          String authorLine(HiddenAuthor a) => switch (a.kind) {
            'music' => l.settingsHiddenAuthorMusic(a.name),
            'audiobook' => l.settingsHiddenAuthorAudiobooks(a.name),
            _ => l.settingsHiddenAuthorAll(a.name),
          };

          return Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (s.isEmpty)
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 24, 16, 16),
                  child: Text(
                    l.settingsHiddenNone,
                    key: const Key('hidden-none'),
                    style: text.bodyMedium?.copyWith(
                      color: cs.onSurfaceVariant,
                    ),
                  ),
                ),
              for (final a in s.authors)
                ListTile(
                  key: Key('hidden-author-${a.id}-${a.kind}'),
                  leading: const Icon(Icons.person_off_outlined),
                  title: Text(authorLine(a)),
                  trailing: TextButton(
                    onPressed: () =>
                        run(() => controller.hideAuthor(a.name, a.kind, false)),
                    child: Text(l.settingsShowAgain),
                  ),
                ),
              for (final b in s.books)
                ListTile(
                  key: Key('hidden-book-${b.id}'),
                  leading: const Icon(Icons.visibility_off_outlined),
                  title: Text(b.title),
                  subtitle: Text(b.author),
                  trailing: TextButton(
                    onPressed: () =>
                        run(() => controller.hideBook('${b.id}', false)),
                    child: Text(l.settingsShowAgain),
                  ),
                ),
            ],
          );
        },
      ),
    );
  }
}
