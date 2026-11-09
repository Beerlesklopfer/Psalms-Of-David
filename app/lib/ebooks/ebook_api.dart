// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import '../server/api_client.dart';
import 'ebook.dart';

/// The e-books of the server (`server/src/api_ebooks.rs`).
class EbookApi {
  EbookApi(this.api);

  final ApiClient api;

  Future<Uint8List> _bytes(String path, {Map<String, Object>? query}) async {
    final res = await api.sendRaw('GET', path, query: query);
    final b = BytesBuilder(copy: false);
    await for (final chunk in res.timeout(const Duration(minutes: 2))) {
      b.add(chunk);
    }
    if (res.statusCode != 200) throw ApiException(res.statusCode, res.statusCode == 404 ? 'not_found' : 'error');
    return b.takeBytes();
  }

  Future<List<Ebook>> list() async {
    final j = jsonDecode(utf8.decode(await _bytes('/api/v1/ebooks')));
    final list = j is Map<String, dynamic> && j['ebooks'] is List ? j['ebooks'] as List : const [];
    return [for (final e in list) ?Ebook.fromJson(e)];
  }

  /// The text of an EPUB as JSON.
  Future<String> text(int id) async => utf8.decode(await _bytes('/api/v1/ebooks/$id/text'));

  Future<Uint8List> cover(int id) => _bytes('/api/v1/ebooks/$id/cover');

  /// A picture of the text of an EPUB.
  Future<Uint8List> picture(int id, String name) => _bytes('/api/v1/ebooks/$id/res', query: {'name': name});

  /// The file itself into [dest]; it gets its name only when it is complete.
  Future<void> file(int id, File dest, {void Function(int bytes)? progress}) async {
    final res = await api.sendRaw('GET', '/api/v1/ebooks/$id/file');
    if (res.statusCode != 200) {
      await res.drain<void>();
      throw ApiException(res.statusCode, 'error');
    }
    await dest.parent.create(recursive: true);
    final partial = partialFileFor(dest);
    final sink = partial.openWrite();
    var n = 0;
    try {
      await for (final chunk in res.timeout(const Duration(minutes: 5))) {
        sink.add(chunk);
        n += chunk.length;
        progress?.call(n);
      }
      await sink.close();
    } catch (_) {
      await sink.close().catchError((_) {});
      if (partial.existsSync()) partial.deleteSync();
      rethrow;
    }
    await partial.rename(dest.path);
  }

  Future<void> saveProgress(int id, EbookProgress p) async {
    final res = await api.sendRaw('PUT', '/api/v1/ebooks/$id/progress', body: p.toJson());
    await res.drain<void>();
    if (res.statusCode >= 400) throw ApiException(res.statusCode, 'error');
  }
}
