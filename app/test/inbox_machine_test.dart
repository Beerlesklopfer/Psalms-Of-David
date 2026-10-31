// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/inbox/inbox_machine.dart';
import 'package:psalm/inbox/inbox_models.dart';

InboxItem task(
  int id, {
  bool canDecide = true,
  bool read = false,
  int job = 40,
  String? jobState = 'awaiting_approval',
}) => InboxItem(
  id: id,
  kind: 'task',
  code: 'cd_awaiting_approval',
  jobId: job + id,
  canDecide: canDecide,
  read: read,
  jobState: jobState,
  args: const {'author': 'A', 'title': 'T'},
);

InboxItem notice(
  int id, {
  bool read = false,
  String code = 'cd_imported',
  String? jobState,
}) => InboxItem(
  id: id,
  kind: 'notice',
  code: code,
  read: read,
  jobId: id,
  jobState: jobState,
);

/// Compares value by value so that a matcher can stand in the expected list (a record cannot hold one).
void expectAll(List<Object?> actual, List<Object?> expected) {
  expect(actual.length, expected.length);
  for (var i = 0; i < actual.length; i++) {
    expect(actual[i], expected[i], reason: 'member $i');
  }
}

InboxPage page(
  List<InboxItem> items, {
  int? unread,
  int? tasksOpen,
  int? next,
}) => InboxPage(
  items: items,
  unread: unread ?? items.where((i) => !i.read).length,
  tasksOpen: tasksOpen ?? items.where((i) => i.canDecide && !i.resolved).length,
  next: next,
);

/// Runs events one after the other and returns the last state with all effects.
(InboxState, List<InboxEffect>) run(InboxState s, List<InboxEvent> events) {
  final fx = <InboxEffect>[];
  for (final e in events) {
    final (n, f) = inboxTransition(s, e);
    s = n;
    fx.addAll(f);
  }
  return (s, fx);
}

InboxState ready(
  List<InboxItem> items, {
  int? unread,
  int? tasksOpen,
  int? next,
}) => run(const InboxState(), [
  const InboxLoad(),
  InboxPageLoaded(
    page(items, unread: unread, tasksOpen: tasksOpen, next: next),
  ),
]).$1;

void main() {
  group('loading', () {
    test('idle -> loading asks for the first page', () {
      final (s, fx) = inboxTransition(const InboxState(), const InboxLoad());
      expect(s.phase, InboxPhase.loading);
      expect(
        fx.single,
        isA<FetchPage>().having((f) => f.after, 'after', isNull),
      );
    });

    test(
      'loading -> ready with the page and the counts of the whole inbox',
      () {
        final s = ready(
          [task(3), notice(2), notice(1, read: true)],
          unread: 9,
          tasksOpen: 4,
          next: 1,
        );
        expectAll(
          [
            s.phase,
            s.items.map((i) => i.id).toList(),
            s.unread,
            s.tasksOpen,
            s.next,
          ],
          [
            InboxPhase.ready,
            [3, 2, 1],
            9,
            4,
            1,
          ],
        );
      },
    );

    test('a second load while loading or ready does nothing', () {
      final (s, fx) = inboxTransition(ready([notice(1)]), const InboxLoad());
      expectAll([s.phase, fx], [InboxPhase.ready, isEmpty]);
    });

    test('the first load failing is the error state, and a refresh from there loads again', () {
      final (e, _) = run(const InboxState(), [
        const InboxLoad(),
        const InboxLoadFailed('network'),
      ]);
      expectAll([e.phase, e.error], [InboxPhase.error, 'network']);
      final (n, fx) = inboxTransition(e, const InboxRefresh());
      expectAll(
        [n.phase, n.error, fx.single],
        [InboxPhase.loading, null, isA<FetchPage>()],
      );
    });

    test('a failed refresh keeps what was shown', () {
      final (s, _) = run(ready([notice(1)]), [
        const InboxRefresh(),
        const InboxLoadFailed('network'),
      ]);
      expectAll(
        [s.phase, s.items.length, s.error],
        [InboxPhase.ready, 1, 'network'],
      );
    });
  });

  group('refreshing', () {
    test('ready -> refreshing keeps the list; the answer replaces it (the server removed what is gone)', () {
      final (r, fx) = inboxTransition(
        ready([notice(2), notice(1)]),
        const InboxRefresh(),
      );
      expectAll(
        [r.phase, r.items.length, fx.single],
        [InboxPhase.refreshing, 2, isA<FetchPage>()],
      );
      final (s, _) = inboxTransition(r, InboxPageLoaded(page([notice(2)])));
      expectAll(
        [s.phase, s.items.map((i) => i.id).toList()],
        [
          InboxPhase.ready,
          [2],
        ],
      );
    });

    test('a refresh while one is on its way asks nothing twice', () {
      final (r, _) = inboxTransition(ready([notice(1)]), const InboxRefresh());
      expect(inboxTransition(r, const InboxRefresh()).$2, isEmpty);
    });

    test('an item that came over the channel while the page was on its way, and is newer than the page, stays', () {
      final (r, _) = inboxTransition(ready([notice(1)]), const InboxRefresh());
      final (a, _) = inboxTransition(
        r,
        InboxWireEvent(InboxArrived(notice(5), unread: 2, tasksOpen: 0)),
      );
      final (s, _) = inboxTransition(
        a,
        InboxPageLoaded(page([notice(1)], unread: 1)),
      );
      expect(s.items.map((i) => i.id).toList(), [5, 1]);
      expect(s.arrived, isEmpty);
    });

    test('an item the page knows is taken from the page, once', () {
      final (r, _) = inboxTransition(ready([notice(1)]), const InboxRefresh());
      final (a, _) = inboxTransition(
        r,
        InboxWireEvent(InboxArrived(task(5), unread: 2, tasksOpen: 1)),
      );
      final (s, _) = inboxTransition(
        a,
        InboxPageLoaded(page([task(5, canDecide: false), notice(1)])),
      );
      expect(s.items.map((i) => i.id).toList(), [5, 1]);
      expect(s.item(5)!.canDecide, isFalse);
    });
  });

  group('paging', () {
    test('more asks after next, appends, and stops at the last page', () {
      final r = ready([notice(3), notice(2)], next: 2);
      final (m, fx) = inboxTransition(r, const InboxMore());
      expectAll([m.loadingMore, (fx.single as FetchPage).after], [true, 2]);
      expect(inboxTransition(m, const InboxMore()).$2, isEmpty);
      final (s, _) = inboxTransition(
        m,
        InboxPageLoaded(page([notice(1)], unread: 3), append: true),
      );
      expectAll(
        [s.items.map((i) => i.id).toList(), s.next, s.loadingMore],
        [
          [3, 2, 1],
          null,
          false,
        ],
      );
      expect(inboxTransition(s, const InboxMore()).$2, isEmpty);
    });

    test('a failed next page leaves the list as it is', () {
      final (m, _) = inboxTransition(
        ready([notice(2)], next: 2),
        const InboxMore(),
      );
      final (s, _) = inboxTransition(
        m,
        const InboxLoadFailed('network', append: true),
      );
      expectAll(
        [s.phase, s.loadingMore, s.items.length],
        [InboxPhase.ready, false, 1],
      );
    });
  });

  group('messages of the channel', () {
    test('a new item goes to the top and the counts are the server\'s', () {
      final (s, fx) = inboxTransition(
        ready([task(2), notice(1)], unread: 2, tasksOpen: 1),
        InboxWireEvent(InboxArrived(notice(3), unread: 3, tasksOpen: 1)),
      );
      expectAll(
        [s.items.map((i) => i.id).toList(), s.unread, s.tasksOpen, fx],
        [
          [3, 2, 1],
          3,
          1,
          isEmpty,
        ],
      );
    });

    test('the same item twice is one item', () {
      final a = InboxWireEvent(
        InboxArrived(notice(3), unread: 1, tasksOpen: 0),
      );
      final (s, _) = run(ready([notice(1)]), [a, a]);
      expect(s.items.map((i) => i.id).toList(), [3, 1]);
    });

    test('an inbox message before the first page is kept for the page', () {
      final (s, _) = run(const InboxState(), [
        const InboxLoad(),
        InboxWireEvent(InboxArrived(notice(7), unread: 1, tasksOpen: 0)),
        InboxPageLoaded(page([notice(3)])),
      ]);
      expect(s.items.map((i) => i.id).toList(), [7, 3]);
    });

    test('inbox_resolved takes the buttons from the task for everybody and counts it as read', () {
      final r = ready([task(2), notice(1)], unread: 2, tasksOpen: 1);
      final (s, _) = inboxTransition(
        r,
        const InboxWireEvent(
          InboxResolvedWire(
            id: 2,
            jobId: 42,
            resolution: 'approved',
            by: 'Bea',
            resolvedMs: 9,
            unread: 1,
            tasksOpen: 0,
          ),
        ),
      );
      final t = s.item(2)!;
      expectAll(
        [
          t.resolved,
          t.resolution,
          t.by,
          t.canDecide,
          t.read,
          s.unread,
          s.tasksOpen,
        ],
        [true, 'approved', 'Bea', false, true, 1, 0],
      );
    });

    test('the task of the starter, who could not decide it, disappears when it is decided', () {
      final r = ready([task(2, canDecide: false)], unread: 1, tasksOpen: 0);
      final (s, _) = inboxTransition(
        r,
        const InboxWireEvent(
          InboxResolvedWire(
            id: 2,
            jobId: 42,
            resolution: 'rejected',
            by: 'Bea',
            resolvedMs: 9,
            unread: 0,
            tasksOpen: 0,
          ),
        ),
      );
      expectAll([s.items, s.unread], [isEmpty, 0]);
    });

    test(
      'inbox_resolved for an item that is not loaded still sets the counts',
      () {
        final (s, _) = inboxTransition(
          ready([notice(1)]),
          const InboxWireEvent(
            InboxResolvedWire(
              id: 99,
              jobId: null,
              resolution: 'expired',
              by: '',
              resolvedMs: null,
              unread: 4,
              tasksOpen: 2,
            ),
          ),
        );
        expectAll([s.unread, s.tasksOpen, s.items.length], [4, 2, 1]);
      },
    );

    test('the snapshot sets the counts at once and reads the list again (nothing is replayed)', () {
      final (s, fx) = inboxTransition(
        ready([notice(1)]),
        const InboxWireEvent(InboxCountsWire(unread: 5, tasksOpen: 2)),
      );
      expectAll(
        [s.unread, s.tasksOpen, s.phase, fx.single],
        [5, 2, InboxPhase.refreshing, isA<FetchPage>()],
      );
    });

    test('the snapshot of the first connect while the list loads does not ask twice', () {
      final (l, _) = inboxTransition(const InboxState(), const InboxLoad());
      final (s, fx) = inboxTransition(
        l,
        const InboxWireEvent(InboxCountsWire(unread: 1, tasksOpen: 0)),
      );
      expectAll([s.phase, s.unread, fx], [InboxPhase.loading, 1, isEmpty]);
    });

    test('the snapshot after an error tries again', () {
      final (e, _) = run(const InboxState(), [
        const InboxLoad(),
        const InboxLoadFailed('network'),
      ]);
      final (s, fx) = inboxTransition(
        e,
        const InboxWireEvent(InboxCountsWire(unread: 1, tasksOpen: 0)),
      );
      expectAll([s.phase, fx.single], [InboxPhase.loading, isA<FetchPage>()]);
    });
  });

  group('marking read', () {
    test('mark read: the item is read, the count falls, the server is told; again does nothing', () {
      final (s, fx) = inboxTransition(
        ready([notice(2), notice(1)]),
        const InboxMarkRead(2),
      );
      expectAll(
        [s.item(2)!.read, s.unread, fx.single],
        [true, 1, isA<PostRead>().having((p) => p.id, 'id', 2)],
      );
      expect(inboxTransition(s, const InboxMarkRead(2)).$2, isEmpty);
    });

    test('the count never goes below zero, an unknown id does nothing', () {
      final r = ready([notice(1)], unread: 0);
      expect(inboxTransition(r, const InboxMarkRead(1)).$1.unread, 0);
      expect(inboxTransition(r, const InboxMarkRead(77)).$2, isEmpty);
    });

    test('mark all read', () {
      final (s, fx) = inboxTransition(
        ready([notice(2), notice(1)]),
        const InboxMarkAllRead(),
      );
      expectAll(
        [s.items.every((i) => i.read), s.unread, fx.single],
        [true, 0, isA<PostReadAll>()],
      );
      expect(inboxTransition(s, const InboxMarkAllRead()).$2, isEmpty);
    });

    test('a mark the server did not take reads the list again', () {
      final (s, fx) = inboxTransition(
        ready([notice(1)]),
        const InboxMarkFailed(),
      );
      expectAll(
        [s.phase, fx.single],
        [InboxPhase.refreshing, isA<FetchPage>()],
      );
    });
  });

  group('deciding', () {
    test(
      'Import: deciding for that item, the effect names the JOB, not the item',
      () {
        final (s, fx) = inboxTransition(
          ready([task(2)]),
          const InboxDecide(2, approve: true),
        );
        expect(s.deciding, {2});
        expect(
          fx.single,
          isA<PostDecide>().having(
            (p) => (p.id, p.jobId, p.approve),
            'id, job, approve',
            (2, 42, true),
          ),
        );
      },
    );

    test('a second press while deciding does nothing', () {
      final (s, _) = inboxTransition(
        ready([task(2)]),
        const InboxDecide(2, approve: true),
      );
      expect(
        inboxTransition(s, const InboxDecide(2, approve: false)).$2,
        isEmpty,
      );
    });

    test(
      'no decision without can_decide, for a decided task or an unknown item',
      () {
        expect(
          inboxTransition(
            ready([task(2, canDecide: false)]),
            const InboxDecide(2, approve: true),
          ).$2,
          isEmpty,
        );
        expect(
          inboxTransition(
            ready([notice(2)]),
            const InboxDecide(2, approve: true),
          ).$2,
          isEmpty,
        );
        expect(
          inboxTransition(
            ready([task(2)]),
            const InboxDecide(9, approve: true),
          ).$2,
          isEmpty,
        );
      },
    );

    test('the response settles the item: done, read, one task less', () {
      final (s, _) = run(ready([task(2), notice(1)], unread: 2, tasksOpen: 1), [
        const InboxDecide(2, approve: true),
        const InboxDecided(2, approve: true),
      ]);
      final t = s.item(2)!;
      expectAll(
        [
          s.deciding,
          t.resolved,
          t.resolution,
          t.canDecide,
          t.read,
          s.unread,
          s.tasksOpen,
        ],
        [isEmpty, true, 'approved', false, true, 1, 0],
      );
    });

    test('Reject is settled as rejected', () {
      final (s, _) = run(ready([task(2)]), [
        const InboxDecide(2, approve: false),
        const InboxDecided(2, approve: false),
      ]);
      expect(s.item(2)!.resolution, 'rejected');
    });

    test('the channel faster than the response: its information stays (who decided)', () {
      final (s, _) = run(ready([task(2)]), [
        const InboxDecide(2, approve: true),
        const InboxWireEvent(
          InboxResolvedWire(
            id: 2,
            jobId: 42,
            resolution: 'approved',
            by: 'Anna',
            resolvedMs: 5,
            unread: 0,
            tasksOpen: 0,
          ),
        ),
        const InboxDecided(2, approve: true),
      ]);
      expectAll([s.item(2)!.by, s.deciding, s.tasksOpen], ['Anna', isEmpty, 0]);
    });

    test('409 decided: the loser sees who decided and what', () {
      final (s, _) = run(ready([task(2)], unread: 1, tasksOpen: 1), [
        const InboxDecide(2, approve: false),
        const InboxDecideFailed(
          2,
          'decided',
          by: 'Bea',
          resolution: 'approved',
        ),
      ]);
      final t = s.item(2)!;
      expectAll(
        [
          t.resolved,
          t.resolution,
          t.by,
          t.canDecide,
          s.deciding,
          s.failures[2],
          s.tasksOpen,
        ],
        [true, 'approved', 'Bea', false, isEmpty, 'decided', 0],
      );
    });

    test('403 forbidden takes the buttons away and says why', () {
      final (s, _) = run(ready([task(2)]), [
        const InboxDecide(2, approve: true),
        const InboxDecideFailed(2, 'forbidden'),
      ]);
      expectAll(
        [s.item(2)!.canDecide, s.failures[2], s.deciding],
        [false, 'forbidden', isEmpty],
      );
    });

    test('409 invalid_move and 404 read the list again', () {
      for (final code in ['invalid_move', 'not_found']) {
        final (s, fx) = run(ready([task(2)]), [
          const InboxDecide(2, approve: true),
          InboxDecideFailed(2, code),
        ]);
        expectAll(
          [s.phase, s.deciding, s.failures[2], fx.last],
          [InboxPhase.refreshing, isEmpty, code, isA<FetchPage>()],
        );
      }
    });

    test('a network error lets the person try again', () {
      final (s, _) = run(ready([task(2)]), [
        const InboxDecide(2, approve: true),
        const InboxDecideFailed(2, 'network'),
      ]);
      expectAll(
        [s.deciding, s.failures[2], s.item(2)!.canDecide],
        [isEmpty, 'network', true],
      );
      final (again, fx) = inboxTransition(
        s,
        const InboxDecide(2, approve: true),
      );
      expectAll([again.failures, fx.single], [isEmpty, isA<PostDecide>()]);
    });
  });

  group('trying again', () {
    InboxState waiting() =>
        ready([notice(2, code: 'cd_read_error', jobState: 'retry_wait')]);

    test('only while the job is in retry_wait', () {
      final (s, fx) = inboxTransition(waiting(), const InboxRetry(2));
      expectAll(
        [s.retrying, fx.single],
        [
          {2},
          isA<PostRetry>().having((p) => p.jobId, 'job', 2),
        ],
      );
      expect(
        inboxTransition(
          ready([notice(2, code: 'cd_read_error', jobState: 'ripping')]),
          const InboxRetry(2),
        ).$2,
        isEmpty,
      );
      expect(
        inboxTransition(
          ready([notice(2, code: 'cd_read_error')]),
          const InboxRetry(2),
        ).$2,
        isEmpty,
      );
    });

    test('done: the job reads again, so the button goes', () {
      final (s, _) = run(waiting(), [
        const InboxRetry(2),
        const InboxRetried(2),
      ]);
      expectAll(
        [s.retrying, s.item(2)!.jobState, s.item(2)!.canRetry],
        [isEmpty, 'ripping', false],
      );
    });

    test(
      'busy keeps the button and says why; invalid_move reads the list again',
      () {
        final (s, _) = run(waiting(), [
          const InboxRetry(2),
          const InboxRetryFailed(2, 'busy'),
        ]);
        expectAll(
          [s.retrying, s.failures[2], s.item(2)!.canRetry],
          [isEmpty, 'busy', true],
        );
        final (m, fx) = run(waiting(), [
          const InboxRetry(2),
          const InboxRetryFailed(2, 'invalid_move'),
        ]);
        expectAll(
          [m.phase, fx.last],
          [InboxPhase.refreshing, isA<FetchPage>()],
        );
      },
    );
  });

  test('an unknown code does not trouble the machine', () {
    final odd = InboxItem(
      id: 4,
      kind: 'notice',
      code: 'cd_from_the_future',
      args: const {'x': 1},
    );
    final (s, _) = run(ready([odd]), [const InboxMarkRead(4)]);
    expectAll([s.item(4)!.read, s.unread], [true, 0]);
  });
}
