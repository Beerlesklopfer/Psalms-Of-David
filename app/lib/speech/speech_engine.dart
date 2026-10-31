// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

/// The longest a sentence may take before the engine gives up on the speech process (the music is lowered meanwhile).
const Duration kSpeechTimeout = Duration(seconds: 30);

/// Speaks a short sentence aloud. The app talks only to this port; the platform decides how.
abstract class SpeechEngine {
  /// False where the platform has no voice (the announcement is then skipped and the notification stays).
  bool get available;

  /// Why [available] is false, as a short code for the settings page (`no_spd_say`, `not_supported`); null when available.
  String? get unavailableReason;

  /// Speaks [text] (in the language [locale], such as `de`, if the voice knows it). Completes when the sentence is spoken or was stopped.
  /// Never throws for a missing voice: it returns at once.
  Future<void> speak(String text, {String? locale});

  /// Stops what is spoken now.
  Future<void> stop();
}

/// A started speech process; replaceable in tests.
abstract class SpeechProcess {
  Future<int> get exitCode;
  void kill();
}

/// Starts a program with separate arguments (never through a shell); replaceable in tests.
typedef SpeechProcessStarter = Future<SpeechProcess> Function(
  String executable,
  List<String> arguments,
);

class _RealProcess implements SpeechProcess {
  _RealProcess(this._p);
  final Process _p;
  @override
  Future<int> get exitCode => _p.exitCode;
  @override
  void kill() => _p.kill();
}

Future<SpeechProcess> _startReal(String exe, List<String> args) async =>
    _RealProcess(await Process.start(exe, args, runInShell: false));

bool _onPath(String name) {
  final path = Platform.environment['PATH'] ?? '';
  for (final dir in path.split(':')) {
    if (dir.isEmpty) continue;
    if (File('$dir/$name').existsSync()) return true;
  }
  return false;
}

/// Linux: `spd-say` of speech-dispatcher. The text is one separate argument after `--` (so a text that starts with `-` is no option),
/// `-w` waits until it is spoken. Without the program the engine is not available.
class SpdSayEngine implements SpeechEngine {
  SpdSayEngine({
    SpeechProcessStarter? start,
    bool Function()? find,
    this.timeout = kSpeechTimeout,
  }) : _start = start ?? _startReal,
       available = (find ?? () => _onPath('spd-say'))();

  /// The name of the program.
  static const executable = 'spd-say';

  final SpeechProcessStarter _start;
  final Duration timeout;
  SpeechProcess? _running;

  @override
  final bool available;

  @override
  String? get unavailableReason => available ? null : 'no_spd_say';

  /// The arguments for a sentence, exposed for tests.
  static List<String> argumentsFor(String text, String? locale) => [
    '-w',
    if (locale != null && locale.isNotEmpty) ...['-l', locale],
    '--',
    text,
  ];

  @override
  Future<void> speak(String text, {String? locale}) async {
    if (!available || text.trim().isEmpty) return;
    try {
      final p = await _start(executable, argumentsFor(text, locale));
      _running = p;
      await p.exitCode.timeout(
        timeout,
        onTimeout: () {
          p.kill();
          return -1;
        },
      );
    } on Object {
      // A voice that fails is no reason to fail: the notification is there.
    } finally {
      _running = null;
    }
  }

  @override
  Future<void> stop() async {
    _running?.kill();
  }
}

/// Platforms without a voice in the app. The mobile engines are not built yet: Android would wrap `TextToSpeech` and iOS
/// `AVSpeechSynthesizer`, for instance through the `flutter_tts` plugin, behind this same port.
class NoSpeechEngine implements SpeechEngine {
  const NoSpeechEngine([this.unavailableReason = 'not_supported']);

  @override
  bool get available => false;

  @override
  final String? unavailableReason;

  @override
  Future<void> speak(String text, {String? locale}) async {}

  @override
  Future<void> stop() async {}
}

/// The engine of this platform.
SpeechEngine platformSpeechEngine() =>
    Platform.isLinux ? SpdSayEngine() : const NoSpeechEngine();
