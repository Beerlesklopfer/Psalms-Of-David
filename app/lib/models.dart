// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';

class Track {
  const Track({
    required this.disc,
    required this.number,
    required this.title,
    required this.length,
    this.path,
    this.partId,
  });

  /// A local file the sound engine can play; null for tracks that only exist as
  /// demo data.
  final String? path;

  /// The id of the part on the server; its file is fetched when it is played.
  final int? partId;
  final int disc;
  final int number;
  final String title;
  final Duration length;
}

class Book {
  const Book({
    required this.id,
    required this.title,
    required this.author,
    required this.color,
    required this.tracks,
    this.cover,
    this.blurb,
    this.kind = 'audiobook',
    this.narrator,
  });

  final String id;
  final String title;
  final String author;
  final Color color;
  final List<Track> tracks;

  /// The product picture (PNG, JPEG or WebP bytes), or null: then the cover is drawn.
  final Uint8List? cover;

  /// The blurb: the text of the back cover, written in the administration. Null if none.
  final String? blurb;

  /// `audiobook` or `music` (later `video`, `ebook`): decides the category of the library.
  final String kind;

  /// Who reads an audiobook; null if the files do not say.
  final String? narrator;

  Duration get total => tracks.fold(Duration.zero, (a, t) => a + t.length);
  int get discs => tracks.map((t) => t.disc).toSet().length;
}
