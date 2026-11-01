// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import '../barcode.dart';
import '../l10n/app_localizations.dart';
import '../models.dart';
import '../player_controller.dart';
import '../server/api_client.dart';
import '../widgets.dart';
import 'qr_scan_screen.dart';

/// Opens a book; a parameter so that a test need not build the book screen.
typedef ItemOpener = void Function(BuildContext context, LibraryItem book);

/// "Is this CD in the family library?" The person holds a CD and scans its barcode with the camera (phones and the Mac) or types the digits
/// (everywhere, and as the fallback where the camera does not work). The local library is asked first (it works offline); when it does not have
/// the code and the app is online, the server is asked, which also knows albums that this device does not have yet.
class BarcodeScanScreen extends StatefulWidget {
  const BarcodeScanScreen({
    super.key,
    this.lookup,
    this.useCamera,
    this.controller,
    this.viewBuilder,
    this.opener,
  });

  /// Asks the server; null without a server (then only the local library answers).
  final BarcodeLookup? lookup;

  /// Whether to show the camera; null: where the platform supports it ([cameraScanSupported]).
  final bool? useCamera;

  /// Given by a test; otherwise the screen makes (and disposes) its own, which looks for product barcodes only.
  final MobileScannerController? controller;
  final ScannerViewBuilder? viewBuilder;
  final ItemOpener? opener;

  static Future<void> route(BuildContext context, {BarcodeLookup? lookup}) =>
      Navigator.of(context).push<void>(
        MaterialPageRoute(builder: (_) => BarcodeScanScreen(lookup: lookup)),
      );

  @override
  State<BarcodeScanScreen> createState() => _BarcodeScanScreenState();
}

/// What the result sheet shows for one album: the item of the server (or the local book) and the local book if there is one.
class _Hit {
  const _Hit({
    required this.title,
    required this.creator,
    required this.kind,
    this.book,
  });

  final String title;
  final String creator;
  final String kind;
  final LibraryItem? book;
}

class _BarcodeScanScreenState extends State<BarcodeScanScreen> {
  final _field = TextEditingController();
  MobileScannerController? _own;
  bool _busy = false;
  String? _error;

  bool get _camera => widget.useCamera ?? cameraScanSupported;

  MobileScannerController get _controller =>
      widget.controller ??
      (_own ??= MobileScannerController(
        formats: const [
          BarcodeFormat.ean13,
          BarcodeFormat.ean8,
          BarcodeFormat.upcA,
          BarcodeFormat.upcE,
        ],
      ));

  @override
  void dispose() {
    _field.dispose();
    _own?.dispose();
    super.dispose();
  }

  void _onDetect(BarcodeCapture capture) {
    if (_busy || !mounted) return;
    for (final b in capture.barcodes) {
      final raw = b.rawValue;
      final gtin = raw == null
          ? null
          : scannedGtin(raw, upcE: b.format == BarcodeFormat.upcE);
      if (gtin == null) continue;
      HapticFeedback.mediumImpact();
      // The first valid code is enough: the camera stops until the answer is closed.
      unawaited(_controller.stop().catchError((Object _) {}));
      unawaited(_check(gtin));
      return;
    }
  }

  Future<void> _submit() async {
    final code = cleanBarcode(_field.text);
    if (!isValidGtin(code)) {
      setState(() => _error = AppLocalizations.of(context).barcodeInvalid);
      return;
    }
    setState(() => _error = null);
    await _check(code);
  }

  /// Looks [code] (a valid GTIN) up and shows the answer in a sheet.
  Future<void> _check(String code) async {
    if (_busy) return;
    setState(() => _busy = true);
    final l = AppLocalizations.of(context);
    final player = PlayerScope.maybeOf(context);
    final library = player?.fullLibrary ?? const <LibraryItem>[];
    final byId = {for (final b in library) b.id: b};
    var hits = [
      for (final b in itemsWithBarcode(library, code))
        _Hit(title: b.title, creator: b.author, kind: b.kind, book: b),
    ];
    String? note;
    if (hits.isEmpty) {
      final lookup = widget.lookup;
      if (lookup == null || (player?.offline?.active ?? false)) {
        note = l.barcodeOfflineNotFound;
      } else {
        try {
          final answer = await lookup(code);
          if (!mounted) return;
          if (answer.invalid) {
            note = l.barcodeInvalid;
          } else {
            hits = [
              for (final i in answer.items)
                _Hit(
                  title: i.title,
                  creator: i.creator,
                  kind: i.kind,
                  book: byId['${i.itemId}'],
                ),
            ];
          }
        } on ApiException catch (e) {
          note = e.code == 'network'
              ? l.barcodeOfflineNotFound
              : l.barcodeLookupFailed;
        } on Object {
          note = l.barcodeLookupFailed;
        }
      }
    }
    if (!mounted) return;
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (sheet) => _ResultSheet(
        code: code,
        hits: hits,
        note: note,
        onOpen: (b) {
          Navigator.of(sheet).pop();
          (widget.opener ?? openItem)(context, b);
        },
      ),
    );
    if (!mounted) return;
    setState(() => _busy = false);
    if (_camera) unawaited(_controller.start().catchError((Object _) {}));
  }

  Widget _cameraError(BuildContext context, MobileScannerException error) {
    final l = AppLocalizations.of(context);
    if (error.errorCode == MobileScannerErrorCode.permissionDenied) {
      return ColoredBox(
        color: Colors.black,
        child: Center(
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Text(
              l.qrScanPermissionDenied,
              key: const ValueKey('barcode-permission-denied'),
              textAlign: TextAlign.center,
              style: const TextStyle(color: Colors.white),
            ),
          ),
        ),
      );
    }
    // No camera or a failure of it: the typed field below does the job.
    return const ColoredBox(color: Colors.black);
  }

  Widget _view(BuildContext context) {
    final build = widget.viewBuilder;
    if (build != null) {
      return build(context, _controller, _onDetect, _cameraError);
    }
    return MobileScanner(
      controller: _controller,
      onDetect: _onDetect,
      errorBuilder: _cameraError,
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final form = Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (!_camera)
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: Text(l.barcodeTypeHint, style: text.bodyMedium),
            ),
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(
                child: TextField(
                  key: const ValueKey('barcode-field'),
                  controller: _field,
                  keyboardType: TextInputType.number,
                  textInputAction: TextInputAction.search,
                  // Digits, spaces and hyphens may be typed or pasted; cleanBarcode strips the separators.
                  inputFormatters: [
                    FilteringTextInputFormatter.allow(RegExp(r'[0-9 \-]')),
                  ],
                  onSubmitted: (_) => unawaited(_submit()),
                  decoration: InputDecoration(
                    labelText: l.barcodeFieldLabel,
                    errorText: _error,
                    border: const OutlineInputBorder(),
                  ),
                ),
              ),
              const SizedBox(width: 12),
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: FilledButton(
                  key: const ValueKey('barcode-check'),
                  onPressed: _busy ? null : () => unawaited(_submit()),
                  child: Text(l.barcodeCheck),
                ),
              ),
            ],
          ),
        ],
      ),
    );
    return Scaffold(
      appBar: AppBar(title: Text(l.barcodeTitle)),
      body: SafeArea(
        child: _camera
            ? Column(
                children: [
                  Expanded(
                    child: Stack(
                      fit: StackFit.expand,
                      children: [
                        ClipRect(child: _view(context)),
                        Positioned(
                          left: 24,
                          right: 24,
                          bottom: 12,
                          child: Text(
                            l.barcodeScanHint,
                            key: const ValueKey('barcode-scan-hint'),
                            textAlign: TextAlign.center,
                            style: text.bodyMedium?.copyWith(
                              color: Colors.white,
                              shadows: const [Shadow(blurRadius: 4)],
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                  form,
                ],
              )
            : SingleChildScrollView(child: form),
      ),
    );
  }
}

class _ResultSheet extends StatelessWidget {
  const _ResultSheet({
    required this.code,
    required this.hits,
    required this.onOpen,
    this.note,
  });

  final String code;
  final List<_Hit> hits;
  final String? note;
  final void Function(LibraryItem book) onOpen;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    return SafeArea(
      child: SingleChildScrollView(
        key: const ValueKey('barcode-result'),
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (hits.isNotEmpty) ...[
              Text(
                l.barcodeInLibrary,
                key: const ValueKey('barcode-in-library'),
                style: text.titleMedium,
              ),
              const SizedBox(height: 8),
              for (final h in hits) _row(context, l, h),
            ] else ...[
              Text(
                l.barcodeNotInLibrary(code),
                key: const ValueKey('barcode-not-in-library'),
                style: text.titleMedium,
              ),
              if (note != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(
                    note!,
                    key: const ValueKey('barcode-note'),
                    style: text.bodyMedium,
                  ),
                ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _row(BuildContext context, AppLocalizations l, _Hit h) {
    final book = h.book;
    final kind = h.kind == 'music' ? l.cdKindMusic : l.cdKindAudiobook;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        children: [
          SizedBox(
            width: 56,
            height: 56,
            child: book == null
                ? const Icon(Icons.album_outlined, size: 40)
                : Cover(book: book),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  h.title,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.bodyLarge,
                ),
                Text(
                  h.creator.isEmpty ? kind : '${h.creator} · $kind',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ],
            ),
          ),
          if (book != null)
            TextButton(
              key: ValueKey('barcode-open-${book.id}'),
              onPressed: () => onOpen(book),
              child: Text(l.barcodeOpen),
            ),
        ],
      ),
    );
  }
}

/// The app-bar button that opens the barcode check. [api] is the server (anything that is a [BarcodeApi] can be asked); null: local library only.
class BarcodeButton extends StatelessWidget {
  const BarcodeButton({super.key, this.api});

  final Object? api;

  @override
  Widget build(BuildContext context) {
    final a = api;
    return IconButton(
      key: const ValueKey('barcode-button'),
      tooltip: AppLocalizations.of(context).barcodeCheckTooltip,
      icon: const Icon(Icons.barcode_reader),
      onPressed: () => BarcodeScanScreen.route(context, lookup: a is BarcodeApi ? a.barcodeLookup : null),
    );
  }
}
