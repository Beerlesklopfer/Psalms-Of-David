// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/audio/playback_settings.dart';
import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/inbox/inbox_controller.dart';
import 'package:psalm/inbox/inbox_models.dart';
import 'package:psalm/inbox/inbox_ui.dart';
import 'package:psalm/l10n/app_localizations.dart';
import 'package:psalm/profile.dart';
import 'package:psalm/speech/announce_section.dart';
import 'package:psalm/speech/announcer_scope.dart';
import 'package:psalm/speech/cd_announcer.dart';
import 'package:psalm/speech/speech_decision.dart';
import 'package:psalm/speech/speech_engine.dart';

import 'inbox_fakes.dart';

const _approver = CdRights(mayInsert: true, noApproval: true, mayApprove: true);
const _starter = CdRights(mayInsert: true);

SpeechContext _ctx({
  bool playing = true,
  String? own = 'dev1',
  CdRights rights = _approver,
  String role = 'member',
  bool enabled = true,
  bool atHome = true,
}) => SpeechContext(
  playing: playing,
  ownDevice: own,
  rights: rights,
  role: role,
  enabled: enabled,
  atHome: atHome,
);

SpeechSkip? _why(
  InboxItem item,
  String? speak,
  String? device,
  SpeechContext c, [
  Set<int> spoken = const {},
]) => whyNotSpoken(item, speak, device, c, spoken);

class FakeEngine implements SpeechEngine {
  FakeEngine({this.available = true, this.fail = false});
  final said = <String>[];
  final locales = <String?>[];
  Completer<void>? hold;
  bool fail;
  int stops = 0;
  @override
  bool available;
  @override
  String? get unavailableReason => available ? null : 'no_spd_say';
  @override
  Future<void> speak(String text, {String? locale}) async {
    said.add(text);
    locales.add(locale);
    if (hold != null) await hold!.future;
    if (fail) throw StateError('voice failed');
  }

  @override
  Future<void> stop() async => stops++;
}

class FakeDuck implements DuckTarget {
  FakeDuck(this.volume);
  @override
  double volume;
  final log = <double>[];
  @override
  void setVolume(double v) {
    volume = v;
    log.add(v);
  }
}

class FakeProc implements SpeechProcess {
  FakeProc(this.done);
  final Completer<int> done;
  bool killed = false;
  @override
  Future<int> get exitCode => done.future;
  @override
  void kill() {
    killed = true;
    if (!done.isCompleted) done.complete(-9);
  }
}

void main() {
  group('the decision', () {
    final msg = inboxNotice(5, code: 'cd_imported');
    test('a message for this device, while playing, with a right: spoken', () {
      expect(_why(msg, 'starter', 'dev1', _ctx()), isNull);
      expect(_why(msg, 'home', '', _ctx()), isNull);
    });
    test('starter: only the device whose name it is', () {
      expect(
        _why(msg, 'starter', 'other', _ctx()),
        SpeechSkip.notForThisDevice,
      );
      expect(
        _why(msg, 'starter', 'dev1', _ctx(own: null)),
        SpeechSkip.notForThisDevice,
      );
      expect(
        _why(msg, 'starter', '', _ctx(own: '')),
        SpeechSkip.notForThisDevice,
      );
    });
    test('home: needs may_approve and the home network', () {
      expect(_why(msg, 'home', '', _ctx(rights: _starter)), SpeechSkip.noRight);
      expect(_why(msg, 'home', '', _ctx(atHome: false)), SpeechSkip.notAtHome);
    });
    test('not while nothing plays', () {
      expect(
        _why(msg, 'starter', 'dev1', _ctx(playing: false)),
        SpeechSkip.notPlaying,
      );
    });
    test('a child, a guest and a profile without a right never speak', () {
      expect(
        _why(msg, 'starter', 'dev1', _ctx(role: 'child')),
        SpeechSkip.noRight,
      );
      expect(
        _why(msg, 'starter', 'dev1', _ctx(role: 'guest')),
        SpeechSkip.noRight,
      );
      expect(
        _why(msg, 'starter', 'dev1', _ctx(rights: const CdRights())),
        SpeechSkip.noRight,
      );
    });
    test('the switch off', () {
      expect(
        _why(msg, 'starter', 'dev1', _ctx(enabled: false)),
        SpeechSkip.switchOff,
      );
    });
    test('only four codes: no decision notice, no expiry, no unknown code', () {
      for (final code in [
        'cd_decided',
        'cd_expired',
        'cd_no_match',
        'cd_whatever',
      ]) {
        expect(
          _why(inboxNotice(6, code: code), 'starter', 'dev1', _ctx()),
          SpeechSkip.wrongCode,
        );
      }
      expect(_why(inboxTask(7), 'starter', 'dev1', _ctx()), isNull);
      for (final code in [
        'cd_awaiting_approval',
        'cd_read_error',
        'cd_present',
      ]) {
        expect(
          _why(inboxNotice(8, code: code), 'starter', 'dev1', _ctx()),
          isNull,
        );
      }
    });
    test('no speak value, or an unknown one: not spoken', () {
      expect(_why(msg, null, null, _ctx()), SpeechSkip.notSpoken);
      expect(_why(msg, 'everybody', null, _ctx()), SpeechSkip.notSpoken);
    });
    test('a read or decided item and a second time: not spoken', () {
      expect(
        _why(
          inboxNotice(5, code: 'cd_imported', read: true),
          'starter',
          'dev1',
          _ctx(),
        ),
        SpeechSkip.read,
      );
      expect(
        _why(inboxTask(7).copyWith(resolved: true), 'home', '', _ctx()),
        SpeechSkip.read,
      );
      expect(
        _why(msg, 'starter', 'dev1', _ctx(), {5}),
        SpeechSkip.alreadySpoken,
      );
    });
    test('home network is always true until the tunnel exists', () {
      expect(atHomeNetwork(), isTrue);
    });
  });

  group('the sentences', () {
    testWidgets(
      'from code and args, in English and German, without a title too',
      (tester) async {
        late AppLocalizations en, de;
        await tester.pumpWidget(
          Builder(
            builder: (c) {
              en = lookupAppLocalizations(const Locale('en'));
              de = lookupAppLocalizations(const Locale('de'));
              return const SizedBox();
            },
          ),
        );
        final t = inboxTask(1, args: {'title': 'Rüm Hart'});
        expect(
          spokenText(en, t),
          'The CD Rüm Hart is done and waits for your approval.',
        );
        expect(
          spokenText(de, t),
          'Die CD Rüm Hart ist fertig und wartet auf deine Freigabe.',
        );
        expect(
          spokenText(en, inboxNotice(2, args: {'title': 'X'})),
          'The CD X was imported.',
        );
        expect(
          spokenText(
            en,
            inboxNotice(2, code: 'cd_read_error', args: {'title': 'X'}),
          ),
          'The CD X has a read error.',
        );
        expect(
          spokenText(
            de,
            inboxNotice(2, code: 'cd_present', args: {'title': 'X'}),
          ),
          'Die CD X ist schon in der Bibliothek.',
        );
        expect(spokenText(en, inboxNotice(2, args: {})), 'A CD was imported.');
        expect(spokenText(en, inboxNotice(2, code: 'cd_decided')), '');
      },
    );
  });

  group('the announcer', () {
    late FakeEngine engine;
    late FakeDuck duck;
    late bool playing;
    late CdAnnouncer a;
    late AppLocalizations l;

    CdAnnouncer make({int queueMax = 3, FakeEngine? e}) {
      engine = e ?? engine;
      return CdAnnouncer(
        engine: engine,
        fade: Duration.zero,
        queueMax: queueMax,
        env: AnnouncerEnv(
          context: () => _ctx(playing: playing),
          l10n: () => l,
          duck: () => duck,
        ),
      );
    }

    setUp(() {
      engine = FakeEngine();
      duck = FakeDuck(0.8);
      playing = true;
      l = lookupAppLocalizations(const Locale('en'));
      a = make();
    });
    tearDown(() => a.dispose());

    test('lowered, spoken, restored to exactly the volume before', () async {
      a.onMessage(
        inboxNotice(5, args: {'title': 'Rüm Hart'}),
        'starter',
        'dev1',
      );
      await a.idle;
      expect(engine.said, ['The CD Rüm Hart was imported.']);
      expect(engine.locales, ['en']);
      expect(duck.log.first, closeTo(0.8 * kSpeechDuckLevel, 1e-9));
      expect(duck.log.last, 0.8);
      expect(duck.volume, 0.8);
    });

    test('restored when the voice fails', () async {
      engine.fail = true;
      a.onMessage(inboxNotice(5), 'starter', 'dev1');
      await a.idle;
      expect(duck.volume, 0.8);
    });

    test('nothing is spoken or lowered when it does not pass the decision, or twice', () async {
      playing = false;
      a.onMessage(inboxNotice(5), 'starter', 'dev1');
      playing = true;
      a.onMessage(inboxNotice(6), 'starter', 'other');
      expect(engine.said, isEmpty);
      expect(duck.log, isEmpty);
      a.onMessage(inboxNotice(7), 'starter', 'dev1');
      a.onMessage(inboxNotice(7), 'starter', 'dev1');
      await a.idle;
      expect(engine.said, hasLength(1));
    });

    test('one at a time, in order; the music stays low between them and is raised once', () async {
      engine.hold = Completer<void>();
      for (final i in [1, 2, 3]) {
        a.onMessage(inboxNotice(i, args: {'title': 'T$i'}), 'starter', 'dev1');
      }
      await Future<void>.delayed(Duration.zero);
      expect(engine.said, ['The CD T1 was imported.']);
      final h = engine.hold!;
      engine.hold = null;
      h.complete();
      await a.idle;
      expect(engine.said, [
        'The CD T1 was imported.',
        'The CD T2 was imported.',
        'The CD T3 was imported.',
      ]);
      expect(
        duck.log.where((v) => v == 0.8).length,
        1,
        reason: 'raised once, at the end',
      );
      expect(duck.volume, 0.8);
    });

    test('a long queue drops the oldest', () async {
      a.dispose();
      a = make(queueMax: 2);
      engine.hold = Completer<void>();
      for (final i in [1, 2, 3, 4]) {
        a.onMessage(inboxNotice(i, args: {'title': 'T$i'}), 'starter', 'dev1');
      }
      await Future<void>.delayed(Duration.zero);
      final h = engine.hold!;
      engine.hold = null;
      h.complete();
      await a.idle;
      expect(engine.said, [
        'The CD T1 was imported.',
        'The CD T3 was imported.',
        'The CD T4 was imported.',
      ]);
    });

    test(
      'closing the app in the middle of a sentence puts the volume back',
      () async {
        engine.hold = Completer<void>();
        a.onMessage(inboxNotice(5), 'starter', 'dev1');
        await Future<void>.delayed(Duration.zero);
        expect(duck.volume, lessThan(0.8));
        a.dispose();
        expect(duck.volume, 0.8);
        expect(engine.stops, 1);
        a = make();
      },
    );

    test(
      'an engine that is not available says nothing and touches no volume',
      () async {
        a.dispose();
        a = make(e: FakeEngine(available: false));
        a.onMessage(inboxNotice(5), 'starter', 'dev1');
        await a.idle;
        expect(engine.said, isEmpty);
        expect(duck.log, isEmpty);
      },
    );

    test('the switch is remembered and on by default', () async {
      final dir = Directory.systemTemp.createTempSync('speech');
      addTearDown(() => dir.deleteSync(recursive: true));
      final store = PlaybackSettingsStore(File('${dir.path}/p.json'));
      final b = CdAnnouncer(engine: engine, env: a.env, store: store);
      expect(b.enabled, isTrue);
      b.setEnabled(false);
      expect(
        CdAnnouncer(engine: engine, env: a.env, store: store).enabled,
        isFalse,
      );
      expect(store.readDuckLevel(), isNull);
      store.writeDuckLevel(0.4);
      expect(b.duckLevel, 0.4);
    });

    test('it takes its messages from the inbox controller', () async {
      final inbox = InboxController(api: FakeInboxApi());
      a.attach(inbox);
      inbox.onInboxMessage(inboxNotice(9), 'starter', 'dev1');
      await a.idle;
      expect(engine.said, hasLength(1));
      a.detach(inbox);
      inbox.onInboxMessage(inboxNotice(10), 'starter', 'dev1');
      expect(engine.said, hasLength(1));
      inbox.dispose();
    });
  });

  group('the Linux engine', () {
    test(
      'text after --, a separate argument, no shell, waits with -w',
      () async {
        String? exe;
        List<String>? args;
        final e = SpdSayEngine(
          find: () => true,
          start: (x, y) async {
            exe = x;
            args = y;
            return FakeProc(Completer<int>()..complete(0));
          },
        );
        await e.speak('--help; rm -rf /', locale: 'de');
        expect(exe, 'spd-say');
        expect(args, ['-w', '-l', 'de', '--', '--help; rm -rf /']);
      },
    );

    test('a missing program: not available, no call, no exception', () async {
      var started = false;
      final e = SpdSayEngine(
        find: () => false,
        start: (x, y) async {
          started = true;
          return FakeProc(Completer<int>());
        },
      );
      expect((e.available, e.unavailableReason), (false, 'no_spd_say'));
      await e.speak('hello');
      expect(started, isFalse);
    });

    test(
      'a start that fails, and a process that hangs, end the sentence',
      () async {
        final bad = SpdSayEngine(
          find: () => true,
          start: (_, _) async => throw const ProcessException('spd-say', []),
        );
        await bad.speak('x');
        late FakeProc p;
        final slow = SpdSayEngine(
          find: () => true,
          timeout: const Duration(milliseconds: 20),
          start: (_, _) async => p = FakeProc(Completer<int>()),
        );
        await slow.speak('x');
        expect(p.killed, isTrue);
      },
    );

    test('the other platforms have a stub that says why', () {
      const e = NoSpeechEngine();
      expect((e.available, e.unavailableReason), (false, 'not_supported'));
    });
  });

  group('the settings switch', () {
    late FakeEngine engine;
    Future<CdAnnouncer?> pump(
      WidgetTester tester, {
      bool available = true,
      CdRights rights = _approver,
      String role = 'member',
      bool withAnnouncer = true,
      Locale locale = const Locale('en'),
    }) async {
      engine = FakeEngine(available: available);
      final api = FakeInboxApi();
      final inbox = InboxController(api: api)..start();
      addTearDown(inbox.dispose);
      final profile = ProfileController()
        ..profile = Profile(name: 'Anna', role: role, cdRights: rights);
      final a = CdAnnouncer(
        engine: engine,
        fade: Duration.zero,
        env: AnnouncerEnv(
          context: () => _ctx(),
          l10n: () => lookupAppLocalizations(locale),
        ),
      );
      addTearDown(a.dispose);
      Widget home = const Scaffold(
        body: SingleChildScrollView(child: InboxTile()),
      );
      if (withAnnouncer) home = AnnouncerScope(announcer: a, child: home);
      tester.view.physicalSize = const Size(800, 2000);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        ProfileScope(
          controller: profile,
          child: InboxScope(
            controller: inbox,
            child: MaterialApp(
              locale: locale,
              localizationsDelegates: AppLocalizations.localizationsDelegates,
              supportedLocales: AppLocalizations.supportedLocales,
              home: home,
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      return a;
    }

    testWidgets(
      'the switch is on, turns off, and the Test button speaks a sample',
      (tester) async {
        final a = await pump(tester);
        expect(find.text('CD announcements'), findsOneWidget);
        expect(
          tester
              .widget<SwitchListTile>(
                find.byKey(const ValueKey('announce-switch')),
              )
              .value,
          isTrue,
        );
        await tester.tap(find.byKey(const ValueKey('announce-test')));
        await tester.pump();
        expect(engine.said, ['This is how the announcements about CDs sound.']);
        await tester.tap(find.byKey(const ValueKey('announce-switch')));
        await tester.pump();
        expect(a!.enabled, isFalse);
      },
    );

    testWidgets('no voice: Test is off and the reason shows', (tester) async {
      await pump(tester, available: false);
      expect(
        tester
            .widget<TextButton>(find.byKey(const ValueKey('announce-test')))
            .onPressed,
        isNull,
      );
      expect(
        find.text(
          'No voice: the program spd-say (speech-dispatcher) is missing.',
        ),
        findsOneWidget,
      );
    });

    testWidgets('hidden without a CD right and without an announcer', (
      tester,
    ) async {
      await pump(tester, rights: const CdRights(), role: 'guest');
      expect(find.byType(AnnounceSection), findsNothing);
      await pump(tester, withAnnouncer: false);
      expect(find.byKey(const ValueKey('announce-switch')), findsNothing);
    });

    testWidgets('in German', (tester) async {
      await pump(tester, locale: const Locale('de'));
      expect(find.text('Ansagen zu CDs'), findsOneWidget);
    });
  });

  group('the device name of the channel', () {
    test('hello carries it', () {
      expect(
        parseCdMessage('{"type":"hello","device":"a1b2c3","heartbeat_s":25}')
            .helloDevice,
        'a1b2c3',
      );
      expect(
        parseCdMessage('{"type":"snapshot","jobs":[]}').helloDevice,
        isNull,
      );
    });
  });
}
