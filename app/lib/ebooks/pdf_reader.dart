// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:pdfrx/pdfrx.dart';

import '../l10n/app_localizations.dart';
import 'ebook.dart';
import 'ebook_controller.dart';

/// The reader of a PDF: its pages as they are, one below the other, to zoom with two fingers; the page is kept as the place
/// (`pdf:<page>`). A PDF that is not on this device is fetched first.
class PdfReaderScreen extends StatefulWidget {
  const PdfReaderScreen({super.key, required this.controller, required this.book});

  final EbookController controller;
  final Ebook book;

  @override
  State<PdfReaderScreen> createState() => _PdfReaderScreenState();
}

class _PdfReaderScreenState extends State<PdfReaderScreen> {
  String? _path;
  Object? _failed;
  int? _page;
  int _pages = 0;
  final PdfViewerController _viewer = PdfViewerController();
  Timer? _save;

  @override
  void initState() {
    super.initState();
    _page = widget.controller.progressOf(widget.book)?.pdfPage;
    widget.controller.pdf(widget.book).then((p) {
      if (mounted) setState(() => _path = p);
    }, onError: (Object e) {
      if (mounted) setState(() => _failed = e);
    });
  }

  @override
  void dispose() {
    if (_save?.isActive ?? false) {
      _save!.cancel();
      _keep();
    }
    super.dispose();
  }

  void _keep() {
    final page = _page;
    if (page == null) return;
    final percent = _pages <= 1 ? 0.0 : ((page - 1) / (_pages - 1)).clamp(0.0, 1.0);
    widget.controller.saveProgress(widget.book, EbookProgress(locator: 'pdf:$page', percent: percent, readAt: DateTime.now().millisecondsSinceEpoch));
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final path = _path;
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.book.title, maxLines: 1, overflow: TextOverflow.ellipsis),
        actions: [
          if (_pages > 0 && _page != null)
            Center(child: Padding(padding: const EdgeInsets.only(right: 16), child: Text(l.ebookPageOf(_page!, _pages), key: const ValueKey('pdf-page')))),
        ],
      ),
      body: _failed != null
          ? Center(child: Padding(padding: const EdgeInsets.all(24), child: Text(l.ebookOpenFailed, key: const ValueKey('reader-failed'))))
          : path == null
              ? const Center(child: CircularProgressIndicator())
              : PdfViewer.file(
                  path,
                  controller: _viewer,
                  initialPageNumber: _page ?? 1,
                  params: PdfViewerParams(
                    onViewerReady: (doc, controller) {
                      if (mounted) setState(() => _pages = doc.pages.length);
                    },
                    onPageChanged: (page) {
                      if (page == null || page == _page) return;
                      setState(() => _page = page);
                      _save?.cancel();
                      _save = Timer(const Duration(seconds: 2), _keep);
                    },
                  ),
                ),
    );
  }
}
