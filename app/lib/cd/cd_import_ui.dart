// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';

import 'package:flutter/material.dart';

import 'package:intl/intl.dart';

import '../format.dart';
import '../l10n/app_localizations.dart';
import '../server/api_client.dart';
import 'cd_errors.dart';
import 'cd_import_controller.dart';
import 'cd_kit.dart';
import 'cd_models.dart';
import 'cd_progress_bar.dart' show cdEndText, cdWaitText;

/// The two CD symbols of the library, left of the refresh button: import and eject. What they
/// show comes from the server (its drive and its jobs), on every platform.
///
/// * Import: only with a shut tray and an audio CD. While an import runs it becomes a progress mark
///   (tap: the dialog with the progress).
/// * Eject: only with a shut tray and some disc; disabled while an import runs.
/// * An open tray, no drive, no server: nothing is shown and nothing is said. Nothing here closes a tray.
class CdActions extends StatefulWidget {
  const CdActions({super.key, required this.kit});

  final CdKit kit;

  @override
  State<CdActions> createState() => _CdActionsState();
}

class _CdActionsState extends State<CdActions> {
  bool _ejecting = false;

  Future<void> _eject(CdDriveInfo drive) async {
    final l = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.maybeOf(context);
    setState(() => _ejecting = true);
    String text;
    try {
      text = await widget.kit.server.eject(device: drive.device) ? l.cdEjected : l.cdNothingToEject;
    } on Object catch (e) {
      text = cdErrorText(l, e is ApiException ? e.code : 'network');
    }
    if (mounted) setState(() => _ejecting = false);
    messenger?.showSnackBar(SnackBar(content: Text(text)));
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return ListenableBuilder(
      listenable: Listenable.merge([widget.kit.drives, widget.kit.tracker]),
      builder: (context, _) {
        final state = widget.kit.drives.value;
        final running = widget.kit.tracker.running;
        final importing = state.ripping || running.isNotEmpty;
        final importable = state.importable;
        final ejectable = state.ejectable;
        final fraction = running.isEmpty ? null : running.first.fraction;
        final percent = fraction == null ? '' : ' ${(fraction * 100).round()} %';
        return Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (importing)
              IconButton(
                key: const Key('cd-progress'),
                tooltip: '${l.cdImportingTooltip}$percent',
                onPressed: () => showCdImportDialog(
                  context,
                  widget.kit,
                  state.drives.firstWhere(
                    (d) => d.ripping,
                    orElse: () =>
                        state.drives.isEmpty ? const CdDriveInfo(device: '') : state.drives.first,
                  ),
                  resumeJobId: running.isEmpty ? null : running.first.id,
                ),
                icon: SizedBox.square(
                  dimension: 20,
                  child: CircularProgressIndicator(strokeWidth: 2, value: fraction),
                ),
              )
            else if (importable != null)
              IconButton(
                key: const Key('cd-import-button'),
                tooltip: l.cdImportButton,
                onPressed: () => showCdImportDialog(context, widget.kit, importable),
                icon: const Icon(Icons.album),
              ),
            if (ejectable != null)
              IconButton(
                key: const Key('cd-eject-button'),
                tooltip: l.cdEjectTooltip,
                onPressed: importing || _ejecting ? null : () => _eject(ejectable),
                icon: const Icon(Icons.eject),
              ),
          ],
        );
      },
    );
  }
}

Future<void> showCdImportDialog(
  BuildContext context,
  CdKit kit,
  CdDriveInfo drive, {
  String? resumeJobId,
}) async {
  final c = kit.controllerFor(drive, resumeJobId: resumeJobId);
  unawaited(c.identify());
  try {
    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (_) => CdImportDialog(controller: c),
    );
  } finally {
    c.dispose();
    unawaited(kit.drives.reload());
  }
}

class CdImportDialog extends StatefulWidget {
  const CdImportDialog({super.key, required this.controller});

  final CdImportController controller;

  @override
  State<CdImportDialog> createState() => _CdImportDialogState();
}

class _CdImportDialogState extends State<CdImportDialog> {
  final _author = TextEditingController();
  final _title = TextEditingController();
  final _disc = TextEditingController();
  final _total = TextEditingController();
  final _search = TextEditingController();

  CdImportController get c => widget.controller;

  @override
  void initState() {
    super.initState();
    c.addListener(_sync);
    // The lookup may have finished before the dialog was built.
    _fill();
  }

  int _filledIndex = -1;

  /// Copies the controller's guess into the fields when a new guess arrives (not on each keystroke).
  void _fill() {
    if (c.phase != CdPhase.confirm) return;
    final key = c.candidateIndex;
    if (key == _filledIndex) return;
    _filledIndex = key;
    _author.text = c.author;
    _title.text = c.title;
    _disc.text = '${c.disc}';
    _total.text = c.discsTotal?.toString() ?? '';
  }

  void _sync() => setState(_fill);

  @override
  void dispose() {
    c.removeListener(_sync);
    for (final t in [_author, _title, _disc, _total, _search]) {
      t.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final ended =
        c.phase == CdPhase.done ||
        c.phase == CdPhase.failed ||
        c.phase == CdPhase.cancelled ||
        c.phase == CdPhase.ended;
    return AlertDialog(
      title: Text(l.cdImportButton),
      content: SizedBox(width: 440, child: SingleChildScrollView(child: _body(l))),
      actions: [
        if (ended)
          FilledButton(
            key: const Key('cd-close'),
            onPressed: () async {
              // The result is closed on the server too, so the list of tracks does not stay there.
              final nav = Navigator.of(context);
              await c.dismiss();
              nav.pop();
            },
            child: Text(l.cdClose),
          )
        else if (c.phase == CdPhase.running) ...[
          // The import runs on the server: stopping it is one button, leaving the dialog (it goes on without it, and the strip
          // under the app bar opens it again) is the other.
          TextButton(key: const Key('cd-cancel'), onPressed: c.cancel, child: Text(l.cdStopImport)),
          FilledButton(
            key: const Key('cd-hide'),
            onPressed: () => Navigator.pop(context),
            child: Text(l.cdClose),
          ),
        ] else ...[
          if (c.phase != CdPhase.starting)
            TextButton(
              key: const Key('cd-cancel'),
              onPressed: () async {
                final running = c.phase == CdPhase.running;
                await c.cancel();
                // Stopping a running rip is the server's work and arrives as the end of the job; the form just closes.
                if (!running && context.mounted) Navigator.pop(context);
              },
              child: Text(l.cancel),
            ),
          if (c.phase == CdPhase.confirm)
            FilledButton(
              key: const Key('cd-save'),
              onPressed: c.canSave ? c.save : null,
              child: Text(c.resumeOffer != null && !c.rereadFromStart ? l.cdContinue : l.cdSave),
            ),
          if (c.phase == CdPhase.present) ...[
            TextButton(
              key: const Key('cd-force'),
              onPressed: _askForce,
              child: Text(l.cdImportAnyway),
            ),
            if (!(c.present?.complete ?? false))
              FilledButton(key: const Key('cd-heal'), onPressed: c.heal, child: Text(l.cdHeal)),
          ],
        ],
      ],
    );
  }

  Widget _body(AppLocalizations l) {
    switch (c.phase) {
      case CdPhase.identifying:
        return _progress(l.cdIdentifying, null);
      case CdPhase.present:
        return _presentView(l);
      case CdPhase.confirm:
        return _form(l);
      case CdPhase.starting:
        return _progress(l.cdStarting, null);
      case CdPhase.ended:
        return Text(cdEndText(l, c.job!), key: const Key('cd-ended'));
      case CdPhase.running:
        final j = c.job;
        if (j != null && j.waiting) {
          return Text(cdWaitText(l, j), key: const Key('cd-waiting'));
        }
        final text = switch (j?.phase) {
          'encoding' => l.cdEncoding,
          'uploading' || 'placing' || 'importing' => l.cdAddingToLibrary,
          _ => l.cdReading(j?.currentTrack ?? 1, j?.tracksTotal ?? c.trackCount),
        };
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _progress(text, j?.fraction),
            if (c.resumeAnswer == 'mismatch')
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(
                  cdErrorText(l, 'resume_mismatch'),
                  key: const Key('cd-resume-note'),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
            if (c.resumeAnswer == 'resumed')
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(
                  l.cdResumed,
                  key: const Key('cd-resume-note'),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
            _trackStates(l),
            if (c.buffer != null) _bufferLine(l, c.buffer!),
          ],
        );
      case CdPhase.done:
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.cdDone),
            if (c.doneNote != null && c.doneNote != 'locked')
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(cdErrorText(l, c.doneNote!), key: const Key('cd-done-note')),
              ),
            if (c.doneNote == 'locked')
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(cdErrorText(l, 'locked'), key: const Key('cd-done-note')),
              ),
            if (c.doneNote != null && c.doneDetail != null && c.doneDetail!.isNotEmpty)
              ExpansionTile(title: Text(l.cdErrDetails), children: [SelectableText(c.doneDetail!)]),
          ],
        );
      case CdPhase.cancelled:
        return Text(l.cdCancelled);
      case CdPhase.failed:
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              c.failureCode == 'disc_exists' && c.failureTaken.isNotEmpty
                  ? l.cdErrTaken(c.failureTaken.join(', '))
                  : cdErrorText(l, c.failureCode ?? 'no_disc'),
              key: const Key('cd-error'),
            ),
            if (c.failureDetail != null && c.failureDetail!.isNotEmpty)
              ExpansionTile(
                title: Text(l.cdErrDetails),
                children: [SelectableText(c.failureDetail!)],
              ),
          ],
        );
    }
  }

  Future<void> _askForce() async {
    final l = AppLocalizations.of(context);
    final yes = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(l.cdForceTitle),
        content: Text(l.cdForceBody),
        actions: [
          TextButton(
            key: const Key('cd-force-no'),
            onPressed: () => Navigator.pop(ctx, false),
            child: Text(l.cancel),
          ),
          FilledButton(
            key: const Key('cd-force-yes'),
            onPressed: () => Navigator.pop(ctx, true),
            child: Text(l.cdForceConfirm),
          ),
        ],
      ),
    );
    if (yes == true) await c.importAnyway();
  }

  /// "This CD is in the library already": what is missing and what is damaged, by name.
  Widget _presentView(AppLocalizations l) {
    final p = c.present!;
    final t = Theme.of(context).textTheme;
    String name(int n) {
      final number = n.toString().padLeft(2, '0');
      final title = c.titleOf(n);
      return title.isEmpty ? l.cdTrackDefault(number) : '$number  $title';
    }

    String why(CdDamaged d) {
      final have = d.haveSeconds == null ? '' : formatDuration(Duration(seconds: d.haveSeconds!));
      final want = d.wantSeconds == null ? '' : formatDuration(Duration(seconds: d.wantSeconds!));
      return switch (d.reason) {
        'empty' => l.cdDamagedEmpty,
        'unreadable' => l.cdDamagedUnreadable,
        'length' => l.cdDamagedLength(have, want),
        _ => l.cdDamagedOther,
      };
    }

    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(l.cdPresentTitle, style: t.titleSmall, key: const Key('cd-present')),
        const SizedBox(height: 4),
        Text(
          p.author.isEmpty
              ? l.cdPresentBookDisc(p.book, p.disc)
              : l.cdPresentBookAuthorDisc(p.book, p.author, p.disc),
        ),
        const SizedBox(height: 8),
        if (p.complete)
          Text(
            l.cdPresentComplete(p.tracksInLibrary, p.tracksOnDisc),
            key: const Key('cd-present-complete'),
          )
        else ...[
          Text(l.cdPresentCount(p.tracksInLibrary, p.tracksOnDisc)),
          if (p.missing.isNotEmpty) ...[
            const SizedBox(height: 8),
            Text(l.cdMissing, style: t.labelLarge),
            for (final n in p.missing) Text(name(n), key: Key('cd-missing-$n')),
          ],
          if (p.damaged.isNotEmpty) ...[
            const SizedBox(height: 8),
            Text(l.cdDamaged, style: t.labelLarge),
            for (final d in p.damaged)
              Text('${name(d.number)}: ${why(d)}', key: Key('cd-damaged-${d.number}')),
          ],
          const SizedBox(height: 8),
          Text(l.cdHealHint, style: t.bodySmall),
        ],
      ],
    );
  }

  /// The colour of a state, the same meaning as on the web page: quiet while waiting, the accent for the track that is read or checked, amber
  /// for "read, waits for the check", blue while converting, green when done, red for an error.
  Color _stateColor(BuildContext context, String state) {
    final cs = Theme.of(context).colorScheme;
    // Dark enough on the light page and light enough on the dark one (the same amber, blue and green as the web page: yellow on ochre was hard to read).
    final dark = Theme.of(context).brightness == Brightness.dark;
    return switch (state) {
      'reading' || 'verifying' => cs.primary,
      'read' || 'unverified' => dark ? const Color(0xFFE0B25A) : const Color(0xFF7A4D00),
      'rereading' => dark ? Colors.deepOrange.shade300 : Colors.deepOrange.shade900,
      'encoding' => dark ? const Color(0xFF8DB8E3) : const Color(0xFF1F4E79),
      'done' => dark ? const Color(0xFF8FC4A8) : const Color(0xFF2C4A3E),
      'failed' => cs.error,
      _ => cs.onSurfaceVariant,
    };
  }

  IconData _stateIcon(String state) => switch (state) {
    'reading' || 'verifying' || 'encoding' || 'rereading' => Icons.autorenew,
    'read' => Icons.hourglass_bottom,
    'done' => Icons.check_circle_outline,
    'unverified' => Icons.warning_amber_outlined,
    'failed' => Icons.error_outline,
    _ => Icons.schedule,
  };

  String _stateText(AppLocalizations l, CdTrackState t) => switch (t.state) {
    'reading' =>
      t.fraction == null ? l.cdStateReading : l.cdStateReadingPercent((t.fraction! * 100).round()),
    'read' => l.cdStateRead,
    'rereading' =>
      t.fraction == null
          ? l.cdStateRereading
          : l.cdStateRereadingPercent((t.fraction! * 100).round()),
    'verifying' => l.cdStateVerifying,
    'encoding' => l.cdStateEncoding,
    'done' => l.cdStateDone,
    'unverified' => l.cdStateUnverified,
    'failed' => t.error.isEmpty ? l.cdStateFailed : cdErrorText(l, t.error),
    _ => l.cdStateWaiting,
  };

  /// The chosen tracks with their state: waiting, being read with the share, checked, converted, done, failed.
  Widget _trackStates(AppLocalizations l) {
    final list = c.trackStates;
    if (list.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 200),
        child: ListView.builder(
          key: const Key('cd-track-states'),
          shrinkWrap: true,
          itemCount: list.length,
          itemBuilder: (context, i) {
            final t = list[i];
            final number = t.number.toString().padLeft(2, '0');
            return ListTile(
              key: Key('cd-state-${t.number}'),
              dense: true,
              visualDensity: VisualDensity.compact,
              contentPadding: EdgeInsets.zero,
              leading: Icon(_stateIcon(t.state), size: 18, color: _stateColor(context, t.state)),
              title: Text(
                t.title.isEmpty ? l.cdTrackDefault(number) : '$number  ${t.title}',
                overflow: TextOverflow.ellipsis,
              ),
              trailing: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 170),
                child: Text(
                  _stateText(l, t),
                  key: Key('cd-state-text-${t.number}'),
                  overflow: TextOverflow.ellipsis,
                  textAlign: TextAlign.end,
                  style: TextStyle(color: _stateColor(context, t.state)),
                ),
              ),
            );
          },
        ),
      ),
    );
  }

  /// "Buffer 41 s of 230 s · reads at 4.1x": how much audio waits in the ring of the reader, when something does (an empty one is the normal
  /// case, the encoder is faster than the drive), and the speed the drive reads at as the server measured it. Small and quiet.
  Widget _bufferLine(AppLocalizations l, CdBuffer b) {
    final parts = [
      if (b.seconds >= 1) l.cdBuffer(b.seconds, b.max),
      if (b.speedX != null) l.cdSpeed(b.speedX!.toStringAsFixed(1)),
    ];
    if (parts.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Text(
        parts.join(' · '),
        key: const Key('cd-buffer'),
        style: Theme.of(context).textTheme.bodySmall
            ?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant),
      ),
    );
  }

  /// The note about an interrupted rip of this disc, with the one box that turns "continue" into "read again".
  Widget _resumeBlock(AppLocalizations l) {
    final r = c.drive.resume;
    if (r == null) return const SizedBox.shrink();
    final locale = Localizations.localeOf(context).toString();
    final when = r.interruptedAt == null
        ? ''
        : DateFormat.yMd(locale).add_Hm().format(r.interruptedAt!);
    if (c.resumeLost) {
      return Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Text(l.cdResumeLost, key: const Key('cd-resume-lost')),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (c.resumeOffer != null)
          Text(
            l.cdResumeNote(r.tracksDone.length, r.tracksTotal, when),
            key: const Key('cd-resume-offer'),
          ),
        if (c.resumeOffer != null)
          CheckboxListTile(
            key: const Key('cd-reread'),
            contentPadding: EdgeInsets.zero,
            controlAffinity: ListTileControlAffinity.leading,
            value: c.rereadFromStart,
            title: Text(l.cdRereadFromStart),
            onChanged: (v) => c.setReread(v ?? false),
          ),
      ],
    );
  }

  Widget _progress(String text, double? value) => Column(
    mainAxisSize: MainAxisSize.min,
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Text(text),
      const SizedBox(height: 12),
      LinearProgressIndicator(value: value),
    ],
  );

  /// All tracks with a checkbox, number, title and length; every track is chosen at first.
  Widget _trackList(AppLocalizations l) {
    final t = Theme.of(context).textTheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                l.cdTracksSummary(
                  c.selectedCount,
                  c.trackCount,
                  formatDuration(c.selectedDuration),
                ),
                key: const Key('cd-summary'),
                style: t.bodySmall,
              ),
            ),
            IconButton(
              key: const Key('cd-select-all'),
              tooltip: l.cdSelectAll,
              visualDensity: VisualDensity.compact,
              icon: const Icon(Icons.select_all),
              onPressed: c.selectAll,
            ),
            IconButton(
              key: const Key('cd-select-none'),
              tooltip: l.cdSelectNone,
              visualDensity: VisualDensity.compact,
              icon: const Icon(Icons.deselect),
              onPressed: c.selectNone,
            ),
            IconButton(
              key: const Key('cd-select-invert'),
              tooltip: l.cdSelectInvert,
              visualDensity: VisualDensity.compact,
              icon: const Icon(Icons.flip),
              onPressed: c.invertSelection,
            ),
          ],
        ),
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 240),
          child: ListView.builder(
            key: const Key('cd-tracks'),
            shrinkWrap: true,
            itemCount: c.trackCount,
            itemBuilder: (context, i) {
              final tr = c.tracks[i];
              final number = tr.number.toString().padLeft(2, '0');
              final title = i < c.trackTitles.length && c.trackTitles[i].isNotEmpty
                  ? c.trackTitles[i]
                  : l.cdTrackDefault(number);
              return CheckboxListTile(
                key: Key('cd-track-${tr.number}'),
                dense: true,
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                value: c.selected.contains(tr.number),
                onChanged: (_) => c.toggle(tr.number),
                title: Text(
                  c.trackTitles.length > i && c.trackTitles[i].isNotEmpty
                      ? '$number  $title'
                      : title,
                  overflow: TextOverflow.ellipsis,
                ),
                secondary: tr.seconds > 0
                    ? Text(formatDuration(Duration(seconds: tr.seconds.round())))
                    : null,
              );
            },
          ),
        ),
      ],
    );
  }

  Widget _form(AppLocalizations l) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          c.lookupFound
              ? l.cdFound
              : (c.lookupError != null ? cdErrorText(l, c.lookupError!) : l.cdNotFound),
          key: const Key('cd-lookup-note'),
        ),
        const SizedBox(height: 12),
        _resumeBlock(l),
        // Not the right release: MusicBrainz can be searched by hand; what it finds is added to the matches.
        Row(
          children: [
            Expanded(
              child: TextField(
                key: const Key('cd-search'),
                controller: _search,
                decoration: InputDecoration(labelText: l.cdSearchLabel),
                textInputAction: TextInputAction.search,
                onSubmitted: c.searching ? null : c.search,
              ),
            ),
            const SizedBox(width: 8),
            OutlinedButton(
              key: const Key('cd-search-go'),
              onPressed: c.searching ? null : () => c.search(_search.text),
              child: Text(l.cdSearchGo),
            ),
          ],
        ),
        if (c.searchError != null)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Text(
              c.searchError == 'not_found' ? l.cdSearchNothing : cdErrorText(l, c.searchError!),
              key: const Key('cd-search-error'),
            ),
          ),
        const SizedBox(height: 8),
        if (c.candidates.length > 1)
          DropdownButtonFormField<int>(
            key: const Key('cd-candidate'),
            // Expanded: a long title is cut with "..." inside the field (without it the row overflowed, and a debug build draws the striped warning).
            isExpanded: true,
            initialValue: c.candidateIndex,
            decoration: InputDecoration(labelText: l.cdCandidate),
            items: [
              for (var i = 0; i < c.candidates.length; i++)
                DropdownMenuItem(
                  value: i,
                  child: Text(
                    '${c.candidates[i].author} – ${c.candidates[i].title}',
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
            ],
            onChanged: (i) => i == null ? null : c.selectCandidate(i),
          ),
        TextField(
          key: const Key('cd-author'),
          controller: _author,
          decoration: InputDecoration(labelText: l.cdAuthor),
          onChanged: (v) => c.update(author: v),
        ),
        TextField(
          key: const Key('cd-title'),
          controller: _title,
          decoration: InputDecoration(labelText: l.cdTitle),
          onChanged: (v) => c.update(title: v),
        ),
        Row(
          children: [
            Expanded(
              child: TextField(
                key: const Key('cd-disc'),
                controller: _disc,
                keyboardType: TextInputType.number,
                decoration: InputDecoration(labelText: l.cdDiscNumber),
                onChanged: (v) => c.update(disc: int.tryParse(v) ?? 0),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: TextField(
                key: const Key('cd-total'),
                controller: _total,
                keyboardType: TextInputType.number,
                decoration: InputDecoration(labelText: l.cdDiscsTotal),
                onChanged: (v) => c.update(discsTotal: int.tryParse(v) ?? 0),
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        DropdownButtonFormField<String>(
          key: const Key('cd-kind'),
          initialValue: c.kind,
          decoration: InputDecoration(labelText: l.cdKind),
          items: [
            DropdownMenuItem(value: 'audiobook', child: Text(l.cdKindAudiobook)),
            DropdownMenuItem(value: 'music', child: Text(l.cdKindMusic)),
          ],
          onChanged: (v) => c.update(kind: v),
        ),
        CheckboxListTile(
          key: const Key('cd-eject'),
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          value: c.eject,
          title: Text(l.cdEjectAfter),
          onChanged: (v) => c.update(eject: v ?? false),
        ),
        const SizedBox(height: 8),
        _trackList(l),
      ],
    );
  }
}
