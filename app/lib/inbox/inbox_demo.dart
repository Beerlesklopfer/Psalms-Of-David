// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'inbox_models.dart';

/// The example inbox of "Try with demo data": a task, a notice and an expired CD, so that the card, the badge and the page show.
List<InboxItem> demoInboxItems({DateTime? now}) {
  final t = (now ?? DateTime.now()).millisecondsSinceEpoch;
  const hour = 3600 * 1000;
  return [
    InboxItem(
      id: 3,
      kind: 'task',
      code: 'cd_awaiting_approval',
      jobId: 30,
      createdMs: t - hour,
      canDecide: true,
      jobState: 'awaiting_approval',
      args: const {
        'job_id': 30,
        'author': 'Ken Follett',
        'title': 'Sturz der Titanen',
        'disc': 1,
        'discs_total': 12,
        'tracks': 14,
        'by': 'Anna',
      },
    ),
    InboxItem(
      id: 2,
      kind: 'notice',
      code: 'cd_imported',
      jobId: 20,
      createdMs: t - 5 * hour,
      args: const {
        'job_id': 20,
        'author': 'Johann Sebastian Bach',
        'title': 'Weihnachtsoratorium',
        'disc': 1,
        'discs_total': 2,
        'tracks': 12,
        'album_id': null,
      },
    ),
    InboxItem(
      id: 1,
      kind: 'notice',
      code: 'cd_expired',
      jobId: 10,
      createdMs: t - 70 * 24 * hour,
      read: true,
      args: const {
        'job_id': 10,
        'author': 'Theodor Storm',
        'title': 'Der Schimmelreiter',
        'disc': 1,
      },
    ),
  ];
}
