// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../format.dart';
import '../l10n/app_localizations.dart';
import '../offline/offline_controller.dart';
import '../screens/confirm_dialog.dart';
import 'ebook.dart';
import 'ebook_controller.dart';
import 'epub_reader.dart';
import 'pdf_reader.dart';

/// Opens [e] in its reader; offline only what is on this device (a short note says why).
Future<void> openEbook(BuildContext context, EbookController c, Ebook e) async {
  final offline = OfflineScope.maybeOf(context)?.active ?? false;
  if ((offline || c.api == null) && !c.isKept(e)) {
    ScaffoldMessenger.maybeOf(context)
      ?..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(key: const ValueKey('ebook-not-here'), content: Text(AppLocalizations.of(context).ebookNotDownloaded)));
    return;
  }
  await Navigator.of(context).push(MaterialPageRoute<void>(
    builder: (_) => e.isPdf ? PdfReaderScreen(controller: c, book: e) : EpubReaderScreen(controller: c, book: e),
  ));
}

/// The rubric "E-books" of the library: what is begun first, then all, by author; each one opens in the reader and is loaded onto
/// the device to read offline.
class EbooksBody extends StatefulWidget {
  const EbooksBody({super.key, required this.controller});

  final EbookController controller;

  @override
  State<EbooksBody> createState() => _EbooksBodyState();
}

class _EbooksBodyState extends State<EbooksBody> {
  String _filter = '';
  bool _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_started) return;
    _started = true;
    final offline = OfflineScope.maybeOf(context)?.active ?? false;
    widget.controller.load(offline: offline);
  }

  bool _fits(Ebook e) {
    if (_filter.isEmpty) return true;
    final f = _filter.toLowerCase();
    return e.title.toLowerCase().contains(f) || e.author.toLowerCase().contains(f);
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
        listenable: widget.controller,
        builder: (context, _) {
          final c = widget.controller;
          final l = AppLocalizations.of(context);
          final text = Theme.of(context).textTheme;
          final cs = Theme.of(context).colorScheme;
          final offlineCtl = OfflineScope.maybeOf(context);
          final offline = offlineCtl?.active ?? false;
          // Offline as the library: only what is on this device, or (the tap on the bar) all of it, the others dimmed.
          final onlyKept = offlineCtl?.downloadedOnly ?? false;
          final all = [for (final e in c.books) if (_fits(e) && (!onlyKept || c.isKept(e))) e];
          final begun = [
            for (final e in all)
              if ((c.progressOf(e)?.percent ?? 0) > 0 && (c.progressOf(e)?.percent ?? 0) < 0.995) e,
          ]..sort((a, b) => (c.progressOf(b)?.readAt ?? 0).compareTo(c.progressOf(a)?.readAt ?? 0));
          return SliverPadding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 24),
            sliver: SliverList.list(children: [
              if (c.books.length > 8)
                Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: TextField(
                    key: const ValueKey('ebook-filter'),
                    decoration: InputDecoration(prefixIcon: const Icon(Icons.search), hintText: l.ebookFilter, isDense: true),
                    onChanged: (v) => setState(() => _filter = v.trim()),
                  ),
                ),
              if (c.stale && !offline && c.api != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: Text(l.ebookStale, key: const ValueKey('ebook-stale'), style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
                ),
              if (c.loaded && c.books.isEmpty)
                Padding(padding: const EdgeInsets.symmetric(vertical: 24), child: Text(l.ebookNone, key: const ValueKey('ebook-none')))
              else if (!c.loaded)
                const Padding(padding: EdgeInsets.all(24), child: Center(child: CircularProgressIndicator()))
              else ...[
                if (begun.isNotEmpty) ...[
                  Text(l.ebookContinue, style: text.titleMedium),
                  const SizedBox(height: 8),
                  for (final e in begun) _EbookCard(key: ValueKey('ebook-begun-${e.id}'), controller: c, book: e, offline: offline),
                  const SizedBox(height: 16),
                ],
                Text(l.ebookAll, style: text.titleMedium),
                const SizedBox(height: 8),
                for (final e in all) _EbookCard(key: ValueKey('ebook-${e.id}'), controller: c, book: e, offline: offline),
              ],
            ]),
          );
        },
      );
}

class _EbookCard extends StatelessWidget {
  const _EbookCard({super.key, required this.controller, required this.book, required this.offline});

  final EbookController controller;
  final Ebook book;
  final bool offline;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final locale = Localizations.localeOf(context).toLanguageTag();
    final cs = Theme.of(context).colorScheme;
    final c = controller;
    final kept = c.isKept(book);
    final outdated = c.outdated(book);
    final loading = c.loading[book.id];
    final p = c.progressOf(book);
    final meta = [
      book.isPdf ? 'PDF' : 'EPUB',
      if (p != null && p.percent > 0) l.ebookRead((p.percent * 100).round()),
      if (loading != null) l.showLoading(formatBytes(locale, loading)) else if (outdated) l.showOutdated else if (kept) l.showOnDevice,
    ].join(' · ');
    final dim = (offline || c.api == null) && !kept;
    final card = Card(
      margin: const EdgeInsets.only(bottom: 8),
      clipBehavior: Clip.antiAlias,
      child: ListTile(
        contentPadding: const EdgeInsets.fromLTRB(8, 4, 4, 4),
        leading: SizedBox(width: 44, height: 64, child: _Cover(controller: c, book: book)),
        title: Text(book.title, maxLines: 2, overflow: TextOverflow.ellipsis),
        subtitle: Text([if (book.author.isNotEmpty) book.author, meta].join('\n'), maxLines: 3),
        isThreeLine: book.author.isNotEmpty,
        onTap: () => openEbook(context, c, book),
        trailing: loading != null
            ? const Padding(padding: EdgeInsets.all(12), child: SizedBox.square(dimension: 20, child: CircularProgressIndicator(strokeWidth: 2)))
            : (kept && !outdated)
                ? IconButton(
                    key: ValueKey('ebook-remove-${book.id}'),
                    tooltip: l.ebookRemove,
                    icon: Icon(Icons.offline_pin, color: cs.primary),
                    onPressed: () async {
                      if (await confirmRemoval(context, title: l.ebookRemove, text: l.ebookRemoveAsk(book.title), action: l.ebookRemove)) {
                        await c.remove(book);
                      }
                    },
                  )
                : IconButton(
                    key: ValueKey('ebook-download-${book.id}'),
                    tooltip: outdated ? l.showUpdate : l.ebookDownload,
                    icon: Icon(outdated ? Icons.sync : Icons.download_outlined),
                    onPressed: offline || c.api == null
                        ? null
                        : () async {
                            final ok = await c.download(book);
                            if (!context.mounted) return;
                            ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(ok ? l.ebookDownloaded : (c.error == 'space' ? l.showSpaceLow : l.ebookFailed))));
                          },
                  ),
      ),
    );
    return dim ? Opacity(key: ValueKey('ebook-dim-${book.id}'), opacity: 0.5, child: card) : card;
  }
}

class _Cover extends StatelessWidget {
  const _Cover({required this.controller, required this.book});

  final EbookController controller;
  final Ebook book;

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final none = DecoratedBox(
      decoration: BoxDecoration(color: cs.surfaceContainerHighest, borderRadius: BorderRadius.circular(4)),
      child: Icon(book.isPdf ? Icons.picture_as_pdf_outlined : Icons.auto_stories_outlined, color: cs.onSurfaceVariant),
    );
    return FutureBuilder<Uint8List?>(
      future: controller.cover(book),
      builder: (context, snap) {
        final b = snap.data;
        if (b == null) return none;
        return ClipRRect(
          borderRadius: BorderRadius.circular(4),
          child: Image.memory(b, fit: BoxFit.cover, cacheWidth: 132, errorBuilder: (_, _, _) => none),
        );
      },
    );
  }
}
