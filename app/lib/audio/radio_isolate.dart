// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:collection';
import 'dart:isolate';
import 'dart:typed_data';

import 'audio_backend.dart';
import 'radio_stream.dart';

/// Runs the network side of internet radio ([RadioStreamSession]: HTTP connection,
/// reconnects, ICY filter) in a background isolate, so a busy UI isolate cannot starve
/// the engine of bytes.
///
/// The native engine handle stays in the UI isolate: it is freed on dispose and
/// `psalm_engine_stream_open` replaces the queue on a new load, so a second isolate
/// pushing into it directly could touch freed memory or write stale data. The
/// background isolate therefore only sends the bytes over; this class pushes them into
/// the real [StreamSink] on the UI isolate.
///
/// Back pressure: the isolate may have at most [kRadioInFlightLimit] bytes that the UI
/// side has not yet handed to the engine. Beyond that its sink answers 0 and the
/// session waits [kStreamPushRetry], as it does for a full engine queue.

/// Bytes the isolate may send before the UI isolate has acknowledged them.
const int kRadioInFlightLimit = 256 * 1024;

/// How long [IsolatedRadioSession.close] waits for the isolate to end.
const Duration kRadioCloseWait = Duration(seconds: 1);

// Message tags, isolate to UI.
const String _tagPort = 'port';
const String _tagData = 'data';
const String _tagTitle = 'title';
const String _tagDetails = 'details';
const String _tagFail = 'fail';

class IsolatedRadioSession {
  IsolatedRadioSession({
    required this.url,
    required this.sink,
    this.onTitle,
    this.onDetails,
    this.reconnectDelay = kStreamReconnectDelay,
    this.reconnectAttempts = kStreamReconnectAttempts,
    this.connectTimeout = kStreamConnectTimeout,
    this.idleTimeout = kStreamIdleTimeout,
  });

  final Uri url;

  /// The real sink; only ever called on the isolate that owns this object.
  final StreamSink sink;
  final void Function(String? title)? onTitle;
  final void Function(StreamDetails details)? onDetails;
  final Duration reconnectDelay;
  final int reconnectAttempts;
  final Duration connectTimeout;
  final Duration idleTimeout;

  final ReceivePort _inbox = ReceivePort();
  final ReceivePort _errors = ReceivePort();
  final Queue<Uint8List> _pending = Queue<Uint8List>();
  final Completer<void> _done = Completer<void>();
  Isolate? _isolate;
  SendPort? _toIsolate;
  Timer? _retry;
  int _offset = 0; // bytes of _pending.first already taken by the engine
  bool _started = false;
  bool _closed = false;
  bool _exited = false;
  bool _failReceived = false;
  (String, AudioErrorKind)? _heldFail;

  /// Completes when the isolate has ended or the session was closed.
  Future<void> get done => _done.future;

  void start() {
    if (_started) return;
    _started = true;
    _inbox.listen(_onMessage);
    _errors.listen((_) {}); // the exit message tells the rest
    _spawn();
  }

  Future<void> _spawn() async {
    try {
      final isolate = await Isolate.spawn<_Args>(
        _radioIsolateMain,
        _Args(
          _inbox.sendPort,
          url.toString(),
          reconnectDelay.inMilliseconds,
          reconnectAttempts,
          connectTimeout.inMilliseconds,
          idleTimeout.inMilliseconds,
        ),
        onExit: _inbox
            .sendPort, // arrives as a null message, in order with the data
        onError: _errors.sendPort,
        errorsAreFatal: true,
      );
      if (_closed) {
        isolate.kill(priority: Isolate.immediate);
      } else {
        _isolate = isolate;
      }
    } on Object {
      _exited = true;
      _failOrHold('connection lost', AudioErrorKind.streamConnection);
      if (!_done.isCompleted) _done.complete();
    }
  }

  void _onMessage(dynamic message) {
    if (message == null) {
      _exited = true;
      if (_closed) {
        _teardown();
        return;
      }
      if (!_failReceived) {
        _failOrHold('connection lost', AudioErrorKind.streamConnection);
      }
      if (!_done.isCompleted) _done.complete();
      return;
    }
    if (_closed) return; // late messages after close are ignored
    final list = message as List<Object?>;
    switch (list[0]) {
      case _tagPort:
        _toIsolate = list[1] as SendPort;
      case _tagData:
        final bytes = (list[1] as TransferableTypedData)
            .materialize()
            .asUint8List();
        if (bytes.isNotEmpty) _pending.add(bytes);
        _pump();
      case _tagTitle:
        onTitle?.call(list[1] as String?);
      case _tagDetails:
        onDetails?.call(_detailsFrom(list[1] as Map<Object?, Object?>));
      case _tagFail:
        _failReceived = true;
        _failOrHold(list[1] as String, AudioErrorKind.values[list[2] as int]);
    }
  }

  /// A failure is reported after the bytes that came before it, as with the direct
  /// session where the engine had taken everything by then.
  void _failOrHold(String message, AudioErrorKind kind) {
    _heldFail = (message, kind);
    _pump();
  }

  void _pump() {
    _retry?.cancel();
    _retry = null;
    while (!_closed && _pending.isNotEmpty) {
      final head = _pending.first;
      final n = sink.push(
        _offset == 0 ? head : Uint8List.sublistView(head, _offset),
      );
      if (n < 0) {
        // The engine closed the stream: stop everything.
        close();
        return;
      }
      if (n == 0) {
        _retry = Timer(kStreamPushRetry, _pump);
        return;
      }
      _toIsolate?.send(n);
      _offset += n;
      if (_offset >= head.length) {
        _pending.removeFirst();
        _offset = 0;
      }
    }
    final fail = _heldFail;
    if (!_closed && fail != null && _pending.isEmpty) {
      _heldFail = null;
      sink.fail(fail.$1, fail.$2);
    }
  }

  /// Stops at once: nothing is pushed to the sink afterwards, the isolate is killed.
  Future<void> close() {
    if (_closed) return _done.future;
    _closed = true;
    _retry?.cancel();
    _retry = null;
    _pending.clear();
    _heldFail = null;
    _toIsolate?.send('close');
    _isolate?.kill(priority: Isolate.immediate);
    _isolate = null;
    if (_exited || !_started) {
      _teardown();
    } else {
      // Keep the port until the exit message (or a short wait) so nothing leaks.
      Timer(kRadioCloseWait, _teardown);
    }
    return _done.future;
  }

  void _teardown() {
    _inbox.close();
    _errors.close();
    if (!_done.isCompleted) _done.complete();
  }
}

StreamDetails _detailsFrom(Map<Object?, Object?> m) => StreamDetails(
  name: m['name'] as String?,
  genre: m['genre'] as String?,
  description: m['description'] as String?,
  website: m['website'] as String?,
  bitrateKbps: m['bitrateKbps'] as int?,
  format: m['format'] as String?,
);

Map<String, Object?> _detailsToMap(StreamDetails d) => {
  'name': d.name,
  'genre': d.genre,
  'description': d.description,
  'website': d.website,
  'bitrateKbps': d.bitrateKbps,
  'format': d.format,
};

class _Args {
  const _Args(
    this.toUi,
    this.url,
    this.reconnectMs,
    this.attempts,
    this.connectMs,
    this.idleMs,
  );
  final SendPort toUi;
  final String url;
  final int reconnectMs;
  final int attempts;
  final int connectMs;
  final int idleMs;
}

/// The sink inside the isolate: sends the bytes over and refuses more while too many
/// are not yet acknowledged.
class _IsolateSink implements StreamSink {
  _IsolateSink(this._toUi);
  final SendPort _toUi;
  int inFlight = 0;

  @override
  int push(Uint8List data) {
    if (inFlight >= kRadioInFlightLimit) return 0;
    inFlight += data.length;
    _toUi.send([
      _tagData,
      TransferableTypedData.fromList([data]),
    ]);
    return data.length;
  }

  @override
  void fail(String message, AudioErrorKind kind) =>
      _toUi.send([_tagFail, message, kind.index]);
}

Future<void> _radioIsolateMain(_Args args) async {
  final commands = ReceivePort();
  final sink = _IsolateSink(args.toUi);
  final session = RadioStreamSession(
    url: Uri.parse(args.url),
    sink: sink,
    onTitle: (t) => args.toUi.send([_tagTitle, t]),
    onDetails: (d) => args.toUi.send([_tagDetails, _detailsToMap(d)]),
    reconnectDelay: Duration(milliseconds: args.reconnectMs),
    reconnectAttempts: args.attempts,
    connectTimeout: Duration(milliseconds: args.connectMs),
    idleTimeout: Duration(milliseconds: args.idleMs),
  );
  commands.listen((m) {
    if (m is int) {
      sink.inFlight -= m;
    } else if (m == 'close') {
      session.close();
    }
  });
  args.toUi.send([_tagPort, commands.sendPort]);
  session.start();
  await session.done;
  commands.close(); // nothing left to wait for: the isolate ends
}
