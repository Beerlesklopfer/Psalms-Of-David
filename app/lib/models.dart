// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';

/// A track counts as heard when the position got within this distance of its end. The server uses the
/// same rule on the position reports (3 s), so the check mark shown at once agrees with the next load.
const heardTail = Duration(seconds: 3);

/// A time the server gives as Unix milliseconds; null if absent or not a number.
DateTime? dateFromMs(Object? ms) => ms is int ? DateTime.fromMillisecondsSinceEpoch(ms) : null;

class Track {
  const Track({
    required this.disc,
    required this.number,
    required this.title,
    required this.length,
    this.path,
    this.partId,
    this.url,
    this.live = false,
    this.plays = 0,
    this.lastPlayedAt,
    this.itemId,
    this.bookTitle,
    this.bookAuthor,
  });

  /// The id of the book (item) this part belongs to, when the track is in a playlist: progress goes to it. Null
  /// for a track of its own book.
  final String? itemId;

  /// The title and author of that book, for a track in a playlist.
  final String? bookTitle;
  final String? bookAuthor;

  /// How often this user heard the track to its end (from the server); 0 if never or unknown.
  final int plays;

  /// When it was last heard to the end; null if never or unknown.
  final DateTime? lastPlayedAt;

  /// Where the audio is on the internet (a podcast episode or a radio stream): it is played from the publisher,
  /// the server does not carry it.
  final String? url;

  /// A radio stream: it has no end and no length, and cannot be seeked.
  final bool live;

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
    this.coverUrl,
    this.blurb,
    this.kind = 'audiobook',
    this.narrator,
    this.genre,
    this.plays = 0,
    this.lastPlayedAt,
    this.hidden = false,
    this.owner,
  });

  /// The nickname of the family member the book belongs to ("Music of Anna"); null if it belongs to nobody in particular.
  final String? owner;

  /// This user hid the book from their lists (they are shown only with the setting "show hidden", dimmed).
  final bool hidden;

  /// How often this user heard the whole book (the fewest plays of any track, not the sum); 0 if unknown.
  final int plays;

  /// When the book was last heard (to the end of a track); null if never or unknown.
  final DateTime? lastPlayedAt;

  final String id;
  final String title;
  final String author;
  final Color color;
  final List<Track> tracks;

  /// The product picture (PNG, JPEG or WebP bytes), or null: then the cover is drawn.
  final Uint8List? cover;

  /// Where the picture can be fetched from, for a desktop that shows it itself (the sound menu): a station's logo.
  final String? coverUrl;

  /// The blurb: the text of the back cover, written in the administration. Null if none.
  final String? blurb;

  /// `audiobook` or `music` (later `video`, `ebook`): decides the category of the library.
  final String kind;

  /// Who reads an audiobook; null if the files do not say.
  final String? narrator;

  /// The genre of the list that the family looks after (music is shown by it); null if none.
  final String? genre;

  /// A radio station (`kind` is `radio`): live, no end.
  bool get isStream => kind == 'radio';

  Duration get total => tracks.fold(Duration.zero, (a, t) => a + t.length);
  int get discs => tracks.map((t) => t.disc).toSet().length;

  /// The numbers of the CDs the book has, in order: 1, 2 and 4 when CD 3 is not (yet) there. A list of the CDs goes by these, not by
  /// the count: counting from 1 to [discs] would show an empty CD 3 and never reach CD 4.
  List<int> get discNumbers => (tracks.map((t) => t.disc).toSet().toList()..sort());
}
