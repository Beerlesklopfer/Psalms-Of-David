// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:url_launcher/url_launcher.dart';

import '../l10n/app_localizations.dart';

/// What the scanner hands back: the text of a pairing code, or the wish to choose a picture instead (no camera, or the person asked for it).
class QrScanOutcome {
  const QrScanOutcome.text(String this.text) : usePicture = false;
  const QrScanOutcome.picture() : text = null, usePicture = true;

  final String? text;
  final bool usePicture;
}

/// Phones and the Mac scan with the camera; the other systems read the QR code from a picture.
bool get cameraScanSupported => Platform.isAndroid || Platform.isIOS || Platform.isMacOS;

/// The first text in [values] that is a pairing address (`david://pair?...`), or null.
String? firstPairingAddress(Iterable<String?> values) {
  for (final v in values) {
    final text = v?.trim();
    if (text != null && text.startsWith('david://pair?')) return text;
  }
  return null;
}

/// Builds the camera view; a parameter so that a test can put a fake in place of the plugin (there is no camera in a test).
typedef ScannerViewBuilder = Widget Function(
  BuildContext context,
  MobileScannerController controller,
  void Function(BarcodeCapture capture) onDetect,
  Widget Function(BuildContext context, MobileScannerException error) errorBuilder,
);

/// Full-screen camera that reads the QR code of a pairing (the text under the QR code on the administration page). It closes itself with a
/// [QrScanOutcome] as soon as it has a pairing address; other QR codes are ignored with one short hint. The person can cancel (pops `null`) or ask for a
/// picture instead.
class QrScanScreen extends StatefulWidget {
  const QrScanScreen({super.key, this.controller, this.viewBuilder, this.openSettings = _openAppSettings});

  /// Given by a test; otherwise the screen makes (and disposes) its own, which looks for QR codes only.
  final MobileScannerController? controller;
  final ScannerViewBuilder? viewBuilder;

  /// Opens the settings of the app, where the camera can be allowed (iOS); false if that is not possible here.
  final Future<bool> Function() openSettings;

  static Future<QrScanOutcome?> route(BuildContext context) =>
      Navigator.of(context).push<QrScanOutcome>(MaterialPageRoute(fullscreenDialog: true, builder: (_) => const QrScanScreen()));

  @override
  State<QrScanScreen> createState() => _QrScanScreenState();
}

Future<bool> _openAppSettings() async {
  if (!Platform.isIOS) return false;
  try {
    return await launchUrl(Uri.parse('app-settings:'));
  } on Object {
    return false;
  }
}

class _QrScanScreenState extends State<QrScanScreen> {
  late final MobileScannerController _controller = widget.controller ?? MobileScannerController(formats: const [BarcodeFormat.qrCode]);
  bool _accepted = false;
  bool _hintShown = false;
  bool _showHint = false;
  Timer? _hintTimer;

  @override
  void dispose() {
    _hintTimer?.cancel();
    if (widget.controller == null) _controller.dispose();
    super.dispose();
  }

  void _onDetect(BarcodeCapture capture) {
    if (_accepted || !mounted) return;
    final found = firstPairingAddress(capture.barcodes.map((b) => b.rawValue));
    if (found != null) {
      _accepted = true;
      HapticFeedback.mediumImpact();
      // The camera stops at once: one scan is enough.
      unawaited(_controller.stop().catchError((Object _) {}));
      Navigator.of(context).pop(QrScanOutcome.text(found));
      return;
    }
    // Another QR code is in front of the camera: say so once, keep scanning.
    if (capture.barcodes.isNotEmpty && !_hintShown) {
      _hintShown = true;
      setState(() => _showHint = true);
      _hintTimer = Timer(const Duration(seconds: 4), () {
        if (mounted) setState(() => _showHint = false);
      });
    }
  }

  void _usePicture() => Navigator.of(context).pop(const QrScanOutcome.picture());

  Widget _error(BuildContext context, MobileScannerException error) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    if (error.errorCode != MobileScannerErrorCode.permissionDenied) {
      // No camera (a computer without one, the simulator) or a failure of it: the picture chooser takes over.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && !_accepted) _usePicture();
      });
      return const ColoredBox(color: Colors.black);
    }
    return ColoredBox(
      color: Colors.black,
      child: Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(Icons.no_photography_outlined, color: Colors.white70, size: 48),
              const SizedBox(height: 16),
              Text(l.qrScanPermissionDenied, key: const ValueKey('qr-permission-denied'), textAlign: TextAlign.center, style: text.bodyLarge?.copyWith(color: Colors.white)),
              const SizedBox(height: 16),
              if (Platform.isIOS)
                FilledButton(
                  key: const ValueKey('qr-open-settings'),
                  onPressed: () async {
                    if (!await widget.openSettings()) _usePicture();
                  },
                  child: Text(l.qrScanOpenSettings),
                ),
              TextButton(key: const ValueKey('qr-use-picture-denied'), onPressed: _usePicture, child: Text(l.qrScanUsePicture)),
            ],
          ),
        ),
      ),
    );
  }

  Widget _view(BuildContext context) {
    final build = widget.viewBuilder;
    if (build != null) return build(context, _controller, _onDetect, _error);
    return MobileScanner(controller: _controller, onDetect: _onDetect, errorBuilder: _error);
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    return Scaffold(
      backgroundColor: Colors.black,
      body: Stack(
        fit: StackFit.expand,
        children: [
          _view(context),
          const IgnorePointer(child: CustomPaint(painter: _FramePainter())),
          SafeArea(
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                  child: Row(
                    children: [
                      IconButton(
                        key: const ValueKey('qr-cancel'),
                        tooltip: l.cancel,
                        color: Colors.white,
                        icon: const Icon(Icons.close),
                        onPressed: () => Navigator.of(context).pop(),
                      ),
                      Expanded(child: Text(l.qrScanTitle, style: text.titleMedium?.copyWith(color: Colors.white))),
                      // The torch only where the device has one.
                      ValueListenableBuilder<MobileScannerState>(
                        valueListenable: _controller,
                        builder: (context, state, _) {
                          if (state.torchState == TorchState.unavailable) return const SizedBox.shrink();
                          final on = state.torchState == TorchState.on;
                          return IconButton(
                            key: const ValueKey('qr-torch'),
                            tooltip: l.qrScanTorch,
                            color: Colors.white,
                            icon: Icon(on ? Icons.flash_on : Icons.flash_off),
                            onPressed: () => unawaited(_controller.toggleTorch().catchError((Object _) {})),
                          );
                        },
                      ),
                    ],
                  ),
                ),
                const Spacer(),
                if (_showHint)
                  Container(
                    key: const ValueKey('qr-not-pairing'),
                    margin: const EdgeInsets.symmetric(horizontal: 24),
                    padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
                    decoration: BoxDecoration(color: Colors.red.shade900, borderRadius: BorderRadius.circular(8)),
                    child: Text(l.qrScanNotPairing, style: text.bodyMedium?.copyWith(color: Colors.white)),
                  ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(24, 12, 24, 4),
                  child: Text(l.qrScanHint, textAlign: TextAlign.center, style: text.bodyMedium?.copyWith(color: Colors.white)),
                ),
                TextButton(
                  key: const ValueKey('qr-use-picture'),
                  onPressed: _usePicture,
                  child: Text(l.qrScanUsePicture, style: const TextStyle(color: Colors.white)),
                ),
                const SizedBox(height: 8),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// A dark veil over the picture with a clear square in the middle: where the QR code goes.
class _FramePainter extends CustomPainter {
  const _FramePainter();

  @override
  void paint(Canvas canvas, Size size) {
    final side = size.shortestSide * 0.65;
    final square = RRect.fromRectAndRadius(Rect.fromCenter(center: size.center(Offset.zero), width: side, height: side), const Radius.circular(16));
    final veil = Path.combine(PathOperation.difference, Path()..addRect(Offset.zero & size), Path()..addRRect(square));
    canvas.drawPath(veil, Paint()..color = Colors.black54);
    canvas.drawRRect(
      square,
      Paint()
        ..color = Colors.white
        ..style = PaintingStyle.stroke
        ..strokeWidth = 3,
    );
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}
