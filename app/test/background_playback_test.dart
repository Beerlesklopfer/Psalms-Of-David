// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:ui';

import 'package:audio_service/audio_service.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/audio/psalm_audio_handler.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/server/server_source.dart';

class _Source extends MediaSource {
  final fetched = <int>[];
  final progress = <String>[];

  @override
  Future<String> fetchPart(Track track) async {
    fetched.add(track.partId!);
    return '/cache/${track.partId}.audio';
  }

  @override
  Future<void> saveProgress(Book book, Track track, Duration position, {required bool finished}) async {
    progress.add('${track.partId}@${position.inSeconds}${finished ? ' finished' : ''}');
  }

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}

Book _book() => Book(
  id: '1',
  title: 'Album',
  author: 'Artist',
  color: const Color(0xFF000000),
  tracks: [
    for (var i = 0; i < 3; i++) Track(disc: 1, number: i + 1, title: 'T$i', length: const Duration(seconds: 120), partId: 10 + i),
  ],
);

void main() {
  late FakeBackend backend;
  late _Source source;
  late PlayerController p;
  late PsalmAudioHandler handler;
  var now = Duration.zero;

  setUp(() {
    now = Duration.zero;
    backend = FakeBackend(clock: () => now, fileLength: const Duration(seconds: 120));
    source = _Source();
    p = PlayerController([_book()], backend: backend, source: source);
    handler = PsalmAudioHandler()..attach(p);
  });
  tearDown(() {
    handler.detach(p);
    p.dispose();
  });

  testWidgets('the system pausing the engine (a call, unplugged headphones) pauses the controller and tells the server', (tester) async {
    p.open(p.library.first, track: 0);
    await tester.pump(const Duration(milliseconds: 100));
    expect(p.playing, isTrue);
    now = const Duration(seconds: 7);
    backend.pauseFromOutside();
    expect(p.playing, isFalse);
    expect(p.position, const Duration(seconds: 7));
    expect(source.progress.last, '10@7');
    backend.playFromOutside();
    expect(p.playing, isTrue);
    p.toggle();
  });

  testWidgets('the notification shows the track, the album and the artist, and follows play and pause', (tester) async {
    p.open(p.library.first, track: 1);
    await tester.pump(const Duration(milliseconds: 100));
    final item = handler.mediaItem.value!;
    expect((item.title, item.album, item.artist, item.duration), ('T1', 'Album', 'Artist', const Duration(seconds: 120)));
    expect(handler.playbackState.value.playing, isTrue);
    expect(handler.playbackState.value.controls.map((c) => c.action), contains(MediaAction.pause));
    p.toggle();
    expect(handler.playbackState.value.playing, isFalse);
    expect(handler.playbackState.value.controls.map((c) => c.action), contains(MediaAction.play));
  });

  testWidgets('the buttons of the notification and the headset go through the controller', (tester) async {
    p.open(p.library.first, track: 0);
    await tester.pump(const Duration(milliseconds: 100));
    await handler.pause();
    expect(p.playing, isFalse);
    await handler.play();
    expect(p.playing, isTrue);
    await handler.skipToNext();
    expect(p.trackIndex, 1);
    await handler.seek(const Duration(seconds: 30));
    expect(p.position, const Duration(seconds: 30));
    await handler.fastForward();
    expect(p.position, const Duration(seconds: 60));
    await handler.skipToPrevious();
    expect(p.trackIndex, 1, reason: 'after the first seconds it goes to the start of the track');
    expect(p.position, Duration.zero);
    p.toggle();
  });

  testWidgets('the next track is fetched before this one ends', (tester) async {
    p.open(p.library.first, track: 0);
    await tester.pump(const Duration(milliseconds: 100));
    source.fetched.clear();
    now = const Duration(seconds: 60);
    await tester.pump(const Duration(seconds: 1));
    expect(source.fetched, isEmpty, reason: 'far from the end nothing is fetched');
    now = const Duration(seconds: 100);
    await tester.pump(const Duration(seconds: 1));
    expect(source.fetched, [11]);
    await tester.pump(const Duration(seconds: 1));
    expect(source.fetched, [11], reason: 'once');
    p.toggle();
  });

  test('the extension point for the car answers nothing yet', () async {
    expect(await handler.getChildren(AudioService.browsableRootId), isEmpty);
    expect(await handler.getMediaItem('1:0'), isNull);
  });
}
