// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

// The loading life cycle of a list from the server (podcasts, radio stations) as a PURE function:
// `listTransition(state, event) -> (state, effects)`. The controllers (`PodcastsController`, `RadioController`) are the shell: they carry
// out [ListFetch] and feed the answer back; the `Body` widgets switch over the state. The document is docs/spec-state-machines.md.

// ---- States -----------------------------------------------------------------------------------------

sealed class ListState<T> {
  const ListState();
}

/// Nothing asked yet.
class ListIdle<T> extends ListState<T> {
  const ListIdle();
}

/// The first answer is awaited.
class ListLoading<T> extends ListState<T> {
  const ListLoading();
}

/// [items] is never empty (see [ListEmpty]).
class ListLoaded<T> extends ListState<T> {
  const ListLoaded(this.items);
  final List<T> items;
}

/// The server answered with no items.
class ListEmpty<T> extends ListState<T> {
  const ListEmpty();
}

/// The last answer was an error ([error] is its code).
class ListFailed<T> extends ListState<T> {
  const ListFailed(this.error);
  final String error;
}

/// The offline view shows something else; [under] is the state of the list, which keeps following the answers.
class ListOfflineView<T> extends ListState<T> {
  const ListOfflineView(this.under);
  final ListState<T> under;
}

/// There is no server to ask.
class ListNeedsServer<T> extends ListState<T> {
  const ListNeedsServer();
}

// ---- Events -----------------------------------------------------------------------------------------

sealed class ListEvent<T> {
  const ListEvent();
}

/// The first load.
class ListLoad<T> extends ListEvent<T> {
  const ListLoad();
}

/// Ask again; what is shown stays until the answer comes.
class ListRefresh<T> extends ListEvent<T> {
  const ListRefresh();
}

/// The server answered.
class ListResult<T> extends ListEvent<T> {
  const ListResult(this.items);
  final List<T> items;
}

/// The server could not be asked or refused ([error] is the code).
class ListError<T> extends ListEvent<T> {
  const ListError(this.error);
  final String error;
}

class ListServerChanged<T> extends ListEvent<T> {
  const ListServerChanged({required this.hasServer});
  final bool hasServer;
}

class ListOfflineChanged<T> extends ListEvent<T> {
  const ListOfflineChanged({required this.active});
  final bool active;
}

// ---- Effects ----------------------------------------------------------------------------------------

sealed class ListEffect {
  const ListEffect();
}

/// Ask the server for the list (the shell answers with [ListResult] or [ListError]).
class ListFetch extends ListEffect {
  const ListFetch();
  @override
  bool operator ==(Object other) => other is ListFetch;
  @override
  int get hashCode => 1;
}

typedef ListResultOf<T> = (ListState<T> state, List<ListEffect> effects);

ListResultOf<T> _ignore<T>(ListState<T> s) => (s, const []);

ListResultOf<T> listTransition<T>(ListState<T> state, ListEvent<T> event) {
  // The offline view wraps any state and passes the answers on.
  if (state is ListOfflineView<T>) {
    if (event is ListOfflineChanged<T>) return event.active ? _ignore(state) : (state.under, const []);
    final (under, fx) = listTransition(state.under, event);
    return identical(under, state.under) && fx.isEmpty ? _ignore(state) : (ListOfflineView<T>(under), fx);
  }
  switch (event) {
    case ListOfflineChanged(:final active):
      return active ? (ListOfflineView<T>(state), const []) : _ignore(state);
    case ListLoad():
      return state is ListIdle<T> ? (ListLoading<T>(), const [ListFetch()]) : _ignore(state);
    case ListRefresh():
      return switch (state) {
        ListLoading<T>() || ListLoaded<T>() || ListEmpty<T>() || ListFailed<T>() => (state, const [ListFetch()]),
        _ => _ignore(state),
      };
    case ListResult(:final items):
      return switch (state) {
        ListLoading<T>() || ListLoaded<T>() || ListEmpty<T>() || ListFailed<T>() => (items.isEmpty ? ListEmpty<T>() : ListLoaded<T>(items), const []),
        _ => _ignore(state),
      };
    case ListError(:final error):
      return switch (state) {
        ListLoading<T>() || ListLoaded<T>() || ListEmpty<T>() || ListFailed<T>() => (ListFailed<T>(error), const []),
        _ => _ignore(state),
      };
    case ListServerChanged(:final hasServer):
      return switch (state) {
        ListNeedsServer<T>() when hasServer => (ListLoading<T>(), const [ListFetch()]),
        ListIdle<T>() when !hasServer => (ListNeedsServer<T>(), const []),
        ListLoading<T>() || ListLoaded<T>() || ListEmpty<T>() || ListFailed<T>() when !hasServer => (ListNeedsServer<T>(), const []),
        _ => _ignore(state),
      };
  }
}
