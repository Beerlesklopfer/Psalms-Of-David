// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:share_plus/share_plus.dart';

import '../l10n/app_localizations.dart';
import '../models.dart';

/// The address (`host:port`) of the connected server for the pages below; null in the demo.
class ServerAddressScope extends InheritedWidget {
  const ServerAddressScope({super.key, required this.address, required super.child});

  final String? address;

  static String? of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<ServerAddressScope>()?.address;

  @override
  bool updateShouldNotify(ServerAddressScope old) => old.address != address;
}

/// Passes a medium on through the share sheet of the system (title, creator and the address of the server). Shown only if the server allows it ([Book.canShare]: an administrator or a member).
class ShareButton extends StatelessWidget {
  const ShareButton({super.key, required this.book, this.address, this.share = _share});

  final Book book;

  /// `host:port` of the server (default: the connected one); without any the text carries no link.
  final String? address;

  /// Opens the share sheet with the text; replaced in tests.
  final Future<void> Function(String text, String subject) share;

  static Future<void> _share(String text, String subject) => SharePlus.instance.share(ShareParams(text: text, subject: subject));

  @override
  Widget build(BuildContext context) {
    if (!book.canShare) return const SizedBox.shrink();
    final l = AppLocalizations.of(context);
    return IconButton(
      key: const ValueKey('share-book'),
      tooltip: l.shareTooltip,
      icon: const Icon(Icons.share_outlined),
      onPressed: () {
        final line = l.shareText(book.title, book.author);
        final server = address ?? ServerAddressScope.of(context);
        share(server == null ? line : '$line\nhttps://$server/', book.title);
      },
    );
  }
}
