// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/demo_data.dart';
import 'package:psalm/player_controller.dart';

void main() {
  testWidgets(
    'a tick moves the position listenable without notifying the controller',
    (tester) async {
      final p = PlayerController(demoItems)..toggle();
      var coarse = 0;
      p.addListener(() => coarse++);
      final seen = <Duration>[];
      p.positionListenable.addListener(
        () => seen.add(p.positionListenable.value),
      );
      final start = p.position;

      await tester.pump(const Duration(milliseconds: 500));
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pump(const Duration(milliseconds: 500));

      expect(seen.length, 3);
      expect(p.positionListenable.value, p.position);
      expect(p.position, greaterThan(start));
      expect(coarse, 0, reason: 'a plain position tick is not a coarse change');
      p.dispose();
    },
  );

  testWidgets('only the widget on the position listenable rebuilds on a tick', (
    tester,
  ) async {
    final p = PlayerController(demoItems)..toggle();
    var coarseBuilds = 0, positionBuilds = 0;
    await tester.pumpWidget(
      Directionality(
        textDirection: TextDirection.ltr,
        child: Column(
          children: [
            ListenableBuilder(
              listenable: p,
              builder: (_, _) {
                coarseBuilds++;
                return Text('${p.playing}');
              },
            ),
            ValueListenableBuilder<Duration>(
              valueListenable: p.positionListenable,
              builder: (_, d, _) {
                positionBuilds++;
                return Text('${d.inSeconds}');
              },
            ),
          ],
        ),
      ),
    );
    final c0 = coarseBuilds, p0 = positionBuilds;

    for (var i = 0; i < 4; i++) {
      await tester.pump(const Duration(milliseconds: 500));
    }
    expect(positionBuilds - p0, 4);
    expect(coarseBuilds, c0);

    p.toggle(); // a coarse change still reaches the controller listeners
    await tester.pump();
    expect(coarseBuilds, c0 + 1);
    p.dispose();
  });
}
