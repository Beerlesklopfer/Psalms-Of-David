// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/foundation.dart';

import '../server/api_client.dart';
import 'list_machine.dart';

/// The shell of [listTransition] for a list from the server: holds the [state], asks the server when the machine says [ListFetch] and tells
/// the listeners when an answer changed the state. The widgets switch over [state].
abstract class ListController<T> extends ChangeNotifier {
  ListController({required bool hasServer}) {
    send(ListServerChanged<T>(hasServer: hasServer));
  }

  ListState<T> state = const ListIdle();
  bool _disposed = false;

  /// Asks the server; throws [ApiException].
  Future<List<T>> fetchItems();

  /// Called with each answer before it goes to the machine.
  void itemsArrived(List<T> items) {}

  /// Feeds [event] to the machine and returns its effects. No notification: the callers decide.
  List<ListEffect> send(ListEvent<T> event) {
    final (next, effects) = listTransition(state, event);
    state = next;
    return effects;
  }

  /// The offline view is on or off. Called from `build` of the widgets that read the scope, so it never notifies; the build that
  /// calls it reads the state right after.
  void noteOffline({required bool active}) => send(ListOfflineChanged<T>(active: active));

  /// Loads the list the first time, asks again later.
  Future<void> load() async {
    final effects = send(state is ListIdle<T> ? ListLoad<T>() : ListRefresh<T>());
    if (!effects.any((e) => e is ListFetch)) return;
    try {
      final items = await fetchItems();
      itemsArrived(items);
      send(ListResult<T>(items));
    } on ApiException catch (e) {
      send(ListError<T>(e.code));
    }
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }
}
