// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:crypto/crypto.dart';

import '../server/api_client.dart';
import 'photo_backup_api.dart';
import 'photo_device.dart';

/// How many pictures are hashed and offered to the server at once (the server takes up to 1000 in one `have`).
const kPhotoBatch = 50;

/// The size of one piece of an upload (the server takes up to 32 MiB). Small enough that a piece goes through in the time limit of a
/// request also over a slow VPN.
const kPhotoPiece = 4 * 1024 * 1024;

/// The largest preview the server takes (`THUMB_MAX`).
const kPhotoThumbMax = 512 * 1024;

/// A picture or video of the device.
class LocalPhoto {
  LocalPhoto({required this.id, required this.name, required this.mime, required this.takenAt, required this.modified, required this.open, this.video = false, this.thumbnail});

  /// The id of the gallery of the device.
  final String id;
  final String name;
  final String mime;
  final DateTime takenAt;

  /// When the file last changed: an edited picture is a new one for the backup.
  final DateTime modified;
  final bool video;

  /// The file itself (the original, with its location data); null when it cannot be read.
  final Future<File?> Function() open;

  /// A small JPEG of it (also of a video), made by the gallery; sent with the picture for the other devices.
  final Future<List<int>?> Function()? thumbnail;

  String get key => '$id@${modified.millisecondsSinceEpoch ~/ 1000}';
}

/// The pictures of the device (Android: `MediaStore`; replaced in tests).
abstract class PhotoLibrary {
  /// The person allowed the app to read the pictures.
  Future<bool> hasAccess();

  /// Asks the person (the dialog of the system); true when allowed.
  Future<bool> requestAccess();

  /// The pictures taken since [since] (all with null), oldest first; videos only with [videos]; `camera` or `all` as [sources].
  Future<List<LocalPhoto>> list({DateTime? since, required bool videos, required String sources});

  /// Removes what [LocalPhoto.open] copied into the cache of the app.
  Future<void> cleanUp() async {}

  /// Puts a received original into the gallery (folder "David"); the new entry of the gallery, null when it could not be written.
  Future<LocalPhoto?> saveToGallery(File file, {required String name, required String mime, required DateTime takenAt}) async => null;
}

/// Why a run stopped before it was through, or did nothing.
enum PhotoRunStop {
  /// The server has the backup off, or the profile is a guest.
  serverOff,

  /// The backup of the profile is off, or it has no picture folder.
  inactive,

  /// This device was not switched on.
  deviceOff,

  /// The app may not read the pictures.
  noAccess,

  /// The server could not be reached, or the connection broke.
  network,

  /// The server is full, or took the backup away in the middle (`quota_full`, `disk_full`, `backup_off`, `no_folder`).
  refused,

  /// The person paused it, or the system ended the background job.
  stopped,

  /// Another run of this device is busy.
  busy,
}

/// What a run did.
class PhotoRunResult {
  PhotoRunResult({this.sent = 0, this.known = 0, this.skipped = 0, this.waiting = 0, this.received = 0, this.stop, this.settings});

  /// Sent in this run.
  int sent;

  /// Found on the server already (from another device, or an earlier run that was not marked).
  int known;

  /// Refused for a reason that stays (too large, a type the server does not take).
  int skipped;

  /// Still to send.
  int waiting;

  /// Received from the other devices of the person (bisync).
  int received;

  PhotoRunStop? stop;

  /// The settings the run worked with (to register the background job with its conditions).
  PhotoSettings? settings;

  @override
  String toString() => 'PhotoRunResult(sent $sent, known $known, skipped $skipped, waiting $waiting, received $received, stop $stop)';
}

/// Codes of the server that refuse one file for good, as long as the rules stay: the file is not offered again.
const _skipCodes = {'too_large', 'wrong_type'};

/// Codes that end the run: nothing else would get through either.
const _stopCodes = {'quota_full', 'disk_full', 'backup_off', 'no_folder', 'bad_request'};

/// One run of the backup (`docs/spec-photo-sync.md`, section 5): the pictures since the day it was switched on → their hashes (read once) →
/// `have` → the missing ones in pieces, each going on where the server is → marked done. The SHA-256 of the file is its upload id, so an
/// upload broken off by a lost connection or a stopped job goes on in the next run where it was, and two devices with the same picture send
/// it once.
class PhotoSync {
  PhotoSync({required this.api, required this.library, required this.store, this.now = DateTime.now, this.onProgress});

  final PhotoBackupApi api;
  final PhotoLibrary library;
  final PhotoDeviceStore store;
  final DateTime Function() now;

  /// Called after each picture: how many are done of how many this run looks at.
  final void Function(int done, int total)? onProgress;

  bool _stop = false;

  /// Ends the run after the current piece (the pause button, or the system ending the job).
  void stop() => _stop = true;

  Future<PhotoRunResult> run() async {
    if (!store.tryLock()) return PhotoRunResult(stop: PhotoRunStop.busy);
    final state = store.load();
    try {
      final r = await _run(state);
      // What waits is known once the gallery was looked at, also when the run stopped in the middle.
      if (r.settings != null && (r.stop == null || r.stop == PhotoRunStop.stopped || r.stop == PhotoRunStop.network || r.stop == PhotoRunStop.refused)) {
        state.waiting = r.waiting;
      }
      return r;
    } finally {
      state.lastRunAt = now();
      store.save(state);
      store.unlock();
      await library.cleanUp();
    }
  }

  Future<PhotoRunResult> _run(PhotoDeviceState state) async {
    final device = store.loadSettings();
    final PhotoSettings? settings;
    try {
      settings = await api.settings();
    } on ApiException catch (e) {
      return PhotoRunResult(stop: e.status == 0 ? PhotoRunStop.network : PhotoRunStop.refused);
    }
    if (settings == null) return PhotoRunResult(stop: PhotoRunStop.serverOff);
    final result = PhotoRunResult(settings: settings);
    if (!settings.active) return result..stop = PhotoRunStop.inactive;
    if (!device.enabled || (device.since == null && !device.olderToo)) return result..stop = PhotoRunStop.deviceOff;
    if (!await library.hasAccess()) return result..stop = PhotoRunStop.noAccess;

    final maxBytes = settings.maxFileMb * 1024 * 1024;
    // A refusal for good counts as long as the rules of the server are the same; a larger limit or videos switched on try again.
    final rules = '${settings.maxFileMb}/${settings.videosOn}';
    final photos = await library.list(since: device.from, videos: settings.videosOn, sources: device.sources);
    final todo = photos.where((p) {
      final e = state.entries[p.key];
      return e == null || (!e.done && e.skipped != rules);
    }).toList();
    result.waiting = todo.length;
    var done = 0;
    onProgress?.call(done, todo.length);

    for (var i = 0; i < todo.length; i += kPhotoBatch) {
      if (_stop) return result..stop = PhotoRunStop.stopped;
      final batch = todo.sublist(i, (i + kPhotoBatch).clamp(0, todo.length));
      // The hashes, read once per picture: a long video is not read again in the next run.
      final ready = <LocalPhoto>[];
      for (final p in batch) {
        if (_stop) return result..stop = PhotoRunStop.stopped;
        var entry = state.entries[p.key];
        if (entry == null) {
          final file = await p.open();
          if (file == null || !file.existsSync()) {
            state.addError(PhotoError(now(), 'unreadable', p.name));
            continue;
          }
          final size = await file.length();
          if (size == 0) continue;
          entry = PhotoEntry((await sha256.bind(file.openRead()).first).toString(), size);
          state.entries[p.key] = entry;
          store.touchLock();
        }
        if (entry.size > maxBytes) {
          entry.skipped = rules;
          state.addError(PhotoError(now(), 'too_large', p.name));
          result.skipped++;
          result.waiting--;
          continue;
        }
        ready.add(p);
      }
      store.save(state);
      if (ready.isEmpty) continue;

      final Set<String> missing;
      try {
        missing = await api.missing([for (final p in ready) (sha256: state.entries[p.key]!.sha256, size: state.entries[p.key]!.size)]);
      } on ApiException catch (e) {
        return result..stop = _stopOf(e, state);
      }
      for (final p in ready) {
        if (_stop) return result..stop = PhotoRunStop.stopped;
        final entry = state.entries[p.key]!;
        if (!missing.contains(entry.sha256)) {
          entry.done = true;
          result.known++;
        } else {
          final PieceAnswer answer;
          try {
            answer = await _upload(p, entry);
          } on ApiException catch (e) {
            store.save(state);
            return result..stop = _stopOf(e, state);
          } on FileSystemException {
            state.addError(PhotoError(now(), 'unreadable', p.name));
            continue;
          }
          switch (answer) {
            case PieceDone(:final photo, :final known):
              entry.done = true;
              known ? result.known++ : result.sent++;
              state.lastSentAt = now();
              if (!known) await _sendThumb(p, photo);
            case PieceRefused(:final code) when _skipCodes.contains(code):
              entry.skipped = rules;
              state.addError(PhotoError(now(), code, p.name));
              result.skipped++;
            case PieceRefused(code: 'hash_mismatch'):
              // The file changed after it was read: the next run reads it again.
              state.entries.remove(p.key);
              state.addError(PhotoError(now(), 'hash_mismatch', p.name));
              continue;
            case PieceRefused(:final code):
              state.addError(PhotoError(now(), code, p.name));
              store.save(state);
              return result..stop = _stopCodes.contains(code) ? PhotoRunStop.refused : PhotoRunStop.stopped;
            case PiecePartial():
              // stopped in the middle of the file: it goes on next time
              store.save(state);
              return result..stop = PhotoRunStop.stopped;
          }
        }
        result.waiting--;
        onProgress?.call(++done, todo.length);
        store.save(state);
      }
    }
    await _receive(state, device, settings, result);
    return result;
  }

  /// The preview of a picture that was just sent, for the other devices. Without one they show a placeholder: it is no reason to stop.
  Future<void> _sendThumb(LocalPhoto p, int photo) async {
    try {
      final jpeg = await p.thumbnail?.call();
      if (jpeg != null && jpeg.isNotEmpty && jpeg.length <= kPhotoThumbMax) await api.putThumb(photo, jpeg);
    } on ApiException {
      // the picture is kept; the preview is missing
    }
  }

  /// Bisync: the pictures the other devices of the person sent, from the day this device was switched on (or all of them with "also the
  /// older ones"), as previews in the app or as originals in the gallery. A picture the device holds itself is not fetched.
  Future<void> _receive(PhotoDeviceState state, PhotoDeviceSettings device, PhotoSettings settings, PhotoRunResult result) async {
    if (settings.effective['mode'] != 'bi' || device.receive == 'off') return;
    if (device.olderToo && !state.receivedOlder) {
      state.changesSince = 0;
      state.receivedOlder = true;
    }
    final mine = {for (final e in state.entries.values) e.sha256};
    final have = {for (final r in state.received) r.id};
    final from = device.from;
    while (true) {
      if (_stop) {
        result.stop = PhotoRunStop.stopped;
        return;
      }
      final ({List<RemotePhoto> photos, int next}) page;
      try {
        page = await api.changes(state.changesSince);
      } on ApiException catch (e) {
        if (e.code != 'not_bisync') result.stop = _stopOf(e, state);
        return;
      }
      for (final r in page.photos) {
        if (_stop) {
          result.stop = PhotoRunStop.stopped;
          return;
        }
        final wanted = !have.contains(r.id) &&
            !mine.contains(r.sha256) &&
            (from == null || !r.takenAt.isBefore(from)) &&
            (settings.videosOn || !r.video);
        if (wanted) {
          final rec = ReceivedPhoto(id: r.id, sha256: r.sha256, size: r.size, name: r.name, mime: r.mime, takenAt: r.takenAt);
          try {
            if (device.receive == 'gallery') {
              final incoming = File('${store.receivedDir.path}/incoming-${r.id}');
              try {
                await api.download(r.id, incoming, r.sha256);
                final saved = await library.saveToGallery(incoming, name: r.name, mime: r.mime, takenAt: r.takenAt);
                if (saved == null) {
                  state.addError(PhotoError(now(), 'unwritable', r.name));
                } else {
                  // The copy in the gallery is known: the backup does not read it again.
                  state.entries[saved.key] = PhotoEntry(r.sha256, r.size, done: true);
                  mine.add(r.sha256);
                  rec.gallery = true;
                }
              } finally {
                if (incoming.existsSync()) incoming.deleteSync();
              }
            }
            if (r.thumb) {
              final jpeg = await api.thumb(r.id);
              if (jpeg != null) {
                store.thumbFile(r.id)
                  ..parent.createSync(recursive: true)
                  ..writeAsBytesSync(jpeg, flush: true);
                rec.thumb = true;
              }
            }
          } on ApiException catch (e) {
            result.stop = _stopOf(e, state);
            store.save(state);
            return;
          } on IOException {
            // A cut-off or broken download: the same picture next time (the cursor stays before it).
            state.addError(PhotoError(now(), 'network', r.name));
            result.stop = PhotoRunStop.network;
            store.save(state);
            return;
          }
          state.received.add(rec);
          have.add(r.id);
          result.received++;
        }
        state.changesSince = r.id;
        store.save(state);
        store.touchLock();
      }
      state.changesSince = page.next;
      store.save(state);
      if (page.photos.isEmpty) return;
    }
  }

  PhotoRunStop _stopOf(ApiException e, PhotoDeviceState state) {
    state.addError(PhotoError(now(), e.code));
    return e.status == 0 ? PhotoRunStop.network : PhotoRunStop.refused;
  }

  /// Sends the file in pieces from where the server is, until it is complete or refused.
  Future<PieceAnswer> _upload(LocalPhoto p, PhotoEntry entry) async {
    final file = await p.open();
    if (file == null) throw const FileSystemException('the picture cannot be read');
    final raf = await file.open();
    try {
      final clientId = entry.sha256;
      var start = await api.received(clientId) ?? 0;
      while (true) {
        if (_stop) return PiecePartial(start);
        await raf.setPosition(start);
        final bytes = await raf.read(kPhotoPiece.clamp(0, entry.size - start));
        final answer = await api.putPiece(
          clientId: clientId,
          sha256: entry.sha256,
          size: entry.size,
          mime: p.mime,
          start: start,
          bytes: bytes,
          name: p.name,
          takenAt: p.takenAt,
        );
        store.touchLock();
        switch (answer) {
          case PiecePartial(:final received):
            start = received;
          case PieceRefused(code: 'wrong_offset', :final received?):
            start = received;
          case PieceDone() || PieceRefused():
            return answer;
        }
      }
    } finally {
      await raf.close();
    }
  }
}
