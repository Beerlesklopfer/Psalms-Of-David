// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import '../server/app_paths.dart';

/// Which pictures of the device are looked at.
const kPhotoSources = ['camera', 'all'];

/// How many errors the device keeps to show.
const kPhotoErrorsKept = 10;

/// A picture the backup could not send, for the list on the screen.
class PhotoError {
  PhotoError(this.at, this.code, [this.name]);

  factory PhotoError.fromJson(Map<String, dynamic> j) =>
      PhotoError(DateTime.fromMillisecondsSinceEpoch(j['at'] as int? ?? 0), j['code'] as String? ?? 'error', j['name'] as String?);

  final DateTime at;

  /// The code of the server (`too_large`, `quota_full`, ...) or of the device (`network`, `unreadable`, `no_access`).
  final String code;
  final String? name;

  Map<String, Object?> toJson() => {'at': at.millisecondsSinceEpoch, 'code': code, 'name': ?name};
}

/// What the device knows of one of its pictures: the hash (read once), and whether the server has it or refused it for good.
class PhotoEntry {
  PhotoEntry(this.sha256, this.size, {this.done = false, this.skipped});

  factory PhotoEntry.fromJson(Map<String, dynamic> j) =>
      PhotoEntry(j['s'] as String, j['n'] as int, done: j['d'] == true, skipped: j['k'] as String?);

  final String sha256;
  final int size;

  /// The server has the file.
  bool done;

  /// The server refused it for a reason that stays (too large, a type it does not take): the rules of the server then
  /// (`<max_file_mb>/<videos>`); it is not tried again until they change.
  String? skipped;

  Map<String, Object?> toJson() => {'s': sha256, 'n': size, if (done) 'd': true, 'k': ?skipped};
}

/// The settings of the backup on this device (`docs/spec-photo-sync.md`, section 2a "Device"). Only the screen writes them, in a file of
/// their own, so a run in the background never puts back an older choice.
class PhotoDeviceSettings {
  PhotoDeviceSettings({this.enabled = false, this.since, this.olderToo = false, this.sources = 'camera'});

  factory PhotoDeviceSettings.fromJson(Map<String, dynamic> j) => PhotoDeviceSettings(
        enabled: j['enabled'] == true,
        since: j['since'] is int ? DateTime.fromMillisecondsSinceEpoch(j['since'] as int) : null,
        olderToo: j['older_too'] == true,
        sources: kPhotoSources.contains(j['sources']) ? j['sources'] as String : 'camera',
      );

  /// This device takes part (asked once; off until the person switches it on).
  bool enabled;

  /// Only pictures taken from then on: the day the device was first switched on (an old gallery is not sent by accident).
  DateTime? since;

  /// Also the pictures before [since] (the explicit button "also the older ones").
  bool olderToo;

  /// `camera` (the camera folder) or `all` (every picture of the device).
  String sources;

  /// Where the run starts: nothing before the device was switched on, unless the older ones were asked for.
  DateTime? get from => olderToo ? null : since;

  Map<String, Object?> toJson() =>
      {'enabled': enabled, 'since': ?since?.millisecondsSinceEpoch, if (olderToo) 'older_too': true, 'sources': sources};
}

/// What the backup has done on this device, in a small file of the app that only a run writes (one at a time, see [PhotoDeviceStore.tryLock]).
class PhotoDeviceState {
  PhotoDeviceState({Map<String, PhotoEntry>? entries, List<PhotoError>? errors, this.lastRunAt, this.lastSentAt, this.waiting, this.constraints})
      : entries = entries ?? {},
        errors = errors ?? [];

  factory PhotoDeviceState.fromJson(Map<String, dynamic> j) => PhotoDeviceState(
        entries: {
          if (j['entries'] is Map)
            for (final e in (j['entries'] as Map).entries)
              if (e.value is Map) '${e.key}': PhotoEntry.fromJson((e.value as Map).cast<String, dynamic>()),
        },
        errors: [
          if (j['errors'] is List)
            for (final e in j['errors'] as List)
              if (e is Map) PhotoError.fromJson(e.cast<String, dynamic>()),
        ],
        lastRunAt: j['last_run'] is int ? DateTime.fromMillisecondsSinceEpoch(j['last_run'] as int) : null,
        lastSentAt: j['last_sent'] is int ? DateTime.fromMillisecondsSinceEpoch(j['last_sent'] as int) : null,
        waiting: j['waiting'] as int?,
        constraints: j['constraints'] as String?,
      );

  /// Per picture of the device (`<id>@<modified>`, so an edited picture is read again) what is known of it.
  final Map<String, PhotoEntry> entries;

  /// The last errors, newest first.
  final List<PhotoError> errors;

  DateTime? lastRunAt;

  /// When the last picture reached the server from this device.
  DateTime? lastSentAt;

  /// How many pictures waited after the last run.
  int? waiting;

  /// The conditions the background job was registered with, to register it again only when they change.
  String? constraints;

  void addError(PhotoError e) {
    errors.insert(0, e);
    if (errors.length > kPhotoErrorsKept) errors.removeRange(kPhotoErrorsKept, errors.length);
  }

  Map<String, Object?> toJson() => {
        'entries': {for (final e in entries.entries) e.key: e.value.toJson()},
        'errors': [for (final e in errors) e.toJson()],
        'last_run': ?lastRunAt?.millisecondsSinceEpoch,
        'last_sent': ?lastSentAt?.millisecondsSinceEpoch,
        'waiting': ?waiting,
        'constraints': ?constraints,
      };
}

/// The files of [PhotoDeviceSettings] and [PhotoDeviceState]. Written through a new file that is renamed over the old one, so a run that is stopped in the middle
/// leaves the last whole state.
class PhotoDeviceStore {
  PhotoDeviceStore(this.file, this.settingsFile);

  factory PhotoDeviceStore.standard() =>
      PhotoDeviceStore(File('${AppPaths.base.path}/photo_backup.json'), File('${AppPaths.base.path}/photo_device.json'));

  final File file;
  final File settingsFile;

  static Map<String, dynamic>? _read(File f) {
    try {
      return jsonDecode(f.readAsStringSync()) as Map<String, dynamic>;
    } on Object {
      return null;
    }
  }

  static void _write(File f, Object json) {
    try {
      f.parent.createSync(recursive: true);
      final temp = File('${f.path}.new');
      temp.writeAsStringSync(jsonEncode(json), flush: true);
      temp.renameSync(f.path);
    } on FileSystemException {
      // The next run reads the pictures again; the server says what it has.
    }
  }

  PhotoDeviceState load() => switch (_read(file)) { final j? => PhotoDeviceState.fromJson(j), null => PhotoDeviceState() };

  void save(PhotoDeviceState s) => _write(file, s.toJson());

  PhotoDeviceSettings loadSettings() => switch (_read(settingsFile)) { final j? => PhotoDeviceSettings.fromJson(j), null => PhotoDeviceSettings() };

  void saveSettings(PhotoDeviceSettings s) => _write(settingsFile, s.toJson());

  /// One run at a time on this device (the app and the background job): a lock file, taken over when it is older than [stale].
  File get _lock => File('${file.path}.lock');

  bool tryLock({Duration stale = const Duration(minutes: 30)}) {
    try {
      file.parent.createSync(recursive: true);
      _lock.createSync(exclusive: true);
      return true;
    } on FileSystemException {
      try {
        if (DateTime.now().difference(_lock.lastModifiedSync()) > stale) {
          _lock.setLastModifiedSync(DateTime.now());
          return true;
        }
      } on FileSystemException {
        // gone in between: try once more next time
      }
      return false;
    }
  }

  /// Keeps the lock fresh during a long run.
  void touchLock() {
    try {
      _lock.setLastModifiedSync(DateTime.now());
    } on FileSystemException {
      // nothing to do
    }
  }

  void unlock() {
    try {
      _lock.deleteSync();
    } on FileSystemException {
      // already gone
    }
  }
}
