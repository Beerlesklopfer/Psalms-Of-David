// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:flutter/foundation.dart' show ValueListenable, ValueNotifier;

import 'audio_backend.dart';
import 'radio_isolate.dart';
import 'radio_stream.dart';

// The C interface of libpsalm_audio.so (audio/include/psalm_audio.h). An engine is an opaque 64-bit handle.
typedef _Version = Uint32 Function();
typedef _VersionD = int Function();
typedef _New = Uint64 Function(Pointer<Utf8>);
typedef _NewD = int Function(Pointer<Utf8>);
typedef _NewNull = Uint64 Function(Uint32);
typedef _NewNullD = int Function(int);
typedef _Free = Int32 Function(Uint64);
typedef _FreeD = int Function(int);
typedef _Load = Int64 Function(Uint64, Pointer<Utf8>, Uint64, Int32, Pointer<Utf8>);
typedef _LoadD = int Function(int, Pointer<Utf8>, int, int, Pointer<Utf8>);
typedef _Cmd = Int32 Function(Uint64);
typedef _CmdD = int Function(int);
typedef _Seek = Int32 Function(Uint64, Uint64);
typedef _SeekD = int Function(int, int);
typedef _Count = Int32 Function(Uint64, Uint32);
typedef _CountD = int Function(int, int);
typedef _Vol = Int32 Function(Uint64, Float);
typedef _VolD = int Function(int, double);
typedef _I64 = Int64 Function(Uint64);
typedef _I64D = int Function(int);
typedef _F32 = Float Function(Uint64);
typedef _F32D = double Function(int);
typedef _Err = IntPtr Function(Pointer<Utf8>, IntPtr);
typedef _ErrD = int Function(Pointer<Utf8>, int);
typedef _EnqueueNext = Int32 Function(Uint64, Pointer<Utf8>);
typedef _EnqueueNextD = int Function(int, Pointer<Utf8>);
typedef _StreamOpen = Int64 Function(Uint64, Int32, Pointer<Utf8>);
typedef _StreamOpenD = int Function(int, int, Pointer<Utf8>);
typedef _StreamPush = Int64 Function(Uint64, Pointer<Uint8>, IntPtr);
typedef _StreamPushD = int Function(int, Pointer<Uint8>, int);
typedef _StreamFail = Int32 Function(Uint64, Pointer<Utf8>);
typedef _StreamFailD = int Function(int, Pointer<Utf8>);
typedef _EngineErr = IntPtr Function(Uint64, Pointer<Utf8>, IntPtr);
typedef _EngineErrD = int Function(int, Pointer<Utf8>, int);

/// The major version of the C interface this binding is written for (`PSALM_API_VERSION >> 16` in the header). The library
/// is refused when its major number differs: the interface changed in a way that is not compatible.
const int kPsalmApiMajor = 2;

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
/// Every call returns at once. [load] only starts a worker thread in the engine that opens the file; until it is done the
/// engine's state is "loading", which this class reports as the state the caller asked for (playing, or paused if
/// `play: false`), so that the player's machine sees what it saw when [load] still waited. A file that cannot be opened (or
/// decoded) shows up as [BackendState.failed] with [errorMessage] a moment later. A file that does not exist is still
/// refused by [load] itself (a stat, no I/O worth waiting for), because the player treats a failed load differently from a
/// playback that fails.
///
/// Paths that start with `http://` or `https://` are internet radio: the network
/// part is Dart ([IsolatedRadioSession], which runs a [RadioStreamSession] in a background isolate) and the engine decodes the pushed MP3 bytes.
/// Such a stream has no length, cannot seek, and reports titles through [streamTitle].
class PipeWireBackend implements AudioBackend, StreamInfo, ExternalControl {
  /// The engine plays the queued next track without a gap: it pre-opens the file on a worker thread and splices it in at the exact
  /// sample when the loaded one ends (the encoder delay and padding of MP3 are trimmed). The switch is counted by the engine
  /// (`psalm_engine_track_generation`) and reported through [onAdvanced]; [state] never becomes `ended` in between.
  @override
  bool get supportsGapless => true;

  @override
  void enqueueNext(String path) {
    if (isStreamUrl(path) || _session != null) return;
    final native = path.toNativeUtf8();
    try {
      _ok(_enqueueNext(_e, native), 'cannot queue $path');
    } finally {
      calloc.free(native);
    }
    // The switch is noticed by polling; this timer covers the time when nobody reads the position.
    _advanceTimer ??= Timer.periodic(const Duration(milliseconds: 200), (_) => _pollAdvanced());
  }

  @override
  void clearNext() => _ok(_clearNext(_e), 'clearNext failed');

  void Function()? _onAdvanced;
  Timer? _advanceTimer;
  int _seenGeneration = 0;

  @override
  set onAdvanced(void Function()? callback) => _onAdvanced = callback;

  /// Never called: nothing outside the engine starts, pauses or moves it on Linux.
  @override
  set onExternalChange(void Function()? callback) {}

  /// Calls [onAdvanced] once for every switch the engine made since the last look. Done before the position or state of the new
  /// track is returned, so the player has moved on to the new track when it reads them.
  void _pollAdvanced() {
    if (_disposed) return;
    final generation = _trackGeneration(_engine);
    if (generation <= _seenGeneration) return;
    _seenGeneration = generation;
    _onAdvanced?.call();
  }

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
      _enqueueNext = lib.lookupFunction<_EnqueueNext, _EnqueueNextD>('psalm_engine_enqueue_next'),
      _clearNext = lib.lookupFunction<_Cmd, _CmdD>('psalm_engine_clear_next'),
      _trackGeneration = lib.lookupFunction<_I64, _I64D>('psalm_engine_track_generation'),
      _engineLastError = lib.lookupFunction<_EngineErr, _EngineErrD>('psalm_engine_last_error'),
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
  /// Throws [AudioException] if the library or PipeWire is not there, or if the library
  /// has another major interface version than this binding ([kPsalmApiMajor]).
  static PipeWireBackend open({String? target, String? libraryPath}) {
    final lib = _openLibrary(libraryPath);
    final newEngine = lib.lookupFunction<_New, _NewD>('psalm_engine_new');
    final nativeTarget = target == null ? nullptr.cast<Utf8>() : target.toNativeUtf8();
    try {
      final engine = newEngine(nativeTarget);
      if (engine == 0) {
        throw AudioException(_readError(lib.lookupFunction<_Err, _ErrD>('psalm_last_error'), 'cannot start the sound engine'));
      }
      return PipeWireBackend._(engine, lib);
    } finally {
      if (target != null) calloc.free(nativeTarget);
    }
  }

  /// An engine with no output (nothing is played; no PipeWire needed): decodes, keeps time, switches tracks. For tests.
  /// [speed]: 0 as fast as the decoder delivers, 1 real time, n times n.
  static PipeWireBackend openNull({String? libraryPath, int speed = 0}) {
    final lib = _openLibrary(libraryPath);
    final engine = lib.lookupFunction<_NewNull, _NewNullD>('psalm_engine_new_null')(speed);
    if (engine == 0) {
      throw AudioException(_readError(lib.lookupFunction<_Err, _ErrD>('psalm_last_error'), 'cannot start the null engine'));
    }
    return PipeWireBackend._(engine, lib);
  }

  static DynamicLibrary _openLibrary(String? libraryPath) {
    final path = libraryPath ?? Platform.environment['PSALM_AUDIO_LIB'] ?? 'libpsalm_audio.so';
    final DynamicLibrary lib;
    try {
      lib = DynamicLibrary.open(path);
    } on ArgumentError catch (e) {
      throw AudioException('cannot load $path: ${e.message}');
    }
    if (!lib.providesSymbol('psalm_abi_version')) {
      throw AudioException('$path is an old version of the sound engine (no psalm_abi_version); rebuild it with "cargo build --release"');
    }
    final version = lib.lookupFunction<_Version, _VersionD>('psalm_abi_version')();
    if (version >> 16 != kPsalmApiMajor) {
      throw AudioException(
        '$path has the interface version ${version >> 16}.${version & 0xffff}, this app needs $kPsalmApiMajor.x; rebuild the library or the app',
      );
    }
    return lib;
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

  final int _engine;
  final _FreeD _free;
  final _LoadD _load;
  final _CmdD _play, _pause, _stop, _stateOf;
  final _EngineErrD _engineLastError;
  final _EnqueueNextD _enqueueNext;
  final _CmdD _clearNext;
  final _I64D _trackGeneration;
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

  int get _e {
    if (_disposed) throw StateError('the backend was disposed');
    return _engine;
  }

  void _ok(int result, String what) {
    if (result == 0) return;
    // The reason of a failed call is kept per engine.
    final buffer = calloc<Uint8>(512).cast<Utf8>();
    try {
      final n = _engineLastError(_engine, buffer, 512);
      throw AudioException(n == 0 ? _readError(_lastError, what) : buffer.toDartString());
    } finally {
      calloc.free(buffer);
    }
  }

  /// What the caller last asked for; reported while the engine is still loading.
  bool _wantPlay = true;

  @override
  void load(String path, {Duration start = Duration.zero, bool play = true, String title = ''}) {
    _closeStream();
    _wantPlay = play;
    if (isStreamUrl(path)) {
      _loadStream(path, play, title);
      return;
    }
    if (!File(path).existsSync()) {
      _failure = 'cannot load $path: no such file';
      throw AudioException(_failure!);
    }
    final nativePath = path.toNativeUtf8();
    final nativeTitle = title.toNativeUtf8();
    try {
      // The result is the id of the request (1 or more), or -1 if the call itself failed.
      _ok(_load(_e, nativePath, start.inMilliseconds, play ? 1 : 0, nativeTitle) < 0 ? -1 : 0, 'cannot load $path');
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
      _ok(_streamOpen(_e, play ? 1 : 0, nativeTitle) < 0 ? -1 : 0, 'cannot open the stream');
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
  void play() {
    _ok(_play(_e), 'play failed');
    _wantPlay = true;
  }

  @override
  void pause() {
    _ok(_pause(_e), 'pause failed');
    _wantPlay = false;
  }

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
  BackendState get state {
    _pollAdvanced();
    return _nativeState;
  }

  BackendState get _nativeState => _failure != null
      ? BackendState.failed
      : switch (_stateOf(_e)) {
          1 => BackendState.playing,
          2 => BackendState.paused,
          3 => BackendState.ended,
          4 => BackendState.failed,
          5 => BackendState.buffering,
          // 6: a file is being opened by the engine's worker. The player's machine has no such state; it sees what was asked for.
          6 => _wantPlay ? BackendState.playing : BackendState.paused,
          _ => BackendState.idle,
        };

  @override
  Duration get position {
    _pollAdvanced();
    final ms = _positionMs(_e);
    return Duration(milliseconds: ms < 0 ? 0 : ms);
  }

  @override
  Duration? get duration {
    _pollAdvanced();
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
    _advanceTimer?.cancel();
    _advanceTimer = null;
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
