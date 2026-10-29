// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

/// The pictures of the radio stations (the `logo_url` the server keeps for a station), fetched once and kept while the app runs.
class StationLogos {
  StationLogos._();

  /// The largest picture taken in; a logo is small, anything bigger is not one.
  static const int maxBytes = 1024 * 1024;

  /// How long a fetch may take.
  static const Duration timeout = Duration(seconds: 6);

  static final Map<String, Future<Uint8List?>> _cache = {};

  /// The bytes of the picture at [url], or null if there is none, it is too big, is no picture or cannot be fetched.
  static Future<Uint8List?> load(String? url) {
    if (url == null || url.isEmpty) return Future.value(null);
    return _cache.putIfAbsent(url, () => _fetch(url));
  }

  /// Forgets what was fetched (for tests).
  static void clear() => _cache.clear();

  static Future<Uint8List?> _fetch(String url) async {
    final uri = Uri.tryParse(url);
    if (uri == null || !(uri.isScheme('http') || uri.isScheme('https'))) return null;
    final client = HttpClient()..connectionTimeout = timeout;
    try {
      final request = await client.getUrl(uri).timeout(timeout);
      final response = await request.close().timeout(timeout);
      final type = response.headers.contentType;
      if (response.statusCode != 200 || type == null || type.primaryType != 'image') {
        await response.drain<void>();
        return null;
      }
      final out = BytesBuilder(copy: false);
      await for (final chunk in response.timeout(timeout)) {
        out.add(chunk);
        if (out.length > maxBytes) return null;
      }
      return out.isEmpty ? null : out.takeBytes();
    } on Object {
      // A logo that cannot be fetched is no reason to bother the listener: the drawn cover stays.
      return null;
    } finally {
      client.close(force: true);
    }
  }
}
