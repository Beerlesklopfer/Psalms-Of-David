// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:connectivity_plus/connectivity_plus.dart';
import 'package:disk_space_plus/disk_space_plus.dart';

/// What the gate of a bulk download needs to know about the device: how much room is left, and whether the connection costs money.
/// An abstraction so that the tests (and a system that cannot tell) need no plugin.
abstract class DownloadEnvironment {
  /// The bytes still free on the volume that holds [path], or null when the system does not say.
  Future<int?> freeBytes(String path);

  /// True on an unmetered connection (Wi-Fi, a cable), false on mobile data or without a connection, null when it cannot be told.
  Future<bool?> unmetered();
}

/// The answer of the plugins `disk_space_plus` and `connectivity_plus`; a failure of a plugin is "not known", never an error.
class PluginDownloadEnvironment implements DownloadEnvironment {
  const PluginDownloadEnvironment();

  @override
  Future<int?> freeBytes(String path) async {
    try {
      final megabytes = await DiskSpacePlus().getFreeDiskSpaceForPath(path);
      return megabytes == null ? null : (megabytes * 1024 * 1024).floor();
    } on Object {
      return null;
    }
  }

  @override
  Future<bool?> unmetered() async {
    try {
      final now = await Connectivity().checkConnectivity();
      if (now.contains(ConnectivityResult.wifi) || now.contains(ConnectivityResult.ethernet)) return true;
      if (now.contains(ConnectivityResult.mobile) || now.contains(ConnectivityResult.none)) return false;
      return null;
    } on Object {
      return null;
    }
  }
}
