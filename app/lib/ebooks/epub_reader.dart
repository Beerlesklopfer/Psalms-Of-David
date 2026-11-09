// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:scrollable_positioned_list/scrollable_positioned_list.dart';

import '../l10n/app_localizations.dart';
import 'ebook.dart';
import 'ebook_controller.dart';

/// How the reader looks: the size of the letters, light, sepia or dark, with or without serifs. Kept on this device for all books.
class ReaderLook {
  const ReaderLook({this.scale = 1.0, this.theme = 'light', this.serif = true});

  static ReaderLook read(EbookShelf shelf) {
    final f = File('${shelf.root.path}/look.json');
    try {
      final j = jsonDecode(f.readAsStringSync()) as Map<String, dynamic>;
      return ReaderLook(
        scale: (j['scale'] is num ? (j['scale'] as num).toDouble() : 1.0).clamp(0.7, 2.2),
        theme: const ['light', 'sepia', 'dark'].contains(j['theme']) ? j['theme'] as String : 'light',
        serif: j['serif'] != false,
      );
    } on Object {
      return const ReaderLook();
    }
  }

  void write(EbookShelf shelf) {
    try {
      shelf.root.createSync(recursive: true);
      File('${shelf.root.path}/look.json').writeAsStringSync(jsonEncode({'scale': scale, 'theme': theme, 'serif': serif}));
    } on FileSystemException {
      // Not kept: the next start looks as before.
    }
  }

  ReaderLook copy({double? scale, String? theme, bool? serif}) => ReaderLook(scale: scale ?? this.scale, theme: theme ?? this.theme, serif: serif ?? this.serif);

  final double scale;
  final String theme;
  final bool serif;

  Color get paper => switch (theme) { 'sepia' => const Color(0xFFF4ECD8), 'dark' => const Color(0xFF121212), _ => Colors.white };
  Color get ink => switch (theme) { 'sepia' => const Color(0xFF3B2F1E), 'dark' => const Color(0xFFDADADA), _ => const Color(0xFF1B1B1B) };
  Color get faint => ink.withValues(alpha: 0.6);
  String? get family => serif ? 'serif' : null;
}

/// The reader of an EPUB: one chapter at a time, block by block as the server made the text; the place is kept per block, on this
/// device at once and on the server (`epub:<chapter>:<block>`). The table of contents, the size of the letters and the colours are in
/// the bar at the top; at the end of a chapter the next one follows with a tap.
class EpubReaderScreen extends StatefulWidget {
  const EpubReaderScreen({super.key, required this.controller, required this.book});

  final EbookController controller;
  final Ebook book;

  @override
  State<EpubReaderScreen> createState() => _EpubReaderScreenState();
}

class _EpubReaderScreenState extends State<EpubReaderScreen> {
  EbookText? _text;
  Object? _failed;
  int _chapter = 0;
  int _startBlock = 0;
  int _block = 0;
  late ReaderLook _look = ReaderLook.read(widget.controller.shelf);
  final ItemScrollController _scroll = ItemScrollController();
  final ItemPositionsListener _positions = ItemPositionsListener.create();
  Timer? _save;

  @override
  void initState() {
    super.initState();
    _positions.itemPositions.addListener(_moved);
    _open();
  }

  @override
  void dispose() {
    _positions.itemPositions.removeListener(_moved);
    if (_save?.isActive ?? false) {
      _save!.cancel();
      _keep();
    }
    super.dispose();
  }

  Future<void> _open() async {
    try {
      final t = await widget.controller.text(widget.book);
      final at = widget.controller.progressOf(widget.book)?.epub;
      if (!mounted) return;
      setState(() {
        _text = t;
        if (at != null && at.$1 < t.chapters.length) {
          _chapter = at.$1;
          _startBlock = at.$2.clamp(0, t.chapters[at.$1].blocks.length - 1);
          _block = _startBlock;
        }
      });
    } on Object catch (e) {
      if (mounted) setState(() => _failed = e);
    }
  }

  /// The first block that shows is where the person is.
  void _moved() {
    final shown = _positions.itemPositions.value.where((p) => p.itemTrailingEdge > 0.02);
    if (shown.isEmpty) return;
    final first = shown.reduce((a, b) => a.index < b.index ? a : b).index;
    final blocks = _text?.chapters[_chapter].blocks.length ?? 0;
    final block = first.clamp(0, blocks == 0 ? 0 : blocks - 1);
    if (block == _block) return;
    _block = block;
    _save?.cancel();
    _save = Timer(const Duration(seconds: 2), _keep);
  }

  void _keep() {
    final t = _text;
    if (t == null) return;
    final p = EbookProgress(locator: 'epub:$_chapter:$_block', percent: t.percentAt(_chapter, _block), readAt: DateTime.now().millisecondsSinceEpoch);
    widget.controller.saveProgress(widget.book, p);
  }

  void _goChapter(int c, {int block = 0}) {
    final t = _text;
    if (t == null || c < 0 || c >= t.chapters.length) return;
    setState(() {
      _chapter = c;
      _startBlock = block;
      _block = block;
    });
    _save?.cancel();
    _keep();
  }

  void _setLook(ReaderLook look) {
    setState(() => _look = look);
    look.write(widget.controller.shelf);
  }

  Future<void> _contents() async {
    final t = _text;
    if (t == null) return;
    final l = AppLocalizations.of(context);
    final entries = t.toc.isNotEmpty ? t.toc : [for (final (i, c) in t.chapters.indexed) TocEntry(title: c.title.isEmpty ? l.ebookChapter(i + 1) : c.title, chapter: i, level: 0)];
    final chosen = await showModalBottomSheet<int>(
      context: context,
      isScrollControlled: true,
      builder: (context) => DraggableScrollableSheet(
        expand: false,
        initialChildSize: 0.7,
        builder: (context, scroll) => ListView(
          controller: scroll,
          children: [
            ListTile(title: Text(l.ebookContents, style: Theme.of(context).textTheme.titleMedium)),
            for (final e in entries)
              ListTile(
                key: ValueKey('toc-${e.chapter}-${e.title}'),
                contentPadding: EdgeInsets.only(left: 16.0 + 16 * e.level, right: 16),
                selected: e.chapter == _chapter,
                title: Text(e.title),
                onTap: () => Navigator.of(context).pop(e.chapter),
              ),
          ],
        ),
      ),
    );
    if (chosen != null) _goChapter(chosen);
  }

  Future<void> _lookSheet() async {
    final l = AppLocalizations.of(context);
    await showModalBottomSheet<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, set) => Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(children: [
                IconButton(key: const ValueKey('look-smaller'), tooltip: l.ebookSmaller, icon: const Icon(Icons.text_decrease), onPressed: () {
                  _setLook(_look.copy(scale: (_look.scale - 0.1).clamp(0.7, 2.2)));
                  set(() {});
                }),
                Expanded(child: Text('${(_look.scale * 100).round()} %', textAlign: TextAlign.center)),
                IconButton(key: const ValueKey('look-larger'), tooltip: l.ebookLarger, icon: const Icon(Icons.text_increase), onPressed: () {
                  _setLook(_look.copy(scale: (_look.scale + 0.1).clamp(0.7, 2.2)));
                  set(() {});
                }),
              ]),
              const SizedBox(height: 8),
              SegmentedButton<String>(
                segments: [
                  ButtonSegment(value: 'light', label: Text(l.ebookLight)),
                  ButtonSegment(value: 'sepia', label: Text(l.ebookSepia)),
                  ButtonSegment(value: 'dark', label: Text(l.ebookDark)),
                ],
                selected: {_look.theme},
                onSelectionChanged: (v) {
                  _setLook(_look.copy(theme: v.first));
                  set(() {});
                },
              ),
              const SizedBox(height: 8),
              SwitchListTile(
                title: Text(l.ebookSerif),
                value: _look.serif,
                onChanged: (v) {
                  _setLook(_look.copy(serif: v));
                  set(() {});
                },
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final t = _text;
    final chapter = t == null || t.chapters.isEmpty ? null : t.chapters[_chapter];
    return Scaffold(
      backgroundColor: _look.paper,
      appBar: AppBar(
        backgroundColor: _look.paper,
        foregroundColor: _look.ink,
        title: Text(chapter?.title.isNotEmpty == true ? chapter!.title : widget.book.title, maxLines: 1, overflow: TextOverflow.ellipsis),
        actions: [
          IconButton(key: const ValueKey('reader-contents'), tooltip: l.ebookContents, icon: const Icon(Icons.toc), onPressed: t == null ? null : _contents),
          IconButton(key: const ValueKey('reader-look'), tooltip: l.ebookLook, icon: const Icon(Icons.format_size), onPressed: _lookSheet),
        ],
      ),
      body: _failed != null
          ? Center(child: Padding(padding: const EdgeInsets.all(24), child: Text(l.ebookOpenFailed, key: const ValueKey('reader-failed'), style: TextStyle(color: _look.ink))))
          : chapter == null
              ? Center(child: t == null ? const CircularProgressIndicator() : Text(l.ebookEmpty, style: TextStyle(color: _look.ink)))
              : SelectionArea(
                  child: ScrollablePositionedList.builder(
                    key: ValueKey('chapter-$_chapter'),
                    itemScrollController: _scroll,
                    itemPositionsListener: _positions,
                    initialScrollIndex: _startBlock,
                    padding: EdgeInsets.symmetric(horizontal: MediaQuery.sizeOf(context).width > 760 ? (MediaQuery.sizeOf(context).width - 700) / 2 : 20, vertical: 16),
                    itemCount: chapter.blocks.length + 1,
                    itemBuilder: (context, i) => i == chapter.blocks.length
                        ? _ChapterEnd(
                            look: _look,
                            hasPrev: _chapter > 0,
                            hasNext: _chapter < t!.chapters.length - 1,
                            onPrev: () => _goChapter(_chapter - 1),
                            onNext: () => _goChapter(_chapter + 1),
                          )
                        : BlockView(key: ValueKey('block-$_chapter-$i'), block: chapter.blocks[i], look: _look, picture: (name) => widget.controller.picture(widget.book, name)),
                  ),
                ),
    );
  }
}

class _ChapterEnd extends StatelessWidget {
  const _ChapterEnd({required this.look, required this.hasPrev, required this.hasNext, required this.onPrev, required this.onNext});

  final ReaderLook look;
  final bool hasPrev;
  final bool hasNext;
  final VoidCallback onPrev;
  final VoidCallback onNext;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final style = OutlinedButton.styleFrom(foregroundColor: look.ink, side: BorderSide(color: look.faint));
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 32),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          if (hasPrev) OutlinedButton.icon(key: const ValueKey('chapter-prev'), style: style, onPressed: onPrev, icon: const Icon(Icons.chevron_left), label: Text(l.ebookPrevChapter)) else const SizedBox(),
          if (hasNext) FilledButton.icon(key: const ValueKey('chapter-next'), onPressed: onNext, icon: const Icon(Icons.chevron_right), label: Text(l.ebookNextChapter)) else Text(l.ebookTheEnd, style: TextStyle(color: look.faint)),
        ],
      ),
    );
  }
}

/// One block of the text, drawn with the look of the reader.
class BlockView extends StatelessWidget {
  const BlockView({super.key, required this.block, required this.look, required this.picture});

  final TextBlock block;
  final ReaderLook look;
  final Future<Uint8List?> Function(String name) picture;

  @override
  Widget build(BuildContext context) {
    final base = TextStyle(color: look.ink, fontSize: 18 * look.scale, height: 1.5, fontFamily: look.family);
    final style = switch (block.kind) {
      'h1' => base.copyWith(fontSize: 27 * look.scale, fontWeight: FontWeight.w700, height: 1.25),
      'h2' => base.copyWith(fontSize: 23 * look.scale, fontWeight: FontWeight.w700, height: 1.3),
      'h3' => base.copyWith(fontSize: 20 * look.scale, fontWeight: FontWeight.w600, height: 1.3),
      'pre' => base.copyWith(fontFamily: 'monospace', fontSize: 15 * look.scale, height: 1.35),
      _ => base,
    };
    if (block.kind == 'hr') return Padding(padding: const EdgeInsets.symmetric(vertical: 16), child: Divider(color: look.faint));
    if (block.kind == 'img') {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 12),
        child: FutureBuilder<Uint8List?>(
          future: block.src == null ? Future.value(null) : picture(block.src!),
          builder: (context, snap) {
            final b = snap.data;
            final alt = block.plain;
            if (b == null) return alt.isEmpty ? const SizedBox.shrink() : Text('[$alt]', style: base.copyWith(color: look.faint, fontStyle: FontStyle.italic));
            return Semantics(label: alt, image: true, child: Image.memory(b, fit: BoxFit.contain, errorBuilder: (_, _, _) => const SizedBox.shrink()));
          },
        ),
      );
    }
    final spans = [
      for (final s in block.spans)
        TextSpan(
          text: s.text,
          style: TextStyle(
            fontWeight: s.bold ? FontWeight.w700 : null,
            fontStyle: s.italic ? FontStyle.italic : null,
            fontSize: s.sup ? (style.fontSize ?? 18) * 0.65 : null,
            fontFeatures: s.sup ? const [FontFeature.superscripts()] : null,
          ),
        ),
    ];
    final text = Text.rich(TextSpan(style: style, children: spans));
    final top = block.kind.startsWith('h') ? 18.0 * look.scale : 0.0;
    return Padding(
      padding: EdgeInsets.only(top: top, bottom: 10 * look.scale),
      child: switch (block.kind) {
        'quote' => Container(
            padding: const EdgeInsets.only(left: 14),
            decoration: BoxDecoration(border: Border(left: BorderSide(color: look.faint, width: 3))),
            child: text,
          ),
        'li' => Row(crossAxisAlignment: CrossAxisAlignment.start, children: [Text('•  ', style: style), Expanded(child: text)]),
        _ => text,
      },
    );
  }
}
