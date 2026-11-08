// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/pictures/show_player.dart';
import 'package:psalm/pictures/show_project.dart';
import 'package:psalm/pictures/show_ui.dart';

/// An unpacked project of three pictures (the second a video) and one track.
Future<ShowProject> project(Directory dir, {String order = 'date', String info = 'both', bool music = true}) async {
  for (final f in ['pictures/0001.jpg', 'pictures/0002.jpg', 'pictures/0003.jpg', 'videos/0002.mp4', 'music/01.mp3']) {
    File('${dir.path}/$f')
      ..createSync(recursive: true)
      ..writeAsBytesSync([0]);
  }
  File('${dir.path}/show.json').writeAsStringSync(jsonEncode({
    'format': 'david-slideshow',
    'version': 1,
    'title': 'Allgäu',
    'made_at': 1,
    'settings': {'order': order, 'seconds': 5, 'transition': 'cut', 'videos': 'on', 'info': info},
    'pictures': [
      {'image': 'pictures/0001.jpg', 'taken_at': DateTime(2017, 7, 1, 12).millisecondsSinceEpoch, 'place': 'Oberstdorf, Deutschland'},
      {'image': 'pictures/0002.jpg', 'video': 'videos/0002.mp4', 'taken_at': DateTime(2018, 8, 2, 12).millisecondsSinceEpoch},
      {'image': 'pictures/0003.jpg', 'taken_at': DateTime(2019, 9, 3, 12).millisecondsSinceEpoch, 'place': 'Kempten'},
    ],
    'music': music ? [{'file': 'music/01.mp3', 'title': 'Air', 'artist': 'Bach'}] : [],
  }));
  return ShowProject.open(dir);
}

Widget app(Widget home) => MaterialApp(
      locale: const Locale('de'),
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: home,
    );

void main() {
  late Directory dir;
  setUp(() => dir = Directory.systemTemp.createTempSync('player'));
  tearDown(() => dir.deleteSync(recursive: true));

  String date() => (find.byKey(const ValueKey('show-date')).evaluate().single.widget as Text).data!;

  testWidgets('the pictures go on by themselves with the date and the place; the keys, the buttons and a pause do what the browser does',
      (tester) async {
    final p = (await tester.runAsync(() => project(dir)))!;
    final music = FakeBackend();
    await tester.pumpWidget(app(ShowPlayerScreen(project: p, musicEngine: () => music)));
    await tester.pump();
    expect(date(), '1. Juli 2017');
    expect(find.text('Oberstdorf, Deutschland'), findsOneWidget);
    expect(music.loadedPath, endsWith('music/01.mp3'), reason: 'the music of the project plays behind it');

    // Five seconds on; a video where videos cannot play here shows as its picture, without a place.
    await tester.pump(const Duration(seconds: 5));
    await tester.pump();
    expect(date(), '2. August 2018');
    expect(find.byKey(const ValueKey('show-place')), findsNothing);
    await tester.pump(const Duration(seconds: 5));
    await tester.pump();
    expect(date(), '3. September 2019');

    // Back with the button (a tap first shows the controls, which hid after three seconds), and round to the end with the arrow of a
    // keyboard.
    await tester.tapAt(const Offset(200, 200));
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('show-prev')));
    await tester.pump();
    expect(date(), '2. August 2018');
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    await tester.pump();
    expect(date(), '1. Juli 2017', reason: 'after the last picture the first comes again');

    // Paused, nothing goes on; going on again, it does.
    await tester.tap(find.byKey(const ValueKey('show-pause')));
    await tester.pump();
    expect(find.text('Angehalten'), findsOneWidget);
    await tester.pump(const Duration(seconds: 12));
    expect(date(), '1. Juli 2017');
    expect(music.state.name, 'paused');
    await tester.sendKeyEvent(LogicalKeyboardKey.space);
    await tester.pump(const Duration(seconds: 5));
    await tester.pump();
    expect(date(), '2. August 2018');
    expect(music.state.name, 'playing');

    // The controls hide after three seconds and come back with a tap.
    await tester.pump(const Duration(seconds: 3));
    await tester.pump(const Duration(milliseconds: 400));
    expect(tester.widget<AnimatedOpacity>(find.ancestor(of: find.byKey(const ValueKey('show-next')), matching: find.byType(AnimatedOpacity))).opacity, 0);
    await tester.tapAt(const Offset(200, 200));
    await tester.pump();
    expect(tester.widget<AnimatedOpacity>(find.ancestor(of: find.byKey(const ValueKey('show-next')), matching: find.byType(AnimatedOpacity))).opacity, 1);

    await tester.pumpWidget(const SizedBox());
    await tester.pump(const Duration(seconds: 1));
  });

  testWidgets('a swipe goes on, a swipe down ends the show', (tester) async {
    final p = (await tester.runAsync(() => project(dir, info: 'date', music: false)))!;
    await tester.pumpWidget(app(Builder(
      builder: (context) => Scaffold(
        body: Center(child: TextButton(onPressed: () => ShowPlayerScreen.open(context, p), child: const Text('go'))),
      ),
    )));
    await tester.tap(find.text('go'));
    await tester.pumpAndSettle(const Duration(milliseconds: 100));
    expect(date(), '1. Juli 2017');
    expect(find.byKey(const ValueKey('show-place')), findsNothing, reason: 'only the date was chosen');
    await tester.fling(find.byType(ShowPlayerScreen), const Offset(-300, 0), 1000);
    await tester.pump();
    expect(date(), '2. August 2018');
    await tester.fling(find.byType(ShowPlayerScreen), const Offset(0, 400), 1500);
    await tester.pumpAndSettle(const Duration(milliseconds: 100));
    expect(find.byType(ShowPlayerScreen), findsNothing);
    expect(find.text('go'), findsOneWidget);
  });

  testWidgets('shuffled, every picture comes once before one comes again', (tester) async {
    final p = (await tester.runAsync(() => project(dir, order: 'random', info: 'date', music: false)))!;
    await tester.pumpWidget(app(ShowPlayerScreen(project: p, random: Random(7))));
    await tester.pump();
    final seen = <String>{date()};
    for (var i = 0; i < 2; i++) {
      await tester.pump(const Duration(seconds: 5));
      await tester.pump();
      seen.add(date());
    }
    expect(seen, hasLength(3));
    await tester.pumpWidget(const SizedBox());
    await tester.pump(const Duration(seconds: 1));
  });

  testWidgets('the rubric lists the slideshows of the device and deletes one after asking', (tester) async {
    final root = Directory('${dir.path}/slideshows');
    final p = (await tester.runAsync(() async {
      await project(Directory('${root.path}/show-1')..createSync(recursive: true));
      return null;
    }));
    expect(p, isNull);
    final c = ShowController(projects: ShowProjects(root: root));
    await tester.runAsync(c.load);
    await tester.pumpWidget(app(Scaffold(body: CustomScrollView(slivers: [PicturesBody(controller: c)]))));
    await tester.pump();
    expect(find.text('Allgäu'), findsOneWidget);
    expect(find.textContaining('3 Bilder'), findsOneWidget);
    expect(find.textContaining('mit Musik'), findsOneWidget);
    expect(find.byKey(const ValueKey('show-new')), findsNothing, reason: 'without a server there is no new one, the old ones play');
    await tester.tap(find.byKey(const ValueKey('show-delete-show-1')));
    await tester.pumpAndSettle();
    expect(find.textContaining('„Allgäu“ von diesem Gerät löschen?'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('confirm-removal')));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 200)));
    await tester.pump();
    expect(find.byKey(const ValueKey('show-none')), findsOneWidget);
    expect(Directory('${root.path}/show-1').existsSync(), isFalse);
  });
}
