// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:image/image.dart' as img;

import '../barcode.dart';
import '../l10n/app_localizations.dart';
import '../models.dart';
import '../player_controller.dart';
import '../profile.dart';
import '../server/api_client.dart';
import '../server/server_source.dart';
import 'avatar_picker.dart';

/// The longest side of a cover the app sends, in pixels: enough for the big picture of the player, small enough for the files it is embedded in.
const int kCoverSide = 1200;

/// Turns a photo into a cover: upright by the orientation of the camera, at most [kCoverSide] pixels on its longest side, JPEG (which also
/// drops the metadata of the camera: place, time). Not cropped: the person photographs the case, and a square cut could take off a title.
/// Null if the bytes are no picture. Runs in a separate isolate: a photo of a phone is large.
Future<Uint8List?> prepareCover(Uint8List source) => compute(_prepareCover, source);

Uint8List? _prepareCover(Uint8List source) {
  final decoded = img.decodeImage(source);
  if (decoded == null) return null;
  final upright = img.bakeOrientation(decoded);
  final longest = upright.width > upright.height ? upright.width : upright.height;
  final small = longest > kCoverSide
      ? img.copyResize(upright, width: upright.width >= upright.height ? kCoverSide : null, height: upright.height > upright.width ? kCoverSide : null, interpolation: img.Interpolation.average)
      : upright;
  return Uint8List.fromList(img.encodeJpg(small, quality: 85));
}

/// The button of an album page that changes its cover with a photo of the case or a picture of the device. Only for the owner of the album
/// and an administrator ([LibraryItem.canLabel]: the server gives the cover the same right as the labels), and only with the server.
class ChangeCoverButton extends StatelessWidget {
  const ChangeCoverButton({super.key, required this.book, this.api, this.picker, this.prepare = prepareCover});

  final LibraryItem book;

  /// The server; by default the one the player is connected to.
  final CoverApi? api;
  final AvatarPicker? picker;
  final Future<Uint8List?> Function(Uint8List) prepare;

  CoverApi? _apiOf(PlayerController p) {
    final source = p.source;
    return source is ServerSource ? source.api : null;
  }

  @override
  Widget build(BuildContext context) {
    final p = PlayerScope.of(context);
    final server = api ?? _apiOf(p);
    final id = int.tryParse(book.id);
    if (!book.canLabel || server == null || id == null) return const SizedBox.shrink();
    return IconButton(
      key: const ValueKey('change-cover'),
      tooltip: AppLocalizations.of(context).coverChange,
      icon: const Icon(Icons.add_a_photo_outlined),
      onPressed: () => _change(context, p, server, id),
    );
  }

  Future<void> _change(BuildContext context, PlayerController p, CoverApi server, int id) async {
    final l = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    final chooser = picker ?? const PlatformAvatarPicker.cover();
    final source = chooser.hasCamera
        ? await showModalBottomSheet<AvatarSource>(
            context: context,
            showDragHandle: true,
            builder: (sheet) => SafeArea(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  ListTile(
                    key: const ValueKey('cover-camera'),
                    leading: const Icon(Icons.photo_camera_outlined),
                    title: Text(l.coverCamera),
                    onTap: () => Navigator.pop(sheet, AvatarSource.camera),
                  ),
                  ListTile(
                    key: const ValueKey('cover-gallery'),
                    leading: const Icon(Icons.photo_library_outlined),
                    title: Text(l.coverGallery),
                    onTap: () => Navigator.pop(sheet, AvatarSource.gallery),
                  ),
                ],
              ),
            ),
          )
        : AvatarSource.gallery;
    if (source == null) return;
    void say(String text) => messenger.showSnackBar(SnackBar(content: Text(text)));
    try {
      final raw = await chooser.pick(source);
      if (raw == null) return;
      final jpeg = await prepare(raw);
      if (jpeg == null) return say(l.coverNoPicture);
      await server.putCover(id, jpeg);
      p.patchItem(book.id, (b) => b.copyWith(cover: jpeg));
      say(l.coverChanged);
    } on AvatarPickerException catch (e) {
      say(e.code == 'denied' ? l.coverDenied : l.coverFailed(e.code));
    } on ApiException catch (e) {
      say(l.coverFailed(e.code));
    }
  }
}
