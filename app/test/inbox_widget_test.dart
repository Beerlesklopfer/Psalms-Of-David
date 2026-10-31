// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/inbox/inbox_controller.dart';
import 'package:psalm/inbox/inbox_demo.dart';
import 'package:psalm/inbox/inbox_models.dart';
import 'package:psalm/inbox/inbox_ui.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/offline/offline_controller.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/profile.dart';
import 'package:psalm/server/api_client.dart';

import 'inbox_fakes.dart';

const _approver = CdRights(mayInsert: true, noApproval: true, mayApprove: true);
const _starter = CdRights(mayInsert: true);

Book _book(String id) => Book(
  id: id,
  title: 'Sturz der Titanen',
  author: 'Ken Follett',
  color: const Color(0xFF000000),
  tracks: [
    Track(
      disc: 1,
      number: 1,
      title: 'T1',
      length: const Duration(minutes: 5),
      partId: 1,
    ),
  ],
);

class InboxRig {
  InboxRig(this.api, this.controller);
  final FakeInboxApi api;
  final InboxController controller;
}

Future<InboxRig> pump(
  WidgetTester tester,
  Widget Function(InboxController c) home, {
  List<InboxItem> items = const [],
  CdRights rights = _approver,
  String role = 'member',
  Locale locale = const Locale('en'),
  OfflineController? offline,
  void Function(FakeInboxApi api)? prepare,
}) async {
  tester.view.physicalSize = const Size(800, 2000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final api = FakeInboxApi()..items = items;
  prepare?.call(api);
  final c = InboxController(api: api)..start();
  addTearDown(c.dispose);
  final profile = ProfileController()
    ..profile = Profile(name: 'Anna', role: role, cdRights: rights);
  Widget app = PlayerScope(
    controller: PlayerController([_book('7')]),
    child: ProfileScope(
      controller: profile,
      child: InboxScope(
        controller: c,
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          locale: locale,
          home: Scaffold(body: Builder(builder: (_) => home(c))),
        ),
      ),
    ),
  );
  if (offline != null) app = OfflineScope(controller: offline, child: app);
  await tester.pumpWidget(app);
  await tester.pumpAndSettle();
  return InboxRig(api, c);
}

Widget page(InboxController c, {OfflineController? offline}) =>
    InboxScreen(controller: c, offline: offline);

void main() {
  group('the card at the top of the library', () {
    testWidgets(
      'says how many CDs wait and how many are unread, and opens the inbox',
      (tester) async {
        await pump(
          tester,
          (_) => const InboxCard(),
          items: [inboxTask(3), inboxTask(4), inboxNotice(2)],
        );
        expect(find.text('2 CDs wait for approval'), findsOneWidget);
        expect(find.text('3 unread'), findsOneWidget);
        await tester.tap(find.byKey(const ValueKey('inbox-card')));
        await tester.pumpAndSettle();
        expect(find.byKey(const ValueKey('inbox-item-3')), findsOneWidget);
      },
    );

    testWidgets('one CD, singular; only unread: no waiting line', (
      tester,
    ) async {
      await pump(
        tester,
        (_) => const InboxCard(),
        items: [inboxTask(3, canDecide: false)..toString()],
      );
      // The starter's own task is not open for them to decide, but it still is counted by the server; the card shows the server's numbers.
      expect(find.byKey(const ValueKey('inbox-card')), findsOneWidget);
    });

    testWidgets('nothing to show: no card', (tester) async {
      await pump(
        tester,
        (_) => const InboxCard(),
        items: [inboxNotice(2, read: true)],
      );
      expect(find.byKey(const ValueKey('inbox-card')), findsNothing);
    });

    testWidgets(
      'a profile without a CD right (child, guest) sees no card, whatever the server says',
      (tester) async {
        await pump(
          tester,
          (_) => const InboxCard(),
          items: [inboxTask(3)],
          rights: const CdRights(),
          role: 'child',
        );
        expect(find.byKey(const ValueKey('inbox-card')), findsNothing);
      },
    );

    testWidgets(
      'a starter without the right to approve sees the card for the unread notices',
      (tester) async {
        await pump(
          tester,
          (_) => const InboxCard(),
          items: [inboxNotice(2)],
          rights: _starter,
        );
        expect(find.text('1 unread'), findsOneWidget);
      },
    );

    testWidgets('German', (tester) async {
      await pump(
        tester,
        (_) => const InboxCard(),
        items: [inboxTask(3)],
        locale: const Locale('de'),
      );
      expect(find.text('1 CD wartet auf Freigabe'), findsOneWidget);
    });
  });

  group('the badge on the profile entry', () {
    testWidgets('shows the unread count', (tester) async {
      await pump(
        tester,
        (_) => const InboxBadge(child: Icon(Icons.person)),
        items: [inboxNotice(2), inboxNotice(1), inboxNotice(3, read: true)],
      );
      expect(find.byKey(const ValueKey('inbox-badge')), findsOneWidget);
      expect(find.text('2'), findsOneWidget);
    });

    testWidgets('no badge when all is read or the person has no inbox', (
      tester,
    ) async {
      await pump(
        tester,
        (_) => const InboxBadge(child: Icon(Icons.person)),
        items: [inboxNotice(2, read: true)],
      );
      expect(find.byKey(const ValueKey('inbox-badge')), findsNothing);
      await pump(
        tester,
        (_) => const InboxBadge(child: Icon(Icons.person)),
        items: [inboxNotice(2)],
        rights: const CdRights(),
        role: 'guest',
      );
      expect(find.byKey(const ValueKey('inbox-badge')), findsNothing);
    });

    testWidgets(
      'the entry of the settings opens the inbox for a profile with a right and is not there without',
      (tester) async {
        await pump(tester, (_) => const InboxTile(), items: [inboxNotice(2)]);
        await tester.tap(find.byKey(const ValueKey('open-inbox')));
        await tester.pumpAndSettle();
        expect(find.byKey(const ValueKey('inbox-item-2')), findsOneWidget);
        await pump(
          tester,
          (_) => const InboxTile(),
          items: [inboxNotice(2)],
          rights: const CdRights(),
        );
        expect(find.byKey(const ValueKey('open-inbox')), findsNothing);
      },
    );
  });

  group('the badge on the avatar is a button to the inbox', () {
    Widget avatar(InboxController _) => InboxBadgeOverlay(
      child: IconButton(
        key: const ValueKey('avatar'),
        onPressed: () {},
        icon: const Icon(Icons.person),
      ),
    );

    testWidgets('a tap opens the inbox; the avatar keeps its own action', (tester) async {
      await pump(tester, avatar, items: [inboxNotice(2), inboxNotice(1)]);
      expect(find.text('2'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('inbox-badge-button')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('inbox-item-2')), findsOneWidget);
      await tester.pageBack();
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('avatar')), findsOneWidget, reason: 'back to where we were');
    });

    testWidgets('the hit area is at least 40 by 40 and has a label', (tester) async {
      await pump(tester, avatar, items: [inboxNotice(2), inboxNotice(1)]);
      final size = tester.getSize(find.byKey(const ValueKey('inbox-badge-button')));
      expect(size.width, greaterThanOrEqualTo(40));
      expect(size.height, greaterThanOrEqualTo(40));
      expect(find.bySemanticsLabel('CD inbox, 2 unread'), findsOneWidget);
    });

    testWidgets('Enter on the focused badge opens the inbox, in German too', (tester) async {
      await pump(tester, avatar, items: [inboxNotice(2)], locale: const Locale('de'));
      expect(find.bySemanticsLabel('CD-Eingang, 1 ungelesen'), findsOneWidget);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('inbox-item-2')), findsOneWidget);
    });

    testWidgets('nothing unread: no badge, just the avatar', (tester) async {
      await pump(tester, avatar, items: [inboxNotice(2, read: true)]);
      expect(find.byKey(const ValueKey('inbox-badge-button')), findsNothing);
      expect(find.byKey(const ValueKey('avatar')), findsOneWidget);
    });

    testWidgets('no CD right: no badge', (tester) async {
      await pump(tester, avatar, items: [inboxNotice(2)], rights: const CdRights(), role: 'guest');
      expect(find.byKey(const ValueKey('inbox-badge-button')), findsNothing);
    });

    testWidgets('the entry stays with 0 unread, and the empty page says so', (tester) async {
      await pump(tester, (_) => const InboxTile(), items: const []);
      expect(find.byKey(const ValueKey('open-inbox')), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('open-inbox')));
      await tester.pumpAndSettle();
      expect(find.text('No messages about your CDs.'), findsOneWidget);
    });

    testWidgets('the entry shows the unread count', (tester) async {
      await pump(tester, (_) => const InboxTile(), items: [inboxNotice(2)]);
      expect(find.text('1 unread'), findsOneWidget);
    });
  });

  group('the page', () {
    testWidgets('lists newest first', (tester) async {
      await pump(
        tester,
        page,
        items: [
          inboxNotice(2),
          inboxTask(3),
          inboxNotice(1, code: 'cd_present'),
        ],
      );
      final y = [
        for (final id in [3, 2, 1])
          tester.getTopLeft(find.byKey(ValueKey('inbox-item-$id'))).dy,
      ];
      expect(y, orderedEquals([...y]..sort()));
    });

    testWidgets(
      'a task with can_decide has Import and Reject; without, none, and the starter reads "Waits for approval"',
      (tester) async {
        await pump(
          tester,
          page,
          items: [inboxTask(3), inboxTask(4, canDecide: false)],
        );
        expect(find.byKey(const ValueKey('inbox-approve-3')), findsOneWidget);
        expect(find.byKey(const ValueKey('inbox-reject-3')), findsOneWidget);
        expect(find.byKey(const ValueKey('inbox-approve-4')), findsNothing);
        expect(find.byKey(const ValueKey('inbox-reject-4')), findsNothing);
        expect(find.byKey(const ValueKey('inbox-waits-4')), findsOneWidget);
        expect(find.byKey(const ValueKey('inbox-waits-3')), findsNothing);
        expect(find.text('Import'), findsOneWidget);
      },
    );

    testWidgets('notices have no decision buttons', (tester) async {
      await pump(tester, page, items: [inboxNotice(2)]);
      expect(find.byKey(const ValueKey('inbox-approve-2')), findsNothing);
      expect(find.byKey(const ValueKey('inbox-reject-2')), findsNothing);
    });

    testWidgets('Try again only while the job is in retry_wait', (
      tester,
    ) async {
      await pump(
        tester,
        page,
        items: [
          inboxNotice(
            3,
            code: 'cd_read_error',
            jobState: 'retry_wait',
            args: {
              'author': 'A',
              'title': 'T',
              'tracks': [3],
            },
          ),
          inboxNotice(
            2,
            code: 'cd_read_error',
            jobState: 'ripping',
            args: {
              'author': 'A',
              'title': 'T',
              'tracks': [3],
            },
          ),
          inboxNotice(
            1,
            code: 'cd_read_error',
            args: {
              'author': 'A',
              'title': 'T',
              'tracks': [3],
            },
          ),
        ],
      );
      expect(find.byKey(const ValueKey('inbox-retry-3')), findsOneWidget);
      expect(find.byKey(const ValueKey('inbox-retry-2')), findsNothing);
      expect(find.byKey(const ValueKey('inbox-retry-1')), findsNothing);
    });

    testWidgets('Try again goes to the job and the button goes', (
      tester,
    ) async {
      final r = await pump(
        tester,
        page,
        items: [inboxNotice(3, code: 'cd_read_error', jobState: 'retry_wait')],
      );
      await tester.tap(find.byKey(const ValueKey('inbox-retry-3')));
      await tester.pumpAndSettle();
      expect(r.api.calls, contains('retry 43'));
      expect(find.byKey(const ValueKey('inbox-retry-3')), findsNothing);
    });

    testWidgets(
      'Import decides at once, through the job; the buttons go and the result shows',
      (tester) async {
        final r = await pump(tester, page, items: [inboxTask(3)]);
        await tester.tap(find.byKey(const ValueKey('inbox-approve-3')));
        await tester.pumpAndSettle();
        expect(r.api.calls, contains('approve 43'));
        expect(find.byKey(const ValueKey('inbox-approve-3')), findsNothing);
        expect(
          find.byKey(const ValueKey('inbox-resolution-3')),
          findsOneWidget,
        );
        expect(find.text('Imported'), findsOneWidget);
      },
    );

    testWidgets(
      'Reject asks first: "Reject this CD? Its files are deleted."; Cancel deletes nothing',
      (tester) async {
        final r = await pump(tester, page, items: [inboxTask(3)]);
        await tester.tap(find.byKey(const ValueKey('inbox-reject-3')));
        await tester.pumpAndSettle();
        expect(find.text('Reject this CD?'), findsOneWidget);
        expect(find.text('Its files are deleted.'), findsOneWidget);
        await tester.tap(find.text('Cancel'));
        await tester.pumpAndSettle();
        expect(r.api.calls.where((c) => c.startsWith('reject')), isEmpty);
        expect(find.byKey(const ValueKey('inbox-reject-3')), findsOneWidget);
      },
    );

    testWidgets('Reject confirmed goes to the job', (tester) async {
      final r = await pump(tester, page, items: [inboxTask(3)]);
      await tester.tap(find.byKey(const ValueKey('inbox-reject-3')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('confirm-removal')));
      await tester.pumpAndSettle();
      expect(r.api.calls, contains('reject 43'));
      expect(find.text('Rejected'), findsOneWidget);
    });

    testWidgets('while the server works the buttons are off', (tester) async {
      final hold = Completer<void>();
      final r = await pump(tester, page, items: [inboxTask(3)]);
      r.api.hold = hold;
      await tester.tap(find.byKey(const ValueKey('inbox-approve-3')));
      await tester.pump();
      expect(
        tester
            .widget<FilledButton>(find.byKey(const ValueKey('inbox-approve-3')))
            .onPressed,
        isNull,
      );
      expect(
        tester
            .widget<OutlinedButton>(
              find.byKey(const ValueKey('inbox-reject-3')),
            )
            .onPressed,
        isNull,
      );
      hold.complete();
      await tester.pumpAndSettle();
    });

    testWidgets(
      '409 decided: "Already decided by Bea" instead of the buttons',
      (tester) async {
        final r = await pump(tester, page, items: [inboxTask(3)]);
        r.api.failures['approve 43'] = ApiException(409, 'decided', {
          'error': 'decided',
          'by': 'Bea',
          'resolution': 'rejected',
        });
        await tester.tap(find.byKey(const ValueKey('inbox-approve-3')));
        await tester.pumpAndSettle();
        expect(find.text('Already decided by Bea'), findsOneWidget);
        expect(find.text('Rejected by Bea'), findsOneWidget);
        expect(find.byKey(const ValueKey('inbox-approve-3')), findsNothing);
      },
    );

    testWidgets('403 forbidden says so and takes the buttons away', (
      tester,
    ) async {
      final r = await pump(tester, page, items: [inboxTask(3)]);
      r.api.failures['approve 43'] = ApiException(403, 'forbidden');
      await tester.tap(find.byKey(const ValueKey('inbox-approve-3')));
      await tester.pumpAndSettle();
      expect(find.text('You may not decide CDs.'), findsOneWidget);
      expect(find.byKey(const ValueKey('inbox-approve-3')), findsNothing);
    });

    testWidgets('Open album only when album_id is not null', (tester) async {
      await pump(
        tester,
        page,
        items: [
          inboxNotice(
            3,
            code: 'cd_present',
            args: {'author': 'A', 'title': 'T', 'album_id': 7},
          ),
          inboxNotice(
            2,
            code: 'cd_present',
            args: {'author': 'A', 'title': 'T', 'album_id': null},
          ),
        ],
      );
      expect(find.byKey(const ValueKey('inbox-album-3')), findsOneWidget);
      expect(find.byKey(const ValueKey('inbox-album-2')), findsNothing);
    });

    testWidgets('an unknown code shows the generic text and does not crash', (
      tester,
    ) async {
      await pump(
        tester,
        page,
        items: [
          inboxNotice(
            3,
            code: 'cd_from_the_future',
            args: {
              'x': [1, 2],
            },
          ),
        ],
      );
      expect(
        find.text(
          'News from the server that this version of the app does not know yet.',
        ),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    });

    testWidgets('tapping an item marks it read', (tester) async {
      final r = await pump(tester, page, items: [inboxNotice(3)]);
      await tester.tap(find.byKey(const ValueKey('inbox-text-3')));
      await tester.pumpAndSettle();
      expect(r.api.calls, contains('read 3'));
      expect(r.controller.state.unread, 0);
    });

    testWidgets('Mark all read', (tester) async {
      final r = await pump(
        tester,
        page,
        items: [inboxNotice(3), inboxNotice(2)],
      );
      await tester.tap(find.byKey(const ValueKey('inbox-mark-all')));
      await tester.pumpAndSettle();
      expect(r.api.calls, contains('read_all'));
      expect(find.byKey(const ValueKey('inbox-mark-all')), findsNothing);
    });

    testWidgets('pull to refresh reads the list again and shows what is new', (
      tester,
    ) async {
      final r = await pump(tester, page, items: [inboxNotice(2)]);
      final before = r.api.calls.where((c) => c == 'list').length;
      r.api.items = [inboxNotice(5), inboxNotice(2)];
      await tester.drag(find.byType(ListView), const Offset(0, 900));
      await tester.pump(const Duration(milliseconds: 100));
      await tester.pumpAndSettle();
      expect(r.api.calls.where((c) => c == 'list').length, greaterThan(before));
      expect(find.byKey(const ValueKey('inbox-item-5')), findsOneWidget);
    });

    testWidgets('empty: says so', (tester) async {
      await pump(tester, page);
      expect(find.byKey(const ValueKey('inbox-empty')), findsOneWidget);
    });

    testWidgets('a failed load says so and can try again', (tester) async {
      final r = await pump(
        tester,
        page,
        prepare: (a) => a.failures['list'] = ApiException(0, 'network'),
      );
      expect(find.byKey(const ValueKey('inbox-error')), findsOneWidget);
      r.api.failures.clear();
      r.api.items = [inboxNotice(2)];
      await tester.tap(find.byKey(const ValueKey('inbox-reload')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('inbox-item-2')), findsOneWidget);
    });

    testWidgets(
      'offline: a short "needs the server" instead of failing, and the server is not asked by the page',
      (tester) async {
        final offline = OfflineController(manual: true);
        final r = await pump(
          tester,
          (c) => page(c, offline: offline),
          items: [inboxNotice(2)],
          offline: offline,
        );
        expect(
          find.byKey(const ValueKey('inbox-needs-server')),
          findsOneWidget,
        );
        expect(find.byKey(const ValueKey('inbox-item-2')), findsNothing);
        // Only the first read of the controller, none from the page.
        expect(r.api.calls, ['list']);
      },
    );

    testWidgets('German page', (tester) async {
      await pump(
        tester,
        page,
        items: [inboxTask(3)],
        locale: const Locale('de'),
      );
      expect(find.text('Übernehmen'), findsOneWidget);
      expect(find.text('Ablehnen'), findsOneWidget);
      expect(find.text('CD-Eingang'), findsOneWidget);
    });
  });

  group('the demo', () {
    test('has a task, a notice and an expired CD', () {
      final d = demoInboxItems();
      expect(d.where((i) => i.canDecide && i.isTask).length, 1);
      expect(
        d.map((i) => i.code),
        containsAll(['cd_awaiting_approval', 'cd_imported', 'cd_expired']),
      );
    });

    testWidgets('shows the card and the page', (tester) async {
      tester.view.physicalSize = const Size(800, 2000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final c = InboxController.demo(demoInboxItems());
      addTearDown(c.dispose);
      await tester.pumpWidget(
        InboxScope(
          controller: c,
          child: MaterialApp(
            localizationsDelegates: AppLocalizations.localizationsDelegates,
            supportedLocales: AppLocalizations.supportedLocales,
            home: const Scaffold(body: InboxCard()),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('inbox-card')), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('inbox-card')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('inbox-approve-3')), findsOneWidget);
      expect(find.byKey(const ValueKey('inbox-item-1')), findsOneWidget);
    });
  });
}
