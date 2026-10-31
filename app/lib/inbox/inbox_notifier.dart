// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:dbus/dbus.dart';

/// A button of a system notification.
enum InboxNoticeAction { approve, reject, retry }

/// What the system notification of one inbox item says. The texts are made by the caller (they are in the app's language).
class InboxNotice {
  const InboxNotice({
    required this.id,
    required this.title,
    required this.body,
    this.approveLabel,
    this.rejectLabel,
    this.retryLabel,
  });

  /// The id of the inbox item: the notification of the same item replaces itself.
  final int id;
  final String title;
  final String body;

  /// The labels of the buttons; a null label means no such button.
  final String? approveLabel;
  final String? rejectLabel;
  final String? retryLabel;
}

/// A tap on a notification (`action` null) or on one of its buttons.
class InboxNotifierEvent {
  const InboxNotifierEvent(this.id, this.action);
  final int id;
  final InboxNoticeAction? action;
}

/// The system notifications for messages that arrive while the app runs. When the app is closed nothing is pushed (decided in
/// docs/spec-cd-inbox.md): the next start shows the card and the badge.
abstract class InboxNotifier {
  Future<void> show(InboxNotice notice);

  /// Removes the notification of [id] (the task was decided by somebody else, or read).
  Future<void> close(int id);

  /// Taps and button presses.
  Stream<InboxNotifierEvent> get events;

  Future<void> dispose();
}

/// Where a platform has no mechanism the app uses (today everything but Linux): no notification, the card and the badge still work.
class NoInboxNotifier implements InboxNotifier {
  const NoInboxNotifier();

  @override
  Future<void> show(InboxNotice notice) async {}

  @override
  Future<void> close(int id) async {}

  @override
  Stream<InboxNotifierEvent> get events => const Stream.empty();

  @override
  Future<void> dispose() async {}
}

/// The notifier of this platform.
InboxNotifier platformInboxNotifier() =>
    Platform.isLinux ? LinuxInboxNotifier() : const NoInboxNotifier();

const _notificationsName = 'org.freedesktop.Notifications';
const _notificationsPath = '/org/freedesktop/Notifications';

/// The desktop notification of Linux (`org.freedesktop.Notifications` on the session bus, with the buttons the server of the desktop supports).
/// Without a session bus or a notification server it does nothing.
class LinuxInboxNotifier implements InboxNotifier {
  LinuxInboxNotifier([this._client]);

  DBusClient? _client;
  DBusRemoteObject? _object;
  final _events = StreamController<InboxNotifierEvent>.broadcast();
  final _byItem = <int, int>{};
  final _bySystemId = <int, int>{};
  final _subs = <StreamSubscription<Object?>>[];
  bool _failed = false;

  @override
  Stream<InboxNotifierEvent> get events => _events.stream;

  DBusRemoteObject? _open() {
    if (_failed) return null;
    if (_object != null) return _object;
    try {
      final client = _client ??= DBusClient.session();
      final o = DBusRemoteObject(
        client,
        name: _notificationsName,
        path: DBusObjectPath(_notificationsPath),
      );
      _subs.add(
        DBusRemoteObjectSignalStream(
          object: o,
          interface: _notificationsName,
          name: 'ActionInvoked',
        ).listen((s) {
          final id = (s.values[0] as DBusUint32).value;
          final key = (s.values[1] as DBusString).value;
          final item = _bySystemId[id];
          if (item == null) return;
          _events.add(
            InboxNotifierEvent(item, switch (key) {
              'approve' => InboxNoticeAction.approve,
              'reject' => InboxNoticeAction.reject,
              'retry' => InboxNoticeAction.retry,
              _ => null,
            }),
          );
        }, onError: (_) {}),
      );
      _subs.add(
        DBusRemoteObjectSignalStream(
          object: o,
          interface: _notificationsName,
          name: 'NotificationClosed',
        ).listen((s) {
          final id = (s.values[0] as DBusUint32).value;
          final item = _bySystemId.remove(id);
          if (item != null) _byItem.remove(item);
        }, onError: (_) {}),
      );
      return _object = o;
    } on Object {
      _failed = true;
      return null;
    }
  }

  @override
  Future<void> show(InboxNotice n) async {
    final o = _open();
    if (o == null) return;
    try {
      final actions = <String>[
        // The default action is the tap on the notification itself.
        'default', '',
        if (n.approveLabel != null) ...['approve', n.approveLabel!],
        if (n.rejectLabel != null) ...['reject', n.rejectLabel!],
        if (n.retryLabel != null) ...['retry', n.retryLabel!],
      ];
      final reply = await o.callMethod(_notificationsName, 'Notify', [
        DBusString('Psalms'),
        DBusUint32(_byItem[n.id] ?? 0),
        DBusString('com.the-abraham-box.psalm'),
        DBusString(n.title),
        DBusString(n.body),
        DBusArray.string(actions),
        DBusDict.stringVariant({
          'desktop-entry': DBusString('com.the-abraham-box.psalm'),
        }),
        DBusInt32(-1),
      ], replySignature: DBusSignature('u'));
      final system = (reply.returnValues.first as DBusUint32).value;
      _byItem[n.id] = system;
      _bySystemId[system] = n.id;
    } on Object {
      // No notification server: the card and the badge are there.
    }
  }

  @override
  Future<void> close(int id) async {
    final system = _byItem.remove(id);
    final o = _object;
    if (system == null || o == null) return;
    _bySystemId.remove(system);
    try {
      await o.callMethod(_notificationsName, 'CloseNotification', [
        DBusUint32(system),
      ], replySignature: DBusSignature(''));
    } on Object {
      // already gone
    }
  }

  @override
  Future<void> dispose() async {
    for (final s in _subs) {
      await s.cancel();
    }
    _subs.clear();
    await _client?.close();
    await _events.close();
  }
}
