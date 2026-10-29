// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'app_paths.dart';

/// What the device keeps after pairing: where the server is, which certificate it
/// must show, and the token. Stored in a file only the user can read.
class Credentials {
  Credentials({
    required this.serverId,
    required this.address,
    required this.fingerprint,
    required this.token,
    required this.deviceName,
    this.ca = false,
  });

  /// The id of the server, to find it again by mDNS when its address changes.
  final String serverId;
  final String address;
  final String? fingerprint;

  /// The server has a certificate from a certificate authority: its name is checked, there is no fingerprint.
  final bool ca;
  final String token;
  final String deviceName;

  Map<String, Object?> toJson() => {
    'server_id': serverId,
    'address': address,
    'fingerprint': fingerprint,
    'ca': ca,
    'token': token,
    'device_name': deviceName,
  };

  static Credentials? fromJson(Object? j) {
    if (j is! Map) return null;
    final address = j['address'];
    final token = j['token'];
    if (address is! String || token is! String) return null;
    return Credentials(
      serverId: j['server_id'] as String? ?? '',
      address: address,
      fingerprint: j['fingerprint'] as String?,
      ca: j['ca'] == true,
      token: token,
      deviceName: j['device_name'] as String? ?? '',
    );
  }
}

class CredentialStore {
  CredentialStore(this.file);

  /// The pairing file in the app's own folder (see [AppPaths]).
  factory CredentialStore.standard() => CredentialStore(AppPaths.credentialsFile);

  final File file;

  Credentials? load() {
    try {
      return Credentials.fromJson(jsonDecode(file.readAsStringSync()));
    } on FileSystemException {
      return null;
    } on FormatException {
      return null;
    }
  }

  void save(Credentials c) {
    file.parent.createSync(recursive: true);
    file.writeAsStringSync(jsonEncode(c.toJson()));
    if (!Platform.isWindows) Process.runSync('chmod', ['600', file.path]);
  }

  void clear() {
    try {
      file.deleteSync();
    } on FileSystemException {
      // already gone
    }
  }
}

/// Where downloaded parts are kept: the `parts` folder in the app's own folder (see [AppPaths]).
Directory partCache() => AppPaths.parts;
