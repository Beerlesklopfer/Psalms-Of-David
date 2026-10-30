// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/podcasts/download_machine.dart';

const _web = Download(webAddress: true, onDisk: false);
const _played = Download(webAddress: true, onDisk: true);
const _bad = Download(webAddress: false, onDisk: false);

void main() {
  group('transitions', () {
    final rows = <(String, DownloadState, DownloadEvent, Type, List<DownloadEffect>)>[
      ('NotDownloaded Download', const NotDownloaded(), _web, Queued, const [StartFetch()]),
      ('NotDownloaded Download, played', const NotDownloaded(), _played, Downloaded, const [Register(), Notify()]),
      ('NotDownloaded Download, bad address', const NotDownloaded(), _bad, Failed, const []),
      ('Failed Download retries', const Failed('network'), _web, Queued, const [StartFetch()]),
      ('Cancelled Download retries', const Cancelled(), _web, Queued, const [StartFetch()]),
      ('Downloaded Download, file there', const Downloaded(), _played, Downloaded, const [Register(), Notify()]),
      ('Downloaded Download, file gone', const Downloaded(), _web, Queued, const [StartFetch()]),
      ('Queued FetchStarted', const Queued(), const FetchStarted(), Downloading, const [Notify()]),
      ('Queued Cancel', const Queued(), const Cancel(), Cancelled, const [CancelFetch(), Notify()]),
      ('Downloading Progress', const Downloading(), const Progress(5, 10), Downloading, const [Notify(force: false)]),
      ('Downloading Cancel', const Downloading(received: 3), const Cancel(), Cancelling, const [CancelFetch()]),
      ('Downloading Done', const Downloading(), const Done(10), Downloaded, const [Register(), Notify()]),
      ('Downloading IoError', const Downloading(), const IoError('http'), Failed, const [Notify()]),
      ('Downloading IoError cancelled', const Downloading(), const IoError('cancelled'), Cancelled, const [Notify()]),
      ('Cancelling Progress', const Cancelling(), const Progress(5, 10), Cancelling, const [Notify(force: false)]),
      ('Cancelling IoError cancelled', const Cancelling(), const IoError('cancelled'), Cancelled, const [Notify()]),
      ('Cancelling Done (finished anyway)', const Cancelling(), const Done(1), Downloaded, const [Register(), Notify()]),
      ('Cancelling IoError other', const Cancelling(), const IoError('io'), Failed, const [Notify()]),
      ('Downloaded Delete', const Downloaded(), const Delete(), Deleting, const [DeleteFile()]),
      ('Downloaded DeleteAfterListening on', const Downloaded(), const DeleteAfterListening(enabled: true), Deleting, const [DeleteFile()]),
      ('Deleting Deleted', const Deleting(), const Deleted(), NotDownloaded, const [Notify()]),
      ('Failed Delete clears', const Failed('io'), const Delete(), NotDownloaded, const [Notify()]),
      ('Cancelled Delete clears', const Cancelled(), const Delete(), NotDownloaded, const [Notify()]),
      ('NotDownloaded FileFound', const NotDownloaded(), const FileFound(), Downloaded, const []),
      ('Downloaded FileGone', const Downloaded(), const FileGone(), NotDownloaded, const []),
    ];
    for (final (name, from, event, to, effects) in rows) {
      test(name, () {
        final r = transition(from, event);
        expect(r.$1.runtimeType, to);
        expect(r.$2, effects);
      });
    }
  });

  test('payloads', () {
    expect((transition(const NotDownloaded(), _bad).$1 as Failed).reason, 'scheme');
    expect((transition(const Downloading(), const IoError('too_large')).$1 as Failed).reason, 'too_large');
    final d = transition(const Downloading(), const Progress(5, 10)).$1 as Downloading;
    expect((d.received, d.total), (5, 10));
    final c = transition(const Downloading(received: 3, total: 9), const Cancel()).$1 as Cancelling;
    expect((c.received, c.total), (3, 9));
  });

  group('illegal events are explicit no-ops', () {
    const events = <DownloadEvent>[
      _web, Cancel(), Delete(), DeleteAfterListening(enabled: true), FetchStarted(), Progress(1, 2), Done(1), IoError('io'), Deleted(), FileFound(), FileGone(),
    ];
    // The event types each state reacts to.
    final handled = <String, (DownloadState, Set<Type>)>{
      'NotDownloaded': (const NotDownloaded(), {Download, FileFound}),
      'Queued': (const Queued(), {Cancel, FetchStarted, Done, IoError}),
      'Downloading': (const Downloading(), {Cancel, Progress, Done, IoError}),
      'Cancelling': (const Cancelling(), {Progress, Done, IoError}),
      'Downloaded': (const Downloaded(), {Download, Delete, DeleteAfterListening, FileGone}),
      'Failed': (const Failed('io'), {Download, Delete, FileFound}),
      'Cancelled': (const Cancelled(), {Download, Delete, FileFound}),
      'Deleting': (const Deleting(), {Deleted, FileGone}),
    };
    handled.forEach((name, v) {
      test(name, () {
        for (final e in events.where((e) => !v.$2.contains(e.runtimeType))) {
          final r = transition(v.$1, e);
          expect(r.$1, same(v.$1), reason: '${e.runtimeType}');
          expect(r.$2, isEmpty, reason: '${e.runtimeType}');
        }
      });
    });
    test('DeleteAfterListening off keeps the episode', () {
      const s = Downloaded();
      final r = transition(s, const DeleteAfterListening(enabled: false));
      expect(r.$1, same(s));
      expect(r.$2, isEmpty);
    });
  });
}
