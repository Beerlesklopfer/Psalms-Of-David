// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// What the About page says about who makes the app and whom to tell about a security problem,
// as the EU Cyber Resilience Act (CRA) asks of a product with digital elements. One place, so
// that the facts are changed here and nowhere else.

/// The version of the app; keep in step with `version:` in pubspec.yaml.
const appVersion = '1.0.0';

/// Who makes the app.
const manufacturerName = 'Jörg Bernau';

/// Where to report a vulnerability (also the single point of contact for users).
const securityContact = 'Joerg@bernau.family';

/// Until when security updates are given. `null` while the owner has not decided: the page then
/// says so openly and does not invent a date. Set it as `'2029-12-31'`.
const String? supportUntil = null;

/// Where the source code is: the AGPL asks that everyone who uses the program can get it.
const sourceUrl = 'https://github.com/Beerlesklopfer/Psalms-Of-David';

/// The licence of the app.
const licenseName = 'AGPL-3.0-or-later';
