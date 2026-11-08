// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'audio_backend.dart';
import 'just_audio_backend.dart';
import 'pipewire_backend.dart';

/// The sound engine, where there is one: PipeWire on Linux, ExoPlayer and AVPlayer on Android
/// and iOS, Media Foundation on Windows. Null elsewhere or if the library or PipeWire is missing; the app then runs silently
/// on demo data. The player opens one, the music of a slideshow another.
AudioBackend? openAudioBackend() {
  if (Platform.isAndroid || Platform.isIOS || Platform.isMacOS || Platform.isWindows) return JustAudioBackend();
  if (!Platform.isLinux) return null;
  try {
    return PipeWireBackend.open();
  } on AudioException catch (e) {
    debugPrint('No sound: ${e.message}');
    return null;
  }
}
