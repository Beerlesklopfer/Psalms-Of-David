// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/cd/cd_errors.dart';
import 'package:psalm/l10n/app_localizations.dart';

void main() {
  test(
    'every known code has a readable text in English and German, none shows the raw code',
    () async {
      final en = await AppLocalizations.delegate.load(const Locale('en'));
      final de = await AppLocalizations.delegate.load(const Locale('de'));
      final en_ = cdErrorTexts(en);
      final de_ = cdErrorTexts(de);
      expect(en_.keys, unorderedEquals(kCdErrorCodes));
      for (final c in kCdErrorCodes) {
        for (final t in [en_[c]!, de_[c]!]) {
          expect(t.trim(), isNotEmpty, reason: c);
          expect(
            c.contains('_') ? t.contains(c) : t == c,
            isFalse,
            reason: '$c is shown as words, not as a code',
          );
          expect(t.contains('cdErr'), isFalse);
        }
        expect(de_[c], isNot(en_[c]), reason: '$c is translated');
      }
      expect(
        en_.values.toSet().length,
        greaterThan(kCdErrorCodes.length - 4),
        reason: 'distinct texts (a few may share)',
      );
    },
  );

  test('an unknown code shows the general text with the code in brackets', () async {
    final en = await AppLocalizations.delegate.load(const Locale('en'));
    final de = await AppLocalizations.delegate.load(const Locale('de'));
    expect(cdErrorText(en, 'something_new'), 'The CD import failed (something_new).');
    expect(cdErrorText(de, 'something_new'), 'Der CD-Import ist fehlgeschlagen (something_new).');
  });

  test('every error code of the server interface in the spec is known', () {
    // The codes the spec names in backticks after "Errors" / `error` are read from the document itself.
    final spec = File('${Directory.current.path}/../docs/spec-cd-import-api.md');
    if (!spec.existsSync()) return;
    final text = spec.readAsStringSync();
    // The table "Error codes": every code in its first column is one an app may meet.
    final section = text.substring(text.indexOf('## Error codes'));
    final found = <String>{
      for (final row in section.split('\n').where((r) => r.startsWith('| `')))
        for (final m in RegExp(r'`([a-z_]+)`').allMatches(row.split('|')[1])) m.group(1)!,
    }..removeAll({'bad_upgrade'}); // the WebSocket upgrade error is not shown to the user
    final unknown = found.where((c) => !kCdErrorCodes.contains(c)).toList();
    expect(
      unknown,
      isEmpty,
      reason: 'new server codes need a text in app_en.arb, app_de.arb and kCdErrorCodes',
    );
  });
}
