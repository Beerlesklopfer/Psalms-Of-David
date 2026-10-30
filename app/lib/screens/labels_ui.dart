// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../labels.dart';
import '../models.dart';
import '../player_controller.dart';

/// The labels of a book on its page: a chip for each, and, for the owner and an administrator, a chip that opens the editor.
class LabelsLine extends StatelessWidget {
  const LabelsLine({super.key, required this.book});

  final Book book;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final controller = LabelsScope.maybeOf(context);
    final editable = book.canLabel && controller != null && controller.available;
    if (book.labels.isEmpty && !editable) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Wrap(
        spacing: 8,
        runSpacing: 4,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          for (final name in book.labels)
            Chip(
              key: ValueKey('label-chip-$name'),
              label: Text(name),
              visualDensity: VisualDensity.compact,
            ),
          if (editable)
            ActionChip(
              key: const ValueKey('labels-edit'),
              avatar: const Icon(Icons.sell_outlined, size: 18),
              label: Text(book.labels.isEmpty ? l.labelsEdit : l.labelsTitle),
              visualDensity: VisualDensity.compact,
              onPressed: () => showLabelEditor(context, book),
            ),
        ],
      ),
    );
  }
}

Future<void> showLabelEditor(BuildContext context, Book book) {
  final controller = LabelsScope.maybeOf(context);
  if (controller == null) return Future.value();
  final library = PlayerScope.maybeOf(context)?.library ?? const <Book>[];
  return showDialog<void>(
    context: context,
    builder: (_) => LabelEditor(
      initial: book.labels,
      library: library,
      onSave: (words) => controller.save(book.id, words),
    ),
  );
}

/// Opens the editor for the labels of one title of a book (its own; the ones of the album come on top).
Future<void> showTrackLabelEditor(BuildContext context, Book book, Track track) {
  final controller = LabelsScope.maybeOf(context);
  final part = track.partId;
  if (controller == null || part == null) return Future.value();
  final library = PlayerScope.maybeOf(context)?.library ?? const <Book>[];
  return showDialog<void>(
    context: context,
    builder: (_) => LabelEditor(
      initial: track.labels,
      library: library,
      title: track.title,
      onSave: (words) => controller.saveTrack(book.id, part, words),
    ),
  );
}

/// Opens the editor for the labels of a podcast or a radio station (the same list of labels as for books); [onSave] saves them on the server.
Future<void> showFeedLabelEditor(
  BuildContext context, {
  required List<String> initial,
  required String title,
  required Future<bool> Function(List<String> words) onSave,
}) {
  final controller = LabelsScope.maybeOf(context);
  if (controller == null) return Future.value();
  final library = PlayerScope.maybeOf(context)?.library ?? const <Book>[];
  return showDialog<void>(
    context: context,
    builder: (_) => LabelEditor(
      initial: initial,
      library: library,
      title: title,
      extra: [for (final (name, _) in controller.list.labels) name],
      onSave: onSave,
    ),
  );
}

/// The labels of a podcast or a station in its list entry: a small chip for each and, for those who may set them, a chip that opens the editor.
/// [idKey] makes the keys of the chips unique in a list (`podcast-3`).
class FeedLabelsLine extends StatelessWidget {
  const FeedLabelsLine({
    super.key,
    required this.idKey,
    required this.title,
    required this.labels,
    required this.canLabel,
    required this.onSave,
  });

  final String idKey;
  final String title;
  final List<String> labels;
  final bool canLabel;
  final Future<bool> Function(List<String> words) onSave;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final controller = LabelsScope.maybeOf(context);
    final editable = canLabel && controller != null && controller.available;
    if (labels.isEmpty && !editable) return const SizedBox.shrink();
    // The chips wrap in the space that is left; the editor is a small icon button at the right edge.
    return Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Expanded(
            child: Wrap(
              spacing: 6,
              runSpacing: 2,
              children: [
                for (final name in labels)
                  Chip(
                    key: ValueKey('feed-label-$idKey-$name'),
                    label: Text(name),
                    labelStyle: Theme.of(context).textTheme.labelSmall,
                    visualDensity: VisualDensity.compact,
                    padding: EdgeInsets.zero,
                  ),
              ],
            ),
          ),
          if (editable)
            IconButton(
              key: ValueKey('feed-labels-edit-$idKey'),
              icon: const Icon(Icons.sell_outlined, size: 18),
              tooltip: labels.isEmpty ? l.labelsEdit : l.labelsTitle,
              visualDensity: VisualDensity.compact,
              onPressed: () => showFeedLabelEditor(context, initial: labels, title: title, onSave: onSave),
            ),
        ],
      ),
    );
  }
}

/// The labels of one title as small chips, and, for the owner and an administrator, a button that opens the editor.
class TrackLabels extends StatelessWidget {
  const TrackLabels({super.key, required this.book, required this.track});

  final Book book;
  final Track track;

  /// This user may set the labels of the title: the server says so for the book ([Book.canLabel]), and a server must be there to ask.
  static bool editable(BuildContext context, Book book, Track track) {
    final controller = LabelsScope.maybeOf(context);
    return book.canLabel && controller != null && controller.available && track.partId != null;
  }

  @override
  Widget build(BuildContext context) {
    if (track.labels.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Wrap(
        spacing: 6,
        runSpacing: 2,
        children: [
          for (final name in track.labels)
            Chip(
              key: ValueKey('track-label-${track.partId}-$name'),
              label: Text(name),
              labelStyle: Theme.of(context).textTheme.labelSmall,
              visualDensity: VisualDensity.compact,
              padding: EdgeInsets.zero,
            ),
        ],
      ),
    );
  }
}

/// The button of a title that opens the editor of its labels.
class TrackLabelsButton extends StatelessWidget {
  const TrackLabelsButton({super.key, required this.book, required this.track});

  final Book book;
  final Track track;

  @override
  Widget build(BuildContext context) {
    if (!TrackLabels.editable(context, book, track)) return const SizedBox.shrink();
    final l = AppLocalizations.of(context);
    return IconButton(
      key: ValueKey('track-labels-edit-${track.partId}'),
      tooltip: l.trackLabelsEdit,
      icon: const Icon(Icons.sell_outlined, size: 20),
      visualDensity: VisualDensity.compact,
      onPressed: () => showTrackLabelEditor(context, book, track),
    );
  }
}

/// The words of a book as chips with a cross; a field with suggestions (the labels in use that fit what is typed); Enter or "Add" puts the word
/// among the chips and does not close the window.
class LabelEditor extends StatefulWidget {
  const LabelEditor({
    super.key,
    required this.initial,
    required this.onSave,
    required this.library,
    this.title,
    this.extra = const [],
  });

  /// The labels the book (or the title) has now.
  final List<String> initial;

  /// Saves the words; true when the server took them.
  final Future<bool> Function(List<String> words) onSave;

  /// What the window is about besides "Labels" (the name of a title); null for a book.
  final String? title;

  /// The books that are known: the labels in use are suggested from them.
  final List<Book> library;

  /// More labels in use to suggest (the ones of podcasts and stations that no book carries).
  final List<String> extra;

  @override
  State<LabelEditor> createState() => _LabelEditorState();
}

class _LabelEditorState extends State<LabelEditor> {
  late final List<String> _words = [...widget.initial];
  final _text = TextEditingController();
  final _focus = FocusNode();
  bool _bad = false;
  bool _saving = false;

  @override
  void dispose() {
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  /// Puts what is typed among the chips (several words may be typed, separated by commas).
  void _add(String raw) {
    var bad = false;
    for (final part in raw.split(RegExp(r'[,;]'))) {
      if (part.trim().isEmpty) continue;
      final name = cleanLabel(part);
      if (name == null) {
        bad = true;
        continue;
      }
      if (!_words.any((w) => w.toLowerCase() == name.toLowerCase())) _words.add(name);
    }
    setState(() => _bad = bad);
    if (!bad) _text.clear();
    _focus.requestFocus();
  }

  Future<void> _save() async {
    _add(_text.text);
    if (_bad) return;
    final l = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.maybeOf(context);
    final nav = Navigator.of(context);
    setState(() => _saving = true);
    final ok = await widget.onSave(_words);
    if (ok) {
      nav.pop();
    } else {
      setState(() => _saving = false);
      messenger?.showSnackBar(SnackBar(content: Text(l.labelsFailed)));
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    return AlertDialog(
      title: Text(widget.title == null ? l.labelsTitle : '${l.labelsTitle}: ${widget.title}'),
      content: SizedBox(
        width: 420,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (_words.isEmpty)
              Text(l.labelsNone, style: TextStyle(color: cs.onSurfaceVariant))
            else
              Wrap(
                spacing: 8,
                runSpacing: 4,
                children: [
                  for (final w in _words)
                    InputChip(
                      key: ValueKey('label-word-$w'),
                      label: Text(w),
                      onDeleted: () => setState(() => _words.remove(w)),
                    ),
                ],
              ),
            const SizedBox(height: 12),
            Row(
              children: [
                Expanded(
                  child: RawAutocomplete<String>(
                    textEditingController: _text,
                    focusNode: _focus,
                    optionsBuilder: (value) {
                      final fits = labelSuggestions(widget.library, value.text, _words);
                      final needle = value.text.trim().toLowerCase();
                      final seen = {for (final f in fits) f.toLowerCase(), for (final w in _words) w.toLowerCase()};
                      return [
                        ...fits,
                        for (final e in widget.extra)
                          if (!seen.contains(e.toLowerCase()) && (needle.isEmpty || e.toLowerCase().contains(needle))) e,
                      ];
                    },
                    onSelected: (name) {
                      _add(name);
                    },
                    fieldViewBuilder: (context, controller, focusNode, _) => TextField(
                      key: const ValueKey('labels-entry'),
                      controller: controller,
                      focusNode: focusNode,
                      maxLength: 40,
                      decoration: InputDecoration(
                        hintText: l.labelsNew,
                        counterText: '',
                        errorText: _bad ? l.labelsBad : null,
                        isDense: true,
                      ),
                      // Enter adds the word; it does not close the window.
                      onSubmitted: (v) => _add(v),
                    ),
                    optionsViewBuilder: (context, onSelected, options) => Align(
                      alignment: Alignment.topLeft,
                      child: Material(
                        elevation: 4,
                        borderRadius: BorderRadius.circular(8),
                        child: ConstrainedBox(
                          constraints: const BoxConstraints(maxHeight: 220, maxWidth: 340),
                          child: ListView(
                            padding: EdgeInsets.zero,
                            shrinkWrap: true,
                            children: [
                              for (final o in options)
                                ListTile(
                                  key: ValueKey('label-suggestion-$o'),
                                  dense: true,
                                  title: Text(o),
                                  onTap: () => onSelected(o),
                                ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                OutlinedButton(
                  key: const ValueKey('labels-add'),
                  onPressed: () => _add(_text.text),
                  child: Text(l.labelsAdd),
                ),
              ],
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: _saving ? null : () => Navigator.pop(context), child: Text(l.cancel)),
        FilledButton(
          key: const ValueKey('labels-save'),
          onPressed: _saving ? null : _save,
          child: Text(l.labelsSave),
        ),
      ],
    );
  }
}

/// The labels to filter a list by: chips that break into a new line by themselves. When they would take more than [maxLines] lines they
/// become one combo box with a check box for every label. A tap adds the label, a second tap takes it away; several narrow the list (a book has all).
class LabelFilter extends StatelessWidget {
  const LabelFilter({
    super.key,
    required this.counts,
    required this.chosen,
    required this.onChanged,
    this.maxLines = 3,
  });

  final List<(String, int)> counts;
  final Set<String> chosen;
  final ValueChanged<Set<String>> onChanged;

  /// Up to this many lines the chips are shown; more than that, the combo box.
  final int maxLines;

  bool _isChosen(String name) => chosen.any((c) => c.toLowerCase() == name.toLowerCase());

  void _toggle(String name, bool on) {
    final next = {...chosen}..removeWhere((c) => c.toLowerCase() == name.toLowerCase());
    if (on) next.add(name);
    onChanged(next);
  }

  /// How many lines the chips take at [width]: each is the text, the icon and the padding of a compact chip, with 8 between them.
  int _lines(BuildContext context, double width) {
    final style = Theme.of(context).textTheme.labelLarge ?? const TextStyle();
    final scale = MediaQuery.textScalerOf(context);
    var lines = 1;
    var used = 0.0;
    for (final (name, count) in counts) {
      final painter = TextPainter(text: TextSpan(text: '$name ($count)', style: style), textDirection: TextDirection.ltr, textScaler: scale)..layout();
      final chip = painter.width + 52;
      if (used > 0 && used + 8 + chip > width) {
        lines++;
        used = chip;
      } else {
        used += (used > 0 ? 8 : 0) + chip;
      }
    }
    return lines;
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    if (counts.isEmpty) return const SizedBox.shrink();
    final clear = chosen.isEmpty
        ? null
        : ActionChip(
            key: const ValueKey('label-filter-clear'),
            avatar: const Icon(Icons.close, size: 16),
            label: Text(l.labelsFilterClear),
            visualDensity: VisualDensity.compact,
            onPressed: () => onChanged({}),
          );
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16),
      child: LayoutBuilder(
        builder: (context, box) {
          if (_lines(context, box.maxWidth) <= maxLines) {
            return Wrap(
              spacing: 8,
              runSpacing: 4,
              children: [
                ?clear,
                for (final (name, count) in counts)
                  FilterChip(
                    key: ValueKey('label-filter-$name'),
                    avatar: const Icon(Icons.sell_outlined, size: 16),
                    label: Text('$name ($count)'),
                    selected: _isChosen(name),
                    visualDensity: VisualDensity.compact,
                    onSelected: (on) => _toggle(name, on),
                  ),
              ],
            );
          }
          return Wrap(
            spacing: 8,
            runSpacing: 4,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              MenuAnchor(
                style: const MenuStyle(maximumSize: WidgetStatePropertyAll(Size(double.infinity, 420))),
                menuChildren: [
                  for (final (name, count) in counts)
                    CheckboxMenuButton(
                      key: ValueKey('label-filter-$name'),
                      closeOnActivate: false,
                      value: _isChosen(name),
                      onChanged: (on) => _toggle(name, on ?? false),
                      child: Text('$name ($count)'),
                    ),
                ],
                builder: (context, menu, _) => OutlinedButton.icon(
                  key: const ValueKey('label-filter-combo'),
                  icon: const Icon(Icons.sell_outlined, size: 18),
                  label: Text(chosen.isEmpty ? l.labelsTitle : '${l.labelsTitle} (${chosen.length})'),
                  onPressed: () => menu.isOpen ? menu.close() : menu.open(),
                ),
              ),
              ?clear,
            ],
          );
        },
      ),
    );
  }
}

/// The list of the labels of the library (in the settings): every label with the number of its books; an administrator renames and deletes them.
class LabelsCard extends StatefulWidget {
  const LabelsCard({super.key, required this.controller});

  final LabelsController controller;

  @override
  State<LabelsCard> createState() => _LabelsCardState();
}

class _LabelsCardState extends State<LabelsCard> {
  @override
  void initState() {
    super.initState();
    widget.controller.load();
  }

  Future<void> _rename(String name) async {
    final l = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.maybeOf(context);
    final text = TextEditingController(text: name);
    final to = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.labelsRename),
        content: TextField(
          key: const ValueKey('label-rename-field'),
          controller: text,
          autofocus: true,
          maxLength: 40,
          onSubmitted: (v) => Navigator.pop(context, v),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: Text(l.cancel)),
          FilledButton(
            key: const ValueKey('label-rename-save'),
            onPressed: () => Navigator.pop(context, text.text),
            child: Text(l.labelsSave),
          ),
        ],
      ),
    );
    final clean = to == null ? null : cleanLabel(to);
    if (to == null || clean == name) return;
    if (clean == null) {
      messenger?.showSnackBar(SnackBar(content: Text(l.labelsBad)));
      return;
    }
    if (!await widget.controller.rename(name, clean)) {
      messenger?.showSnackBar(SnackBar(content: Text(l.labelsFailed)));
    }
  }

  Future<void> _delete(String name, int count) async {
    final l = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.maybeOf(context);
    final yes = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(l.labelsDelete),
        content: Text(l.labelsDeleteConfirm(name, count)),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(l.cancel)),
          FilledButton(
            key: const ValueKey('label-delete-yes'),
            onPressed: () => Navigator.pop(context, true),
            child: Text(l.labelsDelete),
          ),
        ],
      ),
    );
    if (yes != true) return;
    if (!await widget.controller.remove(name)) {
      messenger?.showSnackBar(SnackBar(content: Text(l.labelsFailed)));
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) {
        final l = AppLocalizations.of(context);
        final text = Theme.of(context).textTheme;
        final cs = Theme.of(context).colorScheme;
        final list = widget.controller.list;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.labelsTitle, style: text.titleMedium?.copyWith(color: cs.primary)),
            const SizedBox(height: 8),
            Card(
              margin: EdgeInsets.zero,
              clipBehavior: Clip.antiAlias,
              child: list.labels.isEmpty
                  ? Padding(
                      padding: const EdgeInsets.all(16),
                      child: Text(
                        l.labelsNoneInLibrary,
                        key: const ValueKey('labels-none'),
                        style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
                      ),
                    )
                  : Column(
                      children: [
                        for (final (name, count) in list.labels)
                          ListTile(
                            key: ValueKey('label-row-$name'),
                            dense: true,
                            leading: const Icon(Icons.sell_outlined),
                            title: Text(name),
                            subtitle: Text('$count'),
                            trailing: list.canManage && !isReservedLabel(name)
                                ? Row(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      IconButton(
                                        key: ValueKey('label-rename-$name'),
                                        tooltip: l.labelsRename,
                                        icon: const Icon(Icons.edit_outlined),
                                        onPressed: () => _rename(name),
                                      ),
                                      IconButton(
                                        key: ValueKey('label-delete-$name'),
                                        tooltip: l.labelsDelete,
                                        icon: const Icon(Icons.delete_outline),
                                        onPressed: () => _delete(name, count),
                                      ),
                                    ],
                                  )
                                : null,
                          ),
                      ],
                    ),
            ),
          ],
        );
      },
    );
  }
}
