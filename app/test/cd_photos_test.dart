// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/cd/cd_import_controller.dart';
import 'package:psalm/cd/cd_import_ui.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/profile.dart';
import 'package:psalm/server/api_client.dart';

import 'cd_fakes.dart';

class _Picker implements AvatarPicker {
  final asked = <AvatarSource>[];

  @override
  bool get hasCamera => true;

  @override
  Future<Uint8List?> pick(AvatarSource source) async {
    asked.add(source);
    return Uint8List.fromList([9, 9]);
  }
}

void main() {
  late FakeCdServer server;
  late CdJobTracker tracker;

  setUp(() {
    server = FakeCdServer();
    tracker = CdJobTracker(FakeEvents())..start();
  });
  tearDown(() => tracker.dispose());

  Future<CdImportController> open(WidgetTester tester, _Picker picker) async {
    final c = CdImportController(server: server, tracker: tracker, drive: audioDrive);
    await c.identify();
    await tester.pumpWidget(
      PlayerScope(
        controller: PlayerController([]),
        child: MaterialApp(
          locale: const Locale('en'),
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: CdImportDialog(controller: c, picker: picker, preparePhotoFor: (b) async => Uint8List.fromList([0xFF, 0xD8, ...b])),
          ),
        ),
      ),
    );
    await tester.pump();
    return c;
  }

  testWidgets('photos of the case go to the server for the assistant, and the dialog counts them', (tester) async {
    final picker = _Picker();
    final c = await open(tester, picker);
    expect(find.text('Photos of the case for the assistant: 0'), findsOneWidget);
    for (var i = 0; i < 2; i++) {
      await tester.tap(find.byKey(const Key('cd-photo-add')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('picture-camera')));
      await tester.pumpAndSettle();
    }
    expect(picker.asked, [AvatarSource.camera, AvatarSource.camera]);
    expect(server.photos.map((p) => p.first), [0xFF, 0xFF], reason: 'prepared before it is sent');
    expect(find.text('Photos of the case for the assistant: 2'), findsOneWidget);
    c.dispose();
  });

  testWidgets('a refusal of the server is told in the row', (tester) async {
    server.photoError = ApiException(409, 'photo_limit');
    final c = await open(tester, _Picker());
    await tester.tap(find.byKey(const Key('cd-photo-add')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('picture-gallery')));
    await tester.pumpAndSettle();
    expect(find.text('There are 12 photos of this CD already.'), findsOneWidget);
    c.dispose();
  });
}
