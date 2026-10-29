// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/discovery.dart';
import 'package:psalm/server/server_source.dart';

/// The client against a real server (`cargo run --example serve_demo` in
/// server/), over TLS with the pinned certificate and with mDNS. Set
/// PSALM_E2E_PAIRING to the file that the example writes and PSALM_E2E_AUDIO to the
/// audio file it serves.
void main() {
  final pairingFile = Platform.environment['PSALM_E2E_PAIRING'];
  final audio = Platform.environment['PSALM_E2E_AUDIO'];
  final skip = (pairingFile == null || audio == null) ? 'needs the demo server (see the comment)' : null;

  test('pair, list, find a passage, download, report progress, find the server by mDNS', skip: skip, () async {
    final pairing = PairingAddress.parse(File(pairingFile!).readAsStringSync())!;
    expect(pairing.fingerprint, isNotNull, reason: 'the server speaks TLS');

    // A server that shows another certificate than the pinned one is refused.
    final wrong = PairingAddress(
      serverId: pairing.serverId,
      address: pairing.address,
      fingerprint: 'ab' * 32,
      code: pairing.code,
    );
    await expectLater(
      ApiClient.pair(wrong, 'Test'),
      throwsA(isA<ApiException>().having((e) => e.code, 'code', 'certificate')),
    );

    final (api, result) = await ApiClient.pair(pairing, 'Test laptop');
    addTearDown(api.close);
    expect(result.userName, 'Anna');
    expect(result.role, 'member');

    // The code works once.
    await expectLater(
      ApiClient.pair(pairing, 'Second'),
      throwsA(isA<ApiException>().having((e) => e.code, 'code', 'enrollment_invalid')),
    );

    final library = await loadLibrary(api);
    expect(library.books.single.title, 'Bible');
    final track = library.books.single.tracks.single;
    expect(track.partId, isNotNull);

    final passage = await ServerSource(api, Directory.systemTemp).resolvePassage('Johannes 3,16');
    expect(passage!.position, const Duration(seconds: 12));
    expect(passage.allowed, isTrue);
    expect(
      await ServerSource(api, Directory.systemTemp).resolvePassage('Hobbit 1,1'),
      isNull,
      reason: 'an unknown book is no match, not a crash',
    );

    final cache = Directory.systemTemp.createTempSync('psalm-e2e');
    addTearDown(() => cache.deleteSync(recursive: true));
    final path = await ServerSource(api, cache).fetchPart(track);
    expect(File(path).lengthSync(), File(audio!).lengthSync());
    expect(File(path).readAsBytesSync(), File(audio).readAsBytesSync());

    await ServerSource(
      api,
      cache,
    ).saveProgress(library.books.single, track, const Duration(seconds: 21), finished: false);
    await Future<void>.delayed(const Duration(seconds: 2)); // the server writes within a short window
    final again = await loadLibrary(api);
    expect(again.resume[library.books.single.id]!.position, const Duration(seconds: 21));

    await api.logout();
    await expectLater(api.me(), throwsA(isA<ApiException>().having((e) => e.status, 'status', 401)));

    final found = await findServer(pairing.serverId);
    expect(found, isNotNull, reason: 'the server announces itself');
    expect(pairing.address, contains(found!.host));
  });
}
