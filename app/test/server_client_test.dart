// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/credentials.dart';
import 'package:psalm/server/discovery.dart';

void main() {
  group('PairingAddress', () {
    test('reads the address that the server puts in the QR code', () {
      final p = PairingAddress.parse(
          'david://pair?v=1&id=abc&addr=192.168.24.127%3A8443&fp=e74d&code=0123456789abcdef');
      expect(p!.serverId, 'abc');
      expect(p.address, '192.168.24.127:8443');
      expect(p.fingerprint, 'e74d');
      expect(p.code, '0123456789abcdef');
    });

    test('a server without TLS has no fingerprint', () {
      expect(PairingAddress.parse('david://pair?v=1&id=a&addr=localhost%3A8484&code=c')!.fingerprint, isNull);
    });

    test('anything else is refused', () {
      for (final text in [
        '',
        'https://example.org/pair?code=x',
        'david://other?v=1&addr=a&code=c',
        'david://pair?v=2&addr=a&code=c',
        'david://pair?v=1&code=c',
        'david://pair?v=1&addr=a',
      ]) {
        expect(PairingAddress.parse(text), isNull, reason: text);
      }
    });

    test('surrounding spaces from pasting do not matter', () {
      expect(PairingAddress.parse('  david://pair?v=1&addr=a&code=c\n'), isNotNull);
    });
  });

  test('text records are read line by line', () {
    expect(parseTxt('v=1\nid=abc\nname=David Home\napi=/v1'), {'v': '1', 'id': 'abc', 'name': 'David Home', 'api': '/v1'});
    expect(parseTxt('novalue\n=x\nk=a=b'), {'k': 'a=b'});
  });

  test('FoundServer writes IPv6 addresses in brackets', () {
    expect(FoundServer(serverId: 'a', name: 'n', host: '192.168.1.2', port: 80).address, '192.168.1.2:80');
    expect(FoundServer(serverId: 'a', name: 'n', host: 'fe80::1', port: 80).address, '[fe80::1]:80');
  });

  group('CredentialStore', () {
    late Directory dir;
    setUp(() => dir = Directory.systemTemp.createTempSync('psalm-creds'));
    tearDown(() => dir.deleteSync(recursive: true));

    test('keeps what pairing gave, readable by the owner only', () {
      final store = CredentialStore(File('${dir.path}/sub/server.json'));
      expect(store.load(), isNull);
      store.save(Credentials(serverId: 'id', address: 'h:1', fingerprint: 'ff', token: 't', deviceName: 'Laptop'));
      final c = store.load()!;
      expect((c.serverId, c.address, c.fingerprint, c.token, c.deviceName), ('id', 'h:1', 'ff', 't', 'Laptop'));
      if (!Platform.isWindows) {
        expect(store.file.statSync().mode & 0x1ff, 384, reason: 'mode 600');
      }
      store.clear();
      expect(store.load(), isNull);
    });

    test('a damaged file is no pairing', () {
      final file = File('${dir.path}/server.json')..writeAsStringSync('{ not json');
      expect(CredentialStore(file).load(), isNull);
      file.writeAsStringSync('{"address": 5}');
      expect(CredentialStore(file).load(), isNull);
    });
  });
}
