// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:archive/archive.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/pictures/show_api.dart';
import 'package:psalm/pictures/show_project.dart';

/// A project as the server makes it (server/src/show_project.rs), with [show] as its show.json and the files that are named.
File zipOf(Directory dir, Map<String, Object?> show, {Map<String, List<int>> files = const {}, String name = 'p.zip'}) {
  final a = Archive();
  final json = utf8.encode(jsonEncode(show));
  a.add(ArchiveFile('show.json', json.length, json));
  for (final e in files.entries) {
    a.add(ArchiveFile(e.key, e.value.length, e.value));
  }
  final f = File('${dir.path}/$name');
  f.writeAsBytesSync(ZipEncoder().encode(a));
  return f;
}

Map<String, Object?> showJson({int version = 1, List<Map<String, Object?>>? pictures, List<Map<String, Object?>>? music}) => {
      'format': 'david-slideshow',
      'version': version,
      'title': 'Bilder 2017–2019',
      'lang': 'de',
      'made_at': DateTime(2026, 11, 8).millisecondsSinceEpoch,
      'settings': {'order': 'random', 'seconds': 15, 'transition': 'zoom', 'videos': 'on', 'info': 'both'},
      'pictures': pictures ??
          [
            {'image': 'pictures/0001.jpg', 'video': null, 'taken_at': DateTime(2017, 7, 1).millisecondsSinceEpoch, 'place': 'Oberstdorf, Deutschland'},
            {'image': 'pictures/0002.jpg', 'video': 'videos/0002.mp4', 'taken_at': DateTime(2019, 1, 2).millisecondsSinceEpoch, 'place': null},
          ],
      'music': music ?? [{'file': 'music/01.mp3', 'title': 'Air', 'artist': 'Bach'}],
    };

const files = {'pictures/0001.jpg': [1, 2, 3], 'pictures/0002.jpg': [4], 'videos/0002.mp4': [5, 6], 'music/01.mp3': [7]};

void main() {
  late Directory dir;
  late ShowProjects projects;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('show');
    projects = ShowProjects(root: Directory('${dir.path}/slideshows'));
  });
  tearDown(() => dir.deleteSync(recursive: true));

  test('a project file is unpacked into a folder of its own, read and listed; the file itself stays', () async {
    final zip = zipOf(dir, showJson(), files: files);
    final p = await projects.import(zip);
    expect(zip.existsSync(), isTrue);
    expect(p.title, 'Bilder 2017–2019');
    expect((p.settings.order, p.settings.seconds, p.settings.transition, p.settings.info), ('random', 15, 'zoom', 'both'));
    expect(p.pictures.map((e) => e.image), ['pictures/0001.jpg', 'pictures/0002.jpg']);
    expect(p.pictures[0].place, 'Oberstdorf, Deutschland');
    expect(p.pictures[1].video, 'videos/0002.mp4');
    expect(p.pictures[1].takenAt, DateTime(2019, 1, 2));
    expect(p.music.single.title, 'Air');
    expect(File(p.path('videos/0002.mp4')).readAsBytesSync(), [5, 6]);
    expect(p.dir.parent.path, projects.root.path);
    expect(p.bytes, greaterThan(7));

    final listed = await projects.list();
    expect(listed.map((e) => e.id), [p.id]);
    await projects.delete(p);
    expect(await projects.list(), isEmpty);
    expect(p.dir.existsSync(), isFalse);
  });

  test('a file that is no project, a newer one and one that misses a file leave nothing behind', () async {
    final notZip = File('${dir.path}/x.zip')..writeAsStringSync('not a zip');
    await expectLater(projects.import(notZip), throwsA(isA<ShowProjectException>().having((e) => e.reason, 'reason', 'broken')));
    await expectLater(projects.import(zipOf(dir, showJson(version: 2), files: files, name: 'n.zip')),
        throwsA(isA<ShowProjectException>().having((e) => e.reason, 'reason', 'newer')));
    final missing = {...files}..remove('videos/0002.mp4');
    await expectLater(projects.import(zipOf(dir, showJson(), files: missing, name: 'm.zip')),
        throwsA(isA<ShowProjectException>().having((e) => e.reason, 'reason', 'broken')));
    await expectLater(projects.import(zipOf(dir, {'format': 'other', 'version': 1, 'pictures': []}, name: 'o.zip')),
        throwsA(isA<ShowProjectException>().having((e) => e.reason, 'reason', 'broken')));
    expect(projects.root.listSync(), isEmpty, reason: 'no half-unpacked folder stays');
  });

  test('a name in show.json cannot lead out of the folder of the project', () async {
    final evil = showJson(pictures: [
      {'image': '../../secret.jpg', 'taken_at': 0},
    ], music: []);
    await expectLater(projects.import(zipOf(dir, evil, files: files)), throwsA(isA<ShowProjectException>()));
    final odd = showJson(pictures: [
      {'image': 'pictures/0001.jpg', 'video': '/etc/passwd', 'taken_at': 0},
    ], music: [
      {'file': 'music/../../x.mp3', 'title': 'x'},
    ]);
    final p = await projects.import(zipOf(dir, odd, files: files, name: 'odd.zip'));
    expect(p.pictures.single.video, isNull, reason: 'a video outside the folder is not played');
    expect(p.music, isEmpty);
  });

  test('a choice that is not one of the page falls back to its default', () {
    final s = ShowSettings.fromJson({'order': 'x', 'seconds': 4, 'transition': '<b>', 'videos': 'maybe', 'info': 'all'});
    expect((s.order, s.seconds, s.transition, s.videos, s.info), ('date', 8, 'fade', 'on', 'both'));
  });

  test('the query of a project is the query of the list and the choices', () {
    final q = const ShowChoice(sources: ['own', 'f:Allgäu'], fromYear: 2017, toYear: 2019, choices: {'order': 'random', 'videos': 'off'}, music: 3).query();
    expect(q['source'], ['own', 'f:Allgäu']);
    expect(q['from'], '${DateTime(2017).millisecondsSinceEpoch}');
    expect(q['to'], '${DateTime(2020).millisecondsSinceEpoch - 1}');
    expect(q.containsKey('videos') ? q['videos'] : null, 'off', reason: 'without videos the list leaves them out');
    expect((q['order'], q['music']), ('random', '3'));
  });

  test('folders of an import that was broken off are tidied away', () async {
    Directory('${projects.root.path}/.show-1').createSync(recursive: true);
    await projects.tidy();
    expect(projects.root.listSync(), isEmpty);
  });
}
