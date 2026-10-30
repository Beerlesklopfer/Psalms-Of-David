// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/services.dart';

import '../l10n/app_localizations.dart';
import '../server/qr_reader.dart';

Future<Uint8List?> _pickPicture(String title) async {
  const images = XTypeGroup(label: 'images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif']);
  final file = await openFile(acceptedTypeGroups: [images], confirmButtonText: title);
  return file?.readAsBytes();
}

/// Why connecting failed; the screen words it in the language of the app.
enum ConnectError { address, code, limit, network, certificate, qr, signedOut, other }

/// First start: pair this device with the server. The address comes from the
/// administration page (the text under the QR code); scanning the code with the
/// camera is for the phones.
class ConnectScreen extends StatefulWidget {
  const ConnectScreen({
    super.key,
    required this.deviceName,
    required this.onPair,
    required this.onDemo,
    this.onRetry,
    this.error,
    this.searching = false,
    this.initialAddress,
    this.pickPicture = _pickPicture,
  });

  final String deviceName;

  /// Pairs and loads the library; returns why it failed, or null on success.
  final Future<ConnectError?> Function(String address, String deviceName) onPair;
  final VoidCallback onDemo;

  /// Given when a pairing is already stored and the server was just not reachable.
  final Future<void> Function()? onRetry;
  final ConnectError? error;

  /// The app looks for the server again by itself (the pairing is stored, the server was not reachable).
  final bool searching;

  /// A pairing address that came with the start of the program (`psalm david://pair?...`).
  final String? initialAddress;

  /// Lets the person choose a picture with the QR code and returns its bytes; null if none was chosen.
  /// A parameter so that a test can answer without a dialog.
  final Future<Uint8List?> Function(String title) pickPicture;

  @override
  State<ConnectScreen> createState() => _ConnectScreenState();
}

class _ConnectScreenState extends State<ConnectScreen> {
  late final TextEditingController _address = TextEditingController(text: widget.initialAddress);
  late final TextEditingController _name = TextEditingController(text: widget.deviceName);
  ConnectError? _error;
  bool _busy = false;

  @override
  void didUpdateWidget(ConnectScreen old) {
    super.didUpdateWidget(old);
    // The app tried again by itself and found another reason.
    if (old.error != widget.error && !_busy) _error = widget.error;
  }

  @override
  void initState() {
    super.initState();
    _error = widget.error;
    if (widget.initialAddress == null) _takeAddressFromClipboard();
  }

  /// If a pairing address was just copied, it is already in the field.
  Future<void> _takeAddressFromClipboard() async {
    try {
      final text = (await Clipboard.getData(Clipboard.kTextPlain))?.text?.trim();
      if (mounted && text != null && text.startsWith('david://pair?') && _address.text.isEmpty) {
        setState(() => _address.text = text);
      }
    } on PlatformException {
      // No clipboard to read; the field stays empty.
    }
  }

  /// The QR code of the administration, as a picture: a screenshot or a photo.
  Future<void> _readQr() async {
    final l = AppLocalizations.of(context);
    final bytes = await widget.pickPicture(l.qrChooseTitle);
    if (bytes == null || !mounted) return;
    final text = readQrCode(bytes);
    setState(() {
      if (text == null) {
        _error = ConnectError.qr;
      } else {
        _error = null;
        _address.text = text.trim();
      }
    });
  }

  /// A button next to the field, for the cases where Ctrl+V does not reach it.
  Future<void> _paste() async {
    final text = (await Clipboard.getData(Clipboard.kTextPlain))?.text?.trim();
    if (mounted && text != null && text.isNotEmpty) setState(() => _address.text = text);
  }

  @override
  void dispose() {
    _address.dispose();
    _name.dispose();
    super.dispose();
  }

  Future<void> _run(Future<ConnectError?> Function() action) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    final error = await action();
    if (mounted) {
      setState(() {
        _busy = false;
        _error = error;
      });
    }
  }

  String _message(AppLocalizations l, ConnectError e) => switch (e) {
        ConnectError.address => l.connectErrorAddress,
        ConnectError.code => l.connectErrorCode,
        ConnectError.limit => l.connectErrorLimit,
        ConnectError.network => l.connectErrorNetwork,
        ConnectError.certificate => l.connectErrorCertificate,
        ConnectError.qr => l.qrNotFound,
        ConnectError.signedOut => l.connectErrorSignedOut,
        ConnectError.other => l.connectErrorOther,
      };

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final theme = Theme.of(context);
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 520),
          child: ListView(
            shrinkWrap: true,
            padding: const EdgeInsets.all(24),
            children: [
              Text(l.connectTitle, style: theme.textTheme.headlineSmall),
              const SizedBox(height: 12),
              Card(
                margin: EdgeInsets.zero,
                color: theme.colorScheme.primaryContainer.withValues(alpha: 0.25),
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Icon(Icons.info_outline, color: theme.colorScheme.primary),
                      const SizedBox(width: 12),
                      Expanded(child: Text(l.connectNeedsDavid)),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 12),
              Text(l.connectIntro),
              const SizedBox(height: 24),
              TextField(
                controller: _address,
                enabled: !_busy,
                minLines: 1,
                maxLines: 3,
                decoration: InputDecoration(
                  labelText: l.pairingAddressLabel,
                  hintText: 'david://pair?…',
                  suffixIcon: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        tooltip: l.qrButton,
                        icon: const Icon(Icons.qr_code_scanner),
                        onPressed: _busy ? null : _readQr,
                      ),
                      IconButton(
                        tooltip: l.pasteButton,
                        icon: const Icon(Icons.content_paste),
                        onPressed: _busy ? null : _paste,
                      ),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 12),
              TextField(
                controller: _name,
                enabled: !_busy,
                decoration: InputDecoration(labelText: l.deviceNameLabel),
              ),
              if (_error != null) ...[
                const SizedBox(height: 12),
                Text(_message(l, _error!), style: TextStyle(color: theme.colorScheme.error)),
              ],
              if (widget.searching && !_busy) ...[
                const SizedBox(height: 8),
                Row(
                  children: [
                    const SizedBox.square(dimension: 16, child: CircularProgressIndicator(strokeWidth: 2)),
                    const SizedBox(width: 8),
                    Expanded(key: const ValueKey('searching-server'), child: Text(l.connectSearching)),
                  ],
                ),
              ],
              const SizedBox(height: 20),
              FilledButton(
                onPressed: _busy ? null : () => _run(() => widget.onPair(_address.text, _name.text)),
                child: Text(l.connectButton),
              ),
              if (widget.onRetry != null)
                TextButton(
                  onPressed: _busy
                      ? null
                      : () => _run(() async {
                            await widget.onRetry!();
                            return ConnectError.network;
                          }),
                  child: Text(l.retryButton),
                ),
              TextButton(onPressed: _busy ? null : widget.onDemo, child: Text(l.useDemoButton)),
            ],
          ),
        ),
      ),
    );
  }
}
