// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';
import 'package:intl/intl.dart';

import '../format.dart';
import '../l10n/app_localizations.dart';
import 'photo_backup_api.dart';
import 'photo_backup_controller.dart';
import 'photo_device.dart';
import 'photo_sync.dart';
import 'photo_texts.dart';

/// The line of the settings that opens the photo backup; nothing when the server has the backup off or the profile is a guest.
class PhotoBackupCard extends StatefulWidget {
  const PhotoBackupCard({super.key, required this.controller});

  final PhotoBackupController controller;

  @override
  State<PhotoBackupCard> createState() => _PhotoBackupCardState();
}

class _PhotoBackupCardState extends State<PhotoBackupCard> {
  @override
  void initState() {
    super.initState();
    if (!widget.controller.loaded) widget.controller.load();
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
        listenable: widget.controller,
        builder: (context, _) {
          final c = widget.controller;
          if (!c.available) return const SizedBox.shrink();
          final l = AppLocalizations.of(context);
          final cs = Theme.of(context).colorScheme;
          return Padding(
            padding: const EdgeInsets.only(bottom: 24),
            child: Card(
              margin: EdgeInsets.zero,
              clipBehavior: Clip.antiAlias,
              child: ListTile(
                key: const ValueKey('open-photo-backup'),
                leading: Icon(Icons.photo_library_outlined, color: cs.primary),
                title: Text(l.photoBackupTitle),
                subtitle: Text(photoStatusText(l, c)),
                trailing: const Icon(Icons.chevron_right),
                onTap: () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => PhotoBackupScreen(controller: c))),
              ),
            ),
          );
        },
      );
}

/// The state in one sentence: runs, or why not.
String photoStatusText(AppLocalizations l, PhotoBackupController c) {
  final s = c.settings;
  if (s == null) return c.loadError != null ? l.photoLoadFailed : '';
  if (s.effective['enabled'] != 'on') return l.photoStatusInactive;
  if (!s.folder) return l.photoStatusNoFolder;
  if (!c.device.enabled) return c.accessRefused ? l.photoStatusNoAccess : l.photoStatusDeviceOff;
  if (c.access == false) return l.photoStatusNoAccess;
  return l.photoStatusActive;
}

/// The page of the photo backup: this device, the choices of the profile (each a list with the text of the chosen option below it), and
/// what the server keeps.
class PhotoBackupScreen extends StatelessWidget {
  const PhotoBackupScreen({super.key, required this.controller});

  final PhotoBackupController controller;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(title: Text(l.photoBackupTitle)),
      body: ListenableBuilder(
        listenable: controller,
        builder: (context, _) {
          final s = controller.settings;
          return Align(
            alignment: Alignment.topCenter,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 560),
              child: ListView(
                padding: const EdgeInsets.all(16),
                children: [
                  _StatusLine(controller: controller),
                  const SizedBox(height: 16),
                  if (s != null) ...[
                    _DeviceCard(controller: controller, settings: s),
                    const SizedBox(height: 16),
                    _ProfileCard(controller: controller, settings: s),
                    const SizedBox(height: 16),
                    _ServerCard(controller: controller),
                  ],
                ],
              ),
            ),
          );
        },
      ),
    );
  }
}

class _StatusLine extends StatelessWidget {
  const _StatusLine({required this.controller});

  final PhotoBackupController controller;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    final ok = photoStatusText(l, controller) == l.photoStatusActive;
    return Row(
      children: [
        Icon(ok ? Icons.check_circle_outline : Icons.info_outline, color: ok ? cs.primary : cs.onSurfaceVariant),
        const SizedBox(width: 12),
        Expanded(child: Text(photoStatusText(l, controller), key: const ValueKey('photo-status'))),
      ],
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.children});

  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) => Card(
        margin: EdgeInsets.zero,
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(title, style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 12),
              ...children,
            ],
          ),
        ),
      );
}

/// A choice as a list to pick from, with the explanation of the chosen option below it (the others are not shown). An option that cannot
/// be chosen is greyed out, and below the list it says why.
class PhotoChoiceField extends StatelessWidget {
  const PhotoChoiceField({
    super.key,
    required this.id,
    required this.label,
    required this.options,
    required this.value,
    required this.optionLabel,
    required this.explain,
    this.locked = const {},
    this.why,
    this.error,
    this.onChanged,
  });

  final String id;
  final String label;
  final List<String> options;
  final String value;
  final String Function(String option) optionLabel;
  final String Function(String option) explain;

  /// The options that cannot be chosen, with the reason code.
  final Map<String, String> locked;
  final String Function(String why)? why;

  /// The server refused the last change of this choice.
  final String? error;
  final ValueChanged<String>? onChanged;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    final reasons = {for (final w in locked.values) why?.call(w) ?? w};
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          DropdownButtonFormField<String>(
            // The value is part of the key: a new value from the server builds the field anew with it.
            key: ValueKey('photo-choice-$id-$value'),
            initialValue: options.contains(value) ? value : null,
            isExpanded: true,
            decoration: InputDecoration(labelText: label, errorText: error, border: const OutlineInputBorder()),
            items: [
              for (final o in options)
                DropdownMenuItem(
                  value: o,
                  enabled: !locked.containsKey(o) || o == value,
                  child: Text(optionLabel(o), key: ValueKey('photo-option-$id-$o')),
                ),
            ],
            onChanged: onChanged == null ? null : (v) => v != null && v != value ? onChanged!(v) : null,
          ),
          const SizedBox(height: 6),
          Text(explain(value), key: ValueKey('photo-explain-$id'), style: text.bodyMedium),
          for (final r in reasons) ...[
            const SizedBox(height: 4),
            Text(r, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
          ],
        ],
      ),
    );
  }
}

class _DeviceCard extends StatelessWidget {
  const _DeviceCard({required this.controller, required this.settings});

  final PhotoBackupController controller;
  final PhotoSettings settings;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final locale = Localizations.localeOf(context).toLanguageTag();
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    final c = controller;
    final d = c.device;
    final st = c.state;
    final since = d.since;
    final canRun = settings.active && d.enabled && c.access != false;
    return _Section(
      title: l.photoGroupDevice,
      children: [
        PhotoChoiceField(
          id: 'device',
          label: l.photoSetDevice,
          options: const ['off', 'on'],
          value: d.enabled ? 'on' : 'off',
          optionLabel: (o) => o == 'on' ? l.photoOptDeviceOn : l.photoOptDeviceOff,
          explain: (o) => o == 'on' ? l.photoExplainDeviceOn : l.photoExplainDeviceOff,
          onChanged: (o) => c.setDeviceEnabled(o == 'on'),
        ),
        PhotoChoiceField(
          id: 'sources',
          label: l.photoSetSources,
          options: kPhotoSources,
          value: d.sources,
          optionLabel: (o) => o == 'all' ? l.photoOptSourcesAll : l.photoOptSourcesCamera,
          explain: (o) => o == 'all' ? l.photoExplainSourcesAll : l.photoExplainSourcesCamera,
          onChanged: c.setSources,
        ),
        if (since != null) ...[
          Text(d.olderToo ? l.photoSinceAll : l.photoSince(DateFormat.yMd(locale).format(since)), key: const ValueKey('photo-since')),
          if (!d.olderToo)
            Align(
              alignment: Alignment.centerLeft,
              child: TextButton(
                key: const ValueKey('photo-older'),
                onPressed: () => _askOlder(context, l, DateFormat.yMd(locale).format(since)),
                child: Text(l.photoOlderButton),
              ),
            ),
          const SizedBox(height: 8),
        ],
        if (c.running) ...[
          LinearProgressIndicator(value: c.total == 0 ? null : c.done / c.total),
          const SizedBox(height: 6),
          Text(l.photoProgress(c.done, c.total)),
          const SizedBox(height: 8),
        ],
        Row(
          children: [
            if (c.running)
              OutlinedButton.icon(
                key: const ValueKey('photo-pause'),
                icon: const Icon(Icons.pause),
                label: Text(l.photoPause),
                onPressed: c.pause,
              )
            else
              FilledButton.icon(
                key: const ValueKey('photo-now'),
                icon: const Icon(Icons.backup_outlined),
                label: Text(l.photoNow),
                onPressed: canRun ? c.runNow : null,
              ),
          ],
        ),
        const SizedBox(height: 8),
        if (c.last case final r? when !c.running) Text(_runText(l, r), key: const ValueKey('photo-last-run')),
        if (st.lastRunAt case final at?)
          Text(l.photoLastRun(DateFormat.yMd(locale).add_Hm().format(at)), style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
        if (st.waiting case final w?) Text(l.photoWaiting(w), style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
        if (st.errors.isNotEmpty) ...[
          const SizedBox(height: 12),
          Text(l.photoErrorsTitle, style: text.titleSmall),
          for (final e in st.errors.take(5))
            Text(
              '${DateFormat.Md(locale).add_Hm().format(e.at)} · ${photoErrorText(l, e.code)}${e.name == null ? '' : ' · ${e.name}'}',
              style: text.bodySmall,
            ),
        ],
      ],
    );
  }

  String _runText(AppLocalizations l, PhotoRunResult r) => switch (r.stop) {
        null => l.photoRunDone(r.sent, r.known),
        PhotoRunStop.busy => l.photoStopBusy,
        PhotoRunStop.network => l.photoStopNetwork,
        PhotoRunStop.stopped => '${l.photoRunDone(r.sent, r.known)} ${l.photoStopStopped}',
        PhotoRunStop.refused => l.photoStopRefused,
        PhotoRunStop.noAccess => l.photoStatusNoAccess,
        PhotoRunStop.serverOff => l.photoWhyServerOff,
        PhotoRunStop.inactive => l.photoStatusInactive,
        PhotoRunStop.deviceOff => l.photoStatusDeviceOff,
      };

  Future<void> _askOlder(BuildContext context, AppLocalizations l, String date) async {
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.photoOlderTitle),
        content: Text(l.photoOlderBody(date)),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(MaterialLocalizations.of(context).cancelButtonLabel)),
          FilledButton(key: const ValueKey('photo-older-confirm'), onPressed: () => Navigator.pop(context, true), child: Text(l.photoOlderConfirm)),
        ],
      ),
    );
    if (yes == true) controller.includeOlder();
  }
}

class _ProfileCard extends StatelessWidget {
  const _ProfileCard({required this.controller, required this.settings});

  final PhotoBackupController controller;
  final PhotoSettings settings;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    return _Section(
      title: l.photoGroupProfile,
      children: [
        Text(l.photoProfileHint, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
        const SizedBox(height: 12),
        for (final e in kPhotoChoices.entries)
          if (settings.settings[e.key] case final value?)
            PhotoChoiceField(
              id: e.key,
              label: photoChoiceLabel(l, e.key),
              options: e.value,
              value: value,
              optionLabel: (o) => photoOptionLabel(l, e.key, o),
              explain: (o) => photoOptionExplain(l, e.key, o),
              locked: settings.locked[e.key] ?? const {},
              why: (w) => photoWhy(l, w),
              error: switch (controller.refused[e.key]) { final w? => photoWhy(l, w), null => null },
              onChanged: (o) => controller.choose(e.key, o),
            ),
      ],
    );
  }
}

class _ServerCard extends StatelessWidget {
  const _ServerCard({required this.controller});

  final PhotoBackupController controller;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final locale = Localizations.localeOf(context).toLanguageTag();
    final s = controller.server;
    if (s == null) return const SizedBox.shrink();
    return _Section(
      title: l.photoGroupServer,
      children: [
        Text(l.photoServerCount(s.count, formatBytes(locale, s.bytes)), key: const ValueKey('photo-server-count')),
        if (s.quotaBytes case final q?) Text(l.photoServerQuota(formatBytes(locale, q))),
        if (s.lastAt case final at?) Text(l.photoServerLast(DateFormat.yMd(locale).add_Hm().format(at))),
      ],
    );
  }
}
