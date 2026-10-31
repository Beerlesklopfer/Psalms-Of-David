// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/cd/cd_events.dart';
import 'package:psalm/inbox/inbox_api.dart';
import 'package:psalm/inbox/inbox_models.dart';
import 'package:psalm/profile.dart';
import 'package:psalm/server/api_client.dart';

// The item of docs/spec-app-api.md ("The inbox of the CDs"), word for word.
const _task =
    '{"id":17,"kind":"task","code":"cd_awaiting_approval","job_id":42,'
    '"args":{"job_id":42,"author":"Ken Follett","title":"Sturz der Titanen","disc":1,"discs_total":12,"tracks":14,"by":"Anna"},'
    '"created_ms":1793000000000,"read":false,"resolved":false,"can_decide":true,"job_state":"awaiting_approval"}';

/// A stand-in for David: answers by (method, path) with a status and a JSON body, and records every request.
class _Server {
  _Server(this.server) {
    server.listen((req) async {
      seen.add('${req.method} ${req.uri}');
      final r =
          answers['${req.method} ${req.uri.path}'] ??
          (404, {'error': 'not_found'});
      req.response.statusCode = r.$1;
      if (r.$2 != null) {
        req.response.headers.contentType = ContentType.json;
        req.response.write(jsonEncode(r.$2));
      }
      await req.response.close();
    });
  }

  final HttpServer server;
  final seen = <String>[];
  final answers = <String, (int, Object?)>{};

  static Future<_Server> start() async =>
      _Server(await HttpServer.bind(InternetAddress.loopbackIPv4, 0));

  ApiClient get api => ApiClient(address: 'localhost:${server.port}');
}

void main() {
  group('the models', () {
    test('an item has every member of the spec', () {
      final i = InboxItem.fromJson(jsonDecode(_task) as Map<String, dynamic>);
      expect(
        (i.id, i.kind, i.code, i.jobId, i.createdMs),
        (17, 'task', 'cd_awaiting_approval', 42, 1793000000000),
      );
      expect(
        (i.read, i.resolved, i.canDecide, i.jobState),
        (false, false, true, 'awaiting_approval'),
      );
      expect(i.args['author'], 'Ken Follett');
      expect(i.isTask, isTrue);
      expect(i.speak, isNull);
    });

    test('a decided item has resolved_ms, resolution and by; the spoken marks are read', () {
      final i = InboxItem.fromJson({
        'id': 18,
        'kind': 'task',
        'code': 'cd_awaiting_approval',
        'job_id': 5,
        'args': {},
        'created_ms': 1,
        'read': true,
        'resolved': true,
        'resolved_ms': 9,
        'resolution': 'approved',
        'by': 'Bea',
        'can_decide': false,
        'job_state': null,
        'speak': 'home',
        'device': '',
      });
      expect(
        (
          i.resolved,
          i.resolvedMs,
          i.resolution,
          i.by,
          i.jobState,
          i.speak,
          i.device,
        ),
        (true, 9, 'approved', 'Bea', null, 'home', ''),
      );
    });

    test('job_id null and album_id null are fine, an unknown code is kept', () {
      final i = InboxItem.fromJson({
        'id': 1,
        'kind': 'notice',
        'code': 'cd_from_the_future',
        'job_id': null,
        'args': {'album_id': null},
      });
      expect((i.jobId, i.albumId, i.code), (null, null, 'cd_from_the_future'));
      expect(
        InboxItem.fromJson({
          'id': 2,
          'kind': 'notice',
          'code': 'cd_present',
          'args': {'album_id': 7},
        }).albumId,
        7,
      );
    });

    test('a page carries the counts of the whole inbox and next', () {
      final p = InboxPage.fromJson({
        'items': [jsonDecode(_task)],
        'unread': 3,
        'tasks_open': 1,
        'next': 17,
      });
      expect((p.items.length, p.unread, p.tasksOpen, p.next), (1, 3, 1, 17));
      expect(
        InboxPage.fromJson({
          'items': [],
          'unread': 0,
          'tasks_open': 0,
          'next': null,
        }).next,
        isNull,
      );
    });

    test(
      'the profile reads cd_rights, inbox_unread and inbox_tasks_open from /me',
      () {
        final p = Profile.fromJson({
          'nickname': 'Anna',
          'role': 'member',
          'cd_rights': {
            'may_insert': true,
            'no_approval': false,
            'may_approve': true,
          },
          'inbox_unread': 4,
          'inbox_tasks_open': 2,
        });
        expect(
          (
            p.cdRights.mayInsert,
            p.cdRights.noApproval,
            p.cdRights.mayApprove,
            p.cdRights.hasInbox,
          ),
          (true, false, true, true),
        );
        expect((p.inboxUnread, p.inboxTasksOpen), (4, 2));
        // A profile without the members (an older server, a child) has no CD rights.
        expect(
          Profile.fromJson({'nickname': 'Kim', 'role': 'child'})
              .cdRights
              .hasInbox,
          isFalse,
        );
        // And the copy kept for the offline view survives.
        expect(Profile.fromJson(p.toJson()).cdRights.mayApprove, isTrue);
      },
    );
  });

  group('the messages of /cd/events', () {
    test('inbox: a new item with the counts', () {
      final m = parseCdMessage(
        '{"v":1,"type":"inbox","item":$_task,"unread":3,"tasks_open":1}',
      );
      final w = m.inbox as InboxArrived;
      expect((w.item.id, w.unread, w.tasksOpen), (17, 3, 1));
    });

    test('inbox_resolved: who decided what', () {
      final m = parseCdMessage(
        '{"v":1,"type":"inbox_resolved","id":17,"job_id":42,"resolution":"approved","by":"Bea","resolved_ms":1793000100000,"unread":2,"tasks_open":0}',
      );
      final w = m.inbox as InboxResolvedWire;
      expect(
        (
          w.id,
          w.jobId,
          w.resolution,
          w.by,
          w.resolvedMs,
          w.unread,
          w.tasksOpen,
        ),
        (17, 42, 'approved', 'Bea', 1793000100000, 2, 0),
      );
    });

    test('the snapshot carries the counts and still the jobs', () {
      final m = parseCdMessage(
        '{"v":1,"type":"snapshot","busy":false,"jobs":[],"unread":5,"tasks_open":2}',
      );
      final w = m.inbox as InboxCountsWire;
      expect((w.unread, w.tasksOpen), (5, 2));
      expect(m.jobs, isEmpty);
      // An older server sends none: no inbox message.
      expect(
        parseCdMessage('{"v":1,"type":"snapshot","jobs":[]}').inbox,
        isNull,
      );
    });

    test('a broken inbox message is nothing, not an exception', () {
      expect(parseCdMessage('{"type":"inbox","item":5}').inbox, isNull);
      expect(parseCdMessage('{"type":"inbox_resolved"}').inbox, isNull);
    });
  });

  group('the routes', () {
    test(
      'list asks with after and limit in the query and reads next',
      () async {
        final s = await _Server.start();
        addTearDown(() => s.server.close(force: true));
        s.answers['GET /api/v1/inbox'] = (
          200,
          {
            'items': [jsonDecode(_task)],
            'unread': 1,
            'tasks_open': 1,
            'next': 17,
          },
        );
        final p = await ServerInboxApi(s.api).list(after: 30, limit: 20);
        expect(s.seen.single, 'GET /api/v1/inbox?after=30&limit=20');
        expect((p.items.single.id, p.next, p.tasksOpen), (17, 17, 1));
        await ServerInboxApi(s.api).list();
        expect(s.seen.last, 'GET /api/v1/inbox');
      },
    );

    test('422 invalid_paging is an ApiException', () async {
      final s = await _Server.start();
      addTearDown(() => s.server.close(force: true));
      s.answers['GET /api/v1/inbox'] = (422, {'error': 'invalid_paging'});
      await expectLater(
        ServerInboxApi(s.api).list(limit: 0),
        throwsA(
          isA<ApiException>().having(
            (e) => (e.status, e.code),
            'status, code',
            (422, 'invalid_paging'),
          ),
        ),
      );
    });

    test(
      'mark read, mark all read, retry; 404 for an item that is not theirs',
      () async {
        final s = await _Server.start();
        addTearDown(() => s.server.close(force: true));
        s.answers['POST /api/v1/inbox/17/read'] = (204, null);
        s.answers['POST /api/v1/inbox/read_all'] = (204, null);
        s.answers['POST /api/v1/cd/jobs/42/retry'] = (
          202,
          {'job': {}, 'resume': 'x'},
        );
        final api = ServerInboxApi(s.api);
        await api.markRead(17);
        await api.markAllRead();
        await api.retry(42);
        expect(s.seen, [
          'POST /api/v1/inbox/17/read',
          'POST /api/v1/inbox/read_all',
          'POST /api/v1/cd/jobs/42/retry',
        ]);
        await expectLater(
          api.markRead(99),
          throwsA(isA<ApiException>().having((e) => e.status, 'status', 404)),
        );
      },
    );

    test('approve and reject go to the job, not the item; 409 decided keeps by and resolution', () async {
      final s = await _Server.start();
      addTearDown(() => s.server.close(force: true));
      s.answers['POST /api/v1/cd/jobs/42/approve'] = (200, {'job': {}});
      s.answers['POST /api/v1/cd/jobs/43/reject'] = (
        409,
        {'error': 'decided', 'by': 'Bea', 'resolution': 'approved'},
      );
      s.answers['POST /api/v1/cd/jobs/44/approve'] = (
        409,
        {'error': 'invalid_move', 'state': 'imported'},
      );
      s.answers['POST /api/v1/cd/jobs/45/approve'] = (
        403,
        {'error': 'forbidden'},
      );
      final api = ServerInboxApi(s.api);
      await api.approve(42);
      await expectLater(
        api.reject(43),
        throwsA(
          isA<ApiException>().having(
            (e) => (e.status, e.code, e.body?['by'], e.body?['resolution']),
            'decided',
            (409, 'decided', 'Bea', 'approved'),
          ),
        ),
      );
      await expectLater(
        api.approve(44),
        throwsA(
          isA<ApiException>().having((e) => e.code, 'code', 'invalid_move'),
        ),
      );
      await expectLater(
        api.approve(45),
        throwsA(
          isA<ApiException>().having(
            (e) => (e.status, e.code),
            'status, code',
            (403, 'forbidden'),
          ),
        ),
      );
      await expectLater(
        api.approve(46),
        throwsA(isA<ApiException>().having((e) => e.code, 'code', 'not_found')),
      );
    });
  });
}
