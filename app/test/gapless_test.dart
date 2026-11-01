// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/server/server_source.dart';

// Gapless playback (docs/spec-engine-rebuild.md, P2b): controller + FakeBackend with the gapless contract.

class _Source extends MediaSource {
  final progress = <String>[];

  @override
  Future<String> fetchPart(Track track) async => '/cache/${track.partId}.audio';

  @override
  Future<void> saveProgress(LibraryItem book, Track track, Duration position, {required bool finished}) async {
    progress.add('${track.partId}@${position.inSeconds}${finished ? ' finished' : ''}');
  }

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}

LibraryItem _book({String id = '1', bool files = false}) => LibraryItem(
  id: id,
  title: 'Album $id',
  author: 'Artist',
  color: const Color(0xFF000000),
  tracks: [
    for (var i = 0; i < 3; i++)
      Track(disc: 1, number: i + 1, title: 'T$i', length: const Duration(seconds: 120), partId: files ? null : 10 + i, path: files ? '/music/$i.mp3' : null),
  ],
);

void main() {
  late FakeBackend backend;
  late _Source source;
  late PlayerController p;
  var now = Duration.zero;

  void make({bool gapless = true, bool files = false}) {
    now = Duration.zero;
    backend = FakeBackend(clock: () => now, fileLength: const Duration(seconds: 120), gapless: gapless);
    source = _Source();
    p = PlayerController([_book(files: files), _book(id: '2', files: files)], backend: backend, source: files ? null : source);
  }

  // The periodic tick must be stopped before the test ends.
  void gaplessTest(String name, Future<void> Function(WidgetTester t) body) => testWidgets(name, (t) async {
    try {
      await body(t);
    } finally {
      p.dispose();
    }
  });

  Future<void> pump(WidgetTester t, Duration d) async {
    // In steps of the tick, so that the player sees every moment like the real clock shows it.
    for (var left = d; left > Duration.zero;) {
      final step = left > const Duration(milliseconds: 500) ? const Duration(milliseconds: 500) : left;
      now += step;
      await t.pump(step);
      left -= step;
    }
  }

  gaplessTest('the next track is queued before the end, not before the window, and not after the last track', (t) async {
    make();
    p.open(p.library.first, track: 0);
    await t.pump(const Duration(milliseconds: 10));
    await pump(t, const Duration(seconds: 10));
    expect(backend.queuedPath, isNull);
    await pump(t, const Duration(seconds: 70));
    expect(backend.queuedPath, '/cache/11.audio');
    p.open(p.library.first, track: 2);
    await pump(t, const Duration(seconds: 100));
    expect(backend.queuedPath, isNull, reason: 'the last track has no successor');
  });

  gaplessTest('a file on the device is queued without any fetch', (t) async {
    make(files: true);
    p.open(p.library.first, track: 0);
    await t.pump(const Duration(milliseconds: 10));
    await pump(t, const Duration(seconds: 80));
    expect(backend.queuedPath, '/music/1.mp3');
  });

  gaplessTest('a backend without gapless gets no queue', (t) async {
    make(gapless: false);
    p.open(p.library.first, track: 0);
    await t.pump(const Duration(milliseconds: 10));
    await pump(t, const Duration(seconds: 100));
    expect(backend.queuedPath, isNull);
  });

  gaplessTest('at the end the controller advances without a gap: never ended, never loading, progress reported once', (t) async {
    make();
    p.open(p.library.first, track: 0);
    await t.pump(const Duration(milliseconds: 10));
    for (var s = 0.0; s < 130; s += 0.5) {
      await pump(t, const Duration(milliseconds: 500));
      expect(backend.state, BackendState.playing, reason: 'at ${s}s');
      expect(p.playing, isTrue, reason: 'at ${s}s');
      expect(p.loading, isFalse, reason: 'at ${s}s');
    }
    expect(backend.endedSeen, 0);
    expect(backend.advances, 1);
    expect(p.trackIndex, 1);
    expect(backend.loadedPath, '/cache/11.audio');
    expect(p.position.inSeconds, inInclusiveRange(8, 11));
    expect(source.progress.where((e) => e == '10@120 finished'), hasLength(1));
    expect(source.progress.where((e) => e.startsWith('11') && e.contains('finished')), isEmpty);
    expect(p.isTrackPlayed(p.library.first, 0), isTrue);
  });

  gaplessTest('seeking back after an advance works and the queue forms again', (t) async {
    make();
    p.open(p.library.first, track: 0);
    await t.pump(const Duration(milliseconds: 10));
    await pump(t, const Duration(seconds: 125));
    expect(p.trackIndex, 1);
    p.seek(const Duration(seconds: 3));
    expect(backend.position, const Duration(seconds: 3));
    expect(p.trackIndex, 1);
    await pump(t, const Duration(seconds: 80));
    expect(backend.queuedPath, '/cache/12.audio');
    await pump(t, const Duration(seconds: 50));
    expect(p.trackIndex, 2);
    expect(backend.endedSeen, 0);
  });

  gaplessTest('skip within the track keeps the queue, skip across the end and next() replace it', (t) async {
    make();
    p.open(p.library.first, track: 0);
    await t.pump(const Duration(milliseconds: 10));
    await pump(t, const Duration(seconds: 80));
    expect(backend.queuedPath, isNotNull);
    p.skip(const Duration(seconds: 10));
    expect(backend.queuedPath, '/cache/11.audio');
    p.skip(const Duration(seconds: 60));
    await t.pump(const Duration(milliseconds: 10));
    expect(p.trackIndex, 1);
    expect(backend.queuedPath, isNull, reason: 'the old successor is dropped');
    await pump(t, const Duration(seconds: 50));
    expect(backend.queuedPath, '/cache/12.audio', reason: 'the new successor is queued');
    p.next();
    await t.pump(const Duration(milliseconds: 10));
    expect(p.trackIndex, 2);
    expect(backend.queuedPath, isNull);
  });

  gaplessTest('a change of the book clears the queue', (t) async {
    make();
    p.open(p.library.first, track: 0);
    await t.pump(const Duration(milliseconds: 10));
    await pump(t, const Duration(seconds: 80));
    expect(backend.queuedPath, isNotNull);
    p.open(p.library[1], track: 0);
    await t.pump(const Duration(milliseconds: 10));
    expect(backend.queuedPath, isNull);
    expect(p.trackIndex, 0);
  });
}
