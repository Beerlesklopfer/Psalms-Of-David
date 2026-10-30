// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:flutter/foundation.dart' show ValueListenable, ValueNotifier;

import 'audio_backend.dart';
import 'radio_isolate.dart';
import 'radio_stream.dart';

// The C interface of libpsalm_audio.so (audio/include/psalm_audio.h).
typedef _New = Pointer<Void> Function(Pointer<Utf8>);
typedef _Free = Void Function(Pointer<Void>);
typedef _FreeD = void Function(Pointer<Void>);
typedef _Load = Int32 Function(Pointer<Void>, Pointer<Utf8>, Uint64, Int32, Pointer<Utf8>);
typedef _LoadD = int Function(Pointer<Void>, Pointer<Utf8>, int, int, Pointer<Utf8>);
typedef _Cmd = Int32 Function(Pointer<Void>);
typedef _CmdD = int Function(Pointer<Void>);
typedef _Seek = Int32 Function(Pointer<Void>, Uint64);
typedef _SeekD = int Function(Pointer<Void>, int);
typedef _Count = Int32 Function(Pointer<Void>, Uint32);
typedef _CountD = int Function(Pointer<Void>, int);
typedef _Vol = Int32 Function(Pointer<Void>, Float);
typedef _VolD = int Function(Pointer<Void>, double);
typedef _I64 = Int64 Function(Pointer<Void>);
typedef _I64D = int Function(Pointer<Void>);
typedef _F32 = Float Function(Pointer<Void>);
typedef _F32D = double Function(Pointer<Void>);
typedef _Err = IntPtr Function(Pointer<Utf8>, IntPtr);
typedef _ErrD = int Function(Pointer<Utf8>, int);
typedef _StreamOpen = Int32 Function(Pointer<Void>, Int32, Pointer<Utf8>);
typedef _StreamOpenD = int Function(Pointer<Void>, int, Pointer<Utf8>);
typedef _StreamPush = Int64 Function(Pointer<Void>, Pointer<Uint8>, IntPtr);
typedef _StreamPushD = int Function(Pointer<Void>, Pointer<Uint8>, int);
typedef _StreamFail = Int32 Function(Pointer<Void>, Pointer<Utf8>);
typedef _StreamFailD = int Function(Pointer<Void>, Pointer<Utf8>);
typedef _EngineErr = IntPtr Function(Pointer<Void>, Pointer<Utf8>, IntPtr);
typedef _EngineErrD = int Function(Pointer<Void>, Pointer<Utf8>, int);

/// Size of the native buffer one push goes through. A network chunk that is larger
/// is offered in several pieces.
const int kStreamPushBytes = 64 * 1024;

/// Longest error text read back from the engine.
const int _errorBytes = 512;

/// Tells the engine whether [url] is a stream (offered to the network code) or a file path.
bool isStreamUrl(String path) {
  final lower = path.toLowerCase();
  return lower.startsWith('http://') || lower.startsWith('https://');
}

/// The sound engine on Linux: PipeWire, through the Rust library over `dart:ffi`.
///
/// The calls are synchronous. [load] opens and decodes the start of the file and
/// can take a moment; everything else returns at once.
///
/// Paths that start with `http://` or `https://` are internet radio: the network
/// part is Dart ([IsolatedRadioSession], which runs a [RadioStreamSession] in a background isolate) and the engine decodes the pushed MP3 bytes.
/// Such a stream has no length, cannot seek, and reports titles through [streamTitle].
class PipeWireBackend implements AudioBackend, StreamInfo {
  // TODO(gapless, P3): supportsGapless stays false (the AudioBackend default) until the Rust engine has `psalm_engine_enqueue_next`
  // (pre-opens the next source and splices it in at the exact sample, trims the MP3 encoder delay). Then override supportsGapless,
  // enqueueNext and clearNext here and report the switch through ExternalControl.onAdvanced. See docs/spec-engine-rebuild.md, phase P3.
  @override
  bool get supportsGapless => false;

  @override
  void enqueueNext(String path) {}

  @override
  void clearNext() {}

  PipeWireBackend._(this._engine, DynamicLibrary lib)
    : _free = lib.lookupFunction<_Free, _FreeD>('psalm_engine_free'),
      _load = lib.lookupFunction<_Load, _LoadD>('psalm_engine_load'),
      _play = lib.lookupFunction<_Cmd, _CmdD>('psalm_engine_play'),
      _pause = lib.lookupFunction<_Cmd, _CmdD>('psalm_engine_pause'),
      _stop = lib.lookupFunction<_Cmd, _CmdD>('psalm_engine_stop'),
      _seek = lib.lookupFunction<_Seek, _SeekD>('psalm_engine_seek'),
      _setVolume = lib.lookupFunction<_Vol, _VolD>('psalm_engine_set_volume'),
      _setSpeed = lib.lookupFunction<_Vol, _VolD>('psalm_engine_set_speed'),
      _setBuffer = lib.lookupFunction<_Count, _CountD>('psalm_engine_set_buffer'),
      _stateOf = lib.lookupFunction<_Cmd, _CmdD>('psalm_engine_state'),
      _positionMs = lib.lookupFunction<_I64, _I64D>('psalm_engine_position_ms'),
      _durationMs = lib.lookupFunction<_I64, _I64D>('psalm_engine_duration_ms'),
      _volumeOf = lib.lookupFunction<_F32, _F32D>('psalm_engine_volume'),
      _lastError = lib.lookupFunction<_Err, _ErrD>('psalm_last_error'),
      _streamOpen = lib.lookupFunction<_StreamOpen, _StreamOpenD>('psalm_engine_stream_open'),
      _streamPush = lib.lookupFunction<_StreamPush, _StreamPushD>('psalm_engine_stream_push'),
      _streamFail = lib.lookupFunction<_StreamFail, _StreamFailD>('psalm_engine_stream_fail'),
      _engineError = lib.lookupFunction<_EngineErr, _EngineErrD>('psalm_engine_error');

  /// Connects to PipeWire. [target] is the node name or serial of a sink; null uses
  /// the default. [libraryPath] defaults to the environment variable
  /// `PSALM_AUDIO_LIB`, then to `libpsalm_audio.so` from the usual library search.
  /// Throws [AudioException] if the library or PipeWire is not there.
  static PipeWireBackend open({String? target, String? libraryPath}) {
    final path = libraryPath ?? Platform.environment['PSALM_AUDIO_LIB'] ?? 'libpsalm_audio.so';
    final DynamicLibrary lib;
    try {
      lib = DynamicLibrary.open(path);
    } on ArgumentError catch (e) {
      throw AudioException('cannot load $path: ${e.message}');
    }
    final newEngine = lib.lookupFunction<_New, _New>('psalm_engine_new');
    final lastError = lib.lookupFunction<_Err, _ErrD>('psalm_last_error');
    final nativeTarget = target == null ? nullptr.cast<Utf8>() : target.toNativeUtf8();
    try {
      final engine = newEngine(nativeTarget);
      if (engine == nullptr) {
        throw AudioException(_readError(lastError, 'cannot start the sound engine'));
      }
      return PipeWireBackend._(engine, lib);
    } finally {
      if (target != null) calloc.free(nativeTarget);
    }
  }

  static String _readError(_ErrD lastError, String fallback) {
    final buffer = calloc<Uint8>(512).cast<Utf8>();
    try {
      final n = lastError(buffer, 512);
      return n == 0 ? fallback : buffer.toDartString();
    } finally {
      calloc.free(buffer);
    }
  }

  final Pointer<Void> _engine;
  final _FreeD _free;
  final _LoadD _load;
  final _CmdD _play, _pause, _stop, _stateOf;
  final _SeekD _seek;
  final _VolD _setVolume, _setSpeed;
  final _CountD _setBuffer;
  int _bufferSeconds = kBufferDefaultSeconds;
  double _speed = 1.0;
  final _I64D _positionMs, _durationMs;
  final _F32D _volumeOf;
  final _ErrD _lastError;
  final _StreamOpenD _streamOpen;
  final _StreamPushD _streamPush;
  final _StreamFailD _streamFail;
  final _EngineErrD _engineError;
  bool _disposed = false;

  // The live stream in use, if any.
  IsolatedRadioSession? _session;
  Pointer<Uint8>? _pushBuffer;
  String? _failure;
  AudioErrorKind _failureKind = AudioErrorKind.generic;
  final ValueNotifier<String?> _title = ValueNotifier<String?>(null);
  final ValueNotifier<StreamDetails?> _details = ValueNotifier<StreamDetails?>(null);

  Pointer<Void> get _e {
    if (_disposed) throw StateError('the backend was disposed');
    return _engine;
  }

  void _ok(int result, String what) {
    if (result != 0) throw AudioException(_readError(_lastError, what));
  }

  @override
  void load(String path, {Duration start = Duration.zero, bool play = true, String title = ''}) {
    _closeStream();
    if (isStreamUrl(path)) {
      _loadStream(path, play, title);
      return;
    }
    final nativePath = path.toNativeUtf8();
    final nativeTitle = title.toNativeUtf8();
    try {
      _ok(_load(_e, nativePath, start.inMilliseconds, play ? 1 : 0, nativeTitle), 'cannot load $path');
    } finally {
      calloc.free(nativePath);
      calloc.free(nativeTitle);
    }
  }

  void _loadStream(String url, bool play, String title) {
    final Uri uri;
    try {
      uri = Uri.parse(url);
    } on FormatException {
      throw AudioException('not a valid stream address');
    }
    final nativeTitle = title.toNativeUtf8();
    try {
      _ok(_streamOpen(_e, play ? 1 : 0, nativeTitle), 'cannot open the stream');
    } finally {
      calloc.free(nativeTitle);
    }
    _pushBuffer = calloc<Uint8>(kStreamPushBytes);
    _session = IsolatedRadioSession(url: uri, sink: _EngineSink(this), onTitle: (t) => _title.value = t, onDetails: (d) => _details.value = d)..start();
  }

  /// Ends the network side of a live stream; the engine drops its queue itself on
  /// the next load or stop.
  void _closeStream() {
    _session?.close();
    _session = null;
    final buffer = _pushBuffer;
    if (buffer != null) {
      _pushBuffer = null;
      calloc.free(buffer);
    }
    _failure = null;
    _failureKind = AudioErrorKind.generic;
    _title.value = null;
    _details.value = null;
  }

  int _push(Uint8List data) {
    final buffer = _pushBuffer;
    if (_disposed || buffer == null) return -1;
    final n = data.length < kStreamPushBytes ? data.length : kStreamPushBytes;
    buffer.asTypedList(n).setAll(0, Uint8List.sublistView(data, 0, n));
    return _streamPush(_engine, buffer, n);
  }

  void _fail(String message, AudioErrorKind kind) {
    if (_disposed) return;
    _failure = message;
    _failureKind = kind;
    final native = message.toNativeUtf8();
    try {
      _streamFail(_engine, native);
    } finally {
      calloc.free(native);
    }
  }

  @override
  ValueListenable<String?> get streamTitle => _title;

  @override
  ValueListenable<StreamDetails?> get streamDetails => _details;

  @override
  String? get errorMessage {
    if (_disposed) return null;
    if (_failure != null) return _failure;
    final buffer = calloc<Uint8>(_errorBytes).cast<Utf8>();
    try {
      final n = _engineError(_engine, buffer, _errorBytes);
      if (n == 0) return null;
      final text = buffer.toDartString();
      return text == kStreamUnsupportedMessage ? text : text.replaceFirst(RegExp(r'^decode: '), '');
    } finally {
      calloc.free(buffer);
    }
  }

  @override
  AudioErrorKind get errorKind {
    if (_failure != null) return _failureKind;
    return errorMessage == kStreamUnsupportedMessage ? AudioErrorKind.streamUnsupported : AudioErrorKind.generic;
  }

  @override
  void play() => _ok(_play(_e), 'play failed');

  @override
  void pause() => _ok(_pause(_e), 'pause failed');

  @override
  void stop() {
    _closeStream();
    _ok(_stop(_e), 'stop failed');
  }

  @override
  void seek(Duration to) {
    // A live stream cannot seek; the engine ignores it too.
    if (_session != null) return;
    _ok(_seek(_e, to.inMilliseconds), 'seek failed');
  }

  @override
  void setVolume(double volume) => _ok(_setVolume(_e, volume.clamp(0.0, 1.0)), 'volume failed');

  @override
  BackendState get state => _failure != null
      ? BackendState.failed
      : switch (_stateOf(_e)) {
          1 => BackendState.playing,
          2 => BackendState.paused,
          3 => BackendState.ended,
          4 => BackendState.failed,
          5 => BackendState.buffering,
          _ => BackendState.idle,
        };

  @override
  Duration get position {
    final ms = _positionMs(_e);
    return Duration(milliseconds: ms < 0 ? 0 : ms);
  }

  @override
  Duration? get duration {
    final ms = _durationMs(_e);
    return ms < 0 ? null : Duration(milliseconds: ms);
  }

  @override
  void setSpeed(double speed) {
    _ok(_setSpeed(_e, speed.clamp(0.5, 3.0)), 'speed failed');
    _speed = speed.clamp(0.5, 3.0);
  }

  @override
  void setBufferSeconds(int seconds) {
    final n = seconds.clamp(kBufferMinSeconds, kBufferMaxSeconds);
    _ok(_setBuffer(_e, n), 'buffer failed');
    _bufferSeconds = n;
  }

  @override
  int get bufferSeconds => _bufferSeconds;

  @override
  bool get bufferAdjustable => true;

  @override
  double get speed => _speed;

  @override
  double get volume => _volumeOf(_e);

  @override
  void dispose() {
    if (_disposed) return;
    _closeStream();
    _disposed = true;
    _free(_engine);
    _title.dispose();
    _details.dispose();
  }
}

/// Hands the bytes of a radio session to the engine (always on the UI isolate).
class _EngineSink implements StreamSink {
  _EngineSink(this._backend);

  final PipeWireBackend _backend;

  @override
  int push(Uint8List data) => _backend._push(data);

  @override
  void fail(String message, AudioErrorKind kind) => _backend._fail(message, kind);
}
