// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

import 'audio_backend.dart';

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
typedef _Vol = Int32 Function(Pointer<Void>, Float);
typedef _VolD = int Function(Pointer<Void>, double);
typedef _I64 = Int64 Function(Pointer<Void>);
typedef _I64D = int Function(Pointer<Void>);
typedef _F32 = Float Function(Pointer<Void>);
typedef _F32D = double Function(Pointer<Void>);
typedef _Err = IntPtr Function(Pointer<Utf8>, IntPtr);
typedef _ErrD = int Function(Pointer<Utf8>, int);

/// The sound engine on Linux: PipeWire, through the Rust library over `dart:ffi`.
///
/// The calls are synchronous. [load] opens and decodes the start of the file and
/// can take a moment; everything else returns at once.
class PipeWireBackend implements AudioBackend {
  PipeWireBackend._(this._engine, DynamicLibrary lib)
      : _free = lib.lookupFunction<_Free, _FreeD>('psalm_engine_free'),
        _load = lib.lookupFunction<_Load, _LoadD>('psalm_engine_load'),
        _play = lib.lookupFunction<_Cmd, _CmdD>('psalm_engine_play'),
        _pause = lib.lookupFunction<_Cmd, _CmdD>('psalm_engine_pause'),
        _stop = lib.lookupFunction<_Cmd, _CmdD>('psalm_engine_stop'),
        _seek = lib.lookupFunction<_Seek, _SeekD>('psalm_engine_seek'),
        _setVolume = lib.lookupFunction<_Vol, _VolD>('psalm_engine_set_volume'),
        _setSpeed = lib.lookupFunction<_Vol, _VolD>('psalm_engine_set_speed'),
        _stateOf = lib.lookupFunction<_Cmd, _CmdD>('psalm_engine_state'),
        _positionMs = lib.lookupFunction<_I64, _I64D>('psalm_engine_position_ms'),
        _durationMs = lib.lookupFunction<_I64, _I64D>('psalm_engine_duration_ms'),
        _volumeOf = lib.lookupFunction<_F32, _F32D>('psalm_engine_volume'),
        _lastError = lib.lookupFunction<_Err, _ErrD>('psalm_last_error');

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
  double _speed = 1.0;
  final _I64D _positionMs, _durationMs;
  final _F32D _volumeOf;
  final _ErrD _lastError;
  bool _disposed = false;

  Pointer<Void> get _e {
    if (_disposed) throw StateError('the backend was disposed');
    return _engine;
  }

  void _ok(int result, String what) {
    if (result != 0) throw AudioException(_readError(_lastError, what));
  }

  @override
  void load(String path, {Duration start = Duration.zero, bool play = true, String title = ''}) {
    final nativePath = path.toNativeUtf8();
    final nativeTitle = title.toNativeUtf8();
    try {
      _ok(_load(_e, nativePath, start.inMilliseconds, play ? 1 : 0, nativeTitle), 'cannot load $path');
    } finally {
      calloc.free(nativePath);
      calloc.free(nativeTitle);
    }
  }

  @override
  void play() => _ok(_play(_e), 'play failed');

  @override
  void pause() => _ok(_pause(_e), 'pause failed');

  @override
  void stop() => _ok(_stop(_e), 'stop failed');

  @override
  void seek(Duration to) => _ok(_seek(_e, to.inMilliseconds), 'seek failed');

  @override
  void setVolume(double volume) => _ok(_setVolume(_e, volume.clamp(0.0, 1.0)), 'volume failed');

  @override
  BackendState get state => switch (_stateOf(_e)) {
        1 => BackendState.playing,
        2 => BackendState.paused,
        3 => BackendState.ended,
        4 => BackendState.failed,
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
  double get speed => _speed;

  @override
  double get volume => _volumeOf(_e);

  @override
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _free(_engine);
  }
}
