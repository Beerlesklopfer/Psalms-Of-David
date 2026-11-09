// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The e-books of the server (docs/spec-ebooks.md; project owner, 2026-11-09: a reader for biblical exegesis and for leisure): the
//! list, the text of an EPUB as the server makes it (`server/src/ebooks.rs`), and where the person is.
import 'dart:convert';

/// Where a person is in an e-book: `epub:<chapter>:<block>` or `pdf:<page>`, the share read so far, and when (ms).
class EbookProgress {
  const EbookProgress({required this.locator, required this.percent, required this.readAt});

  static EbookProgress? fromJson(Object? j) {
    if (j is! Map<String, dynamic> || j['locator'] is! String) return null;
    return EbookProgress(
      locator: j['locator'] as String,
      percent: (j['percent'] is num ? (j['percent'] as num).toDouble() : 0.0).clamp(0.0, 1.0),
      readAt: j['read_at'] is int ? j['read_at'] as int : 0,
    );
  }

  Map<String, Object> toJson() => {'locator': locator, 'percent': percent, 'read_at': readAt};

  final String locator;
  final double percent;
  final int readAt;

  /// The chapter and the block of an EPUB locator, or null.
  (int, int)? get epub {
    final p = locator.split(':');
    if (p.length != 3 || p[0] != 'epub') return null;
    final (c, b) = (int.tryParse(p[1]), int.tryParse(p[2]));
    return c == null || b == null ? null : (c, b);
  }

  /// The page of a PDF locator (from 1), or null.
  int? get pdfPage {
    final p = locator.split(':');
    return p.length == 2 && p[0] == 'pdf' ? int.tryParse(p[1]) : null;
  }
}

/// An e-book of the list.
class Ebook {
  const Ebook({
    required this.id,
    required this.format,
    required this.title,
    required this.author,
    this.language,
    this.sizeBytes = 0,
    this.mtimeMs = 0,
    this.progress,
  });

  static Ebook? fromJson(Object? j) {
    if (j is! Map<String, dynamic> || j['id'] is! int) return null;
    return Ebook(
      id: j['id'] as int,
      format: j['format'] == 'pdf' ? 'pdf' : 'epub',
      title: '${j['title'] ?? ''}',
      author: '${j['author'] ?? ''}',
      language: j['language'] is String ? j['language'] as String : null,
      sizeBytes: j['size_bytes'] is int ? j['size_bytes'] as int : 0,
      mtimeMs: j['mtime_ms'] is int ? j['mtime_ms'] as int : 0,
      progress: EbookProgress.fromJson(j['progress']),
    );
  }

  Map<String, Object?> toJson() => {
        'id': id,
        'format': format,
        'title': title,
        'author': author,
        'language': language,
        'size_bytes': sizeBytes,
        'mtime_ms': mtimeMs,
        'progress': progress?.toJson(),
      };

  Ebook withProgress(EbookProgress? p) =>
      Ebook(id: id, format: format, title: title, author: author, language: language, sizeBytes: sizeBytes, mtimeMs: mtimeMs, progress: p);

  final int id;
  final String format;
  final String title;
  final String author;
  final String? language;
  final int sizeBytes;

  /// When the file changed on the server; a copy on this device of another version is out of date.
  final int mtimeMs;
  final EbookProgress? progress;

  bool get isPdf => format == 'pdf';
}

/// A run of words with one emphasis.
class TextSpanData {
  const TextSpanData(this.text, {this.bold = false, this.italic = false, this.sup = false});

  final String text;
  final bool bold;
  final bool italic;
  final bool sup;
}

/// A block of a chapter: `h1` … `h3`, `p`, `quote`, `li`, `pre`, `img` (its [src] inside the EPUB), `hr`.
class TextBlock {
  const TextBlock(this.kind, this.spans, {this.src});

  final String kind;
  final List<TextSpanData> spans;
  final String? src;

  String get plain => spans.map((s) => s.text).join();
}

class TextChapter {
  const TextChapter({required this.title, required this.href, required this.blocks});

  final String title;
  final String href;
  final List<TextBlock> blocks;
}

class TocEntry {
  const TocEntry({required this.title, required this.chapter, required this.level});

  final String title;
  final int chapter;
  final int level;
}

/// The text of an EPUB for the reader (`GET /api/v1/ebooks/{id}/text`).
class EbookText {
  const EbookText({required this.title, required this.chapters, required this.toc});

  factory EbookText.parse(String json) {
    final j = jsonDecode(json);
    if (j is! Map<String, dynamic>) throw const FormatException('no text');
    final chapters = <TextChapter>[
      for (final c in (j['chapters'] is List ? j['chapters'] as List : const []).whereType<Map<String, dynamic>>())
        TextChapter(
          title: '${c['title'] ?? ''}',
          href: '${c['href'] ?? ''}',
          blocks: [
            for (final b in (c['blocks'] is List ? c['blocks'] as List : const []).whereType<Map<String, dynamic>>())
              TextBlock(
                '${b['kind'] ?? 'p'}',
                [
                  for (final s in (b['spans'] is List ? b['spans'] as List : const []).whereType<Map<String, dynamic>>())
                    TextSpanData('${s['t'] ?? ''}', bold: s['b'] == true, italic: s['i'] == true, sup: s['sup'] == true),
                ],
                src: b['src'] is String ? b['src'] as String : null,
              ),
          ],
        ),
    ];
    final toc = <TocEntry>[
      for (final t in (j['toc'] is List ? j['toc'] as List : const []).whereType<Map<String, dynamic>>())
        if (t['chapter'] is int && (t['chapter'] as int) < chapters.length)
          TocEntry(title: '${t['title'] ?? ''}', chapter: t['chapter'] as int, level: t['level'] is int ? t['level'] as int : 0),
    ];
    return EbookText(title: '${j['title'] ?? ''}', chapters: chapters, toc: toc);
  }

  final String title;
  final List<TextChapter> chapters;
  final List<TocEntry> toc;

  /// The share of the book before [block] of [chapter], by the blocks (0..1).
  double percentAt(int chapter, int block) {
    final total = chapters.fold<int>(0, (n, c) => n + c.blocks.length);
    if (total == 0) return 0;
    var before = 0;
    for (var i = 0; i < chapter && i < chapters.length; i++) {
      before += chapters[i].blocks.length;
    }
    return ((before + block) / total).clamp(0.0, 1.0);
  }

  /// The pictures the text shows, by their names inside the EPUB.
  Iterable<String> get pictures => chapters.expand((c) => c.blocks).map((b) => b.src).whereType<String>().toSet();
}
