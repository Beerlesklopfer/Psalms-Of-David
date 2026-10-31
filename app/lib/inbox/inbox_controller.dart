// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/foundation.dart';

import '../l10n/app_localizations.dart';
import '../server/api_client.dart';
import 'inbox_api.dart';
import 'inbox_machine.dart';
import 'inbox_models.dart';
import 'inbox_notifier.dart';
import 'inbox_text.dart';

/// Called for every message that arrives over the event channel while the app runs, once, after the machine took it. This is the one seam
/// for what a message causes: today the system notification; phase 5 (spoken messages) adds its own handler with [speak] and [device].
typedef InboxMessageHandler = void Function(
  InboxItem item,
  String? speak,
  String? device,
);

/// The shell of the inbox machine: it holds the state, carries out the effects against the server and feeds the answers back.
/// Pages only read [state] and call the methods below; nothing else changes the state.
class InboxController extends ChangeNotifier {
  InboxController({
    required this.api,
    Stream<InboxWire>? wire,
    InboxNotifier? notifier,
    this.demo = false,
  }) : _notifier = notifier {
    _wireSub = wire?.listen(_onWire);
    _noticeSub = notifier?.events.listen(_onNoticeEvent);
    messageHandlers.add(_notify);
  }

  /// The example inbox of "Try with demo data": nothing goes to a server.
  factory InboxController.demo(List<InboxItem> items) {
    final c = InboxController(api: _DemoInboxApi(items), demo: true);
    c.start();
    return c;
  }

  final InboxApi api;
  final bool demo;
  final InboxNotifier? _notifier;
  StreamSubscription<InboxWire>? _wireSub;
  StreamSubscription<InboxNotifierEvent>? _noticeSub;
  bool _disposed = false;

  InboxState _state = const InboxState();
  InboxState get state => _state;

  /// The things a message causes (the notification; phase 5 adds the speech).
  final List<InboxMessageHandler> messageHandlers = [];

  /// The texts of the app's language; set by the shell from the context (the notification has no context).
  AppLocalizations? l10n;

  /// Asked for by a tap on a notification: the shell opens the inbox page. The value counts the requests.
  final ValueNotifier<int> openRequests = ValueNotifier(0);

  void start() => dispatch(const InboxLoad());

  bool get _busy =>
      _state.phase == InboxPhase.loading ||
      _state.phase == InboxPhase.refreshing;

  /// Read the list again (pull to refresh, a reconnect). Completes when the answer is in, so the refresh indicator stays until then.
  Future<void> refresh() {
    dispatch(const InboxRefresh());
    if (!_busy) return Future.value();
    final done = Completer<void>();
    void check() {
      if (!_busy && !done.isCompleted) {
        removeListener(check);
        done.complete();
      }
    }

    addListener(check);
    return done.future;
  }

  void more() => dispatch(const InboxMore());
  void markRead(int id) => dispatch(InboxMarkRead(id));
  void markAllRead() => dispatch(const InboxMarkAllRead());
  void approve(int id) => dispatch(InboxDecide(id, approve: true));
  void reject(int id) => dispatch(InboxDecide(id, approve: false));
  void retry(int id) => dispatch(InboxRetry(id));

  /// The one function that changes the state.
  void dispatch(InboxEvent event) {
    if (_disposed) return;
    final (next, effects) = inboxTransition(_state, event);
    final changed = !identical(next, _state);
    _state = next;
    if (changed) notifyListeners();
    for (final e in effects) {
      unawaited(_run(e));
    }
  }

  Future<void> _run(InboxEffect e) async {
    switch (e) {
      case FetchPage(:final after):
        try {
          dispatch(
            InboxPageLoaded(
              await api.list(after: after),
              append: after != null,
            ),
          );
        } on ApiException catch (x) {
          dispatch(InboxLoadFailed(x.code, append: after != null));
        } on Object {
          dispatch(InboxLoadFailed('network', append: after != null));
        }
      case PostRead(:final id):
        try {
          await api.markRead(id);
          unawaited(_notifier?.close(id));
        } on Object {
          dispatch(const InboxMarkFailed());
        }
      case PostReadAll():
        try {
          await api.markAllRead();
        } on Object {
          dispatch(const InboxMarkFailed());
        }
      case PostDecide(:final id, :final jobId, :final approve):
        try {
          await (approve ? api.approve(jobId) : api.reject(jobId));
          dispatch(InboxDecided(id, approve: approve));
          unawaited(_notifier?.close(id));
        } on ApiException catch (x) {
          dispatch(
            InboxDecideFailed(
              id,
              x.code,
              by: x.body?['by'] as String?,
              resolution: x.body?['resolution'] as String?,
            ),
          );
        } on Object {
          dispatch(InboxDecideFailed(id, 'network'));
        }
      case PostRetry(:final id, :final jobId):
        try {
          await api.retry(jobId);
          dispatch(InboxRetried(id));
        } on ApiException catch (x) {
          dispatch(InboxRetryFailed(id, x.code));
        } on Object {
          dispatch(InboxRetryFailed(id, 'network'));
        }
    }
  }

  void _onWire(InboxWire w) {
    final known = w is InboxArrived ? _state.item(w.item.id) != null : true;
    dispatch(InboxWireEvent(w));
    switch (w) {
      case InboxArrived(:final item) when !known:
        onInboxMessage(item, item.speak, item.device);
      case InboxResolvedWire(:final id):
        unawaited(_notifier?.close(id));
      default:
    }
  }

  /// A message arrived while the app runs. Every handler in [messageHandlers] gets it.
  void onInboxMessage(InboxItem item, String? speak, String? device) {
    for (final h in List.of(messageHandlers)) {
      h(item, speak, device);
    }
  }

  void _notify(InboxItem item, String? speak, String? device) {
    final n = _notifier;
    final l = l10n;
    if (n == null || l == null || item.read || item.resolved) return;
    unawaited(
      n.show(
        InboxNotice(
          id: item.id,
          title: inboxTitle(l, item),
          body: inboxText(l, item),
          approveLabel: item.canDecide ? l.inboxImport : null,
          rejectLabel: item.canDecide ? l.inboxReject : null,
          retryLabel: item.canRetry ? l.inboxRetry : null,
        ),
      ),
    );
  }

  void _onNoticeEvent(InboxNotifierEvent e) {
    switch (e.action) {
      case InboxNoticeAction.approve:
        approve(e.id);
      case InboxNoticeAction.retry:
        retry(e.id);
      // Reject deletes files: the page asks first, so the button only brings it to the front.
      case InboxNoticeAction.reject || null:
        openRequests.value++;
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _wireSub?.cancel();
    _noticeSub?.cancel();
    unawaited(_notifier?.dispose());
    openRequests.dispose();
    super.dispose();
  }
}

/// The inbox of the demo: reads and decisions succeed without a server.
class _DemoInboxApi implements InboxApi {
  _DemoInboxApi(this._items);
  final List<InboxItem> _items;

  @override
  Future<InboxPage> list({int? after, int? limit}) async => InboxPage(
    items: _items,
    unread: _items.where((i) => !i.read).length,
    tasksOpen: _items.where((i) => i.canDecide && !i.resolved).length,
  );

  @override
  Future<void> markRead(int id) async {}
  @override
  Future<void> markAllRead() async {}
  @override
  Future<void> approve(int jobId) async {}
  @override
  Future<void> reject(int jobId) async {}
  @override
  Future<void> retry(int jobId) async {}
  @override
  Future<void> giveUp(int jobId) async {}
}
