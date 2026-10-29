// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/playlists.dart';
import 'package:psalm/screens/book_screen.dart';
import 'package:psalm/screens/library_screen.dart';
import 'package:psalm/screens/playlists_screen.dart';
import 'package:psalm/server/api_client.dart';

import 'playlist_fakes.dart';

final _winter = Book(
  id: '17',
  title: 'Winter',
  author: 'Follett',
  color: Colors.black,
  tracks: const [
    Track(disc: 1, number: 1, title: 'Chapter 1', length: Duration(minutes: 4), partId: 880),
    Track(disc: 1, number: 2, title: 'Chapter 2', length: Duration(minutes: 4), partId: 881),
  ],
);

void main() {
  late FakePlaylistApi api;
  late PlaylistController c;
  late PlayerController player;

  setUp(() {
    api = FakePlaylistApi();
    c = PlaylistController(api: api);
    player = PlayerController([_winter]);
    player.refresher = () async {};
  });
  tearDown(() => c.dispose());

  Future<void> pumpApp(
    WidgetTester tester,
    Widget home, {
    Locale locale = const Locale('en'),
    bool scope = true,
  }) {
    tester.view.physicalSize = const Size(1400, 2400);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    Widget app = MaterialApp(
      locale: locale,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: home,
    );
    if (scope) app = PlaylistScope(controller: c, child: app);
    return tester.pumpWidget(PlayerScope(controller: player, child: app));
  }

  Future<void> settle(WidgetTester tester) async {
    for (var i = 0; i < 6; i++) {
      await tester.pump(const Duration(milliseconds: 50));
    }
  }

  Future<void> openPlaylists(WidgetTester tester, {Locale locale = const Locale('en')}) async {
    await pumpApp(tester, const Scaffold(body: LibraryScreen()), locale: locale);
    await tester.tap(find.widgetWithText(ChoiceChip, 'Playlists'));
    await settle(tester);
  }

  group('the library', () {
    testWidgets('the chip Playlists is in the first row after Radio, before Videos', (
      tester,
    ) async {
      await pumpApp(tester, const Scaffold(body: LibraryScreen()));
      final x = [
        for (final t in [
          'Music',
          'Audiobooks',
          'Podcasts',
          'Radio',
          'Playlists',
          'Videos',
          'E-Books',
        ])
          tester.getCenter(find.widgetWithText(ChoiceChip, t)).dx,
      ];
      expect(x, orderedEquals([...x]..sort()), reason: '$x');
      expect(find.byIcon(Icons.queue_music), findsWidgets);
    });

    testWidgets('the chip shows the lists with name, number of titles and running time', (
      tester,
    ) async {
      api.add('Car trip', [
        entry(1, 1, 880, 'Chapter 1'),
        entry(2, 2, 881, 'Chapter 2'),
        entry(3, 3, 0, '', available: false),
      ]);
      api.add('Bed time');
      await c.load();
      await openPlaylists(tester);
      expect(find.text('Car trip'), findsOneWidget);
      expect(
        find.text('2 titles · 8 min'),
        findsOneWidget,
        reason: 'the unavailable entry is not counted',
      );
      expect(find.text('Bed time'), findsOneWidget);
      expect(find.text('0 titles · 0 min'), findsOneWidget);
    });

    testWidgets('no lists yet: a hint; Playlists without the scope (demo) does not crash', (
      tester,
    ) async {
      await openPlaylists(tester);
      expect(find.text('No playlists yet. Tap + to make one.'), findsOneWidget);
      await pumpApp(tester, const Scaffold(body: LibraryScreen()), scope: false);
      await tester.tap(find.widgetWithText(ChoiceChip, 'Playlists'));
      await settle(tester);
      expect(find.text('No playlists yet. Tap + to make one.'), findsOneWidget);
    });

    testWidgets('German', (tester) async {
      api.add('Autofahrt');
      await c.load();
      await openPlaylists(tester, locale: const Locale('de'));
      expect(find.text('Autofahrt'), findsOneWidget);
      expect(find.text('0 Titel · 0 Min.'), findsOneWidget);
    });

    testWidgets(
      'New playlist: one field with Save and Cancel; the list appears; a taken name is told in words',
      (tester) async {
        await openPlaylists(tester);
        await tester.tap(find.byKey(const Key('playlist-new')));
        await tester.pumpAndSettle();
        expect(find.widgetWithText(TextButton, 'Cancel'), findsOneWidget);
        expect(find.widgetWithText(FilledButton, 'Save'), findsOneWidget);
        await tester.enterText(find.byType(TextField), 'Car trip');
        await tester.tap(find.widgetWithText(FilledButton, 'Save'));
        await settle(tester);
        expect(api.calls, contains('create Car trip'));
        expect(find.text('Car trip'), findsOneWidget);

        await tester.tap(find.byKey(const Key('playlist-new')));
        await tester.pumpAndSettle();
        await tester.enterText(find.byType(TextField), 'car trip');
        await tester.tap(find.widgetWithText(FilledButton, 'Save'));
        await settle(tester);
        expect(find.text('You have a playlist with this name already.'), findsOneWidget);
      },
    );

    testWidgets('Cancel makes nothing', (tester) async {
      await openPlaylists(tester);
      await tester.tap(find.byKey(const Key('playlist-new')));
      await tester.pumpAndSettle();
      await tester.tap(find.widgetWithText(TextButton, 'Cancel'));
      await tester.pumpAndSettle();
      expect(api.calls.where((x) => x.startsWith('create')), isEmpty);
    });
  });

  group('add to playlist', () {
    Future<void> openBook(
      WidgetTester tester, {
      Locale locale = const Locale('en'),
      bool scope = true,
    }) => pumpApp(
      tester,
      BookScreen(book: _winter),
      locale: locale,
      scope: scope,
    );

    testWidgets('the book and each title have the entry; without the server neither', (
      tester,
    ) async {
      await openBook(tester, scope: false);
      expect(find.byKey(const Key('book-add-to-playlist-17')), findsNothing);
      expect(find.byKey(const Key('track-menu-880')), findsNothing);
      c.api = null;
      await openBook(tester);
      expect(
        find.byKey(const Key('book-add-to-playlist-17')),
        findsNothing,
        reason: 'no server, no playlists to change',
      );
      c.api = api;
      await openBook(tester);
      await tester.pump();
      expect(find.byKey(const Key('book-add-to-playlist-17')), findsOneWidget);
      expect(
        tester.widget<IconButton>(find.byKey(const Key('book-add-to-playlist-17'))).tooltip,
        'Add to playlist',
      );
      expect(find.byKey(const Key('track-menu-880')), findsOneWidget);
    });

    testWidgets('a book: the sheet lists the playlists; a tap adds all its titles and says so', (
      tester,
    ) async {
      api.add('Car trip');
      api.parts.removeWhere((k, v) => v.bookId != 17);
      await c.load();
      await openBook(tester);
      await tester.tap(find.byKey(const Key('book-add-to-playlist-17')));
      await tester.pumpAndSettle();
      expect(find.text('Car trip'), findsOneWidget);
      expect(find.text('New playlist…'), findsOneWidget);
      await tester.tap(find.byKey(const Key('add-to-1')));
      await settle(tester);
      expect(api.calls, contains('addBook 1 17'));
      expect(find.text('Added to “Car trip”.'), findsOneWidget);
      expect(c.details[1]!.entries.length, 2);
    });

    testWidgets('a title: the menu has Add to playlist; the part id goes to the server', (
      tester,
    ) async {
      api.add('Car trip');
      await c.load();
      await openBook(tester);
      await tester.tap(find.byKey(const Key('track-menu-881')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Add to playlist'));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('add-to-1')));
      await settle(tester);
      expect(api.calls, contains('addParts 1 [881]'));
    });

    testWidgets(
      'New playlist… in the sheet: one field, Save / Cancel, then the list is made and the title is added',
      (tester) async {
        await openBook(tester);
        await tester.tap(find.byKey(const Key('track-menu-880')));
        await tester.pumpAndSettle();
        await tester.tap(find.text('Add to playlist'));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const Key('add-to-new')));
        await tester.pumpAndSettle();
        expect(find.widgetWithText(FilledButton, 'Save'), findsOneWidget);
        await tester.enterText(find.byType(TextField), 'Bed time');
        await tester.tap(find.widgetWithText(FilledButton, 'Save'));
        await settle(tester);
        expect(api.calls, containsAllInOrder(['create Bed time', 'addParts 1 [880]']));
        expect(find.text('Added to “Bed time”.'), findsOneWidget);
      },
    );

    testWidgets('a refusal is told in words: the list is full', (tester) async {
      api.add('Car trip');
      await c.load();
      api.failOnce = null;
      await openBook(tester);
      await tester.tap(find.byKey(const Key('book-add-to-playlist-17')));
      await tester.pumpAndSettle();
      api.failWith = ApiException(422, 'playlist_full');
      await tester.tap(find.byKey(const Key('add-to-1')));
      await settle(tester);
      expect(find.text('The playlist is full (at most 5000 titles).'), findsOneWidget);
      expect(find.textContaining('playlist_full'), findsNothing);
    });

    testWidgets('German texts', (tester) async {
      api.add('Autofahrt');
      await c.load();
      await openBook(tester, locale: const Locale('de'));
      expect(
        tester.widget<IconButton>(find.byKey(const Key('book-add-to-playlist-17'))).tooltip,
        'Zur Playlist hinzufügen',
      );
      await tester.tap(find.byKey(const Key('book-add-to-playlist-17')));
      await tester.pumpAndSettle();
      expect(find.text('Neue Playlist …'), findsOneWidget);
      api.failWith = ApiException(409, 'name_taken');
      await tester.tap(find.byKey(const Key('add-to-1')));
      await settle(tester);
      expect(find.text('Du hast schon eine Playlist mit diesem Namen.'), findsOneWidget);
    });
  });

  group('one playlist', () {
    Future<Playlist> prepare() async {
      final p = api.add('Car trip', [
        entry(1, 1, 880, 'Chapter 1'),
        entry(2, 2, 0, '', available: false),
        entry(3, 3, 900, 'Song', bookId: 20, book: 'Album', author: 'Band', seconds: 180),
      ]);
      await c.load();
      return p;
    }

    Future<void> open(WidgetTester tester, {Locale locale = const Locale('en')}) async {
      await openPlaylists(tester, locale: locale);
      await tester.tap(find.byKey(const Key('playlist-1')));
      await settle(tester);
    }

    testWidgets(
      'the entries in order with book and length; the unavailable one is grey and says so',
      (tester) async {
        await prepare();
        await open(tester);
        expect(find.text('Chapter 1'), findsOneWidget);
        expect(find.text('Winter · Follett'), findsOneWidget);
        expect(find.text('Not available'), findsOneWidget);
        expect(find.text('Song'), findsOneWidget);
        expect(find.text('3:00'), findsOneWidget);
        final y = [
          for (final t in ['Chapter 1', 'Not available', 'Song']) tester.getCenter(find.text(t)).dy,
        ];
        expect(y, orderedEquals([...y]..sort()));
        expect(tester.widget<ListTile>(find.byKey(const Key('entry-2'))).enabled, isFalse);
      },
    );

    testWidgets(
      'Play plays the available titles in order and skips the unavailable one; nothing is deleted',
      (tester) async {
        await prepare();
        await open(tester);
        await tester.tap(find.byKey(const Key('playlist-play')));
        await tester.pump();
        expect(player.book?.id, 'pl:1');
        expect(player.book?.tracks.map((t) => t.title), ['Chapter 1', 'Song']);
        player.next();
        expect(player.track?.title, 'Song');
        expect(c.details[1]!.entries.length, 3);
        player.toggle();
      },
    );

    testWidgets('a tap on a title plays from there; a tap on the unavailable one does nothing', (
      tester,
    ) async {
      await prepare();
      await open(tester);
      await tester.tap(find.byKey(const Key('entry-2')));
      await tester.pump();
      expect(player.book?.id, isNot('pl:1'));
      await tester.tap(find.byKey(const Key('entry-3')));
      await tester.pump();
      expect((player.book?.id, player.track?.title), ('pl:1', 'Song'));
      player.toggle();
    });

    testWidgets('remove an entry with its button', (tester) async {
      await prepare();
      await open(tester);
      await tester.tap(find.byKey(const Key('entry-remove-1')));
      await settle(tester);
      expect(api.calls, contains('remove 1 1'));
      expect(find.text('Chapter 1'), findsNothing);
      expect(find.text('Song'), findsOneWidget);
    });

    testWidgets('an entry that cannot be removed is told in words', (tester) async {
      await prepare();
      await open(tester);
      api.failWith = ApiException(404, 'not_found');
      await tester.tap(find.byKey(const Key('entry-remove-3')));
      await settle(tester);
      expect(find.text('The playlist is not there any more.'), findsOneWidget);
    });

    testWidgets('drag an entry to another place: the server gets the new order', (tester) async {
      await prepare();
      await open(tester);
      final handle = find.descendant(
        of: find.byKey(const Key('entry-1')),
        matching: find.byIcon(Icons.drag_handle),
      );
      await tester.drag(handle, const Offset(0, 130));
      await settle(tester);
      expect(api.calls.where((x) => x.startsWith('reorder')), isNotEmpty);
      expect(api.lists[1]!.entries.first.entryId, isNot(1));
      final y = [
        for (final t in ['Not available', 'Chapter 1', 'Song']) tester.getCenter(find.text(t)).dy,
      ];
      expect(
        y,
        orderedEquals([...y]..sort()),
        reason: 'Chapter 1 moved below the unavailable entry: $y',
      );
    });

    testWidgets('Rename: one field with the name in it, Save and Cancel', (tester) async {
      await prepare();
      await open(tester);
      await tester.tap(find.byKey(const Key('playlist-menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Rename'));
      await tester.pumpAndSettle();
      expect(find.widgetWithText(TextField, 'Car trip'), findsOneWidget);
      await tester.enterText(find.byType(TextField), 'Road trip');
      await tester.tap(find.widgetWithText(FilledButton, 'Save'));
      await settle(tester);
      expect(api.calls, contains('rename 1 Road trip'));
      expect(find.text('Road trip'), findsOneWidget);
    });

    testWidgets('Delete asks first: No keeps the list, Yes deletes it and leaves the page', (
      tester,
    ) async {
      await prepare();
      await open(tester);
      await tester.tap(find.byKey(const Key('playlist-menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Delete playlist'));
      await tester.pumpAndSettle();
      expect(find.text('Delete this playlist?'), findsOneWidget);
      expect(find.textContaining('“Car trip” is deleted from your profile.'), findsOneWidget);
      await tester.tap(find.byKey(const Key('playlist-delete-no')));
      await tester.pumpAndSettle();
      expect(api.lists, isNotEmpty);

      await tester.tap(find.byKey(const Key('playlist-menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Delete playlist'));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('playlist-delete-yes')));
      await settle(tester);
      await tester.pumpAndSettle();
      expect(api.lists, isEmpty);
      expect(find.byKey(const Key('playlist-menu')), findsNothing, reason: 'back at the list');
      expect(find.text('No playlists yet. Tap + to make one.'), findsOneWidget);
    });

    testWidgets('an empty list says how to fill it; the download button is there', (tester) async {
      api.add('Empty');
      await c.load();
      await open(tester);
      expect(find.textContaining('Use “Add to playlist”'), findsOneWidget);
      expect(find.byType(DownloadButton), findsOneWidget);
    });

    testWidgets('without the server the copy shows', (tester) async {
      await prepare();
      await c.open(1);
      c.api = null;
      await open(tester);
      expect(find.text('Chapter 1'), findsOneWidget);
      expect(find.text('Not available'), findsOneWidget);
    });

    testWidgets('German', (tester) async {
      await prepare();
      await open(tester, locale: const Locale('de'));
      expect(find.text('Nicht verfügbar'), findsOneWidget);
      expect(
        tester.widget<IconButton>(find.byKey(const Key('playlist-play'))).tooltip,
        'Abspielen',
      );
    });
  });
}
