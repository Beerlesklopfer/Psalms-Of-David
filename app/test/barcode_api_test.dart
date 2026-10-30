// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/server/api_client.dart';

/// A stand-in for David that answers only the exact route path `/api/v1/barcode/{digits}`.
Future<(HttpServer, ApiClient, List<Uri>)> barcodeServer() async {
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  final seen = <Uri>[];
  server.listen((req) {
    seen.add(req.uri);
    final m = RegExp(r'^/api/v1/barcode/([0-9]+)$').firstMatch(req.uri.path);
    Object body;
    var status = 200;
    if (m == null) {
      status = 404;
      body = {'error': 'not_found'};
    } else if (m[1] == '1234567890123') {
      status = 422;
      body = {'error': 'invalid_barcode'};
    } else if (m[1] == '4006381333931') {
      body = {
        'code': m[1],
        'gtin': '0${m[1]}',
        'found': true,
        'items': [
          {'item_id': 7, 'kind': 'music', 'title': 'Album', 'creator': 'Band', 'has_cover': false},
        ],
      };
    } else if (m[1] == '500') {
      status = 500;
      body = {'error': 'boom'};
    } else {
      body = {'code': m[1], 'gtin': '0${m[1]}', 'found': false, 'items': []};
    }
    req.response
      ..statusCode = status
      ..headers.contentType = ContentType.json
      ..write(jsonEncode(body));
    req.response.close();
  });
  return (server, ApiClient(address: 'localhost:${server.port}'), seen);
}

void main() {
  test('a barcode is asked for on the exact route and the answer is read', () async {
    final (server, api, seen) = await barcodeServer();
    addTearDown(() => server.close(force: true));
    final a = await api.barcodeLookup('4006381333931');
    expect(a.found, isTrue);
    expect(a.items.single.title, 'Album');
    expect(seen.single.path, '/api/v1/barcode/4006381333931');
    expect(seen.single.hasQuery, isFalse);
  });

  test('an unknown code is found: false', () async {
    final (server, api, _) = await barcodeServer();
    addTearDown(() => server.close(force: true));
    final a = await api.barcodeLookup('96385074');
    expect(a.found, isFalse);
    expect(a.invalid, isFalse);
  });

  test('invalid_barcode is an answer, other errors throw', () async {
    final (server, api, _) = await barcodeServer();
    addTearDown(() => server.close(force: true));
    expect((await api.barcodeLookup('1234567890123')).invalid, isTrue);
    expect(() => api.barcodeLookup('500'), throwsA(isA<ApiException>()));
  });
}
