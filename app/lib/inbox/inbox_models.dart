// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

// The inbox of the CDs as the server sends it (`docs/spec-app-api.md`, "The inbox of the CDs"): the items, one page of the list, and the
// messages of the event channel. Nothing here is prose: the text is made from `code` and `args` in the app's language (`inbox_text.dart`).

/// What a profile may do with CDs (`GET /me` -> `cd_rights`). All false for a child, a guest and a shared device.
class CdRights {
  const CdRights({
    this.mayInsert = false,
    this.noApproval = false,
    this.mayApprove = false,
  });

  final bool mayInsert;
  final bool noApproval;
  final bool mayApprove;

  /// The inbox exists for these profiles only; everybody else gets the empty one.
  bool get hasInbox => mayInsert || mayApprove;

  factory CdRights.fromJson(Object? j) => j is Map
      ? CdRights(
          mayInsert: j['may_insert'] == true,
          noApproval: j['no_approval'] == true,
          mayApprove: j['may_approve'] == true,
        )
      : const CdRights();

  Map<String, Object?> toJson() => {
    'may_insert': mayInsert,
    'no_approval': noApproval,
    'may_approve': mayApprove,
  };
}

/// One message of the inbox: a notice or a task.
class InboxItem {
  const InboxItem({
    required this.id,
    required this.kind,
    required this.code,
    this.args = const {},
    this.jobId,
    this.createdMs = 0,
    this.read = false,
    this.resolved = false,
    this.resolvedMs,
    this.resolution,
    this.by,
    this.canDecide = false,
    this.jobState,
    this.speak,
    this.device,
  });

  /// The paging key; grows with time.
  final int id;

  /// `notice` or `task`.
  final String kind;

  /// What happened (`cd_imported`, ...). A code this app does not know is shown with a generic text.
  final String code;
  final Map<String, dynamic> args;

  /// The CD job, or null (a disc that was not started).
  final int? jobId;
  final int createdMs;
  final bool read;

  /// A task that is decided: `resolution` is `approved`, `rejected` or `expired`, `by` the nickname (empty for `expired`).
  final bool resolved;
  final int? resolvedMs;
  final String? resolution;
  final String? by;

  /// True only for an open task that this profile may decide now.
  final bool canDecide;

  /// The `cd_state` of the job now (`retry_wait`: "try again" still works).
  final String? jobState;

  /// For the spoken messages of phase 5: `starter` or `home`, and the device of the starter.
  final String? speak;
  final String? device;

  bool get isTask => kind == 'task';

  /// The job waits for somebody to try again.
  bool get canRetry => jobState == 'retry_wait' && jobId != null;

  /// The album the message is about, when `args` carries its id (it can be null: then there is no "Open album").
  int? get albumId => switch (args['album_id']) {
    final int i => i,
    final num n => n.toInt(),
    _ => null,
  };

  factory InboxItem.fromJson(Map<String, dynamic> j) => InboxItem(
    id: (j['id'] as num).toInt(),
    kind: (j['kind'] as String?) ?? 'notice',
    code: (j['code'] as String?) ?? '',
    args: j['args'] is Map
        ? Map<String, dynamic>.from(j['args'] as Map)
        : const {},
    jobId: (j['job_id'] as num?)?.toInt(),
    createdMs: (j['created_ms'] as num?)?.toInt() ?? 0,
    read: j['read'] == true,
    resolved: j['resolved'] == true,
    resolvedMs: (j['resolved_ms'] as num?)?.toInt(),
    resolution: j['resolution'] as String?,
    by: j['by'] as String?,
    canDecide: j['can_decide'] == true,
    jobState: j['job_state'] as String?,
    speak: j['speak'] as String?,
    device: j['device'] as String?,
  );

  InboxItem copyWith({
    bool? read,
    bool? resolved,
    int? resolvedMs,
    String? resolution,
    String? by,
    bool? canDecide,
    String? jobState,
  }) => InboxItem(
    id: id,
    kind: kind,
    code: code,
    args: args,
    jobId: jobId,
    createdMs: createdMs,
    read: read ?? this.read,
    resolved: resolved ?? this.resolved,
    resolvedMs: resolvedMs ?? this.resolvedMs,
    resolution: resolution ?? this.resolution,
    by: by ?? this.by,
    canDecide: canDecide ?? this.canDecide,
    jobState: jobState ?? this.jobState,
    speak: speak,
    device: device,
  );

  @override
  bool operator ==(Object other) =>
      other is InboxItem &&
      other.id == id &&
      other.kind == kind &&
      other.code == code &&
      other.jobId == jobId &&
      other.read == read &&
      other.resolved == resolved &&
      other.resolution == resolution &&
      other.by == by &&
      other.canDecide == canDecide &&
      other.jobState == jobState &&
      other.args.toString() == args.toString();

  @override
  int get hashCode => Object.hash(
    id,
    kind,
    code,
    jobId,
    read,
    resolved,
    resolution,
    by,
    canDecide,
    jobState,
  );
}

/// One page of `GET /inbox`.
class InboxPage {
  const InboxPage({
    required this.items,
    required this.unread,
    required this.tasksOpen,
    this.next,
  });

  final List<InboxItem> items;

  /// The counts of the whole inbox, not of the page.
  final int unread;
  final int tasksOpen;

  /// The `after` of the next page, null on the last one.
  final int? next;

  factory InboxPage.fromJson(Map<String, dynamic> j) => InboxPage(
    items: [
      for (final e in (j['items'] as List? ?? const []))
        if (e is Map<String, dynamic> && e['id'] is num) InboxItem.fromJson(e),
    ],
    unread: (j['unread'] as num?)?.toInt() ?? 0,
    tasksOpen: (j['tasks_open'] as num?)?.toInt() ?? 0,
    next: (j['next'] as num?)?.toInt(),
  );
}

/// What the event channel says about the inbox.
sealed class InboxWire {
  const InboxWire();
}

/// `{"type":"inbox","item":{...},"unread":3,"tasks_open":1}`: a new item.
class InboxArrived extends InboxWire {
  const InboxArrived(
    this.item, {
    required this.unread,
    required this.tasksOpen,
  });
  final InboxItem item;
  final int unread;
  final int tasksOpen;
}

/// `{"type":"inbox_resolved","id":17,...}`: the task is decided for everybody.
class InboxResolvedWire extends InboxWire {
  const InboxResolvedWire({
    required this.id,
    required this.jobId,
    required this.resolution,
    required this.by,
    required this.resolvedMs,
    required this.unread,
    required this.tasksOpen,
  });
  final int id;
  final int? jobId;
  final String resolution;
  final String by;
  final int? resolvedMs;
  final int unread;
  final int tasksOpen;
}

/// The `snapshot` after `hello`: only the counts. The list is read again from the server (nothing is replayed).
class InboxCountsWire extends InboxWire {
  const InboxCountsWire({required this.unread, required this.tasksOpen});
  final int unread;
  final int tasksOpen;
}

/// The inbox part of one decoded message of `/cd/events`, or null if it has none.
InboxWire? parseInboxWire(Map<String, dynamic> j) {
  int n(String key) => (j[key] as num?)?.toInt() ?? 0;
  switch (j['type']) {
    case 'inbox':
      final item = j['item'];
      if (item is! Map<String, dynamic> || item['id'] is! num) return null;
      return InboxArrived(
        InboxItem.fromJson(item),
        unread: n('unread'),
        tasksOpen: n('tasks_open'),
      );
    case 'inbox_resolved':
      if (j['id'] is! num) return null;
      return InboxResolvedWire(
        id: (j['id'] as num).toInt(),
        jobId: (j['job_id'] as num?)?.toInt(),
        resolution: (j['resolution'] as String?) ?? '',
        by: (j['by'] as String?) ?? '',
        resolvedMs: (j['resolved_ms'] as num?)?.toInt(),
        unread: n('unread'),
        tasksOpen: n('tasks_open'),
      );
    case 'snapshot':
      if (j['unread'] is! num && j['tasks_open'] is! num) return null;
      return InboxCountsWire(unread: n('unread'), tasksOpen: n('tasks_open'));
  }
  return null;
}
