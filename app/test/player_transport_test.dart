// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player/engine.dart';
import 'package:psalm/player/transport.dart';
import 'package:psalm/player_controller.dart';

Book _book() => Book(
      id: '7',
      title: 'Book',
      author: 'A',
      color: const Color(0xFF000000),
      tracks: [for (var i = 0; i < 3; i++) Track(disc: 1, number: i + 1, title: 'T${i + 1}', length: const Duration(minutes: 5), path: '/f$i.mp3', itemId: '7', partId: 70 + i)],
    );

void main() {
  test('every command and event survives toMap, JSON and fromMap', () {
    final b = BookData.of(_book());
    final commands = <PlayerCommand>[
      OpenCommand(b, trackIndex: 1, startMs: 5, forceReload: true),
      const ToggleCommand(),
      const PlayCommand(),
      const SeekCommand(10),
      const SelectTrackCommand(2, 3),
      const SkipCommand(-5),
      const NextCommand(),
      const PreviousCommand(),
      const SetSpeedCommand(1.5),
      SetPlaceCommand(book: b, trackIndex: 1, positionMs: 2),
      const SetPlaceCommand(clearBook: true),
      const HaltCommand(),
      const ClearHeardCommand(),
      const SetEnvCommand(offlineActive: true, writesLast: false),
      const FlushCommand(),
      const DisposeCommand(),
    ];
    for (final c in commands) {
      final back = PlayerCommand.fromMap((jsonDecode(jsonEncode(c.toMap())) as Map).cast<String, dynamic>());
      expect(back.runtimeType, c.runtimeType);
      expect(jsonEncode(back.toMap()), jsonEncode(c.toMap()));
    }
    final events = <PlayerEvent>[
      const PlayerSnapshot(
        seq: 3, bookId: '7', trackIndex: 1, positionMs: 9, playing: true, speed: 1.25, loading: false, error: 'x', errorKind: AudioErrorKind.streamServer,
        unplayableIndex: 2, streamTitle: 'A - B', streamDetails: StreamDetails(name: 'n', bitrateKbps: 128),
      ),
      const PositionTick(4, 1, 500),
      const TrackFinished('7', 0),
      const TrackHeard('7', 0),
      const ProgressQueued(bookId: '7', title: 'Book', positionMs: 1, finished: false, item: 7, part: 70),
      const ProgressQueued(bookId: 'ep:5', title: 'E', positionMs: 1, finished: true, episodeId: 5),
      const LastWritten('7', 1, 2, 3),
      const PlaylistPosition('p', 1, 2),
      const PodcastState(5, 1, false),
    ];
    for (final e in events) {
      final back = PlayerEvent.fromMap((jsonDecode(jsonEncode(e.toMap())) as Map).cast<String, dynamic>());
      expect(back.runtimeType, e.runtimeType);
      expect(jsonEncode(back.toMap()), jsonEncode(e.toMap()));
    }
  });

  test('the engine plays a book through a serializing transport: no live object crosses', () {
    final backend = FakeBackend();
    final events = <PlayerEvent>[];
    final t = InProcessTransport((emit) => PlayerEngine(backend: backend, emit: emit), roundTrip: true)..onEvent = events.add;
    t.send(OpenCommand(BookData.of(_book()), trackIndex: 1, startMs: 1000));
    final snap = events.whereType<PlayerSnapshot>().last;
    expect((snap.bookId, snap.trackIndex, snap.playing, snap.loading), ('7', 1, true, false));
    expect(backend.state, BackendState.playing);
    expect(backend.position.inMilliseconds, inInclusiveRange(1000, 1100));
    t.send(const NextCommand());
    expect(events.whereType<PlayerSnapshot>().last.trackIndex, 2);
    t.send(const ToggleCommand());
    expect(events.whereType<PlayerSnapshot>().last.playing, isFalse);
    expect(backend.state, BackendState.paused);
    // Snapshots grow in sequence.
    final seqs = events.whereType<PlayerSnapshot>().map((e) => e.seq).toList();
    expect([...seqs]..sort(), seqs);
    expect(seqs.toSet().length, seqs.length);
    t.send(const DisposeCommand());
  });

  test('the facade works over a serializing transport', () {
    final backend = FakeBackend();
    final p = PlayerController([_book()], backend: backend, transport: InProcessTransport((emit) => PlayerEngine(backend: backend, emit: emit), roundTrip: true));
    p.open(p.library.first, track: 1, start: const Duration(seconds: 3));
    expect((p.trackIndex, p.playing, p.position), (1, true, const Duration(seconds: 3)));
    p.next();
    expect(p.trackIndex, 2);
    p.seek(const Duration(seconds: 40));
    expect(p.position, const Duration(seconds: 40));
    p.setSpeed(1.5);
    expect(p.speed, 1.5);
    p.toggle();
    expect(p.playing, isFalse);
    p.dispose();
  });
}
