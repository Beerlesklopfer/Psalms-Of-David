// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/services.dart';
import 'package:image_picker/image_picker.dart';

import '../profile.dart';

/// The camera and the photos of a phone (image_picker); on a computer, where there is no camera to offer, a file chooser (file_selector).
class PlatformAvatarPicker implements AvatarPicker {
  const PlatformAvatarPicker();

  static bool get _phone => Platform.isAndroid || Platform.isIOS;

  @override
  bool get hasCamera => _phone;

  @override
  Future<Uint8List?> pick(AvatarSource source) async {
    try {
      if (_phone) {
        final file = await ImagePicker().pickImage(
          source: source == AvatarSource.camera ? ImageSource.camera : ImageSource.gallery,
          maxWidth: 1024,
          maxHeight: 1024,
          imageQuality: 90,
          preferredCameraDevice: CameraDevice.front,
        );
        return await file?.readAsBytes();
      }
      if (source == AvatarSource.camera) throw const AvatarPickerException('unavailable');
      const types = XTypeGroup(label: 'images', extensions: ['jpg', 'jpeg', 'png', 'webp', 'gif', 'bmp']);
      final file = await openFile(acceptedTypeGroups: [types]);
      return await file?.readAsBytes();
    } on PlatformException catch (e) {
      // image_picker: camera_access_denied, photo_access_denied, no camera ...
      throw AvatarPickerException(e.code.contains('denied') ? 'denied' : 'unavailable');
    }
  }
}
