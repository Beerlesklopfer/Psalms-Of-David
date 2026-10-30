// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter_test/flutter_test.dart';
import 'package:psalm/lists/list_machine.dart';

typedef S = ListState<String>;
typedef E = ListEvent<String>;

const _loaded = ListLoaded<String>(['a', 'b']);
const _fetch = [ListFetch()];

void main() {
  group('transitions', () {
    final rows = <(String, S, E, Type, List<ListEffect>)>[
      ('Idle Load', const ListIdle(), const ListLoad(), ListLoading<String>, _fetch),
      ('Idle ServerChanged(false)', const ListIdle(), const ListServerChanged(hasServer: false), ListNeedsServer<String>, const []),
      ('Loading Result', const ListLoading(), const ListResult(['a']), ListLoaded<String>, const []),
      ('Loading Result empty', const ListLoading(), const ListResult([]), ListEmpty<String>, const []),
      ('Loading Error', const ListLoading(), const ListError('boom'), ListFailed<String>, const []),
      ('Loading Refresh keeps Loading and asks again', const ListLoading(), const ListRefresh(), ListLoading<String>, _fetch),
      ('Loaded Refresh keeps the list', _loaded, const ListRefresh(), ListLoaded<String>, _fetch),
      ('Loaded Result replaces', _loaded, const ListResult(['c']), ListLoaded<String>, const []),
      ('Loaded Result empty', _loaded, const ListResult([]), ListEmpty<String>, const []),
      ('Loaded Error', _loaded, const ListError('x'), ListFailed<String>, const []),
      ('Empty Refresh', const ListEmpty(), const ListRefresh(), ListEmpty<String>, _fetch),
      ('Empty Result', const ListEmpty(), const ListResult(['a']), ListLoaded<String>, const []),
      ('Failed Refresh keeps the error until answered', const ListFailed('x'), const ListRefresh(), ListFailed<String>, _fetch),
      ('Failed Result', const ListFailed('x'), const ListResult(['a']), ListLoaded<String>, const []),
      ('Failed Error', const ListFailed('x'), const ListError('y'), ListFailed<String>, const []),
      ('Loaded ServerChanged(false)', _loaded, const ListServerChanged(hasServer: false), ListNeedsServer<String>, const []),
      ('Failed ServerChanged(false)', const ListFailed('x'), const ListServerChanged(hasServer: false), ListNeedsServer<String>, const []),
      ('NeedsServer ServerChanged(true)', const ListNeedsServer(), const ListServerChanged(hasServer: true), ListLoading<String>, _fetch),
      ('Loaded OfflineChanged(true)', _loaded, const ListOfflineChanged(active: true), ListOfflineView<String>, const []),
      ('NeedsServer OfflineChanged(true)', const ListNeedsServer(), const ListOfflineChanged(active: true), ListOfflineView<String>, const []),
      ('Offline OfflineChanged(false)', const ListOfflineView(_loaded), const ListOfflineChanged(active: false), ListLoaded<String>, const []),
      ('Offline Result passes to the list', const ListOfflineView(ListLoading()), const ListResult(['z']), ListOfflineView<String>, const []),
      ('Offline Refresh asks', const ListOfflineView(_loaded), const ListRefresh(), ListOfflineView<String>, _fetch),
    ];
    for (final (name, from, event, to, effects) in rows) {
      test(name, () {
        final r = listTransition(from, event);
        expect(r.$1.runtimeType, to);
        expect(r.$2, effects);
      });
    }
  });

  test('payloads', () {
    expect((listTransition<String>(const ListLoading(), const ListResult(['a', 'b'])).$1 as ListLoaded<String>).items, ['a', 'b']);
    expect((listTransition<String>(_loaded, const ListError('e')).$1 as ListFailed<String>).error, 'e');
  });

  test('the answer reaches the list under the offline view and shows after it', () {
    var s = listTransition<String>(const ListLoading(), const ListOfflineChanged(active: true)).$1;
    s = listTransition<String>(s, const ListResult(['a'])).$1;
    s = listTransition<String>(s, const ListOfflineChanged(active: false)).$1;
    expect((s as ListLoaded<String>).items, ['a']);
  });

  test('the server comes back: a list that failed offline is asked again', () {
    var s = listTransition<String>(_loaded, const ListServerChanged(hasServer: false)).$1;
    final r = listTransition<String>(s, const ListServerChanged(hasServer: true));
    expect(r.$1, isA<ListLoading<String>>());
    expect(r.$2, _fetch);
  });

  group('illegal events are explicit no-ops', () {
    const events = <E>[
      ListLoad(), ListRefresh(), ListResult(['q']), ListError('q'), ListServerChanged(hasServer: true), ListServerChanged(hasServer: false), ListOfflineChanged(active: true), ListOfflineChanged(active: false),
    ];
    bool handled(S s, E e) => switch ((s, e)) {
          (ListIdle(), ListLoad()) => true,
          (ListIdle(), ListServerChanged(hasServer: false)) => true,
          (ListLoading() || ListLoaded() || ListEmpty() || ListFailed(), ListRefresh() || ListResult() || ListError()) => true,
          (ListLoading() || ListLoaded() || ListEmpty() || ListFailed(), ListServerChanged(hasServer: false)) => true,
          (ListNeedsServer(), ListServerChanged(hasServer: true)) => true,
          (ListOfflineView(), _) => false,
          (_, ListOfflineChanged(active: true)) => true,
          _ => false,
        };
    final states = <String, S>{
      'Idle': const ListIdle(),
      'Loading': const ListLoading(),
      'Loaded': _loaded,
      'Empty': const ListEmpty(),
      'Failed': const ListFailed('x'),
      'NeedsServer': const ListNeedsServer(),
    };
    states.forEach((name, s) {
      test(name, () {
        for (final e in events.where((e) => !handled(s, e))) {
          final r = listTransition(s, e);
          expect(r.$1, same(s), reason: '${e.runtimeType}');
          expect(r.$2, isEmpty, reason: '${e.runtimeType}');
        }
      });
    });
    test('OfflineView ignores a repeated OfflineChanged(true) and the events its list ignores', () {
      const s = ListOfflineView<String>(ListIdle());
      for (final e in const <E>[ListOfflineChanged(active: true), ListResult(['q']), ListError('q'), ListRefresh(), ListServerChanged(hasServer: true)]) {
        final r = listTransition(s, e);
        expect(r.$1, same(s), reason: '${e.runtimeType}');
        expect(r.$2, isEmpty);
      }
    });
  });
}
