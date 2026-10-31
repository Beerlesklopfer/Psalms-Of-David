// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

// The inbox of the CDs as a PURE state machine: `inboxTransition(state, event) -> (state, effects)`. No clock, no network, no widget.
// [InboxController] is the shell: it carries out the [InboxEffect]s and feeds the answers back as events; the pages only show the state.
// The document is docs/spec-state-machines.md; the contract is docs/spec-app-api.md ("The inbox of the CDs").
//
//   idle --load--> loading --page--> ready --refresh--> refreshing --page--> ready
//                     |                                     |
//                     +----------------failed---------------+--> error --refresh--> loading
//
// The server holds the truth. The WebSocket only adds: a new item, a task that is decided, the counts. Nothing is replayed after a
// reconnect, so every `snapshot` (it follows each connect) makes the machine read the list again.

import 'inbox_models.dart';

enum InboxPhase { idle, loading, ready, refreshing, error }

/// What the machine shows. Immutable; every change goes through [inboxTransition].
class InboxState {
  const InboxState({
    this.phase = InboxPhase.idle,
    this.items = const [],
    this.unread = 0,
    this.tasksOpen = 0,
    this.next,
    this.loadingMore = false,
    this.arrived = const [],
    this.deciding = const {},
    this.retrying = const {},
    this.failures = const {},
    this.error,
  });

  final InboxPhase phase;

  /// Newest first (by id).
  final List<InboxItem> items;

  /// The counts of the whole inbox, as the server last said (not of the loaded part).
  final int unread;
  final int tasksOpen;

  /// The `after` of the next page; null: all is loaded.
  final int? next;
  final bool loadingMore;

  /// Items that came over the channel while a page was on its way; kept if the page does not know them yet.
  final List<InboxItem> arrived;

  /// The tasks (by item id) whose decision is on its way to the server.
  final Set<int> deciding;

  /// The items whose "try again" is on its way.
  final Set<int> retrying;

  /// The last refusal of an action on an item (item id -> code such as `forbidden`, `network`, `busy`); cleared by the next try.
  final Map<int, String> failures;

  /// The code of the failed load, in the phase [InboxPhase.error].
  final String? error;

  bool get isEmpty => items.isEmpty;

  InboxItem? item(int id) {
    for (final i in items) {
      if (i.id == id) return i;
    }
    return null;
  }

  InboxState copyWith({
    InboxPhase? phase,
    List<InboxItem>? items,
    int? unread,
    int? tasksOpen,
    Object? next = _keep,
    bool? loadingMore,
    List<InboxItem>? arrived,
    Set<int>? deciding,
    Set<int>? retrying,
    Map<int, String>? failures,
    Object? error = _keep,
  }) => InboxState(
    phase: phase ?? this.phase,
    items: items ?? this.items,
    unread: unread ?? this.unread,
    tasksOpen: tasksOpen ?? this.tasksOpen,
    next: identical(next, _keep) ? this.next : next as int?,
    loadingMore: loadingMore ?? this.loadingMore,
    arrived: arrived ?? this.arrived,
    deciding: deciding ?? this.deciding,
    retrying: retrying ?? this.retrying,
    failures: failures ?? this.failures,
    error: identical(error, _keep) ? this.error : error as String?,
  );
}

const Object _keep = Object();

// ---- Events -----------------------------------------------------------------------------------------

sealed class InboxEvent {
  const InboxEvent();
}

/// The first load.
class InboxLoad extends InboxEvent {
  const InboxLoad();
}

/// Read the list again; what is shown stays until the answer comes.
class InboxRefresh extends InboxEvent {
  const InboxRefresh();
}

/// The next page was asked for (the end of the list was reached).
class InboxMore extends InboxEvent {
  const InboxMore();
}

/// A page arrived. [append] is true for the page after the one shown.
class InboxPageLoaded extends InboxEvent {
  const InboxPageLoaded(this.page, {this.append = false});
  final InboxPage page;
  final bool append;
}

class InboxLoadFailed extends InboxEvent {
  const InboxLoadFailed(this.code, {this.append = false});
  final String code;
  final bool append;
}

/// A message of the event channel.
class InboxWireEvent extends InboxEvent {
  const InboxWireEvent(this.wire);
  final InboxWire wire;
}

/// The person opened or tapped an item.
class InboxMarkRead extends InboxEvent {
  const InboxMarkRead(this.id);
  final int id;
}

class InboxMarkAllRead extends InboxEvent {
  const InboxMarkAllRead();
}

/// The server did not take a mark; the list is read again to learn what is true.
class InboxMarkFailed extends InboxEvent {
  const InboxMarkFailed();
}

/// The person chose Import (`approve: true`) or Reject for a task.
class InboxDecide extends InboxEvent {
  const InboxDecide(this.id, {required this.approve});
  final int id;
  final bool approve;
}

class InboxDecided extends InboxEvent {
  const InboxDecided(this.id, {required this.approve});
  final int id;
  final bool approve;
}

/// The server refused. [code] is its error code; for `decided` the body says who was faster ([by], [resolution]).
class InboxDecideFailed extends InboxEvent {
  const InboxDecideFailed(this.id, this.code, {this.by, this.resolution});
  final int id;
  final String code;
  final String? by;
  final String? resolution;
}

class InboxRetry extends InboxEvent {
  const InboxRetry(this.id);
  final int id;
}

class InboxRetried extends InboxEvent {
  const InboxRetried(this.id);
  final int id;
}

class InboxRetryFailed extends InboxEvent {
  const InboxRetryFailed(this.id, this.code);
  final int id;
  final String code;
}

// ---- Effects ----------------------------------------------------------------------------------------

sealed class InboxEffect {
  const InboxEffect();
}

/// `GET /inbox`; [after] null for the first page.
class FetchPage extends InboxEffect {
  const FetchPage({this.after});
  final int? after;
}

class PostRead extends InboxEffect {
  const PostRead(this.id);
  final int id;
}

class PostReadAll extends InboxEffect {
  const PostReadAll();
}

class PostDecide extends InboxEffect {
  const PostDecide(this.id, this.jobId, {required this.approve});
  final int id;
  final int jobId;
  final bool approve;
}

class PostRetry extends InboxEffect {
  const PostRetry(this.id, this.jobId);
  final int id;
  final int jobId;
}

typedef InboxStep = (InboxState, List<InboxEffect>);

// ---- The transition ---------------------------------------------------------------------------------

InboxStep inboxTransition(InboxState s, InboxEvent e) => switch (e) {
  InboxLoad() => switch (s.phase) {
    InboxPhase.idle || InboxPhase.error => (
      s.copyWith(phase: InboxPhase.loading, error: null),
      const [FetchPage()],
    ),
    _ => (s, const []),
  },
  InboxRefresh() => switch (s.phase) {
    InboxPhase.idle => (
      s.copyWith(phase: InboxPhase.loading),
      const [FetchPage()],
    ),
    InboxPhase.error => (
      s.copyWith(phase: InboxPhase.loading, error: null),
      const [FetchPage()],
    ),
    InboxPhase.ready => (
      s.copyWith(phase: InboxPhase.refreshing, arrived: const []),
      const [FetchPage()],
    ),
    // A read is on its way already; its answer is as good as a second one.
    InboxPhase.loading || InboxPhase.refreshing => (s, const []),
  },
  InboxMore() =>
    s.phase == InboxPhase.ready && s.next != null && !s.loadingMore
        ? (s.copyWith(loadingMore: true), [FetchPage(after: s.next)])
        : (s, const []),
  InboxPageLoaded() => (_pageLoaded(s, e), const []),
  InboxLoadFailed() =>
    e.append
        ? (s.copyWith(loadingMore: false), const [])
        // A failed refresh keeps what was shown (stale but useful); only the first load has nothing to keep.
        : (
            s.copyWith(
              phase: s.items.isEmpty ? InboxPhase.error : InboxPhase.ready,
              error: e.code,
              arrived: const [],
            ),
            const [],
          ),
  InboxWireEvent() => _wire(s, e.wire),
  InboxMarkRead() => _markRead(s, e.id),
  InboxMarkAllRead() =>
    s.items.every((i) => i.read) && s.unread == 0
        ? (s, const [])
        : (
            s.copyWith(
              items: [
                for (final i in s.items) i.read ? i : i.copyWith(read: true),
              ],
              unread: 0,
            ),
            const [PostReadAll()],
          ),
  InboxMarkFailed() => inboxTransition(s, const InboxRefresh()),
  InboxDecide() => _decide(s, e),
  InboxDecided() => _decided(s, e),
  InboxDecideFailed() => _decideFailed(s, e),
  InboxRetry() => _retry(s, e.id),
  InboxRetried() => (_afterRetry(s, e.id), const []),
  InboxRetryFailed() => _retryFailed(s, e),
};

/// Newest first, one entry per id; the entry of [preferred] wins over the one of [others].
List<InboxItem> _merge(
  Iterable<InboxItem> preferred,
  Iterable<InboxItem> others,
) {
  final byId = <int, InboxItem>{
    for (final i in others) i.id: i,
    for (final i in preferred) i.id: i,
  };
  return byId.values.toList()..sort((a, b) => b.id.compareTo(a.id));
}

InboxState _pageLoaded(InboxState s, InboxPageLoaded e) {
  final p = e.page;
  if (e.append) {
    return s.copyWith(
      items: _merge(p.items, s.items),
      unread: p.unread,
      tasksOpen: p.tasksOpen,
      next: p.next,
      loadingMore: false,
    );
  }
  // A refresh replaces the list (the server removed what is gone), but an item that came over the channel while the page was on its way
  // and is newer than the page is kept: the page may have been made before it.
  final newest = p.items.isEmpty ? 0 : p.items.first.id;
  final keep = [
    for (final i in s.arrived)
      if (i.id > newest) i,
  ];
  return s.copyWith(
    phase: InboxPhase.ready,
    items: _merge(p.items, keep),
    unread: p.unread,
    tasksOpen: p.tasksOpen,
    next: p.next,
    loadingMore: false,
    arrived: const [],
    error: null,
  );
}

InboxStep _wire(InboxState s, InboxWire w) {
  switch (w) {
    case InboxArrived():
      final waiting =
          s.phase == InboxPhase.loading || s.phase == InboxPhase.refreshing;
      return (
        s.copyWith(
          items: _merge([w.item], s.items),
          unread: w.unread,
          tasksOpen: w.tasksOpen,
          arrived: waiting ? [...s.arrived, w.item] : s.arrived,
        ),
        const [],
      );
    case InboxResolvedWire():
      final it = s.item(w.id);
      final deciding = {...s.deciding}..remove(w.id);
      if (it == null) {
        return (
          s.copyWith(
            unread: w.unread,
            tasksOpen: w.tasksOpen,
            deciding: deciding,
          ),
          const [],
        );
      }
      // The starter of a held CD could not decide: the entry of the job goes, the decision comes as a notice of its own.
      final gone = it.isTask && !it.canDecide && !it.resolved;
      final items = gone
          ? [
              for (final i in s.items)
                if (i.id != w.id) i,
            ]
          : [
              for (final i in s.items)
                i.id == w.id
                    ? i.copyWith(
                        read: true,
                        resolved: true,
                        resolvedMs: w.resolvedMs,
                        resolution: w.resolution,
                        by: w.by,
                        canDecide: false,
                      )
                    : i,
            ];
      return (
        s.copyWith(
          items: items,
          unread: w.unread,
          tasksOpen: w.tasksOpen,
          deciding: deciding,
        ),
        const [],
      );
    case InboxCountsWire():
      // The counts show at once; the list is read again because messages may have been missed (nothing is replayed).
      final counted = s.copyWith(unread: w.unread, tasksOpen: w.tasksOpen);
      return inboxTransition(counted, const InboxRefresh());
  }
}

InboxStep _markRead(InboxState s, int id) {
  final it = s.item(id);
  if (it == null || it.read) return (s, const []);
  return (
    s.copyWith(
      items: [for (final i in s.items) i.id == id ? i.copyWith(read: true) : i],
      unread: s.unread > 0 ? s.unread - 1 : 0,
    ),
    [PostRead(id)],
  );
}

Map<int, String> _without(Map<int, String> m, int id) => {...m}..remove(id);

InboxStep _decide(InboxState s, InboxDecide e) {
  final it = s.item(e.id);
  final job = it?.jobId;
  if (it == null ||
      job == null ||
      !it.canDecide ||
      it.resolved ||
      s.deciding.contains(e.id)) {
    return (s, const []);
  }
  return (
    s.copyWith(
      deciding: {...s.deciding, e.id},
      failures: _without(s.failures, e.id),
    ),
    [PostDecide(e.id, job, approve: e.approve)],
  );
}

/// The response of the server settles the item; the `inbox_resolved` message that follows says the same.
InboxStep _decided(InboxState s, InboxDecided e) {
  final it = s.item(e.id);
  final deciding = {...s.deciding}..remove(e.id);
  // The message of the channel may have been faster than the response: it knows who decided, keep it.
  if (it == null || it.resolved) {
    return (s.copyWith(deciding: deciding), const []);
  }
  final wasOpen = !it.resolved;
  return (
    s.copyWith(
      deciding: deciding,
      items: [
        for (final i in s.items)
          i.id == e.id
              ? i.copyWith(
                  read: true,
                  resolved: true,
                  resolution: e.approve ? 'approved' : 'rejected',
                  by: '',
                  canDecide: false,
                )
              : i,
      ],
      // A decided task counts as read and is not open any more.
      unread: wasOpen && !it.read && s.unread > 0 ? s.unread - 1 : s.unread,
      tasksOpen: wasOpen && s.tasksOpen > 0 ? s.tasksOpen - 1 : s.tasksOpen,
    ),
    const [],
  );
}

InboxStep _decideFailed(InboxState s, InboxDecideFailed e) {
  final deciding = {...s.deciding}..remove(e.id);
  final it = s.item(e.id);
  switch (e.code) {
    case 'decided':
      // The slower of two deciders: "already decided by <name>".
      if (it == null || it.resolved) {
        return (s.copyWith(deciding: deciding), const []);
      }
      final wasOpen = !it.resolved;
      return (
        s.copyWith(
          deciding: deciding,
          items: [
            for (final i in s.items)
              i.id == e.id
                  ? i.copyWith(
                      read: true,
                      resolved: true,
                      resolution: e.resolution ?? '',
                      by: e.by ?? '',
                      canDecide: false,
                    )
                  : i,
          ],
          unread: wasOpen && !it.read && s.unread > 0 ? s.unread - 1 : s.unread,
          tasksOpen: wasOpen && s.tasksOpen > 0 ? s.tasksOpen - 1 : s.tasksOpen,
          failures: {...s.failures, e.id: 'decided'},
        ),
        const [],
      );
    case 'forbidden':
      // The rights changed: no buttons any more.
      return (
        s.copyWith(
          deciding: deciding,
          items: [
            for (final i in s.items)
              i.id == e.id ? i.copyWith(canDecide: false) : i,
          ],
          failures: {...s.failures, e.id: e.code},
        ),
        const [],
      );
    case 'invalid_move' || 'not_found':
      // The state is not what the app thinks: read it again.
      final (n, fx) = inboxTransition(
        s.copyWith(deciding: deciding, failures: {...s.failures, e.id: e.code}),
        const InboxRefresh(),
      );
      return (n, fx);
    default:
      return (
        s.copyWith(deciding: deciding, failures: {...s.failures, e.id: e.code}),
        const [],
      );
  }
}

InboxStep _retry(InboxState s, int id) {
  final it = s.item(id);
  if (it == null || !it.canRetry || s.retrying.contains(id)) {
    return (s, const []);
  }
  return (
    s.copyWith(
      retrying: {...s.retrying, id},
      failures: _without(s.failures, id),
    ),
    [PostRetry(id, it.jobId!)],
  );
}

/// The job reads again: "try again" is gone until the server says otherwise.
InboxState _afterRetry(InboxState s, int id) => s.copyWith(
  retrying: {...s.retrying}..remove(id),
  items: [
    for (final i in s.items) i.id == id ? i.copyWith(jobState: 'ripping') : i,
  ],
);

InboxStep _retryFailed(InboxState s, InboxRetryFailed e) {
  final retrying = {...s.retrying}..remove(e.id);
  final failed = s.copyWith(
    retrying: retrying,
    failures: {...s.failures, e.id: e.code},
  );
  return e.code == 'invalid_move' || e.code == 'not_found'
      ? inboxTransition(failed, const InboxRefresh())
      : (failed, const []);
}
