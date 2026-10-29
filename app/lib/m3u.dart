// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

/// The first line of an extended M3U playlist.
const String kM3uHeader = '#EXTM3U';

/// The ending of the playlist files the app writes next to downloads (UTF-8, as the ending says).
const String kM3uEnding = 'm3u8';

/// One line of a playlist file: a downloaded part.
class M3uEntry {
  const M3uEntry({
    required this.duration,
    required this.artist,
    required this.title,
    required this.path,
  });

  /// The length of the part; a stream of unknown length is written as -1.
  final Duration? duration;
  final String artist;
  final String title;

  /// Relative to the folder of the playlist file.
  final String path;
}

/// Text on one line: line breaks and other control characters become blanks, the ends are trimmed.
String _oneLine(String s) => s.replaceAll(RegExp(r'[\u0000-\u001f\u007f\u0085  ]+'), ' ').trim();

/// An extended M3U in UTF-8: `#EXTM3U`, then for each part `#EXTINF:<seconds>,<artist> - <title>` and its
/// path. Lines end with `\n`. A title with a line break stays on one line so that it cannot break the file.
String buildM3u8(List<M3uEntry> entries) {
  final out = StringBuffer('$kM3uHeader\n');
  for (final e in entries) {
    final seconds = e.duration == null ? -1 : (e.duration!.inMilliseconds / 1000).round();
    final artist = _oneLine(e.artist);
    final title = _oneLine(e.title);
    out.write('#EXTINF:$seconds,${artist.isEmpty ? title : '$artist - $title'}\n');
    out.write('${_oneLine(e.path)}\n');
  }
  return out.toString();
}
