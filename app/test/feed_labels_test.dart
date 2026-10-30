// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/labels.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/screens/podcasts_screen.dart';
import 'package:psalm/screens/radio_screen.dart';
import 'package:psalm/screens/search_screen.dart';

import 'labels_test.dart' show FakeLabels;
import 'library_filter_test.dart' show FakeFeeds;

Widget _app(Widget home, {LabelsController? labels}) {
  final app = PlayerScope(
    controller: PlayerController(const <Book>[]),
    child: MaterialApp(
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      locale: const Locale('en'),
      home: Scaffold(body: home),
    ),
  );
  return labels == null ? app : LabelsScope(controller: labels, child: app);
}

FakeFeeds _feeds({bool can = true}) => FakeFeeds()
  ..podcastRows = [
    {'id': 1, 'title': 'Kinderfunk', 'description': 'for kids', 'approved': true, 'unheard': 0, 'labels': ['Kinder'], 'can_label': can},
    {'id': 2, 'title': 'Andacht', 'description': 'daily', 'approved': true, 'unheard': 0, 'labels': ['Gebet'], 'can_label': can},
  ]
  ..stationRows = [
    {'id': 3, 'name': 'Radio A', 'stream_url': 'https://stream.example.org/a', 'approved': true, 'labels': ['Gebet'], 'can_label': can},
    {'id': 4, 'name': 'Radio B', 'stream_url': 'https://stream.example.org/b', 'approved': true, 'labels': [], 'can_label': can},
  ];

void main() {
  test('label counts of podcasts and stations merge with the ones of the books, case ignored', () {
    final feeds = labelCountsOf([['Gebet'], ['gebet', 'Kinder'], <String>[]]);
    expect(feeds, [('Gebet', 2), ('Kinder', 1)]);
    expect(mergeLabelCounts([('gebet', 1), ('Schule', 3)], feeds), [('gebet', 3), ('Schule', 3), ('Kinder', 1)]);
    expect(carriesAll(['Gebet', 'Kinder'], {'kinder', 'gebet'}), isTrue);
    expect(carriesAll(['Gebet'], {'Kinder'}), isFalse);
    expect(carriesAll(const [], {}), isTrue);
  });

  testWidgets('the podcast list shows the labels, filters by them and an allowed user edits them', (tester) async {
    final api = FakeLabels();
    final labels = LabelsController()..api = api;
    final controller = PodcastsController(_feeds());
    addTearDown(controller.dispose);
    await tester.pumpWidget(_app(CustomScrollView(slivers: [PodcastsBody(controller: controller)]), labels: labels));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('feed-label-podcast-1-Kinder')), findsOneWidget);
    expect(find.text('Andacht'), findsOneWidget);

    // The filter narrows the list to the podcasts with the label.
    await tester.tap(find.byKey(const ValueKey('label-filter-Kinder')));
    await tester.pumpAndSettle();
    expect(find.text('Kinderfunk'), findsOneWidget);
    expect(find.text('Andacht'), findsNothing);
    await tester.tap(find.byKey(const ValueKey('label-filter-clear')));
    await tester.pumpAndSettle();
    expect(find.text('Andacht'), findsOneWidget);

    // Editing: the editor opens, a word is added, the labels are sent to the server.
    await tester.tap(find.byKey(const ValueKey('feed-labels-edit-podcast-1')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('labels-entry')), 'Weihnachten');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('labels-save')));
    await tester.pumpAndSettle();
    expect(api.managed, ['podcast:1:Kinder|Weihnachten']);
  });

  testWidgets('a user who may not label sees the labels but no edit chip', (tester) async {
    final controller = RadioController(_feeds(can: false));
    addTearDown(controller.dispose);
    await tester.pumpWidget(_app(CustomScrollView(slivers: [RadioBody(controller: controller)]), labels: LabelsController()..api = FakeLabels()));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('feed-label-station-3-Gebet')), findsOneWidget);
    expect(find.byKey(const ValueKey('feed-labels-edit-station-3')), findsNothing);
  });

  testWidgets('the station list edits labels too', (tester) async {
    final api = FakeLabels();
    final controller = RadioController(_feeds());
    addTearDown(controller.dispose);
    await tester.pumpWidget(_app(CustomScrollView(slivers: [RadioBody(controller: controller)]), labels: LabelsController()..api = api));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('feed-labels-edit-station-4')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('labels-entry')), 'Musik');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('labels-save')));
    await tester.pumpAndSettle();
    expect(api.managed, ['station:4:Musik']);
  });

  testWidgets('the search finds podcasts and stations by name and by label', (tester) async {
    tester.view.physicalSize = const Size(1600, 3000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(_app(SearchScreen(api: _feeds())));
    await tester.pumpAndSettle();

    // Typed: the name.
    await tester.enterText(find.byType(TextField), 'radio a');
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('search-station-3')), findsOneWidget);
    expect(find.byKey(const ValueKey('search-station-4')), findsNothing);

    // The label alone, with nothing typed: everything that carries it, podcasts and stations.
    await tester.enterText(find.byType(TextField), '');
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('label-filter-Gebet')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('search-podcast-2')), findsOneWidget);
    expect(find.byKey(const ValueKey('search-station-3')), findsOneWidget);
    expect(find.byKey(const ValueKey('search-podcast-1')), findsNothing);
  });
}
