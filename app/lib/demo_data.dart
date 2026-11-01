// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import 'models.dart';

/// Placeholder data until the server exists. Titles and authors are real; the
/// number of parts and all durations are made up. Only Follett has real files
/// (discs 1 to 3 are ripped), but even there the durations are placeholders.
List<Track> _tracks(List<int> perDisc) {
  final out = <Track>[];
  for (var d = 0; d < perDisc.length; d++) {
    for (var n = 1; n <= perDisc[d]; n++) {
      final secs = 180 + ((d * 31 + n * 53) % 210);
      out.add(
        Track(
          disc: d + 1,
          number: n,
          title: 'Track ${n.toString().padLeft(2, '0')}',
          length: Duration(seconds: secs),
        ),
      );
    }
  }
  return out;
}

final demoBooks = <LibraryItem>[
  LibraryItem(
    id: 'follett-kinder-der-freiheit',
    title: 'Kinder der Freiheit',
    author: 'Ken Follett',
    color: const Color(0xFF6E1F2B),
    tracks: _tracks([17, 17, 18]),
  ),
  LibraryItem(
    id: 'goethe-faust',
    title: 'Faust',
    author: 'Johann Wolfgang von Goethe',
    color: const Color(0xFF1F2F4D),
    tracks: _tracks([10, 10]),
  ),
  LibraryItem(
    id: 'fontane-effi-briest',
    title: 'Effi Briest',
    author: 'Theodor Fontane',
    color: const Color(0xFF2C4A3E),
    tracks: _tracks([12, 12, 12]),
  ),
  LibraryItem(
    id: 'storm-schimmelreiter',
    title: 'Der Schimmelreiter',
    author: 'Theodor Storm',
    color: const Color(0xFF3B4150),
    tracks: _tracks([8]),
  ),
  LibraryItem(
    id: 'luther-psalmen',
    title: 'Die Psalmen',
    author: 'Martin Luther',
    color: const Color(0xFF4A2B3D),
    tracks: _tracks([15]),
  ),
  LibraryItem(
    id: 'eichendorff-taugenichts',
    title: 'Aus dem Leben eines Taugenichts',
    author: 'Joseph von Eichendorff',
    color: const Color(0xFF5B4326),
    tracks: _tracks([9]),
  ),
];
