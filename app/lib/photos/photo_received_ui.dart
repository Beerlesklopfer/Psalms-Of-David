// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import 'package:share_plus/share_plus.dart';

import '../l10n/app_localizations.dart';
import 'photo_backup_controller.dart';
import 'photo_device.dart';

/// The pictures this device received from the other devices of the person (bisync), newest first, as previews; a tap loads the original.
class ReceivedPhotosScreen extends StatelessWidget {
  const ReceivedPhotosScreen({super.key, required this.controller});

  final PhotoBackupController controller;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l.photoReceivedTitle)),
      body: ListenableBuilder(listenable: controller, builder: (context, _) => _grid(l)),
    );
  }

  Widget _grid(AppLocalizations l) {
    final items = controller.state.received.reversed.toList();
    return items.isEmpty
          ? Center(child: Padding(padding: const EdgeInsets.all(24), child: Text(l.photoReceivedNone, textAlign: TextAlign.center)))
          : GridView.builder(
              padding: const EdgeInsets.all(4),
              gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: 140, mainAxisSpacing: 4, crossAxisSpacing: 4),
              itemCount: items.length,
              itemBuilder: (context, i) => _Tile(key: ValueKey(items[i].id), controller: controller, photo: items[i]),
            );
  }
}

class _Tile extends StatelessWidget {
  const _Tile({super.key, required this.controller, required this.photo});

  final PhotoBackupController controller;
  final ReceivedPhoto photo;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final thumb = controller.store.thumbFile(photo.id);
    return InkWell(
      key: ValueKey('received-${photo.id}'),
      onTap: () => _open(context),
      child: Stack(
        fit: StackFit.expand,
        children: [
          if (photo.thumb && thumb.existsSync())
            Image.file(thumb, fit: BoxFit.cover, errorBuilder: (_, _, _) => _placeholder(cs))
          else
            _placeholder(cs),
          if (photo.video) const Align(alignment: Alignment.center, child: Icon(Icons.play_circle_outline, size: 36, color: Colors.white)),
        ],
      ),
    );
  }

  Widget _placeholder(ColorScheme cs) => ColoredBox(
        color: cs.surfaceContainerHighest,
        child: Icon(photo.video ? Icons.movie_outlined : Icons.image_outlined, color: cs.onSurfaceVariant),
      );

  Future<void> _open(BuildContext context) async {
    final l = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    final navigator = Navigator.of(context);
    final File file;
    try {
      file = await controller.original(photo);
    } on Object {
      messenger.showSnackBar(SnackBar(content: Text(l.photoErrNetwork)));
      return;
    }
    if (photo.video) {
      // The app plays no videos of its own: the person chooses the player.
      await SharePlus.instance.share(ShareParams(files: [XFile(file.path, mimeType: photo.mime)]));
      return;
    }
    await navigator.push(MaterialPageRoute<void>(builder: (_) => _Original(controller: controller, photo: photo, file: file)));
  }
}

class _Original extends StatelessWidget {
  const _Original({required this.controller, required this.photo, required this.file});

  final PhotoBackupController controller;
  final ReceivedPhoto photo;
  final File file;

  @override
  Widget build(BuildContext context) {
    final locale = Localizations.localeOf(context).toLanguageTag();
    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(
        backgroundColor: Colors.black,
        foregroundColor: Colors.white,
        title: Text(photo.name, overflow: TextOverflow.ellipsis),
        actions: [
          if (controller.settings?.trashOn ?? false)
            IconButton(
              key: const ValueKey('received-trash'),
              icon: const Icon(Icons.delete_outline),
              tooltip: AppLocalizations.of(context).photoTrashThis,
              onPressed: () => _trash(context),
            ),
        ],
        bottom: PreferredSize(
          preferredSize: const Size.fromHeight(20),
          child: Text(DateFormat.yMd(locale).add_Hm().format(photo.takenAt), style: const TextStyle(color: Colors.white70)),
        ),
      ),
      body: InteractiveViewer(maxScale: 6, child: Center(child: Image.file(file))),
    );
  }

  Future<void> _trash(BuildContext context) async {
    final l = AppLocalizations.of(context);
    final navigator = Navigator.of(context);
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.photoTrashThis),
        content: Text(l.photoTrashThisBody),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(MaterialLocalizations.of(context).cancelButtonLabel)),
          FilledButton(key: const ValueKey('received-trash-confirm'), onPressed: () => Navigator.pop(context, true), child: Text(l.photoTrashThis)),
        ],
      ),
    );
    if (yes == true && await controller.trashReceived(photo)) navigator.pop();
  }
}
