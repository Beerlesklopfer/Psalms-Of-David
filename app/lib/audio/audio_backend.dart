// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
/// What the player needs from a sound engine. The app talks only to this interface;
/// behind it sits PipeWire on Linux (`PipeWireBackend`), and later ExoPlayer on
/// Android and AVPlayer on iPhone. `FakeBackend` serves tests.
enum BackendState { idle, playing, paused, ended, failed }

class AudioException implements Exception {
  AudioException(this.message);

  final String message;

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

  BackendState get state;

  /// Where the listening is. Counts the audio handed to the sound system, so it
  /// can be a little ahead of what is audible.
  Duration get position;

  /// Length of the file, or null if the file does not say.
  Duration? get duration;

  double get volume;

  double get speed;

  /// Releases the engine. Using the backend afterwards is an error; calling this
  /// twice is fine.
  void dispose();
}
