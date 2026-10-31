// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/inbox/inbox_controller.dart';
import 'package:psalm/inbox/inbox_machine.dart';
import 'package:psalm/inbox/inbox_models.dart';
import 'package:psalm/inbox/inbox_notifier.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/server/api_client.dart';

import 'inbox_fakes.dart';

Future<void> settle() => Future<void>.delayed(Duration.zero);

void main() {
  late FakeInboxApi api;
  late FakeNotifier notifier;
  late StreamController<InboxWire> wire;
  late InboxController c;

  setUp(() async {
    api = FakeInboxApi()..items = [inboxTask(3), inboxNotice(2)];
    notifier = FakeNotifier();
    wire = StreamController<InboxWire>.broadcast();
    c = InboxController(api: api, wire: wire.stream, notifier: notifier);
    c.l10n = await AppLocalizations.delegate.load(const Locale('en'));
    c.start();
    await settle();
  });

  tearDown(() => c.dispose());

  test('start reads the list', () {
    expect(c.state.phase, InboxPhase.ready);
    expect(
      (c.state.items.length, c.state.unread, c.state.tasksOpen),
      (2, 2, 1),
    );
    expect(api.calls, ['list']);
  });

  test('a new message: the machine takes it, the notification has the buttons of the task', () async {
    wire.add(InboxArrived(inboxTask(5), unread: 3, tasksOpen: 2));
    await settle();
    expect(c.state.items.first.id, 5);
    final n = notifier.shown.single;
    expect(
      (n.id, n.approveLabel, n.rejectLabel, n.retryLabel),
      (5, 'Import', 'Reject', null),
    );
    expect(n.body, contains('Ken Follett'));
  });

  test('the notification of a notice has no buttons; of a read error in retry_wait it has Try again', () async {
    wire.add(InboxArrived(inboxNotice(6), unread: 3, tasksOpen: 1));
    wire.add(
      InboxArrived(
        inboxNotice(7, code: 'cd_read_error', jobState: 'retry_wait'),
        unread: 4,
        tasksOpen: 1,
      ),
    );
    await settle();
    expect(
      (notifier.shown[0].approveLabel, notifier.shown[0].retryLabel),
      (null, null),
    );
    expect(notifier.shown[1].retryLabel, 'Try again');
  });

  test('the starter\'s own held CD: a notification without buttons', () async {
    wire.add(
      InboxArrived(inboxTask(8, canDecide: false), unread: 3, tasksOpen: 1),
    );
    await settle();
    expect(
      (notifier.shown.single.approveLabel, notifier.shown.single.rejectLabel),
      (null, null),
    );
  });

  test('the seam: every handler gets (item, speak, device) once, an item already known does not call them again', () async {
    final seen = <(int, String?, String?)>[];
    c.messageHandlers.add(
      (item, speak, device) => seen.add((item.id, speak, device)),
    );
    final spoken = InboxItem(
      id: 9,
      kind: 'notice',
      code: 'cd_imported',
      speak: 'starter',
      device: 'abc123',
    );
    wire.add(InboxArrived(spoken, unread: 3, tasksOpen: 1));
    wire.add(InboxArrived(spoken, unread: 3, tasksOpen: 1));
    await settle();
    expect(seen, [(9, 'starter', 'abc123')]);
  });

  test('inbox_resolved removes the notification of the task', () async {
    wire.add(
      const InboxResolvedWire(
        id: 3,
        jobId: 43,
        resolution: 'approved',
        by: 'Bea',
        resolvedMs: 1,
        unread: 1,
        tasksOpen: 0,
      ),
    );
    await settle();
    expect(notifier.closed, [3]);
    expect(c.state.item(3)!.resolved, isTrue);
  });

  test('the snapshot (after every connect) reads the list again', () async {
    api.items = [inboxNotice(10), inboxTask(3), inboxNotice(2)];
    wire.add(const InboxCountsWire(unread: 3, tasksOpen: 1));
    await settle();
    expect(api.calls, ['list', 'list']);
    expect(c.state.items.map((i) => i.id).toList(), [10, 3, 2]);
  });

  test('Import from the notification decides; Reject and a tap open the page (the page asks before it deletes)', () async {
    notifier.controller.add(
      const InboxNotifierEvent(3, InboxNoticeAction.approve),
    );
    await settle();
    expect(api.calls, contains('approve 43'));
    expect(c.state.item(3)!.resolved, isTrue);
    final opened = c.openRequests.value;
    notifier.controller.add(
      const InboxNotifierEvent(3, InboxNoticeAction.reject),
    );
    notifier.controller.add(const InboxNotifierEvent(2, null));
    await settle();
    expect(c.openRequests.value, opened + 2);
    expect(api.calls.where((x) => x.startsWith('reject')), isEmpty);
  });

  test('approve: deciding shows while the server works, then it is settled and the notification goes', () async {
    api.hold = Completer<void>();
    c.approve(3);
    expect(c.state.deciding, {3});
    api.hold!.complete();
    await settle();
    expect(
      (
        c.state.deciding.isEmpty,
        c.state.item(3)!.resolution,
        c.state.tasksOpen,
      ),
      (true, 'approved', 0),
    );
    expect(notifier.closed, [3]);
  });

  test('409 decided: the loser sees who decided', () async {
    api.failures['approve 43'] = ApiException(409, 'decided', {
      'error': 'decided',
      'by': 'Bea',
      'resolution': 'rejected',
    });
    c.approve(3);
    await settle();
    final t = c.state.item(3)!;
    expect(
      (
        t.resolved,
        t.by,
        t.resolution,
        c.state.failures[3],
        c.state.deciding.isEmpty,
      ),
      (true, 'Bea', 'rejected', 'decided', true),
    );
  });

  test(
    'the network down: the decision fails with network and can be tried again',
    () async {
      api.failures['reject 43'] = ApiException(0, 'network');
      c.reject(3);
      await settle();
      expect(
        (c.state.failures[3], c.state.item(3)!.canDecide),
        ('network', true),
      );
    },
  );

  test(
    'mark read goes to the server; a refused mark reads the list again',
    () async {
      c.markRead(2);
      await settle();
      expect(api.calls, contains('read 2'));
      api.failures['read 3'] = ApiException(404, 'not_found');
      c.markRead(3);
      await settle();
      expect(api.calls.where((x) => x == 'list').length, 2);
    },
  );

  test('mark all read', () async {
    c.markAllRead();
    await settle();
    expect((api.calls.last, c.state.unread), ('read_all', 0));
  });

  test('refresh completes when the answer is in; a failed first load is the error state', () async {
    await c.refresh();
    expect(c.state.phase, InboxPhase.ready);
    final bad = FakeInboxApi()..failures['list'] = ApiException(0, 'network');
    final c2 = InboxController(api: bad)..start();
    await settle();
    expect((c2.state.phase, c2.state.error), (InboxPhase.error, 'network'));
    c2.dispose();
  });

  test('the demo inbox needs no server', () async {
    final d = InboxController.demo([inboxTask(3), inboxNotice(2)]);
    await settle();
    expect((d.demo, d.state.items.length, d.state.tasksOpen), (true, 2, 1));
    d.approve(3);
    await settle();
    expect(d.state.item(3)!.resolved, isTrue);
    d.dispose();
  });
}
