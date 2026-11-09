// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

import 'package:archive/archive_io.dart';

import '../server/app_paths.dart';

/// What `show.json` of a project says it is (`server/src/show_project.rs`).
const kShowFormat = 'david-slideshow';

/// The newest version of the format this app plays.
const kShowVersion = 1;

/// The file next to `show.json` that says which project of the server a copy is, and of which version.
const kShowServerFile = 'server.json';

/// The choices a project was made with.
class ShowSettings {
  const ShowSettings({this.order = 'date', this.seconds = 8, this.transition = 'fade', this.videos = 'on', this.info = 'both'});

  factory ShowSettings.fromJson(Map<String, dynamic> j) {
    String one(String key, List<String> options) => options.contains(j[key]) ? j[key] as String : options.first;
    final seconds = j['seconds'] is int && const [3, 5, 8, 15, 30].contains(j['seconds']) ? j['seconds'] as int : 8;
    return ShowSettings(
      order: one('order', const ['date', 'random']),
      seconds: seconds,
      transition: one('transition', const ['fade', 'zoom', 'cut', 'random']),
      videos: one('videos', const ['on', 'off']),
      info: one('info', const ['both', 'date', 'none']),
    );
  }

  final String order;
  final int seconds;
  final String transition;
  final String videos;
  final String info;
}

/// One picture of a project: its file in the screen size, the video that plays instead (or null), when and where it was taken, and (in a
/// project put together by hand on the server) its own transition and seconds; null: those of the project.
class ShowPicture {
  const ShowPicture({required this.image, this.video, required this.takenAt, this.place, this.transition, this.seconds});

  final String image;
  final String? video;
  final DateTime takenAt;
  final String? place;
  final String? transition;
  final int? seconds;
}

/// One track of the music of a project.
class ShowTrack {
  const ShowTrack({required this.file, required this.title, required this.artist});

  final String file;
  final String title;
  final String artist;
}

/// Why a project cannot be played.
class ShowProjectException implements Exception {
  const ShowProjectException(this.reason);

  /// `broken` (no show.json, wrong format, a file it names is missing), `newer` (a newer version of the format) or `space` (the device is full).
  final String reason;

  @override
  String toString() => 'ShowProjectException: $reason';
}

/// Where the files of a project played from the server come from (`GET /api/v1/pictures/shows/{id}`): each file is fetched into [cache]
/// just before it is shown, nothing is kept after the show. Names: `screen/<key>` a picture, `video/<key>` the MP4 of a video,
/// `part/<id>` a track of the music.
abstract class ShowRemote {
  Directory get cache;

  /// The file of [name] on this device, fetched once; throws when the server cannot give it.
  Future<String> fetch(String name);

  /// The place of a picture (`info` of the server), or null.
  Future<String?> place(String key);

  /// Throws away what was fetched.
  Future<void> dispose();
}

/// The key of a picture in a name of [ShowRemote] (`screen/b12` → `b12`).
String showKeyOf(String name) => name.substring(name.indexOf('/') + 1);

/// A slideshow kept on this device: the unpacked project in its own folder, which plays without the server and without a network.
class ShowProject {
  ShowProject({
    required this.dir,
    required this.title,
    required this.madeAt,
    required this.settings,
    required this.pictures,
    required this.music,
    this.remote,
    this.serverId,
    this.serverRev,
  });

  /// A project of the server to play from there (`GET /api/v1/pictures/shows/{id}`): the keys of the pictures and the tracks of the music,
  /// fetched by [remote] while it plays.
  factory ShowProject.remote(Map<String, dynamic> j, ShowRemote remote) {
    final pictures = <ShowPicture>[
      for (final p in (j['pictures'] is List ? j['pictures'] as List : const []).whereType<Map<String, dynamic>>())
        if (p['key'] is String && RegExp(r'^[A-Za-z0-9_-]+$').hasMatch(p['key'] as String))
          ShowPicture(
            image: 'screen/${p['key']}',
            video: p['video'] == true ? 'video/${p['key']}' : null,
            takenAt: DateTime.fromMillisecondsSinceEpoch(p['taken_at'] is int ? p['taken_at'] as int : 0),
            transition: const ['fade', 'zoom', 'cut'].contains(p['transition']) ? p['transition'] as String : null,
            seconds: p['seconds'] is int && const [3, 5, 8, 15, 30].contains(p['seconds']) ? p['seconds'] as int : null,
          ),
    ];
    return ShowProject(
      dir: remote.cache,
      title: j['title'] is String ? j['title'] as String : '',
      madeAt: DateTime.now(),
      settings: ShowSettings.fromJson(j['settings'] is Map<String, dynamic> ? j['settings'] as Map<String, dynamic> : const {}),
      pictures: pictures,
      music: [
        for (final t in (j['music'] is List ? j['music'] as List : const []).whereType<Map<String, dynamic>>())
          if (t['part_id'] is int) ShowTrack(file: 'part/${t['part_id']}', title: '${t['title'] ?? ''}', artist: '${t['artist'] ?? ''}'),
      ],
      remote: remote,
      serverId: j['id'] is int ? j['id'] as int : null,
      serverRev: j['rev'] is String ? j['rev'] as String : null,
    );
  }

  /// Reads `show.json` of an unpacked project and checks that every file it names lies in the folder.
  static Future<ShowProject> open(Directory dir) async {
    final file = File('${dir.path}/show.json');
    Map<String, dynamic> j;
    try {
      j = jsonDecode(await file.readAsString()) as Map<String, dynamic>;
    } on Object {
      throw const ShowProjectException('broken');
    }
    if (j['format'] != kShowFormat || j['pictures'] is! List) throw const ShowProjectException('broken');
    if (j['version'] is! int || (j['version'] as int) > kShowVersion) throw const ShowProjectException('newer');
    final pictures = <ShowPicture>[];
    for (final p in (j['pictures'] as List).whereType<Map<String, dynamic>>()) {
      final image = _safe(p['image']);
      if (image == null) throw const ShowProjectException('broken');
      pictures.add(ShowPicture(
        image: image,
        video: _safe(p['video']),
        takenAt: DateTime.fromMillisecondsSinceEpoch(p['taken_at'] is int ? p['taken_at'] as int : 0),
        place: p['place'] is String && (p['place'] as String).isNotEmpty ? p['place'] as String : null,
        transition: const ['fade', 'zoom', 'cut'].contains(p['transition']) ? p['transition'] as String : null,
        seconds: p['seconds'] is int && const [3, 5, 8, 15, 30].contains(p['seconds']) ? p['seconds'] as int : null,
      ));
    }
    final music = <ShowTrack>[
      for (final t in (j['music'] is List ? j['music'] as List : const []).whereType<Map<String, dynamic>>())
        if (_safe(t['file']) case final f?) ShowTrack(file: f, title: '${t['title'] ?? ''}', artist: '${t['artist'] ?? ''}'),
    ];
    // A project of the server loaded onto this device: which one and in which version (`server.json`, written by the app).
    int? serverId;
    String? serverRev;
    final mark = File('${dir.path}/$kShowServerFile');
    if (mark.existsSync()) {
      try {
        final m = jsonDecode(await mark.readAsString());
        if (m is Map<String, dynamic>) {
          serverId = m['id'] is int ? m['id'] as int : null;
          serverRev = m['rev'] is String ? m['rev'] as String : null;
        }
      } on Object {
        // No mark: an ordinary project of this device.
      }
    }
    final project = ShowProject(
      serverId: serverId,
      serverRev: serverRev,
      dir: dir,
      title: j['title'] is String ? j['title'] as String : '',
      madeAt: DateTime.fromMillisecondsSinceEpoch(j['made_at'] is int ? j['made_at'] as int : 0),
      settings: ShowSettings.fromJson(j['settings'] is Map<String, dynamic> ? j['settings'] as Map<String, dynamic> : const {}),
      pictures: pictures,
      music: music,
    );
    for (final name in [for (final p in pictures) ...[p.image, ?p.video], for (final t in music) t.file]) {
      if (!File(project.path(name)).existsSync()) throw const ShowProjectException('broken');
    }
    return project;
  }

  /// A name of a file of the project: below `pictures/`, `videos/` or `music/`, no way out of the folder.
  static String? _safe(Object? name) {
    if (name is! String) return null;
    return RegExp(r'^(pictures|videos|music)/[A-Za-z0-9_.-]+$').hasMatch(name) && !name.contains('..') ? name : null;
  }

  final Directory dir;
  final String title;
  final DateTime madeAt;
  final ShowSettings settings;
  final List<ShowPicture> pictures;
  final List<ShowTrack> music;

  /// Played from the server ([ShowProject.remote]); null for a project on this device.
  final ShowRemote? remote;

  /// For a project put together on the server: its id and the mark of the version this copy is of (`rev`); null otherwise.
  final int? serverId;
  final String? serverRev;

  /// The id of the project on this device: the name of its folder.
  String get id => dir.uri.pathSegments.where((s) => s.isNotEmpty).last;

  /// The path of a file of the project.
  String path(String name) => '${dir.path}/$name';

  /// How many bytes the project takes on this device.
  int get bytes {
    var n = 0;
    for (final f in dir.listSync(recursive: true).whereType<File>()) {
      n += f.lengthSync();
    }
    return n;
  }
}

/// The slideshows on this device: `<app folder>/slideshows/<id>/`, each an unpacked project.
class ShowProjects {
  ShowProjects({Directory? root}) : root = root ?? Directory('${AppPaths.base.path}/slideshows');

  final Directory root;

  /// The projects, the newest first; a folder that is not a whole project (an import broken off) is left out.
  Future<List<ShowProject>> list() async {
    if (!root.existsSync()) return [];
    final out = <ShowProject>[];
    for (final d in root.listSync().whereType<Directory>()) {
      if (d.uri.pathSegments.where((s) => s.isNotEmpty).last.startsWith('.')) continue;
      try {
        out.add(await ShowProject.open(d));
      } on ShowProjectException {
        // Not playable here; it stays on the disk until it is deleted.
      }
    }
    out.sort((a, b) => b.madeAt.compareTo(a.madeAt));
    return out;
  }

  /// Unpacks a project file (a ZIP) into a folder of its own and checks it; the file itself is left as it is. A file that is no project
  /// leaves nothing behind.
  Future<ShowProject> import(File zip) async {
    await root.create(recursive: true);
    final id = 'show-${DateTime.now().microsecondsSinceEpoch}';
    final temp = Directory('${root.path}/.$id');
    try {
      final (input, output) = (zip.path, temp.path);
      // Unpacking reads and writes every byte: in an isolate of its own, so the screen keeps moving.
      await Isolate.run(() => extractFileToDisk(input, output, throwOnError: true));
      await ShowProject.open(temp);
      final target = Directory('${root.path}/$id');
      await temp.rename(target.path);
      return await ShowProject.open(target);
    } on ShowProjectException {
      if (temp.existsSync()) await temp.delete(recursive: true);
      rethrow;
    } on Object catch (e) {
      if (temp.existsSync()) await temp.delete(recursive: true);
      // No room left on the device (ENOSPC, 28 on Linux, Android, macOS and iOS).
      if (e is FileSystemException && e.osError?.errorCode == 28) throw const ShowProjectException('space');
      throw const ShowProjectException('broken');
    }
  }

  /// Notes in [project] which project of the server it is a copy of, and of which version.
  Future<ShowProject> markServer(ShowProject project, int id, String rev) async {
    await File('${project.dir.path}/$kShowServerFile').writeAsString(jsonEncode({'id': id, 'rev': rev}));
    return ShowProject.open(project.dir);
  }

  Future<void> delete(ShowProject project) async {
    // Only a folder of this place is deleted.
    if (project.dir.parent.path != root.path) return;
    if (project.dir.existsSync()) await project.dir.delete(recursive: true);
  }

  /// Folders left by an import that was broken off (the app was closed while it unpacked).
  Future<void> tidy() async {
    if (!root.existsSync()) return;
    for (final d in root.listSync().whereType<Directory>()) {
      final name = d.uri.pathSegments.where((s) => s.isNotEmpty).last;
      // Also what a show played from the server fetched, when the app was closed meanwhile.
      if (name.startsWith('.show-') || name.startsWith('.stream-')) await d.delete(recursive: true);
    }
  }
}
