// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:collection';

import 'package:flutter/foundation.dart';

import '../audio/audio_backend.dart';
import '../audio/playback_settings.dart';
import '../inbox/inbox_controller.dart';
import '../inbox/inbox_models.dart';
import '../l10n/app_localizations.dart';
import 'speech_decision.dart';
import 'speech_engine.dart';

/// To what share of the user's volume the music is lowered while a sentence is spoken (a setting may override it).
const double kSpeechDuckLevel = 0.25;

/// How long lowering and raising the music take; the volume goes in [kSpeechFadeSteps] steps.
const Duration kSpeechFade = Duration(milliseconds: 250);
const int kSpeechFadeSteps = 5;

/// How many sentences wait for their turn; when there are more, the oldest is dropped.
const int kSpeechQueueMax = 3;

/// The master volume of the player, the only thing ducking touches (no pause, no seek, so playback stays gapless).
abstract class DuckTarget {
  double get volume;
  void setVolume(double volume);
}

/// The volume of the audio backend as a [DuckTarget].
class BackendDuckTarget implements DuckTarget {
  BackendDuckTarget(this.backend);
  final AudioBackend backend;
  @override
  double get volume => backend.volume;
  @override
  void setVolume(double volume) => backend.setVolume(volume);
}

/// What the announcer asks of the app each time (so the objects it depends on can change under it).
class AnnouncerEnv {
  const AnnouncerEnv({required this.context, required this.l10n, this.duck});
  final SpeechContext Function() context;
  final AppLocalizations? Function() l10n;
  final DuckTarget? Function()? duck;
}

/// Speaks the messages of the CD inbox while something plays: decides ([whyNotSpoken]), queues, lowers the music, speaks, and raises the
/// music again. The second handler next to the notification (`InboxController.messageHandlers`). The setting "CD announcements" lives
/// here and in the playback settings file; [locale] is the language code for the voice.
class CdAnnouncer extends ChangeNotifier {
  CdAnnouncer({
    required this.engine,
    required this.env,
    this.store,
    this.fade = kSpeechFade,
    this.queueMax = kSpeechQueueMax,
  });

  final SpeechEngine engine;
  final AnnouncerEnv env;
  final PlaybackSettingsStore? store;
  final Duration fade;
  final int queueMax;

  final _spoken = <int>{};
  final _queue = Queue<String>();
  bool _running = false;
  bool _disposed = false;
  DuckTarget? _ducked;
  double? _before;
  bool? _enabled;

  /// The switch "CD announcements": on unless the person turned it off.
  bool get enabled => _enabled ??= store?.readAnnounceCds() ?? true;

  void setEnabled(bool v) {
    _enabled = v;
    store?.writeAnnounceCds(v);
    notifyListeners();
  }

  /// The share of the volume the music keeps while a sentence is spoken.
  double get duckLevel =>
      (store?.readDuckLevel() ?? kSpeechDuckLevel).clamp(0.0, 1.0);

  bool get available => engine.available;

  /// Registers on the inbox as a handler next to the notification.
  void attach(InboxController inbox) => inbox.messageHandlers.add(onMessage);

  void detach(InboxController inbox) => inbox.messageHandlers.remove(onMessage);

  /// A new message of the inbox arrived.
  void onMessage(InboxItem item, String? speak, String? device) {
    if (_disposed || !engine.available) return;
    final c = env.context();
    if (whyNotSpoken(
          item,
          speak,
          device,
          SpeechContext(
            playing: c.playing,
            ownDevice: c.ownDevice,
            rights: c.rights,
            role: c.role,
            enabled: enabled,
            atHome: c.atHome,
          ),
          _spoken,
        ) !=
        null) {
      return;
    }
    final l = env.l10n();
    if (l == null) return;
    final text = spokenText(l, item);
    if (text.isEmpty) return;
    _spoken.add(item.id);
    say(text, locale: l.localeName);
  }

  /// Speaks [text] after what is spoken now. The oldest waiting sentence is dropped when more than [queueMax] wait.
  void say(String text, {String? locale}) {
    if (_disposed) return;
    _queue.add(text);
    while (_queue.length > queueMax) {
      _queue.removeFirst();
    }
    _locale = locale;
    if (!_running) unawaited(_drain());
  }

  String? _locale;

  /// The sample sentence of the settings page; speaks at once, whatever plays.
  Future<void> test(AppLocalizations l) async {
    if (!engine.available) return;
    say(l.cdAnnounceTest, locale: l.localeName);
    await idle;
  }

  Completer<void>? _idle;

  /// Completes when nothing is spoken or waiting.
  Future<void> get idle =>
      _running ? (_idle ??= Completer<void>()).future : Future.value();

  Future<void> _drain() async {
    _running = true;
    try {
      while (_queue.isNotEmpty && !_disposed) {
        final text = _queue.removeFirst();
        await _duck();
        try {
          await engine.speak(text, locale: _locale);
        } on Object {
          // The voice failed: the music is raised again below, the notification was shown anyway.
        }
      }
    } finally {
      await _raise();
      _running = false;
      final c = _idle;
      _idle = null;
      if (c != null && !c.isCompleted) c.complete();
    }
  }

  Future<void> _duck() async {
    if (_ducked != null) return;
    final t = env.duck?.call();
    if (t == null) return;
    _ducked = t;
    _before = t.volume;
    await _ramp(t, _before!, _before! * duckLevel);
  }

  Future<void> _raise() async {
    final t = _ducked, before = _before;
    _ducked = null;
    _before = null;
    if (t == null || before == null) return;
    // Back to exactly the volume the person had, also when the fade is cut short.
    await _ramp(t, t.volume, before);
    t.setVolume(before);
  }

  Future<void> _ramp(DuckTarget t, double from, double to) async {
    if (fade == Duration.zero || _disposed) {
      t.setVolume(to);
      return;
    }
    final step = fade ~/ kSpeechFadeSteps;
    for (var i = 1; i <= kSpeechFadeSteps; i++) {
      t.setVolume(from + (to - from) * i / kSpeechFadeSteps);
      await Future<void>.delayed(step);
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _queue.clear();
    unawaited(engine.stop());
    // The music must not stay low when the app ends in the middle of a sentence.
    final t = _ducked, before = _before;
    if (t != null && before != null) t.setVolume(before);
    _ducked = null;
    super.dispose();
  }
}
