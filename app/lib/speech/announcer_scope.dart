// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../audio/playback_settings.dart';
import '../cd/cd_kit.dart';
import '../inbox/inbox_models.dart';
import '../l10n/app_localizations.dart';
import '../player_controller.dart';
import '../profile.dart';
import 'cd_announcer.dart';
import 'speech_decision.dart';
import 'speech_engine.dart';

/// Gives the pages the announcer (the settings switch and the test).
class AnnouncerScope extends InheritedNotifier<CdAnnouncer> {
  const AnnouncerScope({super.key, required CdAnnouncer announcer, required super.child}) : super(notifier: announcer);

  static CdAnnouncer? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<AnnouncerScope>()?.notifier;
}

/// The announcer of the app shell: it reads the player, the profile and the language from the widget tree each time it decides
/// ([context] returns null once the shell is gone), so nothing here is cached. [engine] and [store] are replaceable in tests.
CdAnnouncer createShellAnnouncer(
  BuildContext? Function() context,
  CdKit? kit, {
  SpeechEngine? engine,
  PlaybackSettingsStore? store,
}) {
  PlayerController? player() => context()?.getInheritedWidgetOfExactType<PlayerScope>()?.notifier;
  Profile? profile() => context()?.getInheritedWidgetOfExactType<ProfileScope>()?.notifier?.profile;
  return CdAnnouncer(
    engine: engine ?? platformSpeechEngine(),
    store: store ?? PlaybackSettingsStore.standard(),
    env: AnnouncerEnv(
      context: () {
        final p = profile();
        return SpeechContext(
          playing: player()?.playing ?? false,
          ownDevice: kit?.ownDevice?.call(),
          rights: p?.cdRights ?? const CdRights(),
          role: p?.role ?? '',
          enabled: true,
          atHome: atHomeNetwork(),
        );
      },
      l10n: () {
        final c = context();
        return c == null ? null : Localizations.of<AppLocalizations>(c, AppLocalizations);
      },
      duck: () {
        final b = player()?.backend;
        return b == null ? null : BackendDuckTarget(b);
      },
    ),
  );
}
