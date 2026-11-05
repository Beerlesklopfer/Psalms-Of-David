// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:psalm/barcode.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/profile.dart';
import 'package:psalm/screens/cover_editor.dart';
import 'package:psalm/server/api_client.dart';

class _Server implements CoverApi {
  final sent = <(int, Uint8List)>[];
  ApiException? error;

  @override
  Future<void> putCover(int itemId, Uint8List jpeg) async {
    if (error != null) throw error!;
    sent.add((itemId, jpeg));
  }

  final photos = <(int, Uint8List)>[];

  @override
  Future<int> addPhoto(int itemId, Uint8List jpeg) async {
    if (error != null) throw error!;
    photos.add((itemId, jpeg));
    return photos.length;
  }
}

class _Picker implements AvatarPicker {
  _Picker({this.camera = true});
  final bool camera;
  final asked = <AvatarSource>[];
  Uint8List? bytes = Uint8List.fromList([1, 2, 3]);

  @override
  bool get hasCamera => camera;

  @override
  Future<Uint8List?> pick(AvatarSource source) async {
    asked.add(source);
    return bytes;
  }
}

LibraryItem _album({bool canLabel = true}) => LibraryItem(
  id: '7',
  title: 'Best of Classic',
  author: 'Various Artists',
  color: const Color(0xFF000000),
  kind: 'music',
  canLabel: canLabel,
  tracks: [Track(disc: 1, number: 1, title: 'T', length: const Duration(minutes: 3), partId: 71)],
);

Widget _app(PlayerController p, Widget child) => PlayerScope(
  controller: p,
  child: MaterialApp(
    locale: const Locale('en'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: Scaffold(appBar: AppBar(actions: [child])),
  ),
);

void main() {
  final prepared = Uint8List.fromList([0xFF, 0xD8, 0xFF, 9]);
  Future<Uint8List?> prepare(Uint8List _) async => prepared;

  testWidgets('a photo of the case becomes the cover: sent to the server and shown at once', (tester) async {
    final p = PlayerController([_album()]);
    final server = _Server();
    final picker = _Picker();
    await tester.pumpWidget(_app(p, ChangeCoverButton(book: p.library.first, api: server, picker: picker, prepare: prepare)));
    await tester.tap(find.byKey(const ValueKey('change-cover')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('use-cover')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('picture-camera')));
    await tester.pumpAndSettle();
    expect(picker.asked, [AvatarSource.camera]);
    expect(server.sent.single.$1, 7);
    expect(server.sent.single.$2, prepared);
    expect(p.library.first.cover, prepared);
    expect(find.text('The cover is changed.'), findsOneWidget);
    p.dispose();
  });

  testWidgets('without a camera the files are offered at once; a refusal of the server is told', (tester) async {
    final p = PlayerController([_album()]);
    final server = _Server()..error = ApiException(413, 'cover_size');
    final picker = _Picker(camera: false);
    await tester.pumpWidget(_app(p, ChangeCoverButton(book: p.library.first, api: server, picker: picker, prepare: prepare)));
    await tester.tap(find.byKey(const ValueKey('change-cover')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('use-cover')));
    await tester.pumpAndSettle();
    expect(picker.asked, [AvatarSource.gallery]);
    expect(p.library.first.cover, isNull);
    expect(find.text('The cover could not be changed (cover_size).'), findsOneWidget);
    p.dispose();
  });

  testWidgets('a photo for the assistant goes to the photos of the album, larger, and leaves the cover alone', (tester) async {
    final p = PlayerController([_album()]);
    final server = _Server();
    final big = Uint8List.fromList([0xFF, 0xD8, 0xFF, 1]);
    await tester.pumpWidget(_app(p, ChangeCoverButton(book: p.library.first, api: server, picker: _Picker(), prepare: prepare, preparePhotoFor: (_) async => big)));
    await tester.tap(find.byKey(const ValueKey('change-cover')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('use-photo')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('picture-gallery')));
    await tester.pumpAndSettle();
    expect(server.photos.single.$2, big);
    expect(server.sent, isEmpty);
    expect(p.library.first.cover, isNull);
    expect(find.text('Photo added (1 waiting for the assistant).'), findsOneWidget);
    p.dispose();
  });

  testWidgets('only the owner and an administrator see the button', (tester) async {
    final p = PlayerController([_album(canLabel: false)]);
    await tester.pumpWidget(_app(p, ChangeCoverButton(book: p.library.first, api: _Server(), picker: _Picker())));
    expect(find.byKey(const ValueKey('change-cover')), findsNothing);
    p.dispose();
  });

  test('a photo is turned upright, made smaller and sent as JPEG; something else is no picture', () async {
    final photo = Uint8List.fromList(img.encodePng(img.Image(width: 3000, height: 2000)));
    final cover = await prepareCover(photo);
    final back = img.decodeJpg(cover!)!;
    expect((back.width, back.height), (kCoverSide, 800));
    expect(await prepareCover(Uint8List.fromList([1, 2, 3])), isNull);
    final forAi = img.decodeJpg((await preparePhoto(photo))!)!;
    expect(forAi.width, kPhotoSide, reason: 'the small print stays readable');
  });
}
