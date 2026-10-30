// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/file_formats.dart';
import 'package:psalm/server/device_name.dart';

void main() {
  test('a phone says what it is, not "localhost"', () {
    expect(nameOfAndroid('Google', 'Pixel 8'), 'Google Pixel 8');
    expect(nameOfAndroid('samsung', 'samsung SM-S901B'), 'Samsung SM-S901B');
    expect(nameOfAndroid('', 'Pixel 8'), 'Pixel 8');
    expect(nameOfAndroid('Google', ''), 'Google');
    expect(nameOfIos('Jörgs iPhone', 'iPhone'), 'Jörgs iPhone');
    expect(nameOfIos('iPhone', 'iPhone'), 'iPhone');
    expect(nameOfIos('localhost', 'iPad'), 'iPad');
    expect(nameOfIos('', ''), 'iPhone');
  });

  test('a downloaded file has the ending of its format: AVPlayer opens a local file by its ending', () {
    expect(audioFileEnding(format: 'mp3'), 'mp3');
    expect(audioFileEnding(format: 'M4A'), 'm4a');
    expect(audioFileEnding(format: 'ogg'), 'ogg');
    expect(audioFileEnding(format: 'ogg', mp3: true), 'mp3', reason: 'what the server made into an MP3');
    expect(audioFileEnding(format: '', mime: 'audio/mpeg'), 'mp3');
    expect(audioFileEnding(format: null, mime: 'audio/mp4; codecs=mp4a'), 'm4a');
    expect(audioFileEnding(), 'audio', reason: 'an old server says nothing about the format');
  });

  test('an episode file has the ending of its address, else mp3', () {
    expect(episodeFileEnding('https://x.example/feed/folge-3.mp3'), 'mp3');
    expect(episodeFileEnding('https://x.example/a/b.M4A?token=1'), 'm4a');
    expect(episodeFileEnding('https://x.example/a/b.mp4'), 'm4a');
    expect(episodeFileEnding('https://x.example/stream?id=7'), 'mp3');
  });
}
