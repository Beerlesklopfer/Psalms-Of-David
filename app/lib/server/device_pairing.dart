// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';

import 'api_client.dart';

/// How many devices the own profile has and may have (`GET /api/v1/devices/pairing`); [max] is null without a limit.
class PairingUsage {
  const PairingUsage({required this.used, this.max, required this.full});

  final int used;
  final int? max;
  final bool full;

  factory PairingUsage.fromJson(Map<String, dynamic> j) =>
      PairingUsage(used: j['used'] as int, max: j['max'] as int?, full: j['full'] as bool? ?? false);
}

/// A one-time code for a further device (`POST /api/v1/devices/pairing`).
class PairingCode {
  const PairingCode({required this.code, required this.serverId, required this.expiresIn, required this.used, this.max});

  final String code;
  final String serverId;

  /// How long the code works, counted from the answer.
  final Duration expiresIn;
  final int used;
  final int? max;

  factory PairingCode.fromJson(Map<String, dynamic> j) => PairingCode(
    code: j['code'] as String,
    serverId: j['server_id'] as String? ?? '',
    expiresIn: Duration(seconds: j['expires_in_s'] as int? ?? 0),
    used: j['used'] as int? ?? 0,
    max: j['max'] as int?,
  );
}

/// What the app needs from the server to connect a further device. Only a parent may: a child or a guest gets `403 forbidden`.
abstract class DevicePairingApi {
  Future<PairingUsage> usage();

  /// Throws [ApiException] with `device_limit_reached` (409) when the profile has as many devices as it may.
  Future<PairingCode> makeCode();
}

class ServerDevicePairingApi implements DevicePairingApi {
  ServerDevicePairingApi(this.api);

  final ApiClient api;

  Future<Map<String, dynamic>> _call(String method) async {
    final res = await api.sendRaw(method, '/api/v1/devices/pairing');
    final text = await utf8.decodeStream(res);
    if (res.statusCode >= 400) {
      var code = 'error';
      try {
        code = (jsonDecode(text) as Map<String, dynamic>)['error'] as String? ?? code;
      } on FormatException {
        // not JSON: the generic code
      }
      throw ApiException(res.statusCode, code);
    }
    return jsonDecode(text) as Map<String, dynamic>;
  }

  @override
  Future<PairingUsage> usage() async => PairingUsage.fromJson(await _call('GET'));

  @override
  Future<PairingCode> makeCode() async => PairingCode.fromJson(await _call('POST'));
}
