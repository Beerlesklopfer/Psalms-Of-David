// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/labels.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/book_screen.dart';
import 'package:psalm/screens/library_screen.dart';
import 'package:psalm/server/api_client.dart';

class FakeLabels implements LabelApi {
  final calls = <(int, List<String>)>[];
  final managed = <String>[];
  var current = const LabelList([('Gebet', 2), ('Schule', 3)], true);
  ApiException? refuse;

  @override
  Future<LabelList> list() async => current;

  @override
  Future<void> rename(String name, String to) async {
    managed.add('rename:$name:$to');
    if (refuse != null) throw refuse!;
    current = LabelList([for (final (n, c) in current.labels) (n == name ? to : n, c)], current.canManage);
  }

  @override
  Future<void> delete(String name) async {
    managed.add('delete:$name');
    if (refuse != null) throw refuse!;
    current = LabelList([for (final (n, c) in current.labels) if (n != name) (n, c)], current.canManage);
  }

  final trackCalls = <(int, int, List<String>)>[];

  @override
  Future<List<String>> setTrackLabels(int id, int partId, List<String> labels) async {
    trackCalls.add((id, partId, labels));
    if (refuse != null) throw refuse!;
    return labels;
  }

  @override
  Future<List<String>> setPodcastLabels(int id, List<String> labels) async {
    managed.add('podcast:$id:${labels.join('|')}');
    if (refuse != null) throw refuse!;
    return labels;
  }

  @override
  Future<List<String>> setStationLabels(int id, List<String> labels) async {
    managed.add('station:$id:${labels.join('|')}');
    if (refuse != null) throw refuse!;
    return labels;
  }

  @override
  Future<List<String>> setLabels(int id, List<String> labels) async {
    calls.add((id, labels));
    if (refuse != null) throw refuse!;
    return labels;
  }
}

Book _book(String id, String title, {List<String> labels = const [], bool can = false}) => Book(
  id: id,
  title: title,
  author: 'Autor $id',
  color: const Color(0xFF6E1F2B),
  labels: labels,
  canLabel: can,
  tracks: const [Track(disc: 1, number: 1, title: 'Eins', length: Duration(minutes: 5))],
);

Widget _app(Widget home, List<Book> library, {LabelsController? labels}) {
  final app = MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    locale: const Locale('en'),
    home: home,
  );
  final scoped = labels == null ? app : LabelsScope(controller: labels, child: app);
  return PlayerScope(controller: PlayerController(library), child: scoped);
}

void main() {
  final library = [
    _book('1', 'Eins', labels: ['Gebet', 'Schule']),
    _book('2', 'Zwei', labels: ['schule']),
    _book('3', 'Drei', labels: ['Urlaub']),
    _book('4', 'Vier'),
  ];

  test('labels are counted without regard to case, the most used first', () {
    expect(labelCounts(library), [('Schule', 2), ('Gebet', 1), ('Urlaub', 1)]);
  });

  test('a list is narrowed to the books that carry all the chosen labels', () {
    expect(filterByLabels(library, {}).length, 4);
    expect(filterByLabels(library, {'SCHULE'}).map((b) => b.id), ['1', '2']);
    expect(filterByLabels(library, {'Schule', 'Gebet'}).map((b) => b.id), ['1']);
    expect(filterByLabels(library, {'Gibt es nicht'}), isEmpty);
  });

  test(
    'typing suggests the labels in use: the ones that begin with it first, not the chosen ones',
    () {
      expect(labelSuggestions(library, '', []), ['Schule', 'Gebet', 'Urlaub']);
      expect(labelSuggestions(library, 'ge', []), ['Gebet']);
      expect(labelSuggestions(library, 'ule', []), ['Schule'], reason: 'one that holds the text');
      expect(labelSuggestions(library, '', ['schule']), ['Gebet', 'Urlaub']);
    },
  );

  test('a label is cleaned like the server does', () {
    expect(cleanLabel('  Gemeinsame   Urlaube '), 'Gemeinsame Urlaube');
    expect(cleanLabel(''), isNull);
    expect(cleanLabel('a,b'), isNull);
    expect(cleanLabel('x' * 41), isNull);
  });

  testWidgets(
    'the library has the labels as chips; a tap narrows the list, a second one takes it back',
    (tester) async {
      tester.view.physicalSize = const Size(1000, 2400);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(_app(const Scaffold(body: LibraryScreen()), library));
      await tester.pumpAndSettle();
      expect(find.text('Schule (2)'), findsOneWidget);
      expect(find.text('Eins'), findsWidgets);
      expect(find.text('Vier'), findsWidgets);
      await tester.tap(find.byKey(const ValueKey('label-filter-Schule')));
      await tester.pumpAndSettle();
      expect(find.text('Vier'), findsNothing);
      expect(find.text('Drei'), findsNothing);
      expect(find.text('Zwei'), findsWidgets);
      await tester.tap(find.byKey(const ValueKey('label-filter-clear')));
      await tester.pumpAndSettle();
      expect(find.text('Vier'), findsWidgets);
    },
  );

  group('the page of a book', () {
    late FakeLabels api;
    late LabelsController c;
    var reloads = 0;
    setUp(() {
      api = FakeLabels();
      reloads = 0;
      c = LabelsController(onChanged: () => reloads++)..api = api;
    });
    tearDown(() => c.dispose());

    testWidgets(
      'shows the labels to everybody, and offers the editor only to the owner or an administrator',
      (tester) async {
        final mine = _book('1', 'Eins', labels: ['Gebet'], can: true);
        await tester.pumpWidget(_app(BookScreen(book: mine), library, labels: c));
        await tester.pumpAndSettle();
        expect(find.byKey(const ValueKey('label-chip-Gebet')), findsOneWidget);
        expect(find.byKey(const ValueKey('labels-edit')), findsOneWidget);
        final other = _book('1', 'Eins', labels: ['Gebet']);
        await tester.pumpWidget(_app(BookScreen(book: other), library, labels: c));
        await tester.pumpAndSettle();
        expect(find.byKey(const ValueKey('label-chip-Gebet')), findsOneWidget);
        expect(find.byKey(const ValueKey('labels-edit')), findsNothing);
      },
    );

    testWidgets(
      'the editor suggests labels in use, Enter adds a word and keeps the window, Save sends them all',
      (tester) async {
        tester.view.physicalSize = const Size(1000, 2400);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.reset);
        final mine = _book('1', 'Eins', labels: ['Gebet'], can: true);
        await tester.pumpWidget(_app(BookScreen(book: mine), library, labels: c));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const ValueKey('labels-edit')));
        await tester.pumpAndSettle();
        expect(find.byKey(const ValueKey('label-word-Gebet')), findsOneWidget);
        // Typing suggests: Schule is in use on other books, Gebet is on this one already.
        await tester.enterText(find.byKey(const ValueKey('labels-entry')), 'sch');
        await tester.pumpAndSettle();
        expect(find.byKey(const ValueKey('label-suggestion-Schule')), findsOneWidget);
        await tester.tap(find.byKey(const ValueKey('label-suggestion-Schule')));
        await tester.pumpAndSettle();
        expect(find.byKey(const ValueKey('label-word-Schule')), findsOneWidget);
        // Enter adds the typed word and does not close the window.
        await tester.enterText(find.byKey(const ValueKey('labels-entry')), 'Neu');
        await tester.testTextInput.receiveAction(TextInputAction.done);
        await tester.pumpAndSettle();
        expect(find.byKey(const ValueKey('label-word-Neu')), findsOneWidget);
        expect(
          find.byKey(const ValueKey('labels-save')),
          findsOneWidget,
          reason: 'the window stays open',
        );
        // The cross takes a word away; Save sends what is left.
        await tester.tap(
          find.descendant(
            of: find.byKey(const ValueKey('label-word-Gebet')),
            matching: find.byType(Icon),
          ),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const ValueKey('labels-save')));
        await tester.pumpAndSettle();
        expect(api.calls.single.$1, 1);
        expect(api.calls.single.$2, ['Schule', 'Neu']);
        expect(reloads, 1, reason: 'the library is loaded again');
        expect(
          find.byKey(const ValueKey('labels-save')),
          findsNothing,
          reason: 'the window closes',
        );
      },
    );

    testWidgets(
      'a word that is not usable is refused in the window, and a refusal of the server is told',
      (tester) async {
        tester.view.physicalSize = const Size(1000, 2400);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.reset);
        final mine = _book('1', 'Eins', can: true);
        await tester.pumpWidget(_app(BookScreen(book: mine), library, labels: c));
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const ValueKey('labels-edit')));
        await tester.pumpAndSettle();
        await tester.enterText(find.byKey(const ValueKey('labels-entry')), 'a|b');
        await tester.tap(find.byKey(const ValueKey('labels-add')));
        await tester.pumpAndSettle();
        expect(find.textContaining('not a usable label'), findsOneWidget);
        await tester.enterText(find.byKey(const ValueKey('labels-entry')), 'Gut');
        api.refuse = ApiException(403, 'forbidden');
        await tester.tap(find.byKey(const ValueKey('labels-save')));
        await tester.pumpAndSettle();
        expect(find.text('The labels could not be saved.'), findsOneWidget);
        expect(
          find.byKey(const ValueKey('labels-save')),
          findsOneWidget,
          reason: 'the window stays, nothing is lost',
        );
      },
    );
  });

  group('labels of single titles', () {
    Book withTitles({bool can = false, List<String> bookLabels = const []}) => Book(
      id: '1',
      title: 'Eins',
      author: 'Autor 1',
      color: const Color(0xFF6E1F2B),
      labels: bookLabels,
      canLabel: can,
      trackLabels: const ['Stille'],
      tracks: const [
        Track(
          disc: 1,
          number: 1,
          title: 'Erstes Lied',
          length: Duration(minutes: 3),
          partId: 11,
          labels: ['Stille'],
        ),
        Track(disc: 1, number: 2, title: 'Zweites Lied', length: Duration(minutes: 3), partId: 12),
      ],
    );

    test('a book is found by the labels of its titles, and they are counted once', () {
      final b = withTitles(bookLabels: ['stille', 'Gebet']);
      expect(b.allLabels, ['stille', 'Gebet'], reason: 'case is ignored, no doubles');
      expect(labelCounts([b, withTitles()]), [('stille', 2), ('Gebet', 1)]);
      final books = [withTitles(), _book('2', 'Zwei', labels: ['Gebet'])];
      expect(filterByLabels(books, {'stille'}).map((b) => b.id), ['1']);
      expect(filterByLabels(books, {'Stille', 'Gebet'}), isEmpty, reason: 'a book must carry all');
    });

    testWidgets('the page shows the labels of a title; the owner may edit them, others may not', (tester) async {
      final api = FakeLabels();
      var reloads = 0;
      final c = LabelsController(onChanged: () => reloads++)..api = api;
      addTearDown(c.dispose);
      tester.view.physicalSize = const Size(1000, 2400);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(_app(BookScreen(book: withTitles()), library, labels: c));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('track-label-11-Stille')), findsOneWidget);
      expect(find.byKey(const ValueKey('track-labels-edit-11')), findsNothing, reason: 'not the owner');

      await tester.pumpWidget(_app(BookScreen(book: withTitles(can: true)), library, labels: c));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('track-labels-edit-12')));
      await tester.pumpAndSettle();
      expect(find.textContaining('Zweites Lied'), findsWidgets, reason: 'the window names the title');
      await tester.enterText(find.byKey(const ValueKey('labels-entry')), 'Abend');
      await tester.tap(find.byKey(const ValueKey('labels-add')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('labels-save')));
      await tester.pumpAndSettle();
      expect(api.trackCalls.single.$1, 1);
      expect(api.trackCalls.single.$2, 12);
      expect(api.trackCalls.single.$3, ['Abend']);
      expect(api.calls, isEmpty, reason: 'the labels of the book are not touched');
      expect(reloads, 1);
    });
  });
}
