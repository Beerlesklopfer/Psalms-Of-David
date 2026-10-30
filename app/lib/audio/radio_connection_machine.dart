// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'audio_backend.dart' show AudioErrorKind;

// The life cycle of one radio connection as a PURE function: `transition(policy, state, event) -> (state, effects)`.
// No sockets, no timers: the shell (`RadioStreamSession._run`) carries out the [RadioEffect]s and feeds the results back
// as [RadioEvent]s. The document is docs/spec-state-machines.md.

/// The numbers the machine decides with (set by the session; the defaults are in radio_stream.dart).
class RadioPolicy {
  const RadioPolicy({required this.reconnectDelay, required this.reconnectAttempts, required this.healthyBytes});

  /// Waiting time before a lost connection is opened again.
  final Duration reconnectDelay;

  /// How often in a row a connection is opened again before the stream fails.
  final int reconnectAttempts;

  /// A connection that delivered at least this many bytes was healthy: the count of failures starts again.
  final int healthyBytes;
}

// ---- States -----------------------------------------------------------------------------------------

sealed class RadioState {
  const RadioState();
}

/// Not started.
class RadioIdle extends RadioState {
  const RadioIdle();
}

/// A connection is being opened; [failures] in a row so far.
class RadioConnecting extends RadioState {
  const RadioConnecting(this.failures);
  final int failures;
}

/// The headers were fine and bytes flow; [delivered] of this connection.
class RadioStreaming extends RadioState {
  const RadioStreaming(this.failures, this.delivered);
  final int failures;
  final int delivered;
}

/// Waiting before the next try; [attempt] is the number of failures in a row.
class RadioBackoff extends RadioState {
  const RadioBackoff(this.attempt);
  final int attempt;
}

/// Over for good, reported to the sink.
class RadioFatal extends RadioState {
  const RadioFatal(this.kind, this.message);
  final AudioErrorKind kind;
  final String message;
}

/// Closed by the user (or the sink); nothing happens any more.
class RadioClosed extends RadioState {
  const RadioClosed();
}

// ---- Events -----------------------------------------------------------------------------------------

sealed class RadioEvent {
  const RadioEvent();
}

class RadioStart extends RadioEvent {
  const RadioStart();
}

/// The response headers were accepted (status, content type). The shell holds the headers; the machine only orders [ReportDetails].
class RadioConnected extends RadioEvent {
  const RadioConnected();
}

/// [n] bytes were taken by the sink.
class RadioBytes extends RadioEvent {
  const RadioBytes(this.n);
  final int n;
}

/// The station ended the response.
class RadioPeerClosed extends RadioEvent {
  const RadioPeerClosed();
}

/// An error. [kind] null: worth another try, with [message] as the reason. [kind] set: final, [message] is the text for the sink.
class RadioError extends RadioEvent {
  const RadioError(this.message, {this.kind});
  final String message;
  final AudioErrorKind? kind;
}

/// Connect or idle timeout: a retryable error with the reason 'timeout'.
class RadioTimeout extends RadioEvent {
  const RadioTimeout();
}

class RadioBackoffElapsed extends RadioEvent {
  const RadioBackoffElapsed();
}

class RadioClose extends RadioEvent {
  const RadioClose();
}

// ---- Effects ----------------------------------------------------------------------------------------

sealed class RadioEffect {
  const RadioEffect();
}

/// Connect, read the headers and pump (counts a connection).
class OpenConnection extends RadioEffect {
  const OpenConnection();
  @override
  bool operator ==(Object other) => other is OpenConnection;
  @override
  int get hashCode => 1;
}

/// Wait [delay], then feed [RadioBackoffElapsed].
class Sleep extends RadioEffect {
  const Sleep(this.delay);
  final Duration delay;
  @override
  bool operator ==(Object other) => other is Sleep && other.delay == delay;
  @override
  int get hashCode => delay.hashCode;
}

/// Tell the listener what the headers say about the station.
class ReportDetails extends RadioEffect {
  const ReportDetails();
  @override
  bool operator ==(Object other) => other is ReportDetails;
  @override
  int get hashCode => 2;
}

/// Tell the sink that the stream failed.
class ReportFail extends RadioEffect {
  const ReportFail(this.message, this.kind);
  final String message;
  final AudioErrorKind kind;
  @override
  bool operator ==(Object other) => other is ReportFail && other.message == message && other.kind == kind;
  @override
  int get hashCode => Object.hash(message, kind);
}

/// Shut the HTTP client down at once.
class CloseConnection extends RadioEffect {
  const CloseConnection();
  @override
  bool operator ==(Object other) => other is CloseConnection;
  @override
  int get hashCode => 3;
}

typedef RadioResult = (RadioState state, List<RadioEffect> effects);

/// An illegal event: the same state object and no effects.
RadioResult _ignore(RadioState s) => (s, const []);

RadioResult transition(RadioPolicy policy, RadioState state, RadioEvent event) {
  if (state is RadioClosed) return _ignore(state);
  if (event is RadioClose) return (const RadioClosed(), const [CloseConnection()]);
  switch (state) {
    case RadioIdle():
      return event is RadioStart ? (const RadioConnecting(0), const [OpenConnection()]) : _ignore(state);
    case RadioConnecting():
      return switch (event) {
        RadioConnected() => (RadioStreaming(state.failures, 0), const [ReportDetails()]),
        RadioPeerClosed() => _lost(policy, state.failures, 0, 'the station closed the connection'),
        RadioTimeout() => _lost(policy, state.failures, 0, 'timeout'),
        RadioError() => _error(policy, state.failures, 0, event),
        _ => _ignore(state),
      };
    case RadioStreaming():
      return switch (event) {
        RadioBytes(:final n) => (RadioStreaming(state.failures, state.delivered + n), const []),
        RadioPeerClosed() => _lost(policy, state.failures, state.delivered, 'the station closed the connection'),
        RadioTimeout() => _lost(policy, state.failures, state.delivered, 'timeout'),
        RadioError() => _error(policy, state.failures, state.delivered, event),
        _ => _ignore(state),
      };
    case RadioBackoff():
      return event is RadioBackoffElapsed ? (RadioConnecting(state.attempt), const [OpenConnection()]) : _ignore(state);
    case RadioFatal():
      return _ignore(state);
    case RadioClosed():
      return _ignore(state);
  }
}

RadioResult _error(RadioPolicy p, int failures, int delivered, RadioError e) {
  final kind = e.kind;
  if (kind != null) return (RadioFatal(kind, e.message), [ReportFail(e.message, kind)]);
  return _lost(p, failures, delivered, e.message);
}

/// A connection ended in a way that may be tried again.
RadioResult _lost(RadioPolicy p, int failures, int delivered, String reason) {
  var n = delivered >= p.healthyBytes ? 0 : failures;
  n++;
  if (n > p.reconnectAttempts) {
    final message = 'connection lost: $reason';
    return (RadioFatal(AudioErrorKind.streamConnection, message), [ReportFail(message, AudioErrorKind.streamConnection)]);
  }
  return (RadioBackoff(n), [Sleep(p.reconnectDelay)]);
}
