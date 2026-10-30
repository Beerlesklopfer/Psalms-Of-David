// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:device_info_plus/device_info_plus.dart';
import 'package:flutter/services.dart';

/// The channel of the Android activity (`MainActivity.kt`): it also answers the question for the name the person gave the device.
const _channel = MethodChannel('com.the-abraham-box.psalm/notifications');

/// The name this device proposes for itself when it is paired (the person may change it). The host name of a phone is `localhost`, which says
/// nothing, so the phones say what they are: the name the person gave the device (Android: the setting `device_name`), else "Google Pixel 8";
/// iOS 16 and later give an app no more than the model ("iPhone") unless Apple has granted the entitlement `user-assigned-device-name`, see
/// docs/releasing.md; a computer says its host name.
Future<String> defaultDeviceName({DeviceInfoPlugin? plugin}) async {
  try {
    final info = plugin ?? DeviceInfoPlugin();
    if (Platform.isAndroid) {
      // The name from the system settings first ("Jörgs Pixel"), the model when there is none.
      try {
        final own = (await _channel.invokeMethod<String>('deviceName'))?.trim();
        if (own != null && own.isNotEmpty) return own;
      } on Object {
        // No answer from the activity: the model.
      }
      final a = await info.androidInfo;
      return nameOfAndroid(a.manufacturer, a.model);
    }
    if (Platform.isIOS) {
      final i = await info.iosInfo;
      return nameOfIos(i.name, i.model);
    }
    if (Platform.isMacOS) {
      final m = await info.macOsInfo;
      if (m.computerName.trim().isNotEmpty) return m.computerName.trim();
    }
  } on Object {
    // The plugin says nothing: the host name does.
  }
  return Platform.localHostname;
}

/// "Google Pixel 8": the maker and the model, without saying the maker twice ("samsung SM-S901B" becomes "Samsung SM-S901B").
String nameOfAndroid(String manufacturer, String model) {
  final m = model.trim();
  final maker = manufacturer.trim();
  if (m.isEmpty) return maker.isEmpty ? 'Android' : _capital(maker);
  if (maker.isEmpty || m.toLowerCase().startsWith(maker.toLowerCase())) return _capital(m);
  return '${_capital(maker)} $m';
}

/// The name an iPhone gives: the person's own name for the device before iOS 16, the model ("iPhone", "iPad") after it; empty or `localhost`
/// falls back to the model.
String nameOfIos(String name, String model) {
  final n = name.trim();
  if (n.isNotEmpty && n.toLowerCase() != 'localhost') return n;
  final m = model.trim();
  return m.isEmpty ? 'iPhone' : m;
}

String _capital(String s) => s.isEmpty ? s : s[0].toUpperCase() + s.substring(1);
