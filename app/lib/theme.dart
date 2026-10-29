// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

/// Colors: ink, parchment, muted gold and burgundy. No blue-violet gradient and
/// no crosses or stained-glass motifs; the character comes from color and type.
class Palette {
  static const ink = Color(0xFF121016);
  static const surface = Color(0xFF1C1921);
  static const surfaceHigh = Color(0xFF26222C);
  static const parchment = Color(0xFFEFE7D6);
  static const muted = Color(0xFFA79E8E);
  static const gold = Color(0xFFC4A265);
  static const bordeaux = Color(0xFF7B2332);

  /// The check mark of a track that was heard to the end. A fixed green, readable on both the dark and the paper surface.
  static const played = Color(0xFF2E9E5B);

  static const paper = Color(0xFFF6F0E3);
  static const paperSurface = Color(0xFFEDE5D3);
  static const paperHigh = Color(0xFFE4DAC4);
  static const inkText = Color(0xFF241F1A);
  static const inkMuted = Color(0xFF6B6255);
}

/// Serif fonts for headings. Nothing is downloaded; if the first font is missing
/// on the device, Flutter falls back to the next one.
const serifFallback = ['Georgia', 'Source Serif 4', 'Noto Serif', 'DejaVu Serif', 'serif'];

class PsalmTheme {
  static final dark = _build(
    const ColorScheme.dark(
      surface: Palette.ink,
      onSurface: Palette.parchment,
      onSurfaceVariant: Palette.muted,
      primary: Palette.gold,
      onPrimary: Palette.ink,
      secondary: Palette.bordeaux,
      onSecondary: Palette.parchment,
      surfaceContainer: Palette.surface,
      surfaceContainerHigh: Palette.surfaceHigh,
      outlineVariant: Color(0x33A79E8E),
    ),
  );

  static final light = _build(
    const ColorScheme.light(
      surface: Palette.paper,
      onSurface: Palette.inkText,
      onSurfaceVariant: Palette.inkMuted,
      primary: Palette.bordeaux,
      onPrimary: Palette.paper,
      secondary: Palette.gold,
      onSecondary: Palette.inkText,
      surfaceContainer: Palette.paperSurface,
      surfaceContainerHigh: Palette.paperHigh,
      outlineVariant: Color(0x336B6255),
    ),
  );

  static ThemeData _build(ColorScheme scheme) {
    final base = ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      scaffoldBackgroundColor: scheme.surface,
    );
    final text = base.textTheme.apply(bodyColor: scheme.onSurface, displayColor: scheme.onSurface);
    TextStyle? serif(TextStyle? s) =>
        s?.copyWith(fontFamilyFallback: serifFallback, fontWeight: FontWeight.w600);

    return base.copyWith(
      textTheme: text.copyWith(
        headlineMedium: serif(text.headlineMedium),
        headlineSmall: serif(text.headlineSmall),
        titleLarge: serif(text.titleLarge),
      ),
      navigationBarTheme: NavigationBarThemeData(
        backgroundColor: scheme.surfaceContainer,
        indicatorColor: scheme.secondary.withValues(alpha: 0.35),
        labelTextStyle: WidgetStatePropertyAll(TextStyle(fontSize: 12, color: scheme.onSurface)),
      ),
      navigationRailTheme: NavigationRailThemeData(
        backgroundColor: scheme.surfaceContainer,
        indicatorColor: scheme.secondary.withValues(alpha: 0.35),
      ),
      sliderTheme: SliderThemeData(
        trackHeight: 3,
        activeTrackColor: scheme.primary,
        inactiveTrackColor: scheme.onSurfaceVariant.withValues(alpha: 0.3),
        thumbColor: scheme.primary,
        overlayColor: scheme.primary.withValues(alpha: 0.15),
        thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 6),
      ),
      chipTheme: ChipThemeData(
        backgroundColor: scheme.surfaceContainerHigh,
        selectedColor: scheme.secondary.withValues(alpha: 0.45),
        side: BorderSide.none,
        labelStyle: TextStyle(color: scheme.onSurface),
      ),
      dividerTheme: DividerThemeData(color: scheme.outlineVariant, space: 1),
    );
  }
}
