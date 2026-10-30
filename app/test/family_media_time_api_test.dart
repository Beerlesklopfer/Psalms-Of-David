// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/server/api_client.dart';

/// A stand-in for David that answers only the exact path of the route, the way the real server does (a "?" in the path would not match).
Future<(HttpServer, ApiClient, List<Uri>)> _server() async {
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  final seen = <Uri>[];
  server.listen((req) {
    seen.add(req.uri);
    final ok = req.uri.path == '/api/v1/family/media-time';
    req.response
      ..statusCode = ok ? 200 : 404
      ..headers.contentType = ContentType.json
      ..write(jsonEncode(ok
          ? {
              'from_day': 20743,
              'days': 14,
              'members': [
                {'user_id': 1, 'name': 'Beerchen', 'me': true, 'has_avatar': false, 'kinds': {'music': List.filled(14, 0)}, 'total': List.filled(14, 0)},
              ],
            }
          : {'error': 'not_found'}));
    req.response.close();
  });
  return (server, ApiClient(address: 'localhost:${server.port}'), seen);
}

void main() {
  test('the media time is asked for with the query in the query, not percent-encoded into the path', () async {
    final (server, api, seen) = await _server();
    addTearDown(() => server.close(force: true));
    final time = await api.familyMediaTime(days: 7);
    expect((time['members'] as List).length, 1);
    expect(seen.single.path, '/api/v1/family/media-time');
    expect(seen.single.queryParameters, {'days': '7'});
  });
}
