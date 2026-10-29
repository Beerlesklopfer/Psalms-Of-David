// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/demo_data.dart';
import 'package:psalm/widgets.dart';

Widget _app(PlayerController p, Widget child) => PlayerScope(
  controller: p,
  child: MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: Scaffold(
      body: Align(
        alignment: Alignment.bottomCenter,
        child: SizedBox(width: 400, child: child),
      ),
    ),
  ),
);

void main() {
  bookTests();
  testWidgets('the thin bar of the mini player is a seek bar: a tap jumps there', (tester) async {
    final p = PlayerController(demoBooks)..seek(Duration.zero);
    await tester.pumpWidget(_app(p, const SeekBar(compact: true)));
    final box = tester.getRect(find.byType(Slider));
    await tester.tapAt(Offset(box.left + box.width * 0.75, box.center.dy));
    await tester.pumpAndSettle();
    final length = p.track!.length.inMilliseconds;
    expect(
      p.position.inMilliseconds,
      closeTo(length * 0.75, length * 0.06),
      reason: 'about three quarters',
    );
    p.dispose();
  });

  testWidgets('dragging follows the finger and the jump is made when it is let go', (tester) async {
    final p = PlayerController(demoBooks)..seek(Duration.zero);
    await tester.pumpWidget(_app(p, const SeekBar()));
    final box = tester.getRect(find.byType(Slider));
    final gesture = await tester.startGesture(Offset(box.left + 20, box.center.dy));
    await gesture.moveTo(Offset(box.left + box.width * 0.5, box.center.dy));
    await tester.pump();
    final held = tester.widget<Slider>(find.byType(Slider)).value;
    expect(held, greaterThan(0));
    // The player reports an old position meanwhile (like the engine does): the bar must not jump back.
    p.seek(const Duration(seconds: 1));
    await tester.pump();
    expect(
      tester.widget<Slider>(find.byType(Slider)).value,
      held,
      reason: 'it stays under the finger',
    );
    await gesture.up();
    await tester.pumpAndSettle();
    expect(p.position.inMilliseconds, held.round(), reason: 'let go: the jump is made');
    p.dispose();
  });

  testWidgets('the mini player has it, and without a book there is nothing', (tester) async {
    final p = PlayerController(demoBooks);
    await tester.pumpWidget(_app(p, const MiniPlayer()));
    expect(find.byType(SeekBar), findsOneWidget);
    expect(
      find.byType(LinearProgressIndicator),
      findsNothing,
      reason: 'no display-only bar any more',
    );
    p.dispose();
    final empty = PlayerController(const []);
    await tester.pumpWidget(_app(empty, const SeekBar(compact: true)));
    expect(find.byType(Slider), findsNothing);
    empty.dispose();
  });
}

void bookTests() {
  testWidgets('the bar of the mini player covers the whole book and jumps across tracks', (
    tester,
  ) async {
    final p = PlayerController(demoBooks)..seek(Duration.zero);
    final book = p.book!;
    await tester.pumpWidget(_app(p, const MiniPlayer()));
    expect(p.bookTotal, book.total);
    final box = tester.getRect(find.byType(Slider));
    await tester.tapAt(Offset(box.left + box.width * 0.5, box.center.dy));
    await tester.pumpAndSettle();
    final half = book.total.inMilliseconds / 2;
    expect(
      p.bookPosition.inMilliseconds,
      closeTo(half, book.total.inMilliseconds * 0.06),
      reason: 'half way through the whole book',
    );
    expect(p.trackIndex, greaterThan(0), reason: 'in a later track, not in the first');
    // The mini player shows the played and the total time.
    expect(find.textContaining(' / '), findsWidgets);
    p.dispose();
  });

  test('a place in the book is a track and a place in it', () {
    final p = PlayerController(demoBooks);
    final b = p.book!;
    p.seekBook(Duration.zero);
    expect((p.trackIndex, p.position), (0, Duration.zero));
    final first = b.tracks[0].length;
    p.seekBook(first + const Duration(seconds: 5));
    expect((p.trackIndex, p.position), (1, const Duration(seconds: 5)));
    expect(p.bookPosition, first + const Duration(seconds: 5));
    p.seekBook(b.total * 2);
    expect(p.trackIndex, b.tracks.length - 1);
    expect(p.position, b.tracks.last.length, reason: 'beyond the end is the end');
    p.seekBook(const Duration(days: -1));
    expect((p.trackIndex, p.position), (0, Duration.zero));
    p.dispose();
  });
}
