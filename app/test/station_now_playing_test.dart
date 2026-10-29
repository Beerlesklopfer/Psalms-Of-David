// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/audio_backend.dart';
import 'package:psalm/audio/fake_backend.dart';
import 'package:psalm/audio/mpris.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/player_controller.dart';
import 'package:psalm/podcasts.dart';
import 'package:psalm/server/server_source.dart' show MediaSource, PassageHit;
import 'package:psalm/stream_info_card.dart';
import 'package:psalm/models.dart';
import 'package:psalm/widgets.dart';

/// A fake engine that announces titles like a station does.
class _StationBackend extends FakeBackend implements StreamInfo {
  final _title = ValueNotifier<String?>(null);

  @override
  ValueListenable<String?> get streamTitle => _title;

  @override
  String? get errorMessage => null;

  @override
  AudioErrorKind get errorKind => AudioErrorKind.generic;

  final _details = ValueNotifier<StreamDetails?>(null);

  @override
  ValueListenable<StreamDetails?> get streamDetails => _details;

  void announce(String? t) => _title.value = t;
  void describe(StreamDetails d) => _details.value = d;
}

class _Source extends MediaSource {
  @override
  Future<String> fetchPart(Track track) async => track.url!;

  @override
  Future<void> saveProgress(
    Book book,
    Track track,
    Duration position, {
    required bool finished,
  }) async {}

  @override
  Future<PassageHit?> resolvePassage(String text) async => null;
}

void main() {
  final station = Station.fromJson({
    'id': 3,
    'name': 'DWG Radio',
    'stream_url': 'https://stream.example.org/live.mp3',
    'logo_url': 'https://stream.example.org/logo.png',
  });

  test('a station carries its logo as cover and as address for the desktop', () {
    final logo = Uint8List.fromList([1, 2, 3]);
    final book = station.asBook(logo: logo);
    expect(book.cover, logo);
    expect(book.coverUrl, 'https://stream.example.org/logo.png');
    expect(station.asBook().cover, isNull, reason: 'without a fetched picture the cover is drawn');
  });

  testWidgets(
    'the title the station announces shows in the player, the mini player and the sound menu',
    (tester) async {
      final backend = _StationBackend();
      final p = PlayerController([station.asBook()], backend: backend, source: _Source());
      p.open(station.asBook());
      await tester.pump(const Duration(milliseconds: 100));
      expect(p.nowPlaying, isNull, reason: 'nothing announced yet');
      expect(
        playerProperties(p)['Metadata']!.asStringVariantDict()['xesam:title']!.asString(),
        'DWG Radio',
      );

      backend.announce('Axel Volk - Pfingsten hat alles neu gemacht');
      await tester.pump(const Duration(milliseconds: 700));
      expect(p.nowPlaying, 'Axel Volk - Pfingsten hat alles neu gemacht');
      final meta = playerProperties(p)['Metadata']!.asStringVariantDict();
      expect(meta['xesam:title']!.asString(), 'Axel Volk - Pfingsten hat alles neu gemacht');
      expect(meta['mpris:artUrl']!.asString(), 'https://stream.example.org/logo.png');

      await tester.pumpWidget(
        PlayerScope(
          controller: p,
          child: MaterialApp(
            localizationsDelegates: AppLocalizations.localizationsDelegates,
            supportedLocales: AppLocalizations.supportedLocales,
            home: const Scaffold(body: MiniPlayer()),
          ),
        ),
      );
      expect(find.text('Axel Volk - Pfingsten hat alles neu gemacht'), findsOneWidget);
      expect(
        find.text('DWG Radio'),
        findsWidgets,
        reason: 'the name of the station stays as the title',
      );
      p.dispose();
    },
  );

  testWidgets('the card shows artist, track and what the station says about itself, and waits when there is nothing yet', (tester) async {
    final backend = _StationBackend();
    final p = PlayerController([station.asBook()], backend: backend, source: _Source());
    Future<void> show() => tester.pumpWidget(
      PlayerScope(
        controller: p,
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(body: StreamInfoCard(player: p)),
        ),
      ),
    );
    p.open(station.asBook());
    await tester.pump(const Duration(milliseconds: 100));
    await show();
    expect(find.text('Waiting for the information of the station …'), findsOneWidget);

    backend.describe(const StreamDetails(name: 'DWG Radio', genre: 'Christian', bitrateKbps: 128, format: 'MP3', website: 'https://radio.dwgradio.net/'));
    backend.announce('Axel Volk - Pfingsten hat alles neu gemacht');
    await tester.pump(const Duration(milliseconds: 700));
    await show();
    for (final t in ['Artist', 'Axel Volk', 'Track', 'Pfingsten hat alles neu gemacht', 'Station', 'DWG Radio', 'Genre', 'Christian', 'Quality', '128 kbit/s · MP3', 'https://radio.dwgradio.net/']) {
      expect(find.text(t), findsOneWidget, reason: t);
    }
    p.dispose();
  });
}
