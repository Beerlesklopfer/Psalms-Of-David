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
    this.labels = const [],
    this.format,
    this.mime,
    this.transcode,
  });

  /// The file format as the server names it (`mp3`, `ogg`, `flac`, `m4a`, ...) and its mime type; null from an older server.
  final String? format;
  final String? mime;

  /// `mp3` when the server can make an MP3 of the file for a device that cannot play its format (`?as=mp3`); null otherwise (and from an older server).
  final String? transcode;

  /// The labels of this title of its own (the ones of its album come on top, see [Book.labels]).
  final List<String> labels;

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

  /// The same title with other labels of its own (a change made on the device, before the server has it).
  Track withLabels(List<String> labels) => Track(
    disc: disc,
    number: number,
    title: title,
    length: length,
    path: path,
    partId: partId,
    url: url,
    live: live,
    plays: plays,
    lastPlayedAt: lastPlayedAt,
    itemId: itemId,
    bookTitle: bookTitle,
    bookAuthor: bookAuthor,
    labels: labels,
  );
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
    this.labels = const [],
    this.canLabel = false,
    this.canShare = false,
    this.trackLabels = const [],
  });

  /// The labels that single titles of the book carry; the book is found by them too (the lists filter by [allLabels]).
  final List<String> trackLabels;

  /// The labels of the book and of its titles, without doubles (case ignored).
  List<String> get allLabels {
    if (trackLabels.isEmpty) return labels;
    final seen = {for (final l in labels) l.toLowerCase()};
    return [...labels, for (final l in trackLabels) if (seen.add(l.toLowerCase())) l];
  }

  /// The labels of the book (classic tagging: prayer, school, holidays together), for everybody who sees it.
  final List<String> labels;

  /// This user may set the labels: the owner of the book, or an administrator.
  final bool canLabel;

  /// This user may pass the book on (share it): an administrator or a member; the server decides.
  final bool canShare;

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

  /// The same book with other titles, labels or hiding: what the offline view and a change made without the server need.
  Book copyWith({List<Track>? tracks, List<String>? labels, List<String>? trackLabels, bool? hidden}) => Book(
    id: id,
    title: title,
    author: author,
    color: color,
    tracks: tracks ?? this.tracks,
    cover: cover,
    coverUrl: coverUrl,
    blurb: blurb,
    kind: kind,
    narrator: narrator,
    genre: genre,
    plays: plays,
    lastPlayedAt: lastPlayedAt,
    hidden: hidden ?? this.hidden,
    owner: owner,
    labels: labels ?? this.labels,
    canLabel: canLabel,
    canShare: canShare,
    trackLabels: trackLabels ?? this.trackLabels,
  );
}
