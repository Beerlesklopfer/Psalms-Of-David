// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../l10n/app_localizations.dart';
import '../server/api_client.dart';
import '../server/credentials.dart';
import '../server/device_pairing.dart';

/// "Connect another device" in the settings: how many devices the profile has and the button for the QR code. Only a parent sees it
/// (the server answers a child or a guest with `403`, and then nothing is shown), and only with a server.
class ConnectDeviceCard extends StatefulWidget {
  const ConnectDeviceCard({super.key, required this.api, required this.credentials});

  final DevicePairingApi api;
  final Credentials credentials;

  @override
  State<ConnectDeviceCard> createState() => _ConnectDeviceCardState();
}

class _ConnectDeviceCardState extends State<ConnectDeviceCard> {
  PairingUsage? _usage;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final u = await widget.api.usage();
      if (mounted) setState(() => _usage = u);
    } on Object {
      // Forbidden for a child, or no connection: the card stays away.
      if (mounted) setState(() => _usage = null);
    }
  }

  @override
  Widget build(BuildContext context) {
    final u = _usage;
    if (u == null) return const SizedBox.shrink();
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(bottom: 24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(l.settingsDevices, style: text.titleMedium?.copyWith(color: cs.primary)),
          const SizedBox(height: 8),
          Card(
            margin: EdgeInsets.zero,
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    u.max == null
                        ? l.settingsDevicesNoLimit(u.used)
                        : l.settingsDevicesCount(u.used, u.max!),
                    key: const Key('devices-count'),
                    style: text.bodyLarge,
                  ),
                  const SizedBox(height: 4),
                  Text(
                    l.settingsConnectDeviceHint,
                    style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
                  ),
                  const SizedBox(height: 12),
                  FilledButton.icon(
                    key: const Key('connect-device'),
                    icon: const Icon(Icons.qr_code_2),
                    label: Text(l.settingsConnectDevice),
                    onPressed: () async {
                      await showDialog<void>(
                        context: context,
                        builder: (_) =>
                            ConnectDeviceDialog(api: widget.api, credentials: widget.credentials),
                      );
                      await _load();
                    },
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// The QR code for the other device: made when the dialog opens, valid for a short time (the countdown), once. "New code" makes another.
class ConnectDeviceDialog extends StatefulWidget {
  const ConnectDeviceDialog({super.key, required this.api, required this.credentials});

  final DevicePairingApi api;
  final Credentials credentials;

  @override
  State<ConnectDeviceDialog> createState() => _ConnectDeviceDialogState();
}

class _ConnectDeviceDialogState extends State<ConnectDeviceDialog> {
  PairingCode? _code;
  String? _uri;
  Duration _left = Duration.zero;
  bool _busy = true;
  ApiException? _refused;
  bool _failed = false;
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    unawaited(_make());
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _make() async {
    _timer?.cancel();
    setState(() {
      _busy = true;
      _refused = null;
      _failed = false;
    });
    try {
      final c = await widget.api.makeCode();
      if (!mounted) return;
      final cred = widget.credentials;
      // The address is the one this device uses itself, the way of checking the server is the one it was paired with.
      final uri = PairingAddress(
        serverId: c.serverId.isNotEmpty ? c.serverId : cred.serverId,
        address: cred.address,
        fingerprint: cred.fingerprint,
        code: c.code,
        ca: cred.ca,
      ).toUri();
      setState(() {
        _code = c;
        _uri = uri;
        _left = c.expiresIn;
        _busy = false;
      });
      _timer = Timer.periodic(const Duration(seconds: 1), (_) {
        if (!mounted) return;
        setState(
          () => _left = _left > const Duration(seconds: 1)
              ? _left - const Duration(seconds: 1)
              : Duration.zero,
        );
        if (_left == Duration.zero) _timer?.cancel();
      });
    } on ApiException catch (e) {
      if (mounted) {
        setState(() {
          _refused = e;
          _code = null;
          _uri = null;
          _busy = false;
        });
      }
    } on Object {
      if (mounted) {
        setState(() {
          _failed = true;
          _code = null;
          _uri = null;
          _busy = false;
        });
      }
    }
  }

  String _mmss(Duration d) => '${d.inMinutes}:${(d.inSeconds % 60).toString().padLeft(2, '0')}';

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    final expired = _code != null && _left == Duration.zero;
    final Widget body;
    if (_busy) {
      body = const Center(
        child: Padding(padding: EdgeInsets.all(24), child: CircularProgressIndicator()),
      );
    } else if (_refused?.code == 'device_limit_reached') {
      body = Text(l.pairQrFull, key: const Key('pair-full'));
    } else if (_refused != null || _failed || _uri == null) {
      body = Text(
        l.pairQrError,
        key: const Key('pair-error'),
        style: TextStyle(color: cs.error),
      );
    } else {
      body = Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          // A light square also in the dark theme: a reader needs the contrast.
          Container(
            color: Colors.white,
            padding: const EdgeInsets.all(8),
            child: Opacity(
              opacity: expired ? 0.15 : 1,
              child: QrImageView(
                key: const Key('pair-qr'),
                data: _uri!,
                size: 240,
                backgroundColor: Colors.white,
              ),
            ),
          ),
          const SizedBox(height: 12),
          Text(l.pairQrScan, textAlign: TextAlign.center, style: text.bodyMedium),
          // A computer without a camera: the address of the code can be pasted into the connect screen of the other device.
          TextButton.icon(
            key: const Key('pair-copy'),
            icon: const Icon(Icons.copy, size: 16),
            label: Text(l.pairQrCopy),
            onPressed: expired
                ? null
                : () async {
                    final messenger = ScaffoldMessenger.maybeOf(context);
                    await Clipboard.setData(ClipboardData(text: _uri!));
                    messenger?.showSnackBar(SnackBar(content: Text(l.pairQrCopied)));
                  },
          ),
          const SizedBox(height: 4),
          Text(
            expired ? l.pairQrExpired : l.pairQrExpires(_mmss(_left)),
            key: const Key('pair-left'),
            style: text.bodySmall?.copyWith(color: expired ? cs.error : cs.onSurfaceVariant),
          ),
        ],
      );
    }
    return AlertDialog(
      title: Text(l.settingsConnectDevice),
      content: SizedBox(width: 300, child: SingleChildScrollView(child: body)),
      actions: [
        if (!_busy && _refused?.code != 'device_limit_reached')
          TextButton(key: const Key('pair-new'), onPressed: _make, child: Text(l.pairQrNew)),
        FilledButton(
          key: const Key('pair-close'),
          onPressed: () => Navigator.pop(context),
          child: Text(l.pairQrClose),
        ),
      ],
    );
  }
}
