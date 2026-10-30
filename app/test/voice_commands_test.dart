// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/voice/voice_commands.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('VoiceCommand.parse', () {
    test('reads the five actions', () {
      expect(
        VoiceCommand.parse('{"action":"continue"}')!.action,
        VoiceAction.continueListening,
      );
      expect(VoiceCommand.parse('{"action":"next"}')!.action, VoiceAction.next);
      expect(
        VoiceCommand.parse('{"action":"previous"}')!.action,
        VoiceAction.previous,
      );
      expect(
        VoiceCommand.parse('{"action":"new"}')!.action,
        VoiceAction.showNew,
      );
      final play = VoiceCommand.parse(
        '{"action":"play","query":"  Frohe Weihnachten "}',
      )!;
      expect(play.action, VoiceAction.play);
      expect(play.query, 'Frohe Weihnachten');
    });

    test('ignores unknown actions and broken input', () {
      expect(VoiceCommand.parse('{"action":"delete"}'), isNull);
      expect(VoiceCommand.parse('{"query":"x"}'), isNull);
      expect(VoiceCommand.parse('not json'), isNull);
      expect(VoiceCommand.parse('[1,2]'), isNull);
      expect(VoiceCommand.parse(''), isNull);
      expect(VoiceCommand.parse(null), isNull);
    });

    test('play without a query is ignored', () {
      expect(VoiceCommand.parse('{"action":"play"}'), isNull);
      expect(VoiceCommand.parse('{"action":"play","query":"  "}'), isNull);
    });
  });

  group('VoiceChannel', () {
    const channel = MethodChannel('com.the-abraham-box.psalm/voice');

    test('is null where the platform is not iOS', () async {
      expect(await VoiceChannel(supported: false).takePending(), isNull);
    });

    test('is null when the native side is missing', () async {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, null);
      expect(await VoiceChannel(supported: true).takePending(), isNull);
    });

    test('returns the pending command', () async {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(
            channel,
            (call) async =>
                call.method == 'takePending' ? '{"action":"next"}' : null,
          );
      addTearDown(
        () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
            .setMockMethodCallHandler(channel, null),
      );
      expect(
        (await VoiceChannel(supported: true).takePending())!.action,
        VoiceAction.next,
      );
    });
  });
}
