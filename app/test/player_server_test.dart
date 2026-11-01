// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/server/server_source.dart';

class FakeSource extends MediaSource {
  final fetched = <int>[];
  final progress = <String>[];
  final gates = <int, Completer<void>>{};
  PassageHit? passage;

  @override
  Future<String> fetchPart(Track track) async {
    fetched.add(track.partId!);
    await gates[track.partId!]?.future;
    return '/cache/${track.partId}.audio';
  }

  @override
  Future<void> saveProgress(LibraryItem book, Track track, Duration position, {required bool finished}) async {
    progress.add('${book.id}/${track.partId}@${position.inSeconds}${finished ? ' finished' : ''}');
  }

  @override
  Future<PassageHit?> resolvePassage(String text) async => passage;
}

LibraryItem _book(String id, int firstPart) => LibraryItem(
  id: id,
  title: 'Book $id',
  author: 'A',
  color: const Color(0xFF000000),
  tracks: [
    for (var i = 0; i < 3; i++)
      Track(disc: 1, number: i + 1, title: 'T$i', length: const Duration(seconds: 20), partId: firstPart + i),
  ],
);

void main() {
  late FakeBackend backend;
  late FakeSource source;
  late PlayerController p;

  setUp(() {
    backend = FakeBackend(fileLength: const Duration(seconds: 20));
    source = FakeSource();
    p = PlayerController([_book('1', 10), _book('2', 20)], backend: backend, source: source);
  });
  tearDown(() => p.dispose());

  test('a part is fetched from the server, then played from the resume position', () async {
    source.gates[11] = Completer<void>();
    p.open(p.library.first, track: 1);
    expect(p.loading, isTrue);
    expect(backend.state, BackendState.idle, reason: 'nothing plays while the file is on its way');
    source.gates[11]!.complete();
    await pumpEventQueue();
    expect(p.loading, isFalse);
    expect(backend.loadedPath, '/cache/11.audio');
    expect(backend.state, BackendState.playing);
  });

  test('a file that arrives after the user moved on is not played', () async {
    source.gates[10] = Completer<void>();
    p.open(p.library.first, track: 0);
    p.open(p.library.last, track: 0);
    await pumpEventQueue();
    expect(backend.loadedPath, '/cache/20.audio');
    source.gates[10]!.complete();
    await pumpEventQueue();
    expect(backend.loadedPath, '/cache/20.audio', reason: 'the late file must not take over');
  });

  test('pausing and switching report the position to the server', () async {
    p.open(p.library.first, track: 0);
    await pumpEventQueue();
    p.seek(const Duration(seconds: 7));
    p.toggle();
    expect(source.progress.last, '1/10@7');
    p.toggle();
    await pumpEventQueue();
    p.next();
    expect(source.progress.last.startsWith('1/10@'), isTrue, reason: 'the track that was left is reported');
  });

  testWidgets('the end of every track is reported as finished, not only the end of the list', (tester) async {
    var now = Duration.zero;
    final clocked = FakeBackend(clock: () => now, fileLength: const Duration(seconds: 20));
    final src = FakeSource();
    final q = PlayerController([_book('1', 10)], backend: clocked, source: src);
    q.open(q.library.first, track: 0);
    await tester.pump(const Duration(milliseconds: 100));
    now = const Duration(seconds: 21);
    await tester.pump(const Duration(seconds: 1));
    expect(src.progress.any((r) => r.startsWith('1/10@20') && r.endsWith('finished')), isTrue, reason: src.progress.join(', '));
    expect(q.trackIndex, 1, reason: 'the list goes on with the next track');
    q.dispose();
    await tester.pump();
  });

  test('a failing fetch pauses and shows the error', () async {
    final failing = FakeSource();
    final q = PlayerController([_book('1', 10)], backend: backend, source: _Failing(failing));
    q.open(q.library.first, track: 0);
    await pumpEventQueue();
    expect(q.playing, isFalse);
    expect(q.error, contains('network'));
    expect(q.loading, isFalse);
    q.dispose();
  });

  test('a passage opens its book at the position, unless it is not allowed now', () async {
    source.passage = PassageHit('2', 21, const Duration(seconds: 95), allowed: true);
    expect(await p.openPassage('Johannes 3,16'), isTrue);
    await pumpEventQueue();
    expect(p.book!.id, '2');
    expect(p.trackIndex, 1);
    expect(p.position, const Duration(seconds: 95));
    expect(backend.loadedPath, '/cache/21.audio');

    source.passage = PassageHit('1', 10, Duration.zero, allowed: false);
    expect(await p.openPassage('Johannes 3,16'), isFalse);
    expect(p.book!.id, '2', reason: 'a forbidden passage changes nothing');
    source.passage = null;
    expect(await p.openPassage('Hobbit 1,1'), isFalse);
  });

  test('attaching the server replaces the demo library and restores the resume point', () {
    final loaded = LoadedLibrary([_book('7', 70)], {'7': (track: 2, position: const Duration(seconds: 5))});
    p.attachServer(source, loaded);
    expect(p.library.single.id, '7');
    expect(p.book!.id, '7');
    expect(p.trackIndex, 2);
    expect(p.position, const Duration(seconds: 5));
    expect(p.playing, isFalse);
  });
}

class _Failing extends MediaSource {
  _Failing(this.inner);
  final FakeSource inner;
  @override
  Future<String> fetchPart(Track track) => Future.error(Exception('network'));
  @override
  Future<void> saveProgress(LibraryItem book, Track track, Duration position, {required bool finished}) async {}
  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}
