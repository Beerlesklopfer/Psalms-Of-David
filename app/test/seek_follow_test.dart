// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/widgets.dart';

void main() {
  testWidgets('the bar grows while the player plays', (tester) async {
    var now = Duration.zero;
    final backend = FakeBackend(clock: () => now, fileLength: const Duration(minutes: 6));
    final book = Book(
      id: '1',
      title: 'B',
      author: 'A',
      color: const Color(0xFF000000),
      tracks: [Track(disc: 1, number: 1, title: 'T1', length: const Duration(minutes: 6), path: '/a.mp3')],
    );
    final p = PlayerController([book], backend: backend);
    addTearDown(p.dispose);
    await tester.pumpWidget(
      PlayerScope(
        controller: p,
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: const Scaffold(body: MiniPlayer()),
        ),
      ),
    );
    p.open(book, track: 0);
    await tester.pump();
    final start = tester.widget<Slider>(find.byType(Slider)).value;

    now = const Duration(seconds: 90);
    await tester.pump(const Duration(milliseconds: 600));
    final later = tester.widget<Slider>(find.byType(Slider)).value;
    expect(later, greaterThan(start + 80000), reason: 'the bar follows the position of the player');
    expect(later / const Duration(minutes: 6).inMilliseconds, closeTo(0.25, 0.02));

    now = const Duration(seconds: 180);
    await tester.pump(const Duration(milliseconds: 600));
    expect(tester.widget<Slider>(find.byType(Slider)).value, greaterThan(later + 80000));
    p.toggle();
  });
}
