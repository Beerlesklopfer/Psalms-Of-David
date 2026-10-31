// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/inbox/inbox_models.dart';
import 'package:psalm/inbox/inbox_text.dart';
import 'package:psalm/l10n/app_localizations.dart';

const _args = <String, dynamic>{
  'job_id': 42,
  'author': 'Ken Follett',
  'title': 'Sturz der Titanen',
  'disc': 2,
  'discs_total': 12,
  'tracks': 14,
  'by': 'Anna',
  'approved': true,
  'album_id': 7,
  'candidates': 3,
};

InboxItem item(
  String code, {
  Map<String, dynamic> args = _args,
  bool canDecide = false,
  String kind = 'notice',
}) =>
    InboxItem(id: 1, kind: kind, code: code, args: args, canDecide: canDecide);

void main() {
  late AppLocalizations en;
  late AppLocalizations de;

  setUp(() async {
    en = await AppLocalizations.delegate.load(const Locale('en'));
    de = await AppLocalizations.delegate.load(const Locale('de'));
  });

  test(
    'every code has a text in English and in German, and no raw code shows',
    () {
      for (final code in knownInboxCodes) {
        final i = item(
          code,
          canDecide: code == 'cd_awaiting_approval',
          kind: code == 'cd_awaiting_approval' ? 'task' : 'notice',
        );
        final e = inboxText(en, i);
        final d = inboxText(de, i);
        expect(e, isNotEmpty, reason: code);
        expect(d, isNotEmpty, reason: code);
        expect(e, isNot(en.inboxUnknown), reason: code);
        expect(d, isNot(de.inboxUnknown), reason: code);
        expect(e, isNot(contains('cd_')), reason: code);
        expect(d, isNot(contains('cd_')), reason: code);
        expect(e, isNot(d), reason: '$code is translated');
      }
    },
  );

  test('the codes of the app are the codes of the spec', () {
    final spec = File('${Directory.current.path}/../docs/spec-app-api.md');
    if (!spec.existsSync()) return;
    final text = spec.readAsStringSync();
    final section = text.substring(text.indexOf('**Codes and `args`**'));
    final end = section.indexOf('\n\n', section.indexOf('| `cd_heal_needed`'));
    final table = section.substring(0, end < 0 ? section.length : end);
    final found = <String>{
      for (final row in table.split('\n').where((r) => r.startsWith('| `')))
        for (final m in RegExp(r'`(cd_[a-z_]+)`').allMatches(row.split('|')[1]))
          m.group(1)!,
    };
    expect(found, isNotEmpty);
    expect(
      found.difference(knownInboxCodes.toSet()),
      isEmpty,
      reason: 'a code of the spec without a text',
    );
    expect(
      knownInboxCodes.toSet().difference(found),
      isEmpty,
      reason: 'a text for a code the spec does not have',
    );
  });

  test('the texts carry what the args say', () {
    expect(
      inboxText(en, item('cd_imported')),
      'Ken Follett – Sturz der Titanen (CD 2 of 12) is in the library (14 tracks).',
    );
    expect(
      inboxText(de, item('cd_imported')),
      'Ken Follett – Sturz der Titanen (CD 2 von 12) ist in der Mediathek (14 Titel).',
    );
    expect(
      inboxText(
        en,
        item(
          'cd_read_error',
          args: {
            ..._args,
            'tracks': [3, 7],
          },
        ),
      ),
      contains('3, 7'),
    );
    expect(inboxText(en, item('cd_decided')), startsWith('Anna imported'));
    expect(
      inboxText(en, item('cd_decided', args: {..._args, 'approved': false})),
      startsWith('Anna rejected'),
    );
    expect(inboxText(en, item('cd_ambiguous')), contains('3 releases'));
    expect(
      inboxText(
        en,
        item('cd_awaiting_approval', kind: 'task', canDecide: true),
      ),
      startsWith('New CD from Anna'),
    );
    expect(
      inboxText(
        en,
        item(
          'cd_awaiting_approval',
          kind: 'task',
          canDecide: true,
          args: {..._args, 'by': 'auto'},
        ),
      ),
      contains('put in at the server'),
    );
    // The starter's own held CD: waits for approval.
    expect(
      inboxText(en, item('cd_awaiting_approval', kind: 'task')),
      contains('waits for approval'),
    );
  });

  test('missing or odd args give a shorter sentence, never an exception', () {
    for (final code in knownInboxCodes) {
      for (final args in <Map<String, dynamic>>[
        {},
        {
          'author': null,
          'title': 5,
          'disc': 'x',
          'tracks': 'many',
          'candidates': 'x',
          'by': null,
        },
      ]) {
        expect(inboxText(en, item(code, args: args)), isNotEmpty, reason: code);
        expect(inboxText(de, item(code, args: args)), isNotEmpty, reason: code);
      }
    }
  });

  test('an unknown code shows the generic text, also an empty one', () {
    expect(inboxText(en, item('cd_from_the_future')), en.inboxUnknown);
    expect(inboxText(de, item('')), de.inboxUnknown);
    expect(
      inboxText(en, item('cd_from_the_future')),
      isNot(contains('future')),
    );
  });

  test('the resolution line of a decided task', () {
    InboxItem done(String r, String by) => InboxItem(
      id: 1,
      kind: 'task',
      code: 'cd_awaiting_approval',
      resolved: true,
      resolution: r,
      by: by,
    );
    expect(inboxResolution(en, done('approved', 'Bea')), 'Imported by Bea');
    expect(inboxResolution(en, done('rejected', 'Bea')), 'Rejected by Bea');
    expect(inboxResolution(en, done('approved', '')), 'Imported');
    expect(inboxResolution(de, done('expired', '')), 'Abgelaufen');
    expect(inboxResolution(en, item('cd_imported')), isNull);
  });
}
