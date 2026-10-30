// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/foundation.dart' show ValueListenable;

/// What the player needs from a sound engine. The app talks only to this interface;
/// behind it sits PipeWire on Linux (`PipeWireBackend`), and later ExoPlayer on
/// Android and AVPlayer on iPhone. `FakeBackend` serves tests.
///
/// [buffering] is only used by live streams: the engine collects audio before it
/// starts, or after the network ran dry, and goes on playing by itself.
enum BackendState { idle, playing, paused, ended, failed, buffering }

/// What went wrong, for a caller that wants to show its own (localized) text
/// instead of [AudioException.message].
enum AudioErrorKind {
  generic,

  /// The stream is not MP3 (AAC, Ogg, HLS playlist, a web page ...).
  streamUnsupported,

  /// The connection could not be made or was lost for good.
  streamConnection,

  /// The server answered with an error status.
  streamServer,

  /// The certificate of the server is not trusted.
  streamCertificate,
}

class AudioException implements Exception {
  AudioException(this.message, {this.kind = AudioErrorKind.generic});

  final String message;
  final AudioErrorKind kind;

  @override
  String toString() => 'AudioException: $message';
}

abstract class AudioBackend {
  /// Opens a file at [start]. Plays at once if [play] is true, otherwise starts
  /// paused. Throws [AudioException] if the file cannot be opened.
  void load(String path, {Duration start = Duration.zero, bool play = true, String title = ''});

  void play();
  void pause();
  void stop();

  /// Jumps to [to]; keeps playing or staying paused.
  void seek(Duration to);

  /// 0.0 (silent) to 1.0.
  void setVolume(double volume);

  /// Playback speed, 1.0 is normal; the pitch stays. The range is 0.5 to 3.0 and it
  /// stays set across [load] and [seek].
  void setSpeed(double speed);

  /// Seconds of audio the engine keeps ready between decoding and the output. A bigger buffer bridges a slow disk or network, a
  /// smaller one is heard sooner at a change of track. Applies from the next [load]. Only an engine that has such a buffer
  /// ([bufferAdjustable]) uses it.
  void setBufferSeconds(int seconds);

  int get bufferSeconds;

  /// Whether [setBufferSeconds] changes anything in this engine.
  bool get bufferAdjustable;

  BackendState get state;

  /// Where the listening is. Counts the audio handed to the sound system, so it
  /// can be a little ahead of what is audible.
  Duration get position;

  /// Length of the file, or null if the file does not say.
  Duration? get duration;

  double get volume;

  double get speed;

  /// Whether this engine can play a queued next track without a gap (see [enqueueNext]). Gapless playback is mandatory on every system; an
  /// engine that returns false here is a known gap in the plan (see docs/spec-engine-rebuild.md).
  bool get supportsGapless => false;

  /// The track that follows the loaded one, a file on the disk. A backend that [supportsGapless] plays it without a gap when the loaded one
  /// ends and then reports the switch through [ExternalControl.onAdvanced]; it never reports [BackendState.ended] in between. Replaces an
  /// earlier queued track. Ignored for a stream address, and by a backend that does not support it (the default).
  void enqueueNext(String path) {}

  /// Drops the queued track of [enqueueNext], if there is one.
  void clearNext() {}

  /// Releases the engine. Using the backend afterwards is an error; calling this
  /// twice is fine.
  void dispose();
}

/// Optional extension of a backend that the system can control from outside (the notification, the lock screen, the buttons of headphones and
/// cars, a phone call, unplugged headphones). The player checks `backend is ExternalControl`; backends without it (PipeWire) need nothing.
abstract class ExternalControl {
  /// Called when the engine started or paused by itself, or the position was moved from outside, so that the player can follow.
  /// [state] and [position] of the backend are already up to date then.
  set onExternalChange(void Function()? callback);

  /// Called when the engine moved on to the track queued with [AudioBackend.enqueueNext] by itself (gapless). [AudioBackend.position]
  /// and [AudioBackend.duration] then belong to the new track and [AudioBackend.state] is still playing. Not an [onExternalChange].
  set onAdvanced(void Function()? callback);
}

/// Optional extension of a backend that plays internet radio. The player checks
/// `backend is StreamInfo`; backends without it (just_audio, the fake) need nothing.
abstract class StreamInfo {
  /// The title the station announces now ("Artist - Song"), or null.
  ValueListenable<String?> get streamTitle;

  /// What the station says about itself in the headers of the connection, or null before they have come.
  ValueListenable<StreamDetails?> get streamDetails;

  /// Why the state is [BackendState.failed], if the backend knows.
  String? get errorMessage;

  AudioErrorKind get errorKind;
}

/// The range of the playback buffer in seconds (the engine takes the same) and the value when nobody has set one.
const int kBufferMinSeconds = 1;
const int kBufferMaxSeconds = 30;
const int kBufferDefaultSeconds = 2;

/// What a station says about itself when the connection is made (the `icy-*` headers and the content type).
class StreamDetails {
  const StreamDetails({this.name, this.genre, this.description, this.website, this.bitrateKbps, this.format});

  final String? name;
  final String? genre;
  final String? description;
  final String? website;
  final int? bitrateKbps;

  /// The short name of the audio format, "MP3".
  final String? format;

  bool get isEmpty => [name, genre, description, website, format].every((e) => e == null || e.isEmpty) && bitrateKbps == null;

  @override
  bool operator ==(Object other) =>
      other is StreamDetails &&
      other.name == name &&
      other.genre == genre &&
      other.description == description &&
      other.website == website &&
      other.bitrateKbps == bitrateKbps &&
      other.format == format;

  @override
  int get hashCode => Object.hash(name, genre, description, website, bitrateKbps, format);
}

/// The announced title split in artist and track at the first " - ": ("Axel Volk", "Pfingsten ...").
/// Without that separator the whole text is the track.
({String? artist, String track}) splitStreamTitle(String title) {
  final i = title.indexOf(' - ');
  if (i <= 0 || i + 3 >= title.length) return (artist: null, track: title.trim());
  return (artist: title.substring(0, i).trim(), track: title.substring(i + 3).trim());
}
