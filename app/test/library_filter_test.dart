// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/podcasts.dart';
import 'package:psalm/screens/library_screen.dart';
import 'package:psalm/screens/media_category.dart';
import 'package:psalm/screens/search_screen.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/server/server_source.dart';

LibraryItem _book(String id, String kind, {String? genre}) => LibraryItem(
  id: id,
  title: 'Title $id',
  author: 'Artist',
  color: const Color(0xFF000000),
  kind: kind,
  genre: genre,
  tracks: [Track(disc: 1, number: 1, title: 'T', length: const Duration(minutes: 3), partId: int.parse(id))],
);

class FakeFeeds implements FeedApi {
  List<Map<String, dynamic>> podcastRows = [
    {'id': 1, 'title': 'Kinderfunk', 'description': 'for kids', 'approved': true, 'unheard': 2},
  ];
  List<Map<String, dynamic>> stationRows = [
    {'id': 3, 'name': 'Radio A', 'stream_url': 'https://stream.example.org/a', 'approved': true},
  ];
  Object? error;
  int podcastLoads = 0;
  int stationLoads = 0;

  @override
  Future<List<Map<String, dynamic>>> podcasts() async {
    podcastLoads++;
    if (error != null) throw error!;
    return podcastRows;
  }

  @override
  Future<List<Map<String, dynamic>>> stations() async {
    stationLoads++;
    if (error != null) throw error!;
    return stationRows;
  }

  @override
  Future<Map<String, dynamic>> followPodcast(String url) async => {'approved': true, 'title': 'x'};
  @override
  Future<void> unfollowPodcast(int id) async {}
  @override
  Future<List<Map<String, dynamic>>> episodes(int podcastId) async => [];
  @override
  Future<Map<String, dynamic>> addStation(String name, String streamUrl, {String? homepage}) async => {
    'approved': true,
  };
  @override
  Future<void> removeStation(int id) async {}
}

Widget _app(PlayerController p, Widget home, {Locale locale = const Locale('en')}) => PlayerScope(
  controller: p,
  child: MaterialApp(
    locale: locale,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: Scaffold(body: home),
  ),
);

/// A tall, wide window: the grids are built lazily, and six chips fit in one row.
void _bigWindow(WidgetTester tester) {
  tester.view.physicalSize = const Size(1600, 3000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

void main() {
  final rock = find.byKey(const Key('genre-Rock'));
  final jazz = find.byKey(const Key('genre-Jazz'));
  final all = find.byKey(const Key('genre-all'));
  final other = find.byKey(const Key('genre-other'));

  PlayerController music({List<LibraryItem>? books, List<String> genres = const ['Rock', 'Jazz']}) {
    final p = PlayerController(
      books ??
          [
            _book('1', 'music', genre: 'Jazz'),
            _book('2', 'music', genre: 'Rock'),
            _book('3', 'music', genre: 'Rock'),
            _book('4', 'music'),
            _book('5', 'audiobook'),
          ],
    );
    p.genres = genres;
    return p;
  }

  Future<void> toMusic(WidgetTester tester, PlayerController p, {Locale locale = const Locale('en')}) async {
    await tester.pumpWidget(_app(p, const LibraryScreen(), locale: locale));
    await tester.tap(find.text(locale.languageCode == 'de' ? 'Musik' : 'Music'));
    await tester.pump();
  }

  group('first row', () {
    testWidgets('music, audiobooks, podcasts, radio, videos, e-books from left to right, with icons', (tester) async {
      _bigWindow(tester);
      await tester.pumpWidget(_app(music(), const LibraryScreen()));
      final labels = ['Music', 'Audiobooks', 'Podcasts', 'Radio', 'Videos', 'E-Books'];
      final x = [for (final t in labels) tester.getCenter(find.widgetWithText(ChoiceChip, t)).dx];
      expect(x, orderedEquals([...x]..sort()), reason: '$x');
      expect(find.byIcon(Icons.podcasts), findsOneWidget);
      expect(find.byIcon(Icons.radio), findsOneWidget);
    });

    testWidgets('the search has All and no podcasts and no radio', (tester) async {
      _bigWindow(tester);
      await tester.pumpWidget(_app(music(), const SearchScreen()));
      expect(find.widgetWithText(ChoiceChip, 'All'), findsOneWidget);
      expect(find.widgetWithText(ChoiceChip, 'Podcasts'), findsNothing);
      expect(find.widgetWithText(ChoiceChip, 'Radio'), findsNothing);
      expect(find.widgetWithText(ChoiceChip, 'Music'), findsOneWidget);
    });

    testWidgets('the chips of the library have no All', (tester) async {
      _bigWindow(tester);
      await tester.pumpWidget(_app(music(), const LibraryScreen()));
      expect(find.widgetWithText(ChoiceChip, 'All'), findsNothing);
    });

    test('forSearch decides what the chips hold', () {
      expect(MediaCategory.values.map((c) => c.name), [
        'all',
        'music',
        'audiobooks',
        'podcasts',
        'radio',
        'playlists',
        'videos',
        'ebooks',
        'hidden',
      ]);
    });
  });

  group('podcasts and radio in the library', () {
    testWidgets('Podcasts shows the list of the podcast page, with Add and Refresh in the app bar', (tester) async {
      _bigWindow(tester);
      final feeds = FakeFeeds();
      await tester.pumpWidget(_app(music(), LibraryScreen(api: feeds)));
      await tester.tap(find.widgetWithText(ChoiceChip, 'Podcasts'));
      await tester.pump();
      await tester.pump();
      expect(find.text('Kinderfunk'), findsOneWidget);
      expect(find.byTooltip('Follow a podcast'), findsOneWidget);
      expect(find.byTooltip('Refresh the library'), findsOneWidget);
      expect(find.byKey(genreRowKey), findsNothing);
      final before = feeds.podcastLoads;
      await tester.tap(find.byTooltip('Refresh the library'));
      await tester.pump();
      expect(feeds.podcastLoads, before + 1);
    });

    testWidgets('the one refresh button reloads the library and both lists', (tester) async {
      _bigWindow(tester);
      final feeds = FakeFeeds();
      await tester.pumpWidget(_app(music(), LibraryScreen(api: feeds)));
      await tester.tap(find.widgetWithText(ChoiceChip, 'Podcasts'));
      await tester.pump();
      await tester.pump();
      await tester.tap(find.widgetWithText(ChoiceChip, 'Radio'));
      await tester.pump();
      await tester.pump();
      expect(
        find.byTooltip('Refresh the library'),
        findsOneWidget,
        reason: 'one refresh button, not one for each list',
      );
      final (podcasts, stations) = (feeds.podcastLoads, feeds.stationLoads);
      await tester.tap(find.byTooltip('Refresh the library'));
      await tester.pump();
      expect((feeds.podcastLoads, feeds.stationLoads), (podcasts + 1, stations + 1));
    });

    testWidgets('Radio shows the stations', (tester) async {
      _bigWindow(tester);
      await tester.pumpWidget(_app(music(), LibraryScreen(api: FakeFeeds())));
      await tester.tap(find.widgetWithText(ChoiceChip, 'Radio'));
      await tester.pump();
      await tester.pump();
      expect(find.text('Radio A'), findsWidgets);
      expect(find.byIcon(Icons.radio), findsWidgets);
    });

    testWidgets('without a server they say so, with a failing server they say that', (tester) async {
      _bigWindow(tester);
      await tester.pumpWidget(_app(music(), const LibraryScreen()));
      await tester.tap(find.widgetWithText(ChoiceChip, 'Podcasts'));
      await tester.pump();
      expect(find.text('Podcasts and radio need a connection to your server.'), findsOneWidget);

      final feeds = FakeFeeds()..error = ApiException(0, 'network');
      await tester.pumpWidget(_app(music(), LibraryScreen(api: feeds)));
      await tester.tap(find.widgetWithText(ChoiceChip, 'Radio'));
      await tester.pump();
      await tester.pump();
      expect(find.byType(Text).evaluate().any((e) => (e.widget as Text).data?.contains('server') ?? false), isTrue);
    });
  });

  group('genre row', () {
    testWidgets('only for music: not for audiobooks, not for podcasts', (tester) async {
      _bigWindow(tester);
      final p = music();
      await tester.pumpWidget(_app(p, const LibraryScreen()));
      expect(all, findsNothing, reason: 'audiobooks first');
      await tester.tap(find.text('Music'));
      await tester.pump();
      expect(all, findsOneWidget);
      await tester.tap(find.text('Audiobooks'));
      await tester.pump();
      expect(all, findsNothing);
    });

    testWidgets('All first, the genres in the order of the server with counts, Other last', (tester) async {
      _bigWindow(tester);
      await toMusic(tester, music());
      expect(find.text('All'), findsWidgets);
      expect(find.text('Rock (2)'), findsOneWidget);
      expect(find.text('Jazz (1)'), findsOneWidget);
      expect(find.text('Other (1)'), findsOneWidget);
      final x = [
        for (final f in [all, rock, jazz, other]) tester.getCenter(f).dx,
      ];
      expect(x, orderedEquals([...x]..sort()), reason: 'server order: Rock before Jazz here, Other last: $x');
      expect(tester.widget<FilterChip>(all).selected, isTrue);
    });

    testWidgets('only genres that have an album; Other only when albums have no genre', (tester) async {
      _bigWindow(tester);
      final p = music(
        books: [_book('1', 'music', genre: 'Rock')],
        genres: ['Rock', 'Jazz', 'Blues'],
      );
      await toMusic(tester, p);
      expect(rock, findsOneWidget);
      expect(jazz, findsNothing);
      expect(other, findsNothing);
    });

    testWidgets('a tap chooses exactly one genre and shows only its section; All lifts it', (tester) async {
      _bigWindow(tester);
      await toMusic(tester, music());
      expect(find.text('Title 1'), findsWidgets);
      expect(find.text('Title 2'), findsWidgets);
      await tester.tap(rock);
      await tester.pump();
      expect(find.text('Title 2'), findsWidgets);
      expect(find.text('Title 3'), findsWidgets);
      expect(find.text('Title 1'), findsNothing);
      expect(find.text('Title 4'), findsNothing);
      expect(tester.widget<FilterChip>(rock).selected, isTrue);
      expect(tester.widget<FilterChip>(all).selected, isFalse);
      await tester.tap(jazz);
      await tester.pump();
      expect(tester.widget<FilterChip>(rock).selected, isFalse, reason: 'exactly one');
      expect(find.text('Title 1'), findsWidgets);
      expect(find.text('Title 2'), findsNothing);
      await tester.tap(other);
      await tester.pump();
      expect(find.text('Title 4'), findsWidgets);
      expect(find.text('Title 1'), findsNothing);
      await tester.tap(all);
      await tester.pump();
      expect(find.text('Title 1'), findsWidgets);
      expect(find.text('Title 4'), findsWidgets);
    });

    testWidgets('the choice stays when the category changes and when the library is reloaded', (tester) async {
      _bigWindow(tester);
      final p = music();
      await toMusic(tester, p);
      await tester.tap(rock);
      await tester.pump();
      await tester.tap(find.text('Audiobooks'));
      await tester.pump();
      await tester.tap(find.text('Music'));
      await tester.pump();
      expect(tester.widget<FilterChip>(rock).selected, isTrue);

      p.updateLibrary(
        LoadedLibrary(
          [_book('2', 'music', genre: 'Rock'), _book('3', 'music', genre: 'Rock'), _book('6', 'music', genre: 'Rock')],
          {},
          genres: ['Rock', 'Jazz'],
        ),
      );
      await tester.pump();
      expect(find.text('Rock (3)'), findsOneWidget);
      expect(tester.widget<FilterChip>(rock).selected, isTrue);
    });

    testWidgets('a genre that is gone falls back to All', (tester) async {
      _bigWindow(tester);
      final p = music();
      await toMusic(tester, p);
      await tester.tap(jazz);
      await tester.pump();
      p.updateLibrary(LoadedLibrary([_book('2', 'music', genre: 'Rock')], {}, genres: ['Rock', 'Jazz']));
      await tester.pump();
      expect(jazz, findsNothing);
      expect(tester.widget<FilterChip>(all).selected, isTrue);
      expect(find.text('Title 2'), findsWidgets);
    });

    testWidgets('Other that is gone falls back to All', (tester) async {
      _bigWindow(tester);
      final p = music();
      await toMusic(tester, p);
      await tester.tap(other);
      await tester.pump();
      p.updateLibrary(LoadedLibrary([_book('2', 'music', genre: 'Rock')], {}, genres: ['Rock']));
      await tester.pump();
      expect(tester.widget<FilterChip>(all).selected, isTrue);
    });

    testWidgets('the row scrolls sideways when there are many genres', (tester) async {
      _bigWindow(tester);
      final names = [for (var i = 0; i < 30; i++) 'Genre number $i'];
      final p = music(
        books: [for (var i = 0; i < 30; i++) _book('${i + 1}', 'music', genre: names[i])],
        genres: names,
      );
      await toMusic(tester, p);
      final scroll = find.ancestor(of: all, matching: find.byType(SingleChildScrollView));
      expect(scroll, findsOneWidget);
      expect(tester.widget<SingleChildScrollView>(scroll).scrollDirection, Axis.horizontal);
      await tester.drag(scroll, const Offset(-600, 0));
      await tester.pump();
      expect(tester.takeException(), isNull);
    });

    testWidgets('German texts: Alle and Sonstige', (tester) async {
      _bigWindow(tester);
      await toMusic(tester, music(), locale: const Locale('de'));
      expect(find.text('Alle'), findsWidgets);
      expect(find.text('Sonstige (1)'), findsOneWidget);
    });
  });
}

const genreRowKey = Key('genre-all');
