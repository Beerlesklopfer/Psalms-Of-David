// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:intl/intl.dart';

import 'l10n/app_localizations.dart';

/// 3:07 or 1:02:09. Time values of this kind are the same in all languages.
String formatDuration(Duration d) {
  final s = d.inSeconds.remainder(60).toString().padLeft(2, '0');
  if (d.inHours > 0) {
    final m = d.inMinutes.remainder(60).toString().padLeft(2, '0');
    return '${d.inHours}:$m:$s';
  }
  return '${d.inMinutes}:$s';
}

/// Total running time written out, depending on the language ("14 h 20 min" or
/// "14 Std. 20 Min.").
String formatTotal(AppLocalizations l, Duration d) {
  final m = d.inMinutes.remainder(60);
  return d.inHours > 0 ? l.durationHoursMinutes(d.inHours, m) : l.durationMinutes(d.inMinutes);
}

/// Playback speed with the decimal separator of the language: "1.5×" / "1,5×".
String formatSpeed(String locale, double speed) => '${NumberFormat('0.##', locale).format(speed)}×';

/// A size in bytes as people read it: "812 KB", "1.4 GB" / "1,4 GB" (powers of 1000, like the
/// disk sizes of the system).
String formatBytes(String locale, int bytes) {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  var value = bytes.toDouble();
  var unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  final digits = unit == 0 || value >= 100 ? '0' : '0.#';
  return '${NumberFormat(digits, locale).format(value)} ${units[unit]}';
}
