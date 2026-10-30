// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';

/// Asks before something is taken away: a title, a sentence, Cancel and the button of the action (the look of every such question in the app).
/// True only when the action was chosen.
Future<bool> confirmRemoval(BuildContext context, {required String title, required String text, required String action}) async {
  final l = AppLocalizations.of(context);
  final yes = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(title),
      content: Text(text),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.cancel)),
        FilledButton(key: const ValueKey('confirm-removal'), onPressed: () => Navigator.pop(context, true), child: Text(action)),
      ],
    ),
  );
  return yes == true;
}
