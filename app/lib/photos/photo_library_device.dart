// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:photo_manager/photo_manager.dart';

import 'photo_sync.dart';

/// The permission the backup asks for: pictures and videos, with their location data. Without `ACCESS_MEDIA_LOCATION` Android hands out
/// the files with the location taken out, and the backup would not be the original.
const _permission = PermissionRequestOption(androidPermission: AndroidPermission(type: RequestType.common, mediaLocation: true));

/// How many entries of the gallery are asked for at once.
const _page = 500;

/// The pictures of an Android device, from `MediaStore` (through `photo_manager`).
class DevicePhotoLibrary extends PhotoLibrary {
  /// [background]: in the job of WorkManager there is no window for the dialog of the permission; it is only checked there.
  DevicePhotoLibrary({this.background = false});

  final bool background;

  @override
  Future<bool> hasAccess() async {
    if (background) await PhotoManager.setIgnorePermissionCheck(true);
    final state = await PhotoManager.getPermissionState(requestOption: _permission);
    return state.hasAccess;
  }

  @override
  Future<bool> requestAccess() async => (await PhotoManager.requestPermissionExtend(requestOption: _permission)).hasAccess;

  @override
  Future<List<LocalPhoto>> list({DateTime? since, required bool videos, required String sources}) async {
    final filter = FilterOptionGroup(
      createTimeCond: DateTimeCond(min: since ?? DateTime.fromMillisecondsSinceEpoch(0), max: DateTime.now().add(const Duration(days: 1))),
      orders: const [OrderOption(type: OrderOptionType.createDate, asc: true)],
    );
    final type = videos ? RequestType.common : RequestType.image;
    final paths = await PhotoManager.getAssetPathList(onlyAll: true, type: type, filterOption: filter);
    if (paths.isEmpty) return const [];
    final all = paths.first;
    final out = <LocalPhoto>[];
    for (var page = 0;; page++) {
      final assets = await all.getAssetListPaged(page: page, size: _page);
      for (final a in assets) {
        if (a.type != AssetType.image && a.type != AssetType.video) continue;
        if (sources == 'camera' && !_fromCamera(a.relativePath)) continue;
        final mime = a.mimeType ?? await a.mimeTypeAsync ?? (a.type == AssetType.video ? 'video/mp4' : 'image/jpeg');
        out.add(LocalPhoto(
          id: a.id,
          name: a.title ?? await a.titleAsync,
          mime: mime,
          takenAt: a.createDateTime,
          modified: a.modifiedDateTime,
          video: a.type == AssetType.video,
          open: () => a.originFile,
        ));
      }
      if (assets.length < _page) break;
    }
    return out;
  }

  /// The camera folder (`DCIM/...`), without the screenshots that some devices keep there too. An older Android (before 10) gives no
  /// folder: then the picture counts.
  static bool _fromCamera(String? relativePath) {
    if (relativePath == null || relativePath.isEmpty) return true;
    final p = relativePath.toLowerCase();
    return p.startsWith('dcim/') && !p.contains('screenshot');
  }

  @override
  Future<void> cleanUp() => PhotoManager.clearFileCache();
}
