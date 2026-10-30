// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:intl/intl.dart' show DateFormat;

import '../l10n/app_localizations.dart';
import '../server/server_source.dart';

/// The media time of one member of the family as a line chart: a line for every kind of medium and a thicker one for all together, the time of
/// every day on the vertical axis (minutes, or hours when it is more than two hours).
class MediaTimeChart extends StatelessWidget {
  const MediaTimeChart({super.key, required this.time});

  final FamilyTime time;

  static const _order = ['audiobook', 'music', 'podcast'];

  String _name(AppLocalizations l, String kind) => switch (kind) {
        'audiobook' => l.filterBooks,
        'music' => l.filterMusic,
        'podcast' => l.navPodcasts,
        _ => kind,
      };

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    final kinds = [for (final k in _order) if (time.kinds.containsKey(k)) k, for (final k in time.kinds.keys) if (!_order.contains(k)) k];
    final colours = [cs.secondary, cs.tertiary, const Color(0xFF2C7A7B), cs.outline];
    final series = <_Line>[
      for (var i = 0; i < kinds.length; i++) _Line(_name(l, kinds[i]), time.kinds[kinds[i]]!, colours[i % colours.length], 2),
      _Line(l.chartTotal, time.total, cs.primary, 3.5),
    ];
    final empty = time.total.every((ms) => ms == 0);
    return Card(
      margin: EdgeInsets.zero,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 12, 16, 12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              height: 160,
              child: empty
                  ? Center(child: Text(l.chartNothing, style: TextStyle(color: cs.onSurfaceVariant)))
                  : CustomPaint(
                      size: Size.infinite,
                      painter: _ChartPainter(
                        series: series,
                        fromDay: time.fromDay,
                        axis: cs.outlineVariant,
                        text: cs.onSurfaceVariant,
                        locale: Localizations.localeOf(context).toString(),
                        minutes: l.chartMinutes,
                        hours: l.chartHours,
                      ),
                    ),
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 16,
              runSpacing: 4,
              children: [
                for (final s in series)
                  Row(mainAxisSize: MainAxisSize.min, children: [
                    Container(width: 18, height: s.width, decoration: BoxDecoration(color: s.colour, borderRadius: BorderRadius.circular(2))),
                    const SizedBox(width: 6),
                    Text(s.label, style: Theme.of(context).textTheme.bodySmall),
                  ]),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _Line {
  const _Line(this.label, this.values, this.colour, this.width);

  final String label;
  final List<int> values;
  final Color colour;
  final double width;
}

class _ChartPainter extends CustomPainter {
  _ChartPainter({required this.series, required this.fromDay, required this.axis, required this.text, required this.locale, required this.minutes, required this.hours});

  final List<_Line> series;
  final int fromDay;
  final Color axis;
  final Color text;
  final String locale;
  final String Function(int) minutes;
  final String Function(int) hours;

  /// A round top of the vertical axis for [maxMs]: 10, 20, 30, 60, 120 minutes, then whole hours.
  static int _top(int maxMs) {
    final min = maxMs / 60000;
    for (final step in [10, 20, 30, 60, 120, 180, 240, 360, 480, 720]) {
      if (min <= step) return step;
    }
    return (min / 60).ceil() * 60;
  }

  String _label(int min) => min >= 120 && min % 60 == 0 ? hours(min ~/ 60) : minutes(min);

  void _text(Canvas canvas, String s, Offset at, {bool right = false, bool centre = false}) {
    final p = TextPainter(text: TextSpan(text: s, style: TextStyle(fontSize: 11, color: text)), textDirection: TextDirection.ltr)..layout();
    final dx = right ? at.dx - p.width : (centre ? at.dx - p.width / 2 : at.dx);
    p.paint(canvas, Offset(dx, at.dy - p.height / 2));
  }

  @override
  void paint(Canvas canvas, Size size) {
    const left = 52.0, bottom = 22.0, top = 6.0;
    final w = size.width - left, h = size.height - bottom - top;
    final days = series.first.values.length;
    final maxMs = series.fold<int>(0, (m, s) => math.max(m, s.values.fold(0, math.max)));
    final topMin = _top(maxMs);
    final grid = Paint()..color = axis..strokeWidth = 1;
    // Three horizontal lines with their time, and the base line.
    for (var i = 0; i <= 3; i++) {
      final y = top + h - h * i / 3;
      canvas.drawLine(Offset(left, y), Offset(size.width, y), grid);
      _text(canvas, _label((topMin * i / 3).round()), Offset(left - 6, y), right: true);
    }
    double x(int i) => left + (days == 1 ? w / 2 : w * i / (days - 1));
    double y(int ms) => top + h - h * (ms / 60000) / topMin;
    // The days below: about seven marks.
    final every = (days / 7).ceil();
    for (var i = 0; i < days; i += every) {
      final date = DateTime.fromMillisecondsSinceEpoch((fromDay + i) * 86400000, isUtc: true);
      _text(canvas, DateFormat.Md(locale).format(date), Offset(x(i), size.height - bottom / 2 + 2), centre: true);
    }
    for (final s in series.reversed) {
      final path = Path();
      for (var i = 0; i < days; i++) {
        final point = Offset(x(i), y(s.values[i]));
        i == 0 ? path.moveTo(point.dx, point.dy) : path.lineTo(point.dx, point.dy);
      }
      canvas.drawPath(
        path,
        Paint()
          ..color = s.colour
          ..style = PaintingStyle.stroke
          ..strokeWidth = s.width
          ..strokeJoin = StrokeJoin.round
          ..strokeCap = StrokeCap.round,
      );
    }
  }

  @override
  bool shouldRepaint(_ChartPainter old) => old.series != series || old.fromDay != fromDay || old.axis != axis || old.text != text;
}
