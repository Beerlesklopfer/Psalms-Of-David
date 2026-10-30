// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// What a voice assistant asked for (Siri through the App Intents of the iOS app).
enum VoiceAction { continueListening, play, next, previous, showNew }

/// Raised by the voice handler when the start page is to be shown; the shell listens to it.
final ValueNotifier<int> showStartRequests = ValueNotifier<int>(0);

class VoiceCommand {
  const VoiceCommand(this.action, {this.query = ''});

  final VoiceAction action;

  /// What to play (only for [VoiceAction.play]).
  final String query;

  /// Reads the JSON that the native side wrote: `{"action":"continue"|"play"|"next"|"previous"|"new","query":"..."}`.
  /// Null for anything that is not a known command (broken JSON, unknown action, play without a query).
  static VoiceCommand? parse(String? json) {
    if (json == null || json.trim().isEmpty) return null;
    final Object? data;
    try {
      data = jsonDecode(json);
    } on FormatException {
      return null;
    }
    if (data is! Map) return null;
    final query = data['query'] is String
        ? (data['query'] as String).trim()
        : '';
    switch (data['action']) {
      case 'continue':
        return const VoiceCommand(VoiceAction.continueListening);
      case 'play':
        return query.isEmpty
            ? null
            : VoiceCommand(VoiceAction.play, query: query);
      case 'next':
        return const VoiceCommand(VoiceAction.next);
      case 'previous':
        return const VoiceCommand(VoiceAction.previous);
      case 'new':
        return const VoiceCommand(VoiceAction.showNew);
    }
    return null;
  }
}

/// The channel to the native side, which keeps the command of the last intent until the app takes it.
class VoiceChannel {
  VoiceChannel({MethodChannel? channel, bool? supported})
    : _channel =
          channel ?? const MethodChannel('com.the-abraham-box.psalm/voice'),
      _supported = supported ?? Platform.isIOS;

  final MethodChannel _channel;
  final bool _supported;

  /// The waiting command, which is cleared by this call; null if none, off iOS, or if the native side is missing.
  Future<VoiceCommand?> takePending() async {
    if (!_supported) return null;
    try {
      return VoiceCommand.parse(
        await _channel.invokeMethod<String>('takePending'),
      );
    } on MissingPluginException {
      return null;
    } on PlatformException {
      return null;
    }
  }
}
