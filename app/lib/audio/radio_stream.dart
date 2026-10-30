// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'audio_backend.dart';
import 'radio_connection_machine.dart';

/// Network part of internet radio on Linux. `dart:io` makes the connection (TLS,
/// proxies, certificates), [IcyFilter] strips the Shoutcast/Icecast metadata, and
/// the plain MP3 bytes go to a [StreamSink] (the native engine, in the app).

/// Waiting time before a lost connection is opened again.
const Duration kStreamReconnectDelay = Duration(seconds: 2);

/// How often in a row a connection is opened again before the stream fails. A
/// healthy connection (see [kStreamHealthyBytes]) resets the count.
const int kStreamReconnectAttempts = 3;

/// A connection that delivered at least this many bytes was healthy: the count of
/// reconnect attempts starts again after it. Without such a limit a server that sends
/// a few bytes of junk and hangs up would be retried for ever.
const int kStreamHealthyBytes = 16 * 1024;

/// Longest wait for the connection and the response headers.
const Duration kStreamConnectTimeout = Duration(seconds: 10);

/// If no bytes arrive for this long the connection counts as lost.
const Duration kStreamIdleTimeout = Duration(seconds: 10);

/// Pause before offering bytes again to a full engine queue.
const Duration kStreamPushRetry = Duration(milliseconds: 20);

/// Sent as `User-Agent`; some stream servers refuse requests without one.
const String kStreamUserAgent = 'Psalm';

/// Content types of streams the engine cannot decode (it plays MP3 only). A type
/// that is not listed here is tried: servers often send a wrong or no type.
const Set<String> kUnsupportedStreamTypes = {
  'audio/aac',
  'audio/aacp',
  'audio/ogg',
  'audio/opus',
  'audio/flac',
  'audio/mp4',
  'audio/x-mpegurl',
  'audio/mpegurl',
  'audio/x-scpls',
  'application/ogg',
  'application/vnd.apple.mpegurl',
  'application/x-mpegurl',
  'text/html',
};

/// The text of the error for a stream that is not MP3. The native engine uses the
/// same words (`STREAM_UNSUPPORTED` in audio/src/source.rs).
const String kStreamUnsupportedMessage = 'stream type not supported';

/// Where the MP3 bytes go. The engine's queue never blocks: [push] returns how many
/// bytes it took (0 when full) or -1 when the stream was closed.
abstract interface class StreamSink {
  int push(Uint8List data);
  void fail(String message, AudioErrorKind kind);
}

/// Removes the ICY metadata blocks from a stream. After every [metaInt] bytes of
/// audio the server sends one length byte (times 16 is the size of the text block
/// that follows, 0 means no text). Chunks may end anywhere, also inside a block.
class IcyFilter {
  IcyFilter(this.metaInt, {this.onTitle});

  /// Bytes of audio between two metadata blocks; 0 or less means the server sends none.
  final int metaInt;

  /// Called with the new title, or null when the station announces an empty one.
  final void Function(String? title)? onTitle;

  late int _audioLeft = metaInt;
  int _blockLeft = 0;
  final BytesBuilder _block = BytesBuilder(copy: false);

  /// Returns the audio bytes of [chunk].
  Uint8List process(Uint8List chunk) {
    if (metaInt <= 0) return chunk;
    final out = BytesBuilder(copy: false);
    var i = 0;
    while (i < chunk.length) {
      if (_blockLeft > 0) {
        final n = _blockLeft < chunk.length - i ? _blockLeft : chunk.length - i;
        _block.add(Uint8List.sublistView(chunk, i, i + n));
        _blockLeft -= n;
        i += n;
        if (_blockLeft == 0) _finishBlock();
      } else if (_audioLeft == 0) {
        final size = chunk[i++] * 16;
        if (size == 0) {
          _audioLeft = metaInt;
        } else {
          _blockLeft = size;
        }
      } else {
        final n = _audioLeft < chunk.length - i ? _audioLeft : chunk.length - i;
        out.add(Uint8List.sublistView(chunk, i, i + n));
        _audioLeft -= n;
        i += n;
      }
    }
    return out.takeBytes();
  }

  void _finishBlock() {
    final bytes = _block.takeBytes();
    _audioLeft = metaInt;
    final (found, title) = _parseBlock(bytes);
    if (found) onTitle?.call(title);
  }
}

/// Reads `StreamTitle='...';` from a metadata block; null if there is no title or
/// it is empty.
String? parseStreamTitle(List<int> block) => _parseBlock(block).$2;

/// (whether the block has a `StreamTitle` field at all, its text or null if empty)
(bool, String?) _parseBlock(List<int> block) {
  // The text is zero-padded to a multiple of 16 bytes.
  var end = block.length;
  while (end > 0 && block[end - 1] == 0) {
    end--;
  }
  String text;
  try {
    text = utf8.decode(block.sublist(0, end));
  } on FormatException {
    // Older stations send Latin-1.
    text = latin1.decode(block.sublist(0, end));
  }
  const key = "StreamTitle='";
  final start = text.indexOf(key);
  if (start < 0) return (false, null);
  final from = start + key.length;
  // The value ends at "';" (titles may contain a lone quote).
  var stop = text.indexOf("';", from);
  if (stop < 0) stop = text.endsWith("'") ? text.length - 1 : text.length;
  final title = text.substring(from, stop).trim();
  return (true, title.isEmpty ? null : title);
}

class _Fatal implements Exception {
  _Fatal(this.message, this.kind);
  final String message;
  final AudioErrorKind kind;
}

class _Closed implements Exception {}

/// One radio connection: opens the URL, filters, pushes, and reconnects.
class RadioStreamSession {
  RadioStreamSession({
    required this.url,
    required this.sink,
    this.onTitle,
    this.onDetails,
    HttpClient Function()? clientFactory,
    this.reconnectDelay = kStreamReconnectDelay,
    this.reconnectAttempts = kStreamReconnectAttempts,
    this.connectTimeout = kStreamConnectTimeout,
    this.idleTimeout = kStreamIdleTimeout,
  }) : _clientFactory = clientFactory ?? HttpClient.new;

  final Uri url;
  final StreamSink sink;
  final void Function(String? title)? onTitle;

  /// Called with what the headers of each connection say about the station.
  final void Function(StreamDetails details)? onDetails;
  final Duration reconnectDelay;
  final int reconnectAttempts;
  final Duration connectTimeout;
  final Duration idleTimeout;
  final HttpClient Function() _clientFactory;

  RadioState _state = const RadioIdle();
  HttpClient? _client;
  Future<void>? _running;
  StreamDetails? _details;

  bool get _closed => _state is RadioClosed;

  RadioPolicy get _policy => RadioPolicy(reconnectDelay: reconnectDelay, reconnectAttempts: reconnectAttempts, healthyBytes: kStreamHealthyBytes);

  /// Completes when the session is over (closed, failed, or ended).
  Future<void> get done => _running ?? Future<void>.value();

  /// Number of connections made so far (for tests and logs).
  int connections = 0;

  void start() {
    _running ??= _run();
  }

  /// Stops at once. Nothing is pushed to the sink afterwards.
  Future<void> close() {
    _dispatch(const RadioClose());
    return done;
  }

  /// Feeds [event] to the pure machine and carries out the effects that finish at once; returns the ones that take time.
  List<RadioEffect> _dispatch(RadioEvent event) {
    final (next, effects) = transition(_policy, _state, event);
    _state = next;
    final slow = <RadioEffect>[];
    for (final e in effects) {
      switch (e) {
        case OpenConnection() || Sleep():
          slow.add(e);
        case ReportDetails():
          final d = _details;
          if (d != null) onDetails?.call(d);
        case ReportFail(:final message, :final kind):
          sink.fail(message, kind);
        case CloseConnection():
          _client?.close(force: true);
      }
    }
    return slow;
  }

  Future<void> _run() async {
    try {
      var pending = _dispatch(const RadioStart());
      while (pending.isNotEmpty) {
        final effect = pending.first;
        final RadioEvent event;
        if (effect is Sleep) {
          await Future<void>.delayed(effect.delay);
          event = const RadioBackoffElapsed();
        } else {
          event = await _open();
        }
        pending = _dispatch(event);
      }
    } finally {
      _client?.close(force: true);
      _client = null;
    }
  }

  /// One connection, from the request to its end, as the event that tells the machine how it ended.
  Future<RadioEvent> _open() async {
    try {
      await _connectAndPump();
      return const RadioPeerClosed();
    } on _Closed {
      return const RadioClose();
    } on _Fatal catch (e) {
      return RadioError(e.message, kind: e.kind);
    } on HandshakeException catch (e) {
      return RadioError('certificate error: ${e.message}', kind: AudioErrorKind.streamCertificate);
    } on TlsException catch (e) {
      return RadioError('certificate error: ${e.message}', kind: AudioErrorKind.streamCertificate);
    } on Object catch (e) {
      return RadioError(_describe(e));
    }
  }

  static String _describe(Object e) {
    if (e is SocketException) {
      return e.message.isEmpty ? 'socket error' : e.message;
    }
    if (e is TimeoutException) return 'timeout';
    if (e is HttpException) return e.message;
    return '$e';
  }

  Future<void> _connectAndPump() async {
    connections++;
    final client = _clientFactory()
      ..connectionTimeout = connectTimeout
      ..userAgent = kStreamUserAgent;
    _client = client;
    final request = await client.getUrl(url).timeout(connectTimeout);
    request.headers.set('Icy-MetaData', '1');
    request.headers.set(HttpHeaders.acceptHeader, '*/*');
    final response = await request.close().timeout(connectTimeout);
    if (_closed) throw _Closed();

    final status = response.statusCode;
    if (status < 200 || status >= 300) {
      await response.drain<void>().catchError((Object _) {});
      // A server that is busy or asks to wait may be better in a moment.
      final retry = status >= 500 || status == 408 || status == 429;
      if (retry) throw HttpException('HTTP $status');
      throw _Fatal('HTTP $status', AudioErrorKind.streamServer);
    }
    final type = response.headers.contentType?.mimeType.toLowerCase();
    if (type != null && kUnsupportedStreamTypes.contains(type)) {
      await response.drain<void>().catchError((Object _) {});
      throw _Fatal(kStreamUnsupportedMessage, AudioErrorKind.streamUnsupported);
    }
    _details = _detailsOf(response.headers, type);
    _dispatch(const RadioConnected());
    final metaInt = int.tryParse(response.headers.value('icy-metaint') ?? '') ?? 0;
    final filter = IcyFilter(metaInt, onTitle: onTitle);
    await for (final chunk in response.timeout(idleTimeout)) {
      if (_closed) throw _Closed();
      final audio = filter.process(chunk is Uint8List ? chunk : Uint8List.fromList(chunk));
      await _pushAll(audio);
    }
  }

  /// Offers [data] until the engine has taken it all. Waiting here pauses the
  /// network read, which is the back pressure.
  Future<void> _pushAll(Uint8List data) async {
    var offset = 0;
    while (offset < data.length) {
      if (_closed) throw _Closed();
      final n = sink.push(Uint8List.sublistView(data, offset));
      if (n < 0) throw _Closed();
      if (n == 0) {
        await Future<void>.delayed(kStreamPushRetry);
        continue;
      }
      _dispatch(RadioBytes(n));
      offset += n;
    }
  }
}

/// A header text: a station sends UTF-8 that the HTTP client has read as Latin-1, so it is put right again where it fits.
String? _headerText(HttpHeaders headers, String name) => decodeHeaderText(headers.value(name));

/// [raw] as a station meant it: UTF-8 that was read as Latin-1 is put right again where it fits, else the text stays as it is.
/// Null for nothing or blank.
String? decodeHeaderText(String? raw) {
  final text = raw?.trim();
  if (text == null || text.isEmpty) return null;
  try {
    return utf8.decode(latin1.encode(text)).trim();
  } on Object {
    return text;
  }
}

StreamDetails _detailsOf(HttpHeaders headers, String? mime) => StreamDetails(
  name: _headerText(headers, 'icy-name'),
  genre: _headerText(headers, 'icy-genre'),
  description: _headerText(headers, 'icy-description'),
  website: _headerText(headers, 'icy-url'),
  bitrateKbps: int.tryParse(headers.value('icy-br')?.split(',').first.trim() ?? ''),
  format: switch (mime) {
    'audio/mpeg' || 'audio/mp3' => 'MP3',
    null => null,
    final m => m.replaceFirst('audio/', '').toUpperCase(),
  },
);
