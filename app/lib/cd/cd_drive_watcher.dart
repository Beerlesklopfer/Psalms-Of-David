// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/widgets.dart';

import 'cd_events.dart';
import 'cd_models.dart';

/// Keeps the picture of the server's drives: reads it once, follows the server's `drive` messages,
/// and reads it again when the app returns to the foreground or the channel reconnects. A server
/// that cannot be reached, or no drive, gives [CdDrivesState.none]: the symbols stay hidden and
/// nothing is reported.
class CdDriveWatcher extends ValueNotifier<CdDrivesState> with WidgetsBindingObserver {
  CdDriveWatcher(this._source) : super(CdDrivesState.none);

  final CdEventsSource _source;
  StreamSubscription<CdDrivesState>? _sub;
  bool _started = false;
  bool _disposed = false;

  void start() {
    if (_started || _disposed) return;
    _started = true;
    WidgetsBinding.instance.addObserver(this);
    _sub = _source.driveChanged.listen((s) {
      if (!_disposed) value = s;
    });
    unawaited(reload());
  }

  /// Reads the state from the server.
  Future<void> reload() async {
    if (_disposed) return;
    try {
      final s = await _source.drives();
      if (!_disposed) value = s;
    } on Object {
      // Keep what we knew; with nothing known, nothing is shown.
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) unawaited(reload());
  }

  @override
  void dispose() {
    _disposed = true;
    unawaited(_sub?.cancel());
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }
}
