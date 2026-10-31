// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/profile.dart';
import 'package:psalm/screens/home_screen.dart';
import 'package:psalm/screens/profile_screen.dart';
import 'package:psalm/screens/settings_screen.dart';
import 'package:psalm/server/credentials.dart';
import 'package:psalm/server/api_client.dart';

class FakeProfileApi implements ProfileApi {
  Profile current = const Profile(name: 'Anna', firstName: 'Anna', lastName: 'Muster', role: 'member', language: 'de', deviceCount: 1, deviceLimit: 4);
  Uint8List? picture;
  final calls = <String>[];
  ApiException? refuse;

  void _maybeFail() {
    if (refuse != null) throw refuse!;
  }

  @override
  Future<Profile> get() async => current;

  @override
  Future<Profile> update({String? nickname, String? firstName, String? lastName, String? language}) async {
    calls.add('update:$nickname|$firstName|$lastName|$language');
    _maybeFail();
    current = Profile(
      name: nickname ?? current.name,
      firstName: firstName ?? current.firstName,
      lastName: lastName ?? current.lastName,
      role: current.role,
      language: language == null ? current.language : (language.isEmpty ? null : language),
      hasAvatar: current.hasAvatar,
      avatarVersion: current.avatarVersion,
      deviceCount: current.deviceCount,
      deviceLimit: current.deviceLimit,
    );
    return current;
  }

  @override
  Future<void> changePin(String c, String n) async {
    calls.add('pin:$c|$n');
    _maybeFail();
  }

  @override
  Future<List<ProfileDevice>> devices() async => [ProfileDevice(id: 7, name: 'Phone', shared: false, lastSeen: null, isThis: true)];

  @override
  Future<Uint8List?> avatar() async => picture;

  Profile _withAvatar(Uint8List? bytes) => Profile(
        name: current.name,
        firstName: current.firstName,
        lastName: current.lastName,
        role: current.role,
        language: current.language,
        hasAvatar: bytes != null,
        avatarVersion: bytes == null ? null : (current.avatarVersion ?? 0) + 1,
        deviceCount: current.deviceCount,
        deviceLimit: current.deviceLimit,
      );

  @override
  Future<Profile> putAvatar(Uint8List jpeg) async {
    calls.add('put:${jpeg.length}');
    _maybeFail();
    picture = jpeg;
    return current = _withAvatar(jpeg);
  }

  @override
  Future<Profile> deleteAvatar() async {
    calls.add('delete');
    _maybeFail();
    picture = null;
    return current = _withAvatar(null);
  }
}

class FakePicker implements AvatarPicker {
  FakePicker({this.camera = true, this.result, this.fail});

  final bool camera;
  Uint8List? result;
  AvatarPickerException? fail;
  final asked = <AvatarSource>[];

  @override
  bool get hasCamera => camera;

  @override
  Future<Uint8List?> pick(AvatarSource source) async {
    asked.add(source);
    if (fail != null) throw fail!;
    return result;
  }
}

Uint8List _photo(int w, int h) => Uint8List.fromList(img.encodePng(img.Image(width: w, height: h)));

ProfileController _controller(FakeProfileApi api, {FakePicker? picker, ProfileStore? store}) {
  final c = ProfileController(store: store, picker: picker)..api = api;
  c.prepare = (bytes) async => bytes;
  return c;
}

Widget _app(ProfileController c, Widget home, {Locale locale = const Locale('en')}) => PlayerScope(
      controller: PlayerController(const []),
      child: ProfileScope(
        controller: c,
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          locale: locale,
          home: home,
        ),
      ),
    );

/// A tall window: the page is a list, and what is below the fold is not built.
void _tall(WidgetTester tester) {
  tester.view.physicalSize = const Size(800, 2600);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

void main() {
  group('the picture', () {
    test('is cut square from the middle, made small and sent as JPEG', () async {
      final out = await prepareAvatar(_photo(1600, 900));
      final decoded = img.decodeJpg(out!);
      expect((decoded!.width, decoded.height), (kAvatarSide, kAvatarSide));
      expect(out.length, lessThanOrEqualTo(kAvatarMaxBytes));
      // A small picture is only cut square, not made larger.
      final small = img.decodeJpg((await prepareAvatar(_photo(200, 300)))!);
      expect((small!.width, small.height), (200, 200));
    });

    test('that is no picture is refused', () async {
      expect(await prepareAvatar(Uint8List.fromList([1, 2, 3, 4])), isNull);
    });
  });

  group('the controller', () {
    test('loads the profile, the picture and the devices, and keeps them for the offline view', () async {
      final api = FakeProfileApi()..picture = Uint8List.fromList([1, 2, 3]);
      api.current = const Profile(name: 'Anna', hasAvatar: true, avatarVersion: 5);
      final dir = Directory.systemTemp.createTempSync('profile');
      addTearDown(() {
        if (dir.existsSync()) dir.deleteSync(recursive: true);
      });
      final c = _controller(api, store: ProfileStore(dir));
      await c.load();
      expect((c.profile!.name, c.devices.single.name), ('Anna', 'Phone'));
      expect(c.avatarBytes!.toList(), [1, 2, 3]);
      await Future<void>.delayed(const Duration(milliseconds: 100));
      // A new start without the server shows what was kept.
      final offline = ProfileController(store: ProfileStore(dir));
      expect(offline.profile!.name, 'Anna');
      expect(offline.avatarBytes!.toList(), [1, 2, 3]);
      expect(offline.available, isFalse);
      expect(await offline.save(nickname: 'X'), isFalse);
      expect(offline.error, 'offline');
      await c.forget();
      expect((c.profile, c.avatarBytes, ProfileStore(dir).read()), (null, null, null));
    });

    test('a wrong current PIN keeps until when the next try has to wait, for the countdown', () async {
      final api = FakeProfileApi()..refuse = ApiException(403, 'wrong_pin', {'error': 'wrong_pin', 'retry_after_s': 4});
      final c = _controller(api);
      await c.load();
      final before = DateTime.now();
      expect(await c.changePin('0000', '445566'), isFalse);
      expect(c.error, 'wrong_pin');
      final left = c.pinWaitUntil!.difference(before);
      expect(left >= const Duration(seconds: 4) && left < const Duration(seconds: 5), isTrue, reason: '$left');
      // A refusal without a wait (or a change that works) leaves no countdown.
      api.refuse = ApiException(422, 'invalid_pin');
      expect(await c.changePin('1234', 'x'), isFalse);
      expect(c.pinWaitUntil, isNull);
      api.refuse = null;
      expect(await c.changePin('1234', '445566'), isTrue);
      expect(c.pinWaitUntil, isNull);
    });

    test('the countdown reads like a clock, as on the web pages', () {
      expect(countdownText(const Duration(seconds: 2)), '0:02');
      expect(countdownText(const Duration(milliseconds: 1500)), '0:02', reason: 'a second begun counts');
      expect(countdownText(const Duration(seconds: 1024)), '17:04');
      expect(countdownText(const Duration(seconds: 32768)), '9:06:08');
      expect(countdownText(const Duration(days: 2, hours: 3, minutes: 15)), '2 d 03:15:00');
    });

    test('a refusal of the server is kept as its code and changes nothing', () async {
      final api = FakeProfileApi();
      final c = _controller(api);
      await c.load();
      api.refuse = ApiException(409, 'exists');
      expect(await c.save(nickname: 'Ben'), isFalse);
      expect((c.error, c.profile!.name), ('exists', 'Anna'));
    });
  });

  group('the profile page', () {
    testWidgets('shows the profile read only where the administrator decides, and saves what the person changed', (tester) async {
      _tall(tester);
      final api = FakeProfileApi();
      final c = _controller(api);
      await c.load();
      await tester.pumpWidget(_app(c, const ProfileScreen()));
      await tester.pumpAndSettle();
      expect(find.text('Member'), findsOneWidget);
      expect(find.byKey(const ValueKey('device-7')), findsOneWidget);
      expect(find.text('Devices: 1 of 4'), findsOneWidget);
      await tester.enterText(find.byKey(const ValueKey('profile-nickname')), 'Annchen');
      await tester.tap(find.byKey(const ValueKey('profile-save')));
      await tester.pumpAndSettle();
      expect(api.calls.single, 'update:Annchen|Anna|Muster|de');
      expect(find.text('Saved.'), findsOneWidget);
    });

    testWidgets('a taken nickname is said, in German too', (tester) async {
      _tall(tester);
      final api = FakeProfileApi()..refuse = ApiException(409, 'exists');
      final c = _controller(api);
      await c.load();
      await tester.pumpWidget(_app(c, const ProfileScreen(), locale: const Locale('de')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('profile-save')));
      await tester.pumpAndSettle();
      expect(find.text('Dieser Spitzname ist vergeben.'), findsOneWidget);
      expect(find.text('Mitglied'), findsOneWidget);
    });

    testWidgets('a photo of the camera is sent, shown and can be removed', (tester) async {
      _tall(tester);
      final api = FakeProfileApi();
      final picker = FakePicker(result: _photo(4, 4));
      final c = _controller(api, picker: picker);
      await c.load();
      await tester.pumpWidget(_app(c, const ProfileScreen()));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('avatar-remove')), findsNothing);
      await tester.tap(find.byKey(const ValueKey('avatar-camera')));
      await tester.pumpAndSettle();
      expect(picker.asked, [AvatarSource.camera]);
      expect(api.calls.single.startsWith('put:'), isTrue);
      expect(find.byKey(const ValueKey('avatar-image')), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('avatar-remove')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('confirm-removal')));
      await tester.pumpAndSettle();
      expect(api.calls.last, 'delete');
      expect(find.byKey(const ValueKey('avatar-image')), findsNothing);
    });

    testWidgets('a device without a camera offers the photos only, and a cancelled choice sends nothing', (tester) async {
      _tall(tester);
      final api = FakeProfileApi();
      final picker = FakePicker(camera: false);
      final c = _controller(api, picker: picker);
      await c.load();
      await tester.pumpWidget(_app(c, const ProfileScreen()));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('avatar-camera')), findsNothing);
      await tester.tap(find.byKey(const ValueKey('avatar-gallery')));
      await tester.pumpAndSettle();
      expect(picker.asked, [AvatarSource.gallery]);
      expect(api.calls, isEmpty);
    });

    testWidgets('a camera that may not be used is said and nothing breaks', (tester) async {
      _tall(tester);
      final api = FakeProfileApi();
      final c = _controller(api, picker: FakePicker(fail: const AvatarPickerException('denied')));
      await c.load();
      await tester.pumpWidget(_app(c, const ProfileScreen()));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('avatar-camera')));
      await tester.pumpAndSettle();
      expect(find.textContaining('may not be used'), findsOneWidget);
      expect(api.calls, isEmpty);
    });

    testWidgets('the PIN changes only when the two new entries agree', (tester) async {
      _tall(tester);
      final api = FakeProfileApi();
      final c = _controller(api);
      await c.load();
      await tester.pumpWidget(_app(c, const ProfileScreen()));
      await tester.pumpAndSettle();
      await tester.enterText(find.byKey(const ValueKey('pin-current')), 'old12345');
      await tester.enterText(find.byKey(const ValueKey('pin-new')), 'new12345');
      await tester.enterText(find.byKey(const ValueKey('pin-repeat')), 'other123');
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('pin-save')));
      await tester.pumpAndSettle();
      expect(find.text('The two new entries differ.'), findsOneWidget);
      expect(api.calls, isEmpty);
      await tester.enterText(find.byKey(const ValueKey('pin-repeat')), 'new12345');
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('pin-save')));
      await tester.pumpAndSettle();
      expect(api.calls.single, 'pin:old12345|new12345');
      expect(find.text('The PIN has been changed.'), findsOneWidget);
    });

    testWidgets('without the server the page shows what the app kept and changes nothing', (tester) async {
      _tall(tester);
      final api = FakeProfileApi();
      final c = _controller(api);
      await c.load();
      c.api = null;
      await tester.pumpWidget(_app(c, const ProfileScreen()));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('profile-offline')), findsOneWidget);
      expect(tester.widget<FilledButton>(find.byKey(const ValueKey('profile-save'))).onPressed, isNull);
      expect(tester.widget<OutlinedButton>(find.byKey(const ValueKey('avatar-gallery'))).onPressed, isNull);
    });
  });

  group('the way to the profile', () {
    testWidgets('the settings start with the own profile, which opens the page', (tester) async {
      _tall(tester);
      final c = _controller(FakeProfileApi());
      await c.load();
      final creds = Credentials(serverId: 's', address: 'pdc.example.org:8484', fingerprint: null, token: 't', deviceName: 'Laptop', ca: true);
      await tester.pumpWidget(_app(c, Scaffold(body: SettingsScreen(credentials: creds))));
      await tester.pumpAndSettle();
      expect(find.text('Anna'), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('open-profile')));
      await tester.pumpAndSettle();
      expect(find.text('My profile'), findsWidgets);
      expect(find.byKey(const ValueKey('profile-save')), findsOneWidget);
    });

    testWidgets('the demo has no profile entry in the settings and no picture on the home page', (tester) async {
      _tall(tester);
      await tester.pumpWidget(PlayerScope(
        controller: PlayerController(const []),
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: const Scaffold(body: HomeScreen()),
        ),
      ));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('home-avatar')), findsNothing);
    });

    testWidgets('the picture on the home page opens the profile', (tester) async {
      _tall(tester);
      final c = _controller(FakeProfileApi());
      await c.load();
      await tester.pumpWidget(_app(c, const Scaffold(body: HomeScreen())));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('home-avatar')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('profile-nickname')), findsOneWidget);
    });
  });
}
