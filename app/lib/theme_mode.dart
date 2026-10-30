// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import 'audio/playback_settings.dart';

/// The colour scheme the listener chose in the settings. It is dark until the listener changes it (as the app always was);
/// "system" follows the setting of the device.
final ValueNotifier<ThemeMode> themeModeNotifier = ValueNotifier(ThemeMode.dark);

/// Reads the stored choice at the start of the app.
void loadThemeMode([PlaybackSettingsStore? store]) {
  themeModeNotifier.value = (store ?? PlaybackSettingsStore.standard()).readThemeMode();
}

/// Switches the scheme at once and remembers the choice.
void setThemeMode(ThemeMode mode, [PlaybackSettingsStore? store]) {
  themeModeNotifier.value = mode;
  try {
    (store ?? PlaybackSettingsStore.standard()).writeThemeMode(mode);
  } on Object {
    // No place to remember it (demo, tests): the choice then holds until the app ends.
  }
}
