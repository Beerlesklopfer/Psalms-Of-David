// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:dbus/dbus.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/mpris.dart';
import 'package:psalm/models.dart';
import 'package:psalm/player_controller.dart';

Book _book() => Book(
  id: '7',
  title: 'Winter der Welt',
  author: 'Ken Follett',
  color: const Color(0xFF000000),
  tracks: [
    for (var i = 1; i <= 3; i++) Track(disc: 1, number: i, title: 'Teil $i', length: const Duration(minutes: 10)),
  ],
);

void main() {
  busTest();
  test('the desktop is told what plays: status, title, album, artist and length', () {
    final p = PlayerController([_book()]);
    var props = playerProperties(p);
    expect(props['PlaybackStatus'], DBusString('Paused'));
    expect(props['CanGoNext'], DBusBoolean(true));
    final meta = props['Metadata']!.asStringVariantDict();
    expect(meta['xesam:title'], DBusVariant(DBusString('Teil 1')).value);
    expect(meta['xesam:album'], DBusString('Winter der Welt'));
    expect(meta['xesam:artist']!.asStringArray().toList(), ['Ken Follett']);
    expect(meta['mpris:length'], DBusInt64(const Duration(minutes: 10).inMicroseconds));
    expect(meta['mpris:trackid'], isA<DBusObjectPath>());

    p.toggle();
    expect(playerProperties(p)['PlaybackStatus'], DBusString('Playing'));
    p.next();
    p.next();
    expect(playerProperties(p)['CanGoNext'], DBusBoolean(false), reason: 'the last track has no next');
  });

  test('the media keys play, pause, skip and seek', () {
    final p = PlayerController([_book()]);
    expect(performPlayerMethod(p, 'Play', const []), isTrue);
    expect(p.playing, isTrue);
    performPlayerMethod(p, 'Play', const []);
    expect(p.playing, isTrue, reason: 'Play on a player that plays changes nothing');
    performPlayerMethod(p, 'PlayPause', const []);
    expect(p.playing, isFalse);
    performPlayerMethod(p, 'Pause', const []);
    expect(p.playing, isFalse);

    performPlayerMethod(p, 'Next', const []);
    expect(p.trackIndex, 1);
    performPlayerMethod(p, 'Seek', [DBusInt64(30 * 1000 * 1000)]);
    expect(p.position, const Duration(seconds: 30));
    // A position for the track that plays is taken; for another track it is not.
    final id = playerProperties(p)['Metadata']!.asStringVariantDict()['mpris:trackid']!.asObjectPath();
    performPlayerMethod(p, 'SetPosition', [id, DBusInt64(90 * 1000 * 1000)]);
    expect(p.position, const Duration(seconds: 90));
    performPlayerMethod(p, 'SetPosition', [
      DBusObjectPath('/com/the_abraham_box/psalm/track/other'),
      DBusInt64(5 * 1000 * 1000),
    ]);
    expect(p.position, const Duration(seconds: 90));
    performPlayerMethod(p, 'Previous', const []);
    expect(p.trackIndex, 1, reason: 'after the first seconds, Previous goes to the start of the track');
    expect(p.position, Duration.zero);
    expect(performPlayerMethod(p, 'Unknown', const []), isFalse);
  });

  test('without a book the player is stopped and cannot be played', () {
    final p = PlayerController(const []);
    final props = playerProperties(p);
    expect(props['PlaybackStatus'], DBusString('Stopped'));
    expect(props['CanPlay'], DBusBoolean(false));
    expect(props['CanControl'], DBusBoolean(true));
    performPlayerMethod(p, 'PlayPause', const []);
    expect(p.playing, isFalse);
  });
}

void busTest() {
  test('on the real session bus a media key reaches the player and a change is announced', () async {
    final p = PlayerController([_book()]);
    final service = await MprisService.start(p);
    if (service == null) {
      markTestSkipped('no session bus');
      return;
    }
    final client = DBusClient.session();
    try {
      final remote = DBusRemoteObject(client, name: mprisName, path: DBusObjectPath('/org/mpris/MediaPlayer2'));
      expect((await remote.getProperty('org.mpris.MediaPlayer2', 'Identity')).asString(), 'Psalm');
      expect((await remote.getProperty('org.mpris.MediaPlayer2.Player', 'PlaybackStatus')).asString(), 'Paused');
      // What the media key of the keyboard (or `playerctl play-pause`) sends.
      await remote.callMethod('org.mpris.MediaPlayer2.Player', 'PlayPause', []);
      expect(p.playing, isTrue);
      expect((await remote.getProperty('org.mpris.MediaPlayer2.Player', 'PlaybackStatus')).asString(), 'Playing');
      await remote.callMethod('org.mpris.MediaPlayer2.Player', 'Next', []);
      expect(p.trackIndex, 1);
      final meta = (await remote.getProperty('org.mpris.MediaPlayer2.Player', 'Metadata')).asStringVariantDict();
      expect(meta['xesam:title']!.asString(), 'Teil 2');
    } finally {
      await client.close();
      await service.stop();
    }
  });
}
