// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart' show AudioErrorKind;
import 'package:psalm/audio/radio_connection_machine.dart';

const _p = RadioPolicy(reconnectDelay: Duration(seconds: 2), reconnectAttempts: 3, healthyBytes: 100);
const _open = [OpenConnection()];
const _sleep = [Sleep(Duration(seconds: 2))];

void main() {
  group('transitions', () {
    final rows = <(String, RadioState, RadioEvent, Type, List<RadioEffect>)>[
      ('Idle Start', const RadioIdle(), const RadioStart(), RadioConnecting, _open),
      ('Idle Close', const RadioIdle(), const RadioClose(), RadioClosed, const [CloseConnection()]),
      ('Connecting Connected', const RadioConnecting(1), const RadioConnected(), RadioStreaming, const [ReportDetails()]),
      ('Connecting PeerClosed', const RadioConnecting(0), const RadioPeerClosed(), RadioBackoff, _sleep),
      ('Connecting Timeout', const RadioConnecting(0), const RadioTimeout(), RadioBackoff, _sleep),
      ('Connecting retryable Error', const RadioConnecting(0), const RadioError('boom'), RadioBackoff, _sleep),
      ('Connecting fatal Error', const RadioConnecting(0), const RadioError('HTTP 404', kind: AudioErrorKind.streamServer), RadioFatal, const [ReportFail('HTTP 404', AudioErrorKind.streamServer)]),
      ('Connecting Close', const RadioConnecting(0), const RadioClose(), RadioClosed, const [CloseConnection()]),
      ('Streaming Bytes', const RadioStreaming(0, 5), const RadioBytes(7), RadioStreaming, const []),
      ('Streaming PeerClosed', const RadioStreaming(0, 5), const RadioPeerClosed(), RadioBackoff, _sleep),
      ('Streaming Timeout', const RadioStreaming(0, 5), const RadioTimeout(), RadioBackoff, _sleep),
      ('Streaming Close', const RadioStreaming(0, 5), const RadioClose(), RadioClosed, const [CloseConnection()]),
      ('Backoff elapsed', const RadioBackoff(2), const RadioBackoffElapsed(), RadioConnecting, _open),
      ('Backoff Close', const RadioBackoff(2), const RadioClose(), RadioClosed, const [CloseConnection()]),
      ('Fatal Close', const RadioFatal(AudioErrorKind.generic, 'x'), const RadioClose(), RadioClosed, const [CloseConnection()]),
    ];
    for (final (name, from, event, to, effects) in rows) {
      test(name, () {
        final r = transition(_p, from, event);
        expect(r.$1.runtimeType, to);
        expect(r.$2, effects);
      });
    }
  });

  test('Bytes add up', () {
    final r = transition(_p, const RadioStreaming(1, 5), const RadioBytes(7)).$1 as RadioStreaming;
    expect((r.failures, r.delivered), (1, 12));
  });

  test('failures count up and carry into the next connection', () {
    var s = transition(_p, const RadioConnecting(0), const RadioPeerClosed()).$1;
    expect((s as RadioBackoff).attempt, 1);
    s = transition(_p, s, const RadioBackoffElapsed()).$1;
    expect((s as RadioConnecting).failures, 1);
  });

  test('more failures than attempts is fatal (streamConnection) with the reason', () {
    final r = transition(_p, const RadioConnecting(3), const RadioError('socket error'));
    expect(r.$1, isA<RadioFatal>());
    expect((r.$1 as RadioFatal).kind, AudioErrorKind.streamConnection);
    expect(r.$2, [const ReportFail('connection lost: socket error', AudioErrorKind.streamConnection)]);
  });

  test('exactly the allowed attempts still back off', () {
    expect(transition(_p, const RadioConnecting(2), const RadioPeerClosed()).$1, isA<RadioBackoff>());
  });

  test('healthy bytes reset the count', () {
    final r = transition(_p, const RadioStreaming(3, 100), const RadioPeerClosed());
    expect((r.$1 as RadioBackoff).attempt, 1);
    final bad = transition(_p, const RadioStreaming(3, 99), const RadioPeerClosed());
    expect(bad.$1, isA<RadioFatal>());
  });

  group('illegal events are explicit no-ops', () {
    const all = <RadioEvent>[RadioStart(), RadioConnected(), RadioBytes(1), RadioPeerClosed(), RadioError('x'), RadioTimeout(), RadioBackoffElapsed()];
    final states = <String, (RadioState, Set<Type>)>{
      'Idle': (const RadioIdle(), {RadioStart}),
      'Connecting': (const RadioConnecting(0), {RadioConnected, RadioPeerClosed, RadioError, RadioTimeout}),
      'Streaming': (const RadioStreaming(0, 0), {RadioBytes, RadioPeerClosed, RadioError, RadioTimeout}),
      'Backoff': (const RadioBackoff(1), {RadioBackoffElapsed}),
      'Fatal': (const RadioFatal(AudioErrorKind.generic, 'x'), <Type>{}),
      'Closed': (const RadioClosed(), <Type>{}),
    };
    states.forEach((name, v) {
      test(name, () {
        for (final e in all.where((e) => !v.$2.contains(e.runtimeType))) {
          final r = transition(_p, v.$1, e);
          expect(r.$1, same(v.$1), reason: '${e.runtimeType}');
          expect(r.$2, isEmpty);
        }
      });
    });
    test('Closed ignores Close too', () {
      const s = RadioClosed();
      final r = transition(_p, s, const RadioClose());
      expect(r.$1, same(s));
      expect(r.$2, isEmpty);
    });
  });
}
