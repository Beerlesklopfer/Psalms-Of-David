// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import '../models.dart';

/// Lower case, accents and umlauts removed ("Märchen" -> "marchen"), everything that is not a letter or digit becomes a space.
String foldForSearch(String text) {
  const from = 'àáâãäåāçèéêëēìíîïīñòóôõöøōùúûüūýÿß';
  const to = 'aaaaaaaceeeeeiiiiinoooooooouuuuuyys';
  final out = StringBuffer();
  for (final rune in text.toLowerCase().runes) {
    final c = String.fromCharCode(rune);
    final i = from.indexOf(c);
    if (i >= 0) {
      out.write(c == 'ß' ? 'ss' : to[i]);
    } else if (RegExp(r'[a-z0-9]').hasMatch(c) ||
        rune > 0x7f && RegExp(r'\p{L}', unicode: true).hasMatch(c)) {
      out.write(c);
    } else {
      out.write(' ');
    }
  }
  return out.toString().replaceAll(RegExp(r' +'), ' ').trim();
}

/// The score of [book] for [query], or 0 if some word of the query is found nowhere (title, author, narrator, labels).
/// A word in the title counts most, then the author, then a label; a whole word counts more than part of a word,
/// and a title that equals the query wins over everything.
int voiceScore(LibraryItem book, String query) {
  final words = foldForSearch(query)
      .split(' ')
      .where((w) => w.isNotEmpty)
      .toList();
  if (words.isEmpty) return 0;
  final title = foldForSearch(book.title);
  final author = foldForSearch(book.author);
  final narrator = foldForSearch(book.narrator ?? '');
  final labels = [for (final l in book.allLabels) foldForSearch(l)];
  var score = 0;
  for (final w in words) {
    final pattern = RegExp('(^| )${RegExp.escape(w)}( |\$)');
    if (title.contains(w)) {
      score += pattern.hasMatch(title) ? 12 : 9;
    } else if (author.contains(w)) {
      score += pattern.hasMatch(author) ? 6 : 5;
    } else if (narrator.contains(w)) {
      score += 4;
    } else if (labels.any((l) => l.contains(w))) {
      score += labels.any(pattern.hasMatch) ? 3 : 2;
    } else {
      return 0;
    }
  }
  if (title == words.join(' ')) score += 100;
  return score;
}

/// The books that match [query], best first (equal scores keep the order of [books]).
List<LibraryItem> matchItems(List<LibraryItem> books, String query) {
  final scored = <(int, int, LibraryItem)>[];
  for (var i = 0; i < books.length; i++) {
    final s = voiceScore(books[i], query);
    if (s > 0) scored.add((s, i, books[i]));
  }
  scored.sort(
    (a, b) => b.$1 != a.$1 ? b.$1.compareTo(a.$1) : a.$2.compareTo(b.$2),
  );
  return [for (final s in scored) s.$3];
}

/// The best match for [query], or null if nothing matches.
LibraryItem? bestMatch(List<LibraryItem> books, String query) =>
    matchItems(books, query).firstOrNull;
