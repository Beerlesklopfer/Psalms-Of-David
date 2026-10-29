// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import 'cd_errors.dart';
import 'cd_events.dart';
import 'cd_import_ui.dart';
import 'cd_kit.dart';
import 'cd_models.dart';

/// How long the strip stays after the end of an import, as the result. Closing it earlier is possible.
const Duration kCdResultLinger = Duration(seconds: 8);

/// The word for the phase of a job as the server reports it (`ripping`, `verifying`, `encoding`,
/// `uploading`/`placing`, `importing`); an unknown or missing phase is a neutral "Working".
String cdPhaseLabel(AppLocalizations l, String? phase) => switch (phase) {
  'ripping' || 'reading' => l.cdPhaseReading,
  'verifying' || 'checking' => l.cdPhaseChecking,
  'encoding' => l.cdPhaseEncoding,
  'uploading' || 'placing' => l.cdPhasePlacing,
  'importing' => l.cdPhaseImporting,
  _ => l.cdPhaseWorking,
};

/// The one line of the strip: "Sturz der Titanen, CD 3: Track 3 of 11 · Reading · 40 %". Parts that
/// the job does not know are left out.
String cdProgressLine(AppLocalizations l, CdJobState j) {
  final title = j.title?.trim() ?? '';
  final head = title.isEmpty
      ? null
      : ((j.disc != null && ((j.discsTotal ?? 1) > 1 || j.disc! > 1))
            ? l.cdBarHead(title, j.disc!)
            : title);
  final n = j.currentTrack;
  final tail = [
    if (n != null) l.cdBarTrack(n, j.tracksTotal!),
    cdPhaseLabel(l, j.phase),
    if (j.overall != null) '${(j.overall! * 100).round()} %',
  ].join(' · ');
  // The line says what it is: "Importing: Sturz der Titanen, CD 4: Track 1 of 13 · Reading · 36 %".
  return l.cdBarImporting(head == null ? tail : '$head: $tail');
}

/// A thin strip under the app bar of the library that shows an import while it runs (also one that
/// another device or the web page started): a bar with the share done and one line of text. Tap:
/// the import dialog. After the end the result stays for [kCdResultLinger] (green, or red with the
/// reason), or until it is closed. Without a job it takes no room.
/// The air around the strip: between the app bar and the bar, and between the strip and the chips under it.
const kCdStripGap = 8.0;

class CdProgressStrip extends StatefulWidget {
  const CdProgressStrip({super.key, required this.kit, this.linger = kCdResultLinger});

  final CdKit kit;
  final Duration linger;

  @override
  State<CdProgressStrip> createState() => _CdProgressStripState();
}

class _CdProgressStripState extends State<CdProgressStrip> {
  /// The job shown now and whether it has ended. Only a job seen running gets a result.
  CdJobState? _job;
  final _seen = <String>{};
  final _dismissed = <String>{};
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    widget.kit.tracker.addListener(_changed);
    _changed();
  }

  void _changed() {
    final value = widget.kit.tracker.value;
    final open = widget.kit.tracker.open;
    CdJobState? next = _job;
    if (open.isNotEmpty) {
      next = open.first;
      _seen.add(next.id);
      _timer?.cancel();
    } else if (_job != null && !_job!.finished) {
      // The job we showed has ended (or vanished from the list): show its end.
      final ended = value[_job!.id];
      next = ended ?? _job;
      if (ended != null && ended.finished) {
        _timer?.cancel();
        final id = ended.id;
        _timer = Timer(widget.linger, () {
          _dismissed.add(id);
          if (mounted) setState(() => _job = null);
        });
      }
    } else if (_job != null && _job!.finished) {
      next = value[_job!.id] ?? _job;
    }
    if (next != null &&
        (_dismissed.contains(next.id) || (next.finished && !_seen.contains(next.id)))) {
      next = null;
    }
    if (next != _job) {
      if (mounted) {
        setState(() => _job = next);
      } else {
        _job = next;
      }
    }
  }

  void _close() {
    final j = _job;
    if (j != null) _dismissed.add(j.id);
    _timer?.cancel();
    // The result is closed on the server too, so the list of tracks does not stay on other screens.
    unawaited(widget.kit.server.dismissRip().catchError((Object _) {}));
    setState(() => _job = null);
  }

  @override
  void dispose() {
    widget.kit.tracker.removeListener(_changed);
    _timer?.cancel();
    super.dispose();
  }

  void _open() {
    final j = _job;
    if (j == null) return;
    final drives = widget.kit.drives.value.drives;
    final drive = drives.firstWhere(
      (d) => d.ripping,
      orElse: () => drives.isEmpty ? const CdDriveInfo(device: '') : drives.first,
    );
    showCdImportDialog(context, widget.kit, drive, resumeJobId: j.id);
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    final j = _job;
    Widget child = const SizedBox(width: double.infinity);
    if (j != null) {
      final ok = j.finished && j.state == 'done';
      final failed = j.finished && j.state == 'failed';
      final String text;
      if (ok) {
        final n = j.tracksTotal;
        text = n == null ? l.cdBarDoneNoCount : l.cdBarDone(n);
      } else if (failed) {
        text = cdErrorText(l, j.error ?? 'import_failed');
      } else {
        text = cdProgressLine(l, j);
      }
      final value = j.finished ? 1.0 : j.overall;
      final color = ok ? Colors.green : (failed ? cs.error : cs.primary);
      child = Semantics(
        container: true,
        label: text,
        value: value == null ? null : '${(value * 100).round()} %',
        child: Material(
          key: const Key('cd-strip'),
          color: cs.surfaceContainerHighest,
          child: InkWell(
            onTap: _open,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                LinearProgressIndicator(
                  key: const Key('cd-strip-bar'),
                  value: value,
                  minHeight: 4,
                  color: color,
                  backgroundColor: cs.surfaceContainerHigh,
                ),
                Padding(
                  // The line of text does not sit on the bar.
                  padding: const EdgeInsets.only(left: 16, top: 6, bottom: 6),
                  child: Row(
                    children: [
                      Expanded(
                        child: Text(
                          text,
                          key: const Key('cd-strip-text'),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: ok ? Colors.green : (failed ? cs.error : null)),
                        ),
                      ),
                      if (j.finished)
                        IconButton(
                          key: const Key('cd-strip-close'),
                          tooltip: l.cdClose,
                          visualDensity: VisualDensity.compact,
                          icon: const Icon(Icons.close, size: 18),
                          onPressed: _close,
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    }
    if (j != null) {
      child = Padding(
        padding: const EdgeInsets.symmetric(vertical: kCdStripGap),
        child: child,
      );
    }
    return AnimatedSize(
      duration: const Duration(milliseconds: 150),
      alignment: Alignment.topCenter,
      child: child,
    );
  }
}
