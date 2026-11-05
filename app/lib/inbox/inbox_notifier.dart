// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:dbus/dbus.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';

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

/// The system notifications for messages that arrive while the app runs (the CD inbox, and the news of new media): they come only from David,
/// in the home network; nothing is pushed through Google or Apple (owner, 2026-11-05). When the app is closed nothing comes: the next start shows
/// the card and the badge.
abstract class InboxNotifier {
  Future<void> show(InboxNotice notice);

  /// Removes the notification of [id] (the task was decided by somebody else, or read).
  Future<void> close(int id);

  /// Taps and button presses.
  Stream<InboxNotifierEvent> get events;

  Future<void> dispose();
}

/// Where a platform has no mechanism the app uses (the web, a test): no notification, the card and the badge still work.
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

/// The notifier of this platform: the desktop notification of Linux, the local notifications of Android, iOS, macOS and Windows. One for the
/// whole app (the CD inbox and the news of new media share it: the plugin knows one handler of taps), so disposing it does nothing.
InboxNotifier platformInboxNotifier() => _platform ??= _Shared(
  Platform.isLinux
      ? LinuxInboxNotifier()
      : (Platform.isAndroid || Platform.isIOS || Platform.isMacOS || Platform.isWindows)
      ? PluginInboxNotifier()
      : const NoInboxNotifier(),
);

InboxNotifier? _platform;

/// The app's one notifier: lives as long as the app, whoever "disposes" it.
class _Shared implements InboxNotifier {
  _Shared(this._inner);

  final InboxNotifier _inner;

  @override
  Future<void> show(InboxNotice notice) => _inner.show(notice);

  @override
  Future<void> close(int id) => _inner.close(id);

  @override
  Stream<InboxNotifierEvent> get events => _inner.events;

  @override
  Future<void> dispose() async {}
}

/// The local notifications of the phone or the computer (`flutter_local_notifications`): shown by the app itself, no push service. The buttons
/// (approve, reject, try again) come on Android; elsewhere a tap opens the app. The first notification asks for the permission (Android 13,
/// iOS, macOS).
class PluginInboxNotifier implements InboxNotifier {
  PluginInboxNotifier([FlutterLocalNotificationsPlugin? plugin]) : _plugin = plugin ?? FlutterLocalNotificationsPlugin();

  final FlutterLocalNotificationsPlugin _plugin;
  final _events = StreamController<InboxNotifierEvent>.broadcast();
  Future<bool>? _ready;

  @override
  Stream<InboxNotifierEvent> get events => _events.stream;

  Future<bool> _init() => _ready ??= () async {
    try {
      await _plugin.initialize(
        settings: const InitializationSettings(
          android: AndroidInitializationSettings('@mipmap/ic_launcher'),
          iOS: DarwinInitializationSettings(),
          macOS: DarwinInitializationSettings(),
          windows: WindowsInitializationSettings(appName: 'Psalm', appUserModelId: 'io.onesimus.psalm', guid: '6f1d6c5e-8a2b-4f3d-9a51-3b7e2d9c4a10'),
        ),
        onDidReceiveNotificationResponse: (r) {
          final id = r.id;
          if (id == null) return;
          _events.add(
            InboxNotifierEvent(id, switch (r.actionId) {
              'approve' => InboxNoticeAction.approve,
              'reject' => InboxNoticeAction.reject,
              'retry' => InboxNoticeAction.retry,
              _ => null,
            }),
          );
        },
      );
      await _plugin.resolvePlatformSpecificImplementation<AndroidFlutterLocalNotificationsPlugin>()?.requestNotificationsPermission();
      return true;
    } on Object {
      return false;
    }
  }();

  @override
  Future<void> show(InboxNotice n) async {
    if (!await _init()) return;
    final actions = [
      if (n.approveLabel != null) AndroidNotificationAction('approve', n.approveLabel!),
      if (n.rejectLabel != null) AndroidNotificationAction('reject', n.rejectLabel!),
      if (n.retryLabel != null) AndroidNotificationAction('retry', n.retryLabel!),
    ];
    try {
      await _plugin.show(
        id: n.id,
        title: n.title,
        body: n.body,
        notificationDetails: NotificationDetails(
          android: AndroidNotificationDetails('david', 'David', channelDescription: 'CDs and new media of the family server', actions: actions),
          iOS: const DarwinNotificationDetails(),
          macOS: const DarwinNotificationDetails(),
        ),
      );
    } on Object {
      // A notification that cannot be shown is no error of the app: the card and the badge still tell.
    }
  }

  @override
  Future<void> close(int id) async {
    if (!await _init()) return;
    try {
      await _plugin.cancel(id: id);
    } on Object {
      // nothing to close
    }
  }

  @override
  Future<void> dispose() async => _events.close();
}

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
