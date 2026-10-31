// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:image/image.dart' as img;

import 'inbox/inbox_models.dart' show CdRights;
import 'server/api_client.dart' show ApiException;
import 'server/app_paths.dart';

/// The largest side of the picture that goes to the server, in pixels. The server keeps at most 256 KiB.
const int kAvatarSide = 512;
const int kAvatarMaxBytes = 200 * 1024;

/// A device of the own account.
class ProfileDevice {
  const ProfileDevice({required this.id, required this.name, required this.shared, required this.lastSeen, required this.isThis});

  final int id;
  final String name;
  final bool shared;
  final DateTime? lastSeen;

  /// The device that asks.
  final bool isThis;
}

/// What a person may change about themself (`GET/PUT /api/v1/me`), and what the server tells about them.
class Profile {
  const Profile({
    required this.name,
    this.firstName,
    this.lastName,
    this.role = '',
    this.language,
    this.hasAvatar = false,
    this.avatarVersion,
    this.deviceCount = 0,
    this.deviceLimit,
    this.cdRights = const CdRights(),
    this.inboxUnread = 0,
    this.inboxTasksOpen = 0,
  });

  /// What the person may do with CDs; decides whether the app shows the inbox.
  final CdRights cdRights;

  /// The inbox counts at the time of the last `GET /me` (the live counts are the inbox controller's).
  final int inboxUnread;
  final int inboxTasksOpen;

  /// The nickname: what is shown and what the person signs in with.
  final String name;
  final String? firstName;
  final String? lastName;

  /// `admin`, `member`, `child` or `guest`; read only (the administrator decides).
  final String role;

  /// `en`, `de`, or null: follow the device.
  final String? language;
  final bool hasAvatar;

  /// Changes whenever the picture does.
  final int? avatarVersion;
  final int deviceCount;
  final int? deviceLimit;

  factory Profile.fromJson(Map<String, dynamic> j) => Profile(
        name: (j['nickname'] ?? j['name'] ?? '') as String,
        firstName: j['first_name'] as String?,
        lastName: j['last_name'] as String?,
        role: j['role'] as String? ?? '',
        language: j['language'] as String?,
        hasAvatar: j['has_avatar'] == true,
        avatarVersion: j['avatar_version'] as int?,
        deviceCount: j['device_count'] as int? ?? 0,
        deviceLimit: j['device_limit'] as int?,
        cdRights: CdRights.fromJson(j['cd_rights']),
        inboxUnread: (j['inbox_unread'] as num?)?.toInt() ?? 0,
        inboxTasksOpen: (j['inbox_tasks_open'] as num?)?.toInt() ?? 0,
      );

  Map<String, Object?> toJson() => {
        'nickname': name,
        'first_name': firstName,
        'last_name': lastName,
        'role': role,
        'language': language,
        'has_avatar': hasAvatar,
        'avatar_version': avatarVersion,
        'device_count': deviceCount,
        'device_limit': deviceLimit,
        'cd_rights': cdRights.toJson(),
        'inbox_unread': inboxUnread,
        'inbox_tasks_open': inboxTasksOpen,
      };
}

/// The personal profile on the server (`docs/spec-app-api.md`, "personal profile").
abstract class ProfileApi {
  Future<Profile> get();

  /// Changes what is given; [language] `''` follows the device again.
  Future<Profile> update({String? nickname, String? firstName, String? lastName, String? language});

  Future<void> changePin(String current, String next);

  Future<List<ProfileDevice>> devices();

  /// The picture, or null if there is none.
  Future<Uint8List?> avatar();

  Future<Profile> putAvatar(Uint8List jpeg);

  Future<Profile> deleteAvatar();
}

/// The profile and the picture as the app last got them, so that the offline view shows them without the server.
class ProfileStore {
  ProfileStore(this.dir);

  factory ProfileStore.standard() => ProfileStore(Directory('${AppPaths.base.path}/profile'));

  final Directory dir;

  File get _json => File('${dir.path}/profile.json');
  File get _avatar => File('${dir.path}/avatar');

  Future<void> write(Profile profile, Uint8List? avatar) async {
    try {
      await dir.create(recursive: true);
      await _json.writeAsString(jsonEncode(profile.toJson()));
      if (avatar == null) {
        if (_avatar.existsSync()) await _avatar.delete();
      } else {
        await _avatar.writeAsBytes(avatar);
      }
    } on Object {
      // a cache that cannot be written is only a missing convenience
    }
  }

  (Profile, Uint8List?)? read() {
    try {
      final profile = Profile.fromJson(jsonDecode(_json.readAsStringSync()) as Map<String, dynamic>);
      return (profile, _avatar.existsSync() ? _avatar.readAsBytesSync() : null);
    } on Object {
      return null;
    }
  }

  Future<void> clear() async {
    try {
      if (dir.existsSync()) await dir.delete(recursive: true);
    } on Object {
      // left behind; the next write replaces it
    }
  }
}

/// The own profile in the app: loaded when the server is there, kept on the device for the offline view. A change needs the server (nothing waits in
/// the outbox for it), so offline it says so.
class ProfileController extends ChangeNotifier {
  ProfileController({this.store, this.picker}) {
    final cached = store?.read();
    if (cached != null) {
      profile = cached.$1;
      avatarBytes = cached.$2;
    }
  }

  final ProfileStore? store;

  /// Takes or chooses a picture; null: the one of the platform (replaced in tests).
  AvatarPicker? picker;

  /// Makes the picture ready for the server ([prepareAvatar]); replaced in tests, where an isolate does not run by itself.
  Future<Uint8List?> Function(Uint8List) prepare = prepareAvatar;
  ProfileApi? api;

  Profile? profile;

  /// The picture as it is now (null: none); replaced when the server says it changed.
  Uint8List? avatarBytes;
  List<ProfileDevice> devices = const [];

  /// The code of the last refusal or failure (`network`, `exists`, `wrong_pin` ...), or null.
  String? error;
  bool busy = false;

  /// True once there is a server to ask.
  bool get available => api != null;

  Future<void> load() async {
    final a = api;
    if (a == null) return;
    try {
      final fresh = await a.get();
      final old = profile;
      profile = fresh;
      if (!fresh.hasAvatar) {
        avatarBytes = null;
      } else if (avatarBytes == null || old?.avatarVersion != fresh.avatarVersion) {
        avatarBytes = await a.avatar();
      }
      devices = await a.devices();
      error = null;
      unawaited(store?.write(fresh, avatarBytes));
    } on ApiException catch (e) {
      error = e.code;
    }
    notifyListeners();
  }

  /// Runs a change; true if the server took it.
  Future<bool> _change(Future<void> Function(ProfileApi a) run) async {
    final a = api;
    if (a == null) {
      error = 'offline';
      notifyListeners();
      return false;
    }
    busy = true;
    error = null;
    notifyListeners();
    try {
      await run(a);
      busy = false;
      notifyListeners();
      return true;
    } on ApiException catch (e) {
      error = e.code;
      busy = false;
      notifyListeners();
      return false;
    }
  }

  Future<bool> save({String? nickname, String? firstName, String? lastName, String? language}) => _change((a) async {
        profile = await a.update(nickname: nickname, firstName: firstName, lastName: lastName, language: language);
        unawaited(store?.write(profile!, avatarBytes));
      });

  /// Until when the next try to change the PIN has to wait, after a wrong current PIN (the server says how long); null: no wait.
  DateTime? pinWaitUntil;

  Future<bool> changePin(String current, String next) {
    pinWaitUntil = null;
    return _change((a) async {
      try {
        await a.changePin(current, next);
      } on ApiException catch (e) {
        final s = e.retryAfterS;
        if (s != null) pinWaitUntil = DateTime.now().add(Duration(seconds: s));
        rethrow;
      }
    });
  }

  /// Sends a picture that [prepareAvatar] made.
  Future<bool> setAvatar(Uint8List jpeg) => _change((a) async {
        profile = await a.putAvatar(jpeg);
        avatarBytes = jpeg;
        unawaited(store?.write(profile!, avatarBytes));
      });

  Future<bool> removeAvatar() => _change((a) async {
        profile = await a.deleteAvatar();
        avatarBytes = null;
        unawaited(store?.write(profile!, null));
      });

  /// Forgets everything of this pairing (the person disconnected the app).
  Future<void> forget() async {
    api = null;
    profile = null;
    avatarBytes = null;
    devices = const [];
    error = null;
    await store?.clear();
    notifyListeners();
  }
}

class ProfileScope extends InheritedNotifier<ProfileController> {
  const ProfileScope({super.key, required ProfileController controller, required super.child}) : super(notifier: controller);

  static ProfileController? maybeOf(BuildContext context) => context.dependOnInheritedWidgetOfExactType<ProfileScope>()?.notifier;
}

/// Where a picture comes from.
enum AvatarSource { camera, gallery }

/// Takes a picture with the camera or chooses one (replaced in tests). Returns the bytes of the file, null if the person cancelled; throws
/// [AvatarPickerException] if the camera or the files cannot be used (denied, not there).
abstract class AvatarPicker {
  /// Whether this device has a camera to offer.
  bool get hasCamera;

  Future<Uint8List?> pick(AvatarSource source);
}

class AvatarPickerException implements Exception {
  const AvatarPickerException(this.code);

  /// `denied` or `unavailable`.
  final String code;

  @override
  String toString() => 'AvatarPickerException($code)';
}

/// Cuts a picture to a square from the middle, makes it at most [kAvatarSide] pixels and encodes it as JPEG (which also drops the metadata of the
/// camera: place, time). Null if the bytes are no picture. Runs in a separate isolate: a photo of a phone is large.
Future<Uint8List?> prepareAvatar(Uint8List source) => compute(_prepare, source);

Uint8List? _prepare(Uint8List source) {
  final decoded = img.decodeImage(source);
  if (decoded == null) return null;
  // The orientation of the camera is applied to the pixels, so that the picture stands upright without the metadata.
  final upright = img.bakeOrientation(decoded);
  final side = upright.width < upright.height ? upright.width : upright.height;
  final square = img.copyCrop(upright, x: (upright.width - side) ~/ 2, y: (upright.height - side) ~/ 2, width: side, height: side);
  final small = side > kAvatarSide ? img.copyResize(square, width: kAvatarSide, height: kAvatarSide, interpolation: img.Interpolation.average) : square;
  for (var quality = 88; quality >= 40; quality -= 12) {
    final bytes = Uint8List.fromList(img.encodeJpg(small, quality: quality));
    if (bytes.length <= kAvatarMaxBytes) return bytes;
  }
  return Uint8List.fromList(img.encodeJpg(small, quality: 30));
}
