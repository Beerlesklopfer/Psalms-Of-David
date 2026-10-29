// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/pipewire_backend.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/discovery.dart';
import 'package:psalm/server/server_source.dart';

/// The app against a running `david` that holds the real library: pairs with the
/// address of PSALM_REAL_PAIRING, compares the first part byte for byte with the
/// local file PSALM_REAL_FIRST_FILE, and plays the book through PipeWire into the
/// sink PSALM_TEST_SINK (library PSALM_AUDIO_LIB).
void main() {
  final pairingFile = Platform.environment['PSALM_REAL_PAIRING'];
  final firstFile = Platform.environment['PSALM_REAL_FIRST_FILE'];
  final lib = Platform.environment['PSALM_AUDIO_LIB'];
  final sink = Platform.environment['PSALM_TEST_SINK'];
  final skip = (pairingFile == null || firstFile == null || lib == null || sink == null)
      ? 'needs a running server with the real library (see the comment)'
      : null;

  test('the real library: pair, list, download, play, find by mDNS', skip: skip, () async {
    final pairing = PairingAddress.parse(File(pairingFile!).readAsStringSync())!;
    final (api, result) = await ApiClient.pair(pairing, 'Psalm test');
    addTearDown(api.close);
    stdout.writeln('paired as ${result.userName} (${result.role})');

    final library = await loadLibrary(api);
    final book = library.books.single;
    stdout.writeln(
      '${book.author}: ${book.title}, ${book.tracks.length} parts, ${book.total.inMinutes} min, discs ${book.discs}',
    );
    expect(book.title, 'Kinder der Freiheit');
    expect(book.author, 'Follett');
    expect(book.tracks.first.disc, 1);
    expect(book.tracks.first.number, 1);

    final cache = Directory.systemTemp.createTempSync('psalm-real');
    addTearDown(() => cache.deleteSync(recursive: true));
    final source = ServerSource(api, cache);
    final path = await source.fetchPart(book.tracks.first);
    expect(File(path).readAsBytesSync(), File(firstFile!).readAsBytesSync(), reason: 'the stream is the file');

    final backend = PipeWireBackend.open(target: sink, libraryPath: lib);
    final player = PlayerController(library.books, backend: backend, source: source);
    addTearDown(player.dispose);
    player.attachServer(source, library);
    player.open(book, track: 0);
    await Future<void>.delayed(const Duration(seconds: 8));
    expect(player.error, isNull);
    expect(player.loading, isFalse);
    expect(player.position, greaterThan(const Duration(seconds: 5)));
    stdout.writeln('playing: position ${player.position}');

    player.seek(const Duration(seconds: 90));
    await Future<void>.delayed(const Duration(seconds: 3));
    expect(player.position, greaterThan(const Duration(seconds: 90)));
    player.toggle(); // pause: the position goes to the server
    await Future<void>.delayed(const Duration(seconds: 3));
    final again = await loadLibrary(api);
    final resume = again.resume[book.id]!;
    stdout.writeln('server holds: track ${resume.track}, ${resume.position}');
    expect(resume.track, 0);
    expect(resume.position, greaterThan(const Duration(seconds: 90)));

    final found = await findServer(pairing.serverId);
    expect(found, isNotNull);
    stdout.writeln('mDNS: ${found!.name.isEmpty ? '' : '${found.name} '}${found.address}');
  });
}
