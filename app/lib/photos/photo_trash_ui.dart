// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:intl/intl.dart';

import '../l10n/app_localizations.dart';
import 'photo_backup_api.dart';
import 'photo_backup_controller.dart';
import 'photo_texts.dart';

/// The trash of the server: what a device of the person deleted, with the day it goes for good, and "restore".
class PhotoTrashScreen extends StatefulWidget {
  const PhotoTrashScreen({super.key, required this.controller});

  final PhotoBackupController controller;

  @override
  State<PhotoTrashScreen> createState() => _PhotoTrashScreenState();
}

class _PhotoTrashScreenState extends State<PhotoTrashScreen> {
  @override
  void initState() {
    super.initState();
    widget.controller.loadTrash();
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final locale = Localizations.localeOf(context).toLanguageTag();
    return Scaffold(
      appBar: AppBar(title: Text(l.photoTrashTitle)),
      body: ListenableBuilder(
        listenable: widget.controller,
        builder: (context, _) {
          final c = widget.controller;
          final items = c.trash;
          if (items == null) {
            return c.trashError == null ? const Center(child: CircularProgressIndicator()) : Center(child: Text(photoErrorText(l, c.trashError!)));
          }
          return ListView(
            padding: const EdgeInsets.all(16),
            children: [
              Text(l.photoTrashHint, style: Theme.of(context).textTheme.bodySmall),
              const SizedBox(height: 12),
              if (items.isEmpty) Text(l.photoTrashEmpty, key: const ValueKey('photo-trash-empty')),
              for (final t in items)
                ListTile(
                  key: ValueKey('trashed-${t.id}'),
                  contentPadding: EdgeInsets.zero,
                  leading: SizedBox(width: 56, height: 56, child: _Thumb(key: ValueKey(t.id), controller: c, photo: t)),
                  title: Text(t.name, overflow: TextOverflow.ellipsis),
                  subtitle: Text(l.photoTrashUntil(DateFormat.yMd(locale).format(t.purgeAt))),
                  trailing: TextButton(key: ValueKey('restore-${t.id}'), onPressed: () => c.restore(t), child: Text(l.photoRestore)),
                ),
              if (c.trashError case final e?) ...[
                const SizedBox(height: 12),
                Text(photoErrorText(l, e), style: TextStyle(color: Theme.of(context).colorScheme.error)),
              ],
            ],
          );
        },
      ),
    );
  }
}

class _Thumb extends StatefulWidget {
  const _Thumb({super.key, required this.controller, required this.photo});

  final PhotoBackupController controller;
  final TrashedPhoto photo;

  @override
  State<_Thumb> createState() => _ThumbState();
}

class _ThumbState extends State<_Thumb> {
  // Asked once per picture, not at every rebuild of the list.
  late final Future<List<int>?> _bytes = widget.controller.api.thumb(widget.photo.id).catchError((Object _) => null);

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final placeholder = ColoredBox(
      color: cs.surfaceContainerHighest,
      child: Icon(widget.photo.video ? Icons.movie_outlined : Icons.image_outlined, color: cs.onSurfaceVariant),
    );
    return FutureBuilder<List<int>?>(
      future: _bytes,
      builder: (context, snap) => switch (snap.data) {
        final b? => Image.memory(Uint8List.fromList(b), fit: BoxFit.cover, errorBuilder: (_, _, _) => placeholder),
        null => placeholder,
      },
    );
  }
}
