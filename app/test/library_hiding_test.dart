// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/library_hiding.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/book_screen.dart';
import 'package:psalm/screens/library_screen.dart';
import 'package:psalm/screens/settings_screen.dart';
import 'package:psalm/server/api_client.dart';
import 'package:psalm/widgets.dart';

/// The server, replaced: keeps the state the way the server does and writes down what was asked.
class FakeHiding implements HidingApi {
  HiddenState current = const HiddenState();
  final calls = <String>[];
  ApiException? refuse;

  @override
  Future<HiddenState> state() async => current;

  @override
  Future<HiddenState> setShowHidden(bool show) async {
    calls.add('show:$show');
    if (refuse != null) throw refuse!;
    return current = HiddenState(showHidden: show, books: current.books, authors: current.authors);
  }

  @override
  Future<HiddenState> hideBook(int id, bool hidden) async {
    calls.add('book:$id:$hidden');
    if (refuse != null) throw refuse!;
    final books = [
      ...current.books.where((b) => b.id != id),
      if (hidden) HiddenBook(id: id, title: 'Der Hobbit', author: 'Tolkien'),
    ];
    return current = HiddenState(
      showHidden: current.showHidden,
      books: books,
      authors: current.authors,
    );
  }

  @override
  Future<HiddenState> hideAuthor(String name, String kind, bool hidden) async {
    calls.add('author:$name:$kind:$hidden');
    if (refuse != null) throw refuse!;
    final authors = [
      ...current.authors.where((a) => !(a.name == name && a.kind == kind)),
      if (hidden) HiddenAuthor(id: 7, name: name, kind: kind),
    ];
    return current = HiddenState(
      showHidden: current.showHidden,
      books: current.books,
      authors: authors,
    );
  }
}

Book _book({bool hidden = false, String kind = 'audiobook'}) => Book(
  id: '5',
  title: 'Der Hobbit',
  author: 'Tolkien',
  kind: kind,
  hidden: hidden,
  color: const Color(0xFF6E1F2B),
  tracks: const [Track(disc: 1, number: 1, title: 'Eins', length: Duration(minutes: 5))],
);

Widget _app(Widget home, {HidingController? hiding, Book? book}) {
  final app = MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    locale: const Locale('en'),
    home: home,
  );
  final scoped = hiding == null ? app : HidingScope(controller: hiding, child: app);
  return PlayerScope(controller: PlayerController([?book]), child: scoped);
}

void main() {
  late FakeHiding api;
  late HidingController c;
  var reloads = 0;

  setUp(() {
    api = FakeHiding();
    reloads = 0;
    c = HidingController(onChanged: () => reloads++)..api = api;
  });
  tearDown(() => c.dispose());

  group('the controller', () {
    test(
      'a change goes to the server, the state is the answer, and the lists are loaded again',
      () async {
        expect(await c.hideBook('5', true), isTrue);
        expect(c.state.books.map((b) => b.id).toList(), [5]);
        expect(reloads, 1);
        expect(await c.hideAuthor('Tolkien', 'music', true), isTrue);
        expect(c.state.authorHidden('Tolkien', 'music'), isTrue);
        expect(c.state.authorHidden('Tolkien', 'audiobook'), isFalse, reason: 'another kind');
        expect(await c.hideAuthor('Tolkien', '', true), isTrue);
        expect(
          c.state.authorHidden('tolkien', 'audiobook'),
          isTrue,
          reason: 'both kinds, any case',
        );
        expect(await c.setShowHidden(true), isTrue);
        expect(c.state.showHidden, isTrue);
        expect(api.calls, [
          'book:5:true',
          'author:Tolkien:music:true',
          'author:Tolkien::true',
          'show:true',
        ]);
      },
    );

    test(
      'a refusal is kept as its code, nothing is reloaded, and a book without a number is no item',
      () async {
        api.refuse = ApiException(404, 'not_found');
        expect(await c.hideBook('5', true), isFalse);
        expect((c.error, reloads), ('not_found', 0));
        expect(
          await c.hideBook('pl:3', true),
          isFalse,
          reason: 'a playlist is no book of the server',
        );
        c.api = null;
        expect((c.available, await c.setShowHidden(true)), (false, false));
      },
    );
  });

  group('the screens', () {
    testWidgets('the menu of the book page hides the book and the books of its author', (
      tester,
    ) async {
      await tester.pumpWidget(
        _app(
          BookScreen(book: _book()),
          hiding: c,
          book: _book(),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('hide-menu')));
      await tester.pumpAndSettle();
      expect(find.text('Hide this book'), findsOneWidget);
      expect(find.text('Hide all audiobooks by Tolkien'), findsOneWidget);
      expect(find.text('Hide everything by Tolkien'), findsOneWidget);
      await tester.tap(find.byKey(const Key('hide-item-hide-kind')));
      await tester.pumpAndSettle();
      expect(api.calls, ['author:Tolkien:audiobook:true']);
    });

    testWidgets('the book page says whom the book belongs to, when it belongs to somebody', (
      tester,
    ) async {
      Book owned(String? owner) => Book(
        id: '5',
        title: 'Der Hobbit',
        author: 'Tolkien',
        owner: owner,
        color: const Color(0xFF6E1F2B),
        tracks: const [Track(disc: 1, number: 1, title: 'Eins', length: Duration(minutes: 5))],
      );
      await tester.pumpWidget(_app(BookScreen(book: owned('Beerchen')), book: owned('Beerchen')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('owner-line')), findsOneWidget);
      expect(find.text('Owner: Beerchen'), findsOneWidget);
      await tester.pumpWidget(_app(BookScreen(book: owned(null)), book: owned(null)));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('owner-line')), findsNothing);
    });

    testWidgets('a hidden book offers to be shown again; without a server there is no menu', (
      tester,
    ) async {
      await tester.pumpWidget(
        _app(
          BookScreen(book: _book(hidden: true)),
          hiding: c,
          book: _book(hidden: true),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('hide-menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Show this book again'));
      await tester.pumpAndSettle();
      expect(api.calls, ['book:5:false']);
      // Demo mode, tests: no controller, no menu.
      await tester.pumpWidget(_app(BookScreen(book: _book()), book: _book()));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('hide-menu')), findsNothing);
    });

    testWidgets('a hidden book is dimmed in the list and marked; a long press offers the choices', (
      tester,
    ) async {
      await tester.pumpWidget(
        _app(
          Scaffold(
            body: SizedBox(width: 200, child: BookCard(book: _book(hidden: true))),
          ),
          hiding: c,
          book: _book(hidden: true),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('hidden-mark')), findsOneWidget);
      expect(
        tester
            .widget<Opacity>(
              find.ancestor(of: find.byType(InkWell), matching: find.byType(Opacity)).first,
            )
            .opacity,
        0.5,
      );
      await tester.longPress(find.byType(BookCard));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('hide-sheet-hide-book')));
      await tester.pumpAndSettle();
      expect(api.calls, ['book:5:false']);
    });

    testWidgets('the settings have the switch; the list of what was hidden is not there', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(900, 2400);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await c.hideBook('5', true);
      await tester.pumpWidget(_app(const Scaffold(body: SettingsScreen()), hiding: c));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('show-hidden')), findsOneWidget);
      expect(find.byKey(const Key('hidden-book-5')), findsNothing);
      await tester.tap(find.byKey(const Key('show-hidden')));
      await tester.pumpAndSettle();
      expect(c.state.showHidden, isTrue);
    });

    testWidgets(
      'the library has the rubric "Hidden" while the switch is on: what was hidden, each with "show again"',
      (tester) async {
        tester.view.physicalSize = const Size(900, 2400);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.reset);
        await c.hideBook('5', true);
        await c.hideAuthor('Lindgren', 'music', true);
        await tester.pumpWidget(
          _app(const Scaffold(body: LibraryScreen()), hiding: c),
        );
        await tester.pumpAndSettle();
        // The switch is off: no rubric.
        expect(find.widgetWithText(ChoiceChip, 'Hidden'), findsNothing);
        await c.setShowHidden(true);
        await tester.pumpAndSettle();
        await tester.tap(find.widgetWithText(ChoiceChip, 'Hidden'));
        await tester.pumpAndSettle();
        expect(find.text('Music by Lindgren'), findsOneWidget);
        expect(find.text('Der Hobbit'), findsOneWidget);
        await tester.tap(
          find.descendant(
            of: find.byKey(const Key('hidden-book-5')),
            matching: find.text('Show again'),
          ),
        );
        await tester.pumpAndSettle();
        expect(c.state.books, isEmpty);
        // With the switch off the rubric is gone and the library shows the books again.
        await c.setShowHidden(false);
        await tester.pumpAndSettle();
        expect(find.widgetWithText(ChoiceChip, 'Hidden'), findsNothing);
        expect(find.byKey(const Key('hidden-none')), findsNothing);
      },
    );
  });
}
