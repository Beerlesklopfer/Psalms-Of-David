// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:psalm/inbox/inbox_api.dart';
import 'package:psalm/inbox/inbox_models.dart';
import 'package:psalm/inbox/inbox_notifier.dart';
import 'package:psalm/server/api_client.dart';

InboxItem inboxTask(
  int id, {
  bool canDecide = true,
  String by = 'Anna',
  Map<String, dynamic>? args,
}) => InboxItem(
  id: id,
  kind: 'task',
  code: 'cd_awaiting_approval',
  jobId: id + 40,
  createdMs: 1793000000000,
  canDecide: canDecide,
  jobState: 'awaiting_approval',
  args:
      args ??
      {
        'job_id': id + 40,
        'author': 'Ken Follett',
        'title': 'Sturz der Titanen',
        'disc': 1,
        'discs_total': 12,
        'tracks': 14,
        'by': by,
      },
);

InboxItem inboxNotice(
  int id, {
  String code = 'cd_imported',
  Map<String, dynamic>? args,
  String? jobState,
  bool read = false,
}) => InboxItem(
  id: id,
  kind: 'notice',
  code: code,
  jobId: id + 40,
  createdMs: 1793000000000,
  read: read,
  jobState: jobState,
  args:
      args ??
      {
        'job_id': id + 40,
        'author': 'Ken Follett',
        'title': 'Sturz der Titanen',
        'disc': 1,
        'tracks': 14,
      },
);

/// An inbox server in memory: [items] are the list, every call is recorded, and a failure can be set per call name.
class FakeInboxApi implements InboxApi {
  List<InboxItem> items = [];
  int? next;
  final calls = <String>[];
  final Map<String, ApiException> failures = {};
  Completer<void>? hold;

  Future<void> _go(String name) async {
    calls.add(name);
    if (hold != null) await hold!.future;
    final f = failures[name];
    if (f != null) throw f;
  }

  @override
  Future<InboxPage> list({int? after, int? limit}) async {
    await _go('list${after == null ? '' : ' after $after'}');
    return InboxPage(
      items: items,
      unread: items.where((i) => !i.read).length,
      tasksOpen: items.where((i) => i.canDecide && !i.resolved).length,
      next: after == null ? next : null,
    );
  }

  @override
  Future<void> markRead(int id) => _go('read $id');

  @override
  Future<void> markAllRead() => _go('read_all');

  @override
  Future<void> approve(int jobId) => _go('approve $jobId');

  @override
  Future<void> reject(int jobId) => _go('reject $jobId');

  @override
  Future<void> retry(int jobId) => _go('retry $jobId');
}

class FakeNotifier implements InboxNotifier {
  final shown = <InboxNotice>[];
  final closed = <int>[];
  final controller = StreamController<InboxNotifierEvent>.broadcast();

  @override
  Future<void> show(InboxNotice notice) async => shown.add(notice);

  @override
  Future<void> close(int id) async => closed.add(id);

  @override
  Stream<InboxNotifierEvent> get events => controller.stream;

  @override
  Future<void> dispose() async {}
}
