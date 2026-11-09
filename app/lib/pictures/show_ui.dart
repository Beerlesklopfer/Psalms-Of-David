// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../format.dart';
import '../l10n/app_localizations.dart';
import '../offline/offline_controller.dart';
import '../photos/photo_backup_ui.dart' show PhotoChoiceField;
import '../playlists.dart';
import '../screens/confirm_dialog.dart';
import '../screens/playlists_screen.dart' show PlaylistScope;
import '../server/api_client.dart';
import 'show_api.dart';
import 'show_player.dart';
import 'show_project.dart';
import 'show_texts.dart';

/// The slideshows of this device and the making of new ones from the pictures of the server (docs/spec-pictures.md, step 3).
class ShowController extends ChangeNotifier {
  ShowController({this.api, ShowProjects? projects}) : projects = projects ?? ShowProjects();

  /// The server; null without one (the slideshows on this device still play).
  final ShowApi? api;
  final ShowProjects projects;

  List<ShowProject> list = const [];
  bool loaded = false;

  /// While a project is loaded or unpacked: the bytes so far, and whether it is being unpacked.
  int? loadingBytes;
  bool unpacking = false;

  /// What went wrong the last time (`broken`, `newer`, `empty`, `failed`, `space`), for the screen to say.
  String? error;

  bool get busy => loadingBytes != null || unpacking;

  Future<void> load() async {
    await projects.tidy();
    list = await projects.list();
    loaded = true;
    notifyListeners();
  }

  /// Opens a project file of this device (saved in the browser and brought here); the file stays where it is.
  Future<ShowProject?> importFile(File zip) async {
    error = null;
    unpacking = true;
    notifyListeners();
    try {
      final p = await projects.import(zip);
      list = await projects.list();
      return p;
    } on ShowProjectException catch (e) {
      error = e.reason;
      return null;
    } finally {
      unpacking = false;
      notifyListeners();
    }
  }

  /// Loads the project of [choice] from the server onto this device.
  Future<ShowProject?> save(ShowChoice choice) => _fetch((api, zip, progress) => api.download(choice, zip, progress: progress));

  /// The projects put together on the server (the web editor); empty without a server or while it cannot be reached.
  List<ServerShow> server = const [];
  bool serverLoaded = false;

  /// The server did not answer the last time it was asked for its projects.
  bool serverFailed = false;

  Future<void> loadServer() async {
    final api = this.api;
    if (api == null) return;
    try {
      server = await api.shows();
      serverFailed = false;
    } on Object {
      serverFailed = true;
    }
    serverLoaded = true;
    notifyListeners();
  }

  /// The copy of [show] on this device, or null.
  ShowProject? copyOf(ServerShow show) => list.where((p) => p.serverId == show.id).firstOrNull;

  /// Whether the server has a newer version of [show] than the copy on this device.
  bool outdated(ServerShow show) {
    final copy = copyOf(show);
    return copy != null && copy.serverRev != show.rev;
  }

  /// Loads a project of the server onto this device, with its pictures, videos and music; an older copy of it goes.
  Future<ShowProject?> saveShow(ServerShow show) async {
    final older = [for (final p in list) if (p.serverId == show.id) p];
    var p = await _fetch((api, zip, progress) => api.downloadShow(show.id, zip, progress: progress));
    if (p == null) return null;
    p = await projects.markServer(p, show.id, show.rev);
    for (final o in older) {
      await projects.delete(o);
    }
    list = await projects.list();
    notifyListeners();
    return p;
  }

  /// A project of the server to play from there, without loading it onto this device; null when the server does not answer. The one who
  /// plays it calls `remote.dispose()` afterwards.
  Future<ShowProject?> stream(ServerShow show) async {
    final api = this.api;
    if (api == null) return null;
    try {
      return await api.stream(show.id, Directory('${projects.root.path}/.stream-${DateTime.now().microsecondsSinceEpoch}'));
    } on Object {
      return null;
    }
  }

  Future<ShowProject?> _fetch(Future<void> Function(ShowApi api, File zip, void Function(int bytes) progress) load) async {
    final api = this.api;
    if (api == null || busy) return null;
    error = null;
    loadingBytes = 0;
    notifyListeners();
    final zip = File('${projects.root.path}/.download-${DateTime.now().microsecondsSinceEpoch}.zip');
    try {
      await projects.root.create(recursive: true);
      await load(api, zip, (n) {
        loadingBytes = n;
        notifyListeners();
      });
      loadingBytes = null;
      unpacking = true;
      notifyListeners();
      final p = await projects.import(zip);
      list = await projects.list();
      return p;
    } on ShowProjectException catch (e) {
      error = e.reason;
    } on ApiException catch (e) {
      error = e.code == 'empty' ? 'empty' : 'failed';
    } on FileSystemException catch (e) {
      // ENOSPC on Linux and Android, 28 on macOS and iOS too.
      error = e.osError?.errorCode == 28 ? 'space' : 'failed';
    } on Object {
      error = 'failed';
    } finally {
      loadingBytes = null;
      unpacking = false;
      if (zip.existsSync()) zip.deleteSync();
      notifyListeners();
    }
    return null;
  }

  Future<void> delete(ShowProject p) async {
    await projects.delete(p);
    list = await projects.list();
    notifyListeners();
  }
}

/// The text of an [ShowController.error].
String showErrorText(AppLocalizations l, String code) => switch (code) {
      'broken' || 'newer' => l.showProjectBroken,
      'empty' => l.showEmpty,
      'space' => l.showSpaceLow,
      _ => l.showLoadFailed,
    };

/// The rubric "Pictures" of the library: the slideshows of this device, and the ways to a new one.
class PicturesBody extends StatefulWidget {
  const PicturesBody({super.key, required this.controller});

  final ShowController controller;

  @override
  State<PicturesBody> createState() => _PicturesBodyState();
}

class _PicturesBodyState extends State<PicturesBody> {
  @override
  void initState() {
    super.initState();
    if (!widget.controller.loaded) widget.controller.load();
    widget.controller.loadServer();
  }

  void _say(BuildContext context, String text) => ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));

  /// Plays a project of the server from there; what was fetched for it is thrown away afterwards.
  Future<void> _stream(BuildContext context, ServerShow show) async {
    final l = AppLocalizations.of(context);
    final p = await widget.controller.stream(show);
    if (!context.mounted) {
      await p?.remote?.dispose();
      return;
    }
    if (p == null || p.pictures.isEmpty) {
      await p?.remote?.dispose();
      if (context.mounted) _say(context, p == null ? l.showServerUnreachable : l.showEmpty);
      return;
    }
    await ShowPlayerScreen.open(context, p);
    await p.remote?.dispose();
  }

  Future<void> _download(BuildContext context, ServerShow show) async {
    final l = AppLocalizations.of(context);
    final p = await widget.controller.saveShow(show);
    if (!context.mounted) return;
    _say(context, p != null ? l.showSaved : showErrorText(l, widget.controller.error ?? 'failed'));
  }

  Future<void> _openFile(BuildContext context) async {
    const zip = XTypeGroup(label: 'zip', extensions: ['zip'], mimeTypes: ['application/zip'], uniformTypeIdentifiers: ['public.zip-archive']);
    final l = AppLocalizations.of(context);
    final picked = await openFile(acceptedTypeGroups: [zip], confirmButtonText: l.showOpenFile);
    if (picked == null) return;
    final p = await widget.controller.importFile(File(picked.path));
    if (!context.mounted) return;
    final error = widget.controller.error;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(p != null ? l.showSaved : showErrorText(l, error ?? 'broken'))));
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
        listenable: widget.controller,
        builder: (context, _) {
          final c = widget.controller;
          final l = AppLocalizations.of(context);
          final text = Theme.of(context).textTheme;
          final cs = Theme.of(context).colorScheme;
          // Offline the projects of the server are not asked for; those loaded onto this device still play.
          final offline = OfflineScope.maybeOf(context)?.active ?? false;
          return SliverPadding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 24),
            sliver: SliverList.list(children: [
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  if (c.api != null)
                    FilledButton.icon(
                      key: const ValueKey('show-new'),
                      onPressed: c.busy
                          ? null
                          : () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => ShowCreateScreen(controller: c))),
                      icon: const Icon(Icons.add_photo_alternate_outlined),
                      label: Text(l.showNew),
                    ),
                  OutlinedButton.icon(
                    key: const ValueKey('show-open-file'),
                    onPressed: c.busy ? null : () => _openFile(context),
                    icon: const Icon(Icons.folder_open_outlined),
                    label: Text(l.showOpenFile),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              if (c.busy) ...[_Progress(controller: c), const SizedBox(height: 12)],
              Text(l.showProjectsTitle, style: text.titleMedium),
              const SizedBox(height: 4),
              Text(c.api == null ? l.showNoServer : l.showProjectsHint, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
              if (!showVideosHere && c.list.any((p) => p.pictures.any((e) => e.video != null)))
                Text(l.showNoVideoHere, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
              const SizedBox(height: 8),
              if (c.loaded && c.list.isEmpty)
                Padding(padding: const EdgeInsets.symmetric(vertical: 24), child: Text(l.showProjectsEmpty, key: const ValueKey('show-none')))
              else
                for (final p in c.list) _ProjectCard(controller: c, project: p),
              if (c.api != null) ...[
                const SizedBox(height: 16),
                Text(l.showServerTitle, style: text.titleMedium),
                const SizedBox(height: 4),
                Text(l.showServerHint, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
                const SizedBox(height: 8),
                if (offline || c.serverFailed)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 16),
                    child: Text(l.showServerUnreachable, key: const ValueKey('show-server-unreachable')),
                  )
                else if (c.serverLoaded && c.server.isEmpty)
                  Padding(padding: const EdgeInsets.symmetric(vertical: 16), child: Text(l.showServerEmpty, key: const ValueKey('show-server-none')))
                else
                  for (final s in c.server)
                    _ServerShowCard(
                      controller: c,
                      show: s,
                      onStream: () => _stream(context, s),
                      onDownload: () => _download(context, s),
                    ),
              ],
            ]),
          );
        },
      );
}

class _Progress extends StatelessWidget {
  const _Progress({required this.controller});

  final ShowController controller;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final locale = Localizations.localeOf(context).toLanguageTag();
    final bytes = controller.loadingBytes;
    return Column(
      key: const ValueKey('show-progress'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const LinearProgressIndicator(),
        const SizedBox(height: 6),
        Text(bytes != null ? l.showLoading(formatBytes(locale, bytes)) : l.showUnpacking),
      ],
    );
  }
}

/// A project of the server: a tap plays it (the copy on this device when it is the newest, else from the server), and it is loaded onto
/// this device or brought up to date there.
class _ServerShowCard extends StatelessWidget {
  const _ServerShowCard({required this.controller, required this.show, required this.onStream, required this.onDownload});

  final ShowController controller;
  final ServerShow show;
  final VoidCallback onStream;
  final VoidCallback onDownload;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    final copy = controller.copyOf(show);
    final outdated = controller.outdated(show);
    final busy = controller.busy;
    final meta = [
      l.showCount(show.pictures),
      if (copy != null) outdated ? l.showOutdated : l.showOnDevice,
    ].join(' · ');
    return Card(
      key: ValueKey('show-server-${show.id}'),
      margin: const EdgeInsets.only(bottom: 8),
      child: ListTile(
        contentPadding: const EdgeInsets.fromLTRB(16, 4, 4, 4),
        leading: Icon(outdated ? Icons.sync_problem_outlined : (copy != null ? Icons.offline_pin_outlined : Icons.cloud_outlined), color: outdated ? cs.tertiary : cs.primary),
        title: Text(show.title.isEmpty ? l.showTitle : show.title),
        subtitle: Text(meta, key: ValueKey('show-server-meta-${show.id}')),
        onTap: copy != null && !outdated ? () => ShowPlayerScreen.open(context, copy) : onStream,
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            IconButton(
              key: ValueKey('show-stream-${show.id}'),
              tooltip: l.showStream,
              icon: Icon(Icons.play_circle_outline, color: cs.primary),
              onPressed: onStream,
            ),
            if (copy == null || outdated)
              IconButton(
                key: ValueKey('show-download-${show.id}'),
                tooltip: outdated ? l.showUpdate : l.showDownload,
                icon: Icon(outdated ? Icons.sync : Icons.download_outlined),
                onPressed: busy || show.pictures == 0 ? null : onDownload,
              ),
          ],
        ),
      ),
    );
  }
}

class _ProjectCard extends StatelessWidget {
  const _ProjectCard({required this.controller, required this.project});

  final ShowController controller;
  final ShowProject project;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final locale = Localizations.localeOf(context).toLanguageTag();
    final cs = Theme.of(context).colorScheme;
    final cover = project.pictures.isEmpty ? null : File(project.path(project.pictures.first.image));
    final meta = [
      l.showMeta(l.showCount(project.pictures.length), formatBytes(locale, project.bytes)),
      if (project.music.isNotEmpty) l.showWithMusic,
    ].join(' · ');
    return Card(
      key: ValueKey('show-project-${project.id}'),
      margin: const EdgeInsets.only(bottom: 8),
      clipBehavior: Clip.antiAlias,
      child: ListTile(
        contentPadding: const EdgeInsets.fromLTRB(8, 4, 4, 4),
        leading: ClipRRect(
          borderRadius: BorderRadius.circular(6),
          child: SizedBox(
            width: 64,
            height: 48,
            child: cover == null
                ? ColoredBox(color: cs.surfaceContainerHighest)
                : Image.file(cover, fit: BoxFit.cover, cacheWidth: 192, errorBuilder: (_, _, _) => ColoredBox(color: cs.surfaceContainerHighest)),
          ),
        ),
        title: Text(project.title.isEmpty ? l.showTitle : project.title),
        subtitle: Text(meta),
        onTap: () => ShowPlayerScreen.open(context, project),
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            IconButton(
              tooltip: l.showStart,
              icon: Icon(Icons.play_circle_outline, color: cs.primary),
              onPressed: () => ShowPlayerScreen.open(context, project),
            ),
            IconButton(
              key: ValueKey('show-delete-${project.id}'),
              tooltip: l.showDelete,
              icon: const Icon(Icons.delete_outline),
              onPressed: () async {
                final name = project.title.isEmpty ? l.showTitle : project.title;
                if (await confirmRemoval(context, title: l.showDelete, text: l.showDeleteAsk(name), action: l.showDelete)) {
                  await controller.delete(project);
                }
              },
            ),
          ],
        ),
      ),
    );
  }
}

/// The making of a slideshow: the same choices in the same order as the page `/pictures` of the server, then it is loaded onto this device.
class ShowCreateScreen extends StatefulWidget {
  const ShowCreateScreen({super.key, required this.controller});

  final ShowController controller;

  @override
  State<ShowCreateScreen> createState() => _ShowCreateScreenState();
}

class _ShowCreateScreenState extends State<ShowCreateScreen> {
  List<ShowSource>? _sources;
  bool _failed = false;
  final Set<String> _chosen = {};
  int? _from;
  int? _to;
  final Map<String, String> _choices = {...kShowDefaults};
  int? _music;

  @override
  void initState() {
    super.initState();
    _loadSources();
  }

  Future<void> _loadSources() async {
    setState(() => _failed = false);
    try {
      final s = await widget.controller.api!.sources();
      if (!mounted) return;
      setState(() {
        _sources = s;
        _chosen
          ..clear()
          ..addAll(s.map((e) => e.id));
      });
    } on Object {
      if (mounted) setState(() => _failed = true);
    }
  }

  List<int> get _years {
    final s = (_sources ?? const <ShowSource>[]).where((e) => _chosen.contains(e.id));
    final first = s.map((e) => e.first?.year).whereType<int>().fold<int?>(null, (a, b) => a == null || b < a ? b : a);
    final last = s.map((e) => e.last?.year).whereType<int>().fold<int?>(null, (a, b) => a == null || b > a ? b : a);
    if (first == null || last == null) return const [];
    return [for (var y = first; y <= last; y++) y];
  }

  int get _count => (_sources ?? const <ShowSource>[]).where((e) => _chosen.contains(e.id)).fold(0, (n, e) => n + e.count);

  Future<void> _save() async {
    final choice = ShowChoice(sources: _chosen.toList(), fromYear: _from, toYear: _to, choices: _choices, music: _music);
    final p = await widget.controller.save(choice);
    if (!mounted) return;
    final l = AppLocalizations.of(context);
    if (p == null) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(showErrorText(l, widget.controller.error ?? 'failed'))));
      return;
    }
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(l.showSaved)));
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    final playlists = PlaylistScope.maybeOf(context)?.lists.where((p) => p.items > 0).toList() ?? const <PlaylistSummary>[];
    final years = _years;
    if (_from != null && !years.contains(_from)) _from = null;
    if (_to != null && !years.contains(_to)) _to = null;
    if (_music != null && !playlists.any((p) => p.id == _music)) _music = null;
    return Scaffold(
      appBar: AppBar(title: Text(l.showNew)),
      body: ListenableBuilder(
        listenable: widget.controller,
        builder: (context, _) {
          final c = widget.controller;
          final sources = _sources;
          return Align(
            alignment: Alignment.topCenter,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 560),
              child: ListView(
                padding: const EdgeInsets.all(16),
                children: [
                  if (_failed) ...[
                    Text(l.showSourcesFailed),
                    const SizedBox(height: 8),
                    Align(alignment: Alignment.centerLeft, child: OutlinedButton(onPressed: _loadSources, child: Text(l.retryButton))),
                  ] else if (sources == null)
                    const Center(child: Padding(padding: EdgeInsets.all(32), child: CircularProgressIndicator()))
                  else if (sources.isEmpty)
                    Text(l.showNone, key: const ValueKey('show-no-sources'))
                  else ...[
                    _Group(title: l.showGroupWhich, children: [
                      for (final s in sources)
                        CheckboxListTile(
                          key: ValueKey('show-source-${s.id}'),
                          contentPadding: EdgeInsets.zero,
                          value: _chosen.contains(s.id),
                          onChanged: c.busy ? null : (v) => setState(() => v == true ? _chosen.add(s.id) : _chosen.remove(s.id)),
                          title: Text(s.kind == 'own' ? l.showSourceOwn : s.name, overflow: TextOverflow.ellipsis),
                          subtitle: Text('${switch (s.kind) { 'own' => l.showKindOwn, 'child' => l.showKindChild, _ => l.showKindFolder }} · ${s.count}'),
                        ),
                      Text(l.showWhichHint, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
                    ]),
                    _Group(title: l.showGroupWhen, children: [
                      _YearField(
                        id: 'from',
                        label: l.showSetFrom,
                        years: years,
                        value: _from,
                        all: l.showOptFromAll,
                        explainAll: l.showExplainFromAll,
                        explainYear: (y) => l.showExplainFromYear('$y'),
                        onChanged: c.busy ? null : (v) => setState(() => _from = v),
                      ),
                      _YearField(
                        id: 'to',
                        label: l.showSetTo,
                        years: years.reversed.toList(),
                        value: _to,
                        all: l.showOptToAll,
                        explainAll: l.showExplainToAll,
                        explainYear: (y) => l.showExplainToYear('$y'),
                        onChanged: c.busy ? null : (v) => setState(() => _to = v),
                      ),
                    ]),
                    _Group(title: l.showGroupHow, children: [
                      for (final e in kShowChoices.entries)
                        PhotoChoiceField(
                          id: 'show-${e.key}',
                          label: showChoiceLabel(l, e.key),
                          options: e.value,
                          value: _choices[e.key]!,
                          optionLabel: (o) => showOptionLabel(l, e.key, o),
                          explain: (o) => showOptionExplain(l, e.key, o),
                          onChanged: c.busy ? null : (v) => setState(() => _choices[e.key] = v),
                        ),
                      PhotoChoiceField(
                        id: 'show-music',
                        label: l.showSetMusic,
                        options: ['', for (final p in playlists) '${p.id}'],
                        value: _music == null ? '' : '$_music',
                        optionLabel: (o) => o.isEmpty ? l.showOptMusicOff : playlists.firstWhere((p) => '${p.id}' == o).name,
                        explain: (o) => o.isEmpty
                            ? (playlists.isEmpty ? '${l.showExplainMusicOff} ${l.showMusicNone}' : l.showExplainMusicOff)
                            : l.showExplainMusicList(playlists.firstWhere((p) => '${p.id}' == o).items),
                        onChanged: c.busy ? null : (v) => setState(() => _music = int.tryParse(v)),
                      ),
                    ]),
                    if (c.busy) ...[_Progress(controller: c), const SizedBox(height: 12)],
                    Text(l.showSaveExplain, style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant)),
                    const SizedBox(height: 12),
                    Row(
                      children: [
                        Expanded(child: Text(l.showCount(_count), key: const ValueKey('show-count'))),
                        FilledButton.icon(
                          key: const ValueKey('show-save'),
                          onPressed: c.busy || _chosen.isEmpty ? null : _save,
                          icon: const Icon(Icons.download_outlined),
                          label: Text(l.showSave),
                        ),
                      ],
                    ),
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

class _Group extends StatelessWidget {
  const _Group({required this.title, required this.children});

  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) => Card(
        margin: const EdgeInsets.only(bottom: 16),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
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

/// A year as the beginning or the end of the time, with the text of the chosen one below (a choice like the others).
class _YearField extends StatelessWidget {
  const _YearField({
    required this.id,
    required this.label,
    required this.years,
    required this.value,
    required this.all,
    required this.explainAll,
    required this.explainYear,
    required this.onChanged,
  });

  final String id;
  final String label;
  final List<int> years;
  final int? value;
  final String all;
  final String explainAll;
  final String Function(int year) explainYear;
  final ValueChanged<int?>? onChanged;

  @override
  Widget build(BuildContext context) => PhotoChoiceField(
        id: 'show-$id',
        label: label,
        options: ['', for (final y in years) '$y'],
        value: value == null ? '' : '$value',
        optionLabel: (o) => o.isEmpty ? all : o,
        explain: (o) => o.isEmpty ? explainAll : explainYear(int.parse(o)),
        onChanged: onChanged == null ? null : (v) => onChanged!(int.tryParse(v)),
      );
}
