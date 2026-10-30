// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import '../audio/audio_backend.dart' show AudioErrorKind, BackendState, StreamDetails;
import '../audio/file_formats.dart' show trackPlayableHere;
import '../playlists.dart' show kPlaylistKind;
import 'transport.dart';

// The playback state machine of the player as a PURE function: `transition(state, event) -> (state, effects)`.
// No I/O, no timers, no backend calls and no clock in here: the time comes in with the event ([EngineEvent.nowMs]), every action
// that touches the world is returned as an [Effect] (plain data) for the shell (`PlayerEngine`) to carry out. The document is
// docs/spec-player-state-machine.md (diagrams and the transition table).

/// The three kinds of playable thing. They share the machine; what differs is decided here, not by `if (track.live)` in the shell.
enum MediaKind {
  /// A track of a book, an album or a playlist: position, seek, skip, heard marks, plays.
  track,

  /// An episode of a podcast (`ep:<id>`): like a track, and its place is also noted for the list of downloaded episodes.
  episode,

  /// A radio station: no length, no seek, no skip, no progress; title and details come from the stream.
  live,
}

MediaKind mediaKindOf(BookData b, TrackData? t) {
  if (b.kind == 'radio' || (t?.live ?? false)) return MediaKind.live;
  if (episodeIdOf(b) != null) return MediaKind.episode;
  return MediaKind.track;
}

int? episodeIdOf(BookData b) => b.kind == 'podcast' && b.id.startsWith('ep:') ? int.tryParse(b.id.substring(3)) : null;

/// Why a track is being fetched and loaded.
enum LoadReason { open, next, previous, skip, select, resume }

const _tickMs = 500;
const _heardTailMs = 3000;
const _reportEveryMs = 10000;
const _writeEveryMs = 5000;
const _prefetchWithinMs = 45000;

// ---- Data of the state ----------------------------------------------------------------------------

/// What the machine knows of the world around it (set by [EnvChanged]).
class Env {
  const Env({this.hasBackend = false, this.hasSource = false, this.offlineActive = false, this.writesLast = false});
  final bool hasBackend;
  final bool hasSource;
  final bool offlineActive;
  final bool writesLast;

  Env copyWith({bool? hasBackend, bool? hasSource, bool? offlineActive, bool? writesLast}) => Env(
        hasBackend: hasBackend ?? this.hasBackend,
        hasSource: hasSource ?? this.hasSource,
        offlineActive: offlineActive ?? this.offlineActive,
        writesLast: writesLast ?? this.writesLast,
      );
}

const Object _keep = Object();

/// What every state carries: the open book, the place in it and the bookkeeping.
class Core {
  const Core({
    this.book,
    this.trackIndex = 0,
    this.positionMs = 0,
    this.speed = 1.0,
    this.loadedKey,
    this.streamTitle,
    this.streamDetails,
    this.prefetchedKey,
    this.lastReportMs = 0,
    this.lastWrittenMs = 0,
    this.heard = const {},
    this.env = const Env(),
  });

  final BookData? book;
  final int trackIndex;
  final int positionMs;
  final double speed;

  /// Which track the backend has loaded, as `bookId:trackIndex`.
  final String? loadedKey;
  final String? streamTitle;
  final StreamDetails? streamDetails;
  final String? prefetchedKey;
  final int lastReportMs;
  final int lastWrittenMs;
  final Set<String> heard;
  final Env env;

  TrackData? get track {
    final b = book;
    return b != null && trackIndex >= 0 && trackIndex < b.tracks.length ? b.tracks[trackIndex] : null;
  }

  String? get key => book == null ? null : '${book!.id}:$trackIndex';

  MediaKind? get kind => book == null ? null : mediaKindOf(book!, track);

  /// Does the backend play this track (a file on the device, a part of the server, an episode or a station)?
  bool get useBackend {
    final t = track;
    return env.hasBackend && t != null && (t.path != null || ((t.partId != null || t.url != null) && env.hasSource));
  }

  Core copyWith({
    Object? book = _keep,
    int? trackIndex,
    int? positionMs,
    double? speed,
    Object? loadedKey = _keep,
    Object? streamTitle = _keep,
    Object? streamDetails = _keep,
    Object? prefetchedKey = _keep,
    int? lastReportMs,
    int? lastWrittenMs,
    Set<String>? heard,
    Env? env,
  }) =>
      Core(
        book: identical(book, _keep) ? this.book : book as BookData?,
        trackIndex: trackIndex ?? this.trackIndex,
        positionMs: positionMs ?? this.positionMs,
        speed: speed ?? this.speed,
        loadedKey: identical(loadedKey, _keep) ? this.loadedKey : loadedKey as String?,
        streamTitle: identical(streamTitle, _keep) ? this.streamTitle : streamTitle as String?,
        streamDetails: identical(streamDetails, _keep) ? this.streamDetails : streamDetails as StreamDetails?,
        prefetchedKey: identical(prefetchedKey, _keep) ? this.prefetchedKey : prefetchedKey as String?,
        lastReportMs: lastReportMs ?? this.lastReportMs,
        lastWrittenMs: lastWrittenMs ?? this.lastWrittenMs,
        heard: heard ?? this.heard,
        env: env ?? this.env,
      );
}

// ---- States ---------------------------------------------------------------------------------------

/// The closed set of states. Each old flag combination maps to exactly one of them:
///
/// | old flags                                   | state |
/// |---------------------------------------------|-------|
/// | no book                                     | [Idle] |
/// | book, not playing, no error                 | [Paused] |
/// | playing, track loaded (or the clock runs)   | [Playing] |
/// | playing, the backend reports buffering      | [Buffering] |
/// | loading (file being fetched), playing or not| [Loading] |
/// | error set, not playing                      | [Failed] |
/// | unplayable set, not playing                 | [Unplayable] |
/// | not playing at the end of the last track    | [Ended] |
/// | disposed                                    | [Closed] |
///
/// A "playing" together with an "error" or an "unplayable" cannot be represented.
sealed class EngineState {
  const EngineState(this.core);
  final Core core;

  EngineState withCore(Core c);

  /// Whether the clock (the tick) should run and the person wants sound: playing, buffering, or loading to play.
  bool get wantsPlay => false;

  @override
  String toString() =>
      '$runtimeType(${core.key ?? '-'} @${core.positionMs}${_extra()})';

  String _extra() => '';
}

/// No book is open.
class Idle extends EngineState {
  const Idle(super.core);
  @override
  EngineState withCore(Core c) => Idle(c);
}

/// A book is open, nothing plays. Ready to play.
class Paused extends EngineState {
  const Paused(super.core);
  @override
  EngineState withCore(Core c) => Paused(c);
}

/// The file of the track is being fetched (a part, an episode) or the station is being connected. [wantPlay] says what happens when it
/// arrives. The tick runs but does nothing.
class Loading extends EngineState {
  const Loading(super.core, {required this.wantPlay, required this.reason});
  final bool wantPlay;
  final LoadReason reason;
  @override
  bool get wantsPlay => wantPlay;
  @override
  EngineState withCore(Core c) => Loading(c, wantPlay: wantPlay, reason: reason);
  @override
  String _extra() => ' ${wantPlay ? 'play' : 'hold'} ${reason.name}';
}

class Playing extends EngineState {
  const Playing(super.core);
  @override
  bool get wantsPlay => true;
  @override
  EngineState withCore(Core c) => Playing(c);
}

/// The backend plays nothing for the moment because its buffer is empty (a stream underrun, a slow network); it goes on by itself.
class Buffering extends EngineState {
  const Buffering(super.core);
  @override
  bool get wantsPlay => true;
  @override
  EngineState withCore(Core c) => Buffering(c);
}

/// The last track played to its end.
class Ended extends EngineState {
  const Ended(super.core);
  @override
  EngineState withCore(Core c) => Ended(c);
}

/// The sound engine failed (or the file could not be fetched); [kind] tells a radio stream's reason (unsupported, connection, server,
/// certificate). Playback is stopped. Play tries again.
class Failed extends EngineState {
  const Failed(super.core, this.message, {this.kind = AudioErrorKind.generic});
  final String message;
  final AudioErrorKind kind;
  @override
  EngineState withCore(Core c) => Failed(c, message, kind: kind);
  @override
  String _extra() => ' $message/${kind.name}';
}

/// The track cannot be played on this platform (its format, no MP3 from the server). [index] is the track in the open book.
class Unplayable extends EngineState {
  const Unplayable(super.core, this.index);
  final int index;
  @override
  EngineState withCore(Core c) => Unplayable(c, index);
}

/// Disposed: every event is ignored.
class Closed extends EngineState {
  const Closed(super.core);
  @override
  EngineState withCore(Core c) => Closed(c);
}

// ---- Events ---------------------------------------------------------------------------------------

/// What the backend says now; part of [Tick] and [ExternalChange].
class Observation {
  const Observation({required this.positionMs, required this.state, this.streamTitle, this.streamDetails, this.errorMessage, this.errorKind = AudioErrorKind.generic});
  final int positionMs;
  final BackendState state;
  final String? streamTitle;
  final StreamDetails? streamDetails;
  final String? errorMessage;
  final AudioErrorKind errorKind;
}

sealed class EngineEvent {
  const EngineEvent(this.nowMs);

  /// The clock, in ms since 1970; the machine has no clock of its own.
  final int nowMs;
}

class UserOpen extends EngineEvent {
  const UserOpen(super.nowMs, this.book, {this.trackIndex, this.startMs, this.forceReload = false});
  final BookData book;
  final int? trackIndex;
  final int? startMs;
  final bool forceReload;
}

class UserToggle extends EngineEvent {
  const UserToggle(super.nowMs);
}

/// Play if not playing (the voice "go on").
class UserPlay extends EngineEvent {
  const UserPlay(super.nowMs);
}

class UserSeek extends EngineEvent {
  const UserSeek(super.nowMs, this.toMs);
  final int toMs;
}

class UserSelectTrack extends EngineEvent {
  const UserSelectTrack(super.nowMs, this.trackIndex, this.positionMs);
  final int trackIndex;
  final int positionMs;
}

class UserSkip extends EngineEvent {
  const UserSkip(super.nowMs, this.deltaMs);
  final int deltaMs;
}

class UserNext extends EngineEvent {
  const UserNext(super.nowMs);
}

class UserPrevious extends EngineEvent {
  const UserPrevious(super.nowMs);
}

class SetSpeed extends EngineEvent {
  const SetSpeed(super.nowMs, this.speed);
  final double speed;
}

/// The place was moved from outside without touching the sound (another device went on, the library changed).
class SetPlace extends EngineEvent {
  const SetPlace(super.nowMs, {this.book, this.clearBook = false, this.trackIndex, this.positionMs});
  final BookData? book;
  final bool clearBook;
  final int? trackIndex;
  final int? positionMs;
}

class Halt extends EngineEvent {
  const Halt(super.nowMs);
}

class ClearHeard extends EngineEvent {
  const ClearHeard(super.nowMs);
}

class EnvChanged extends EngineEvent {
  const EnvChanged(super.nowMs, {this.hasBackend, this.hasSource, this.offlineActive, this.writesLast});
  final bool? hasBackend;
  final bool? hasSource;
  final bool? offlineActive;
  final bool? writesLast;
}

class Flush extends EngineEvent {
  const Flush(super.nowMs);
}

class Dispose extends EngineEvent {
  const Dispose(super.nowMs);
}

/// Every 500 ms while the clock runs, with what the backend says (a book or an episode: position; a station: title and details).
class Tick extends EngineEvent {
  const Tick(super.nowMs, this.observation);
  final Observation observation;
}

/// The engine started, paused or moved by itself (notification, headset, a call).
class ExternalChange extends EngineEvent {
  const ExternalChange(super.nowMs, this.observation);
  final Observation observation;
}

/// The file of [key] arrived and the backend has loaded it.
class BackendLoaded extends EngineEvent {
  const BackendLoaded(super.nowMs, this.key);
  final String key;
}

/// Fetching or loading [key] failed. [transcodeFailed]: the server could not make an MP3, the format cannot be played here.
class LoadFailed extends EngineEvent {
  const LoadFailed(super.nowMs, this.key, this.message, {this.kind = AudioErrorKind.generic, this.transcodeFailed = false});
  final String key;
  final String message;
  final AudioErrorKind kind;
  final bool transcodeFailed;
}

/// A call of the backend (load, play, pause, seek, speed) threw. [unload]: forget the loaded track.
class BackendFailed extends EngineEvent {
  const BackendFailed(super.nowMs, this.message, {this.kind = AudioErrorKind.generic, this.unload = true});
  final String message;
  final AudioErrorKind kind;
  final bool unload;
}

class PrefetchFailed extends EngineEvent {
  const PrefetchFailed(super.nowMs, this.key);
  final String key;
}

// ---- Effects --------------------------------------------------------------------------------------

/// What the shell has to do. Plain data; two effects are equal if they have the same fields (the table tests compare them).
sealed class Effect {
  const Effect();
  List<Object?> get props;
  @override
  bool operator ==(Object other) => other.runtimeType == runtimeType && _same((other as Effect).props, props);
  @override
  int get hashCode => Object.hash(runtimeType, Object.hashAll(props));
  @override
  String toString() => '$runtimeType(${props.join(', ')})';
}

bool _same(List<Object?> a, List<Object?> b) {
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}

class LoadTrack extends Effect {
  const LoadTrack(this.path, this.startMs, this.play, this.title);
  final String path;
  final int startMs;
  final bool play;
  final String title;
  @override
  List<Object?> get props => [path, startMs, play, title];
}

/// Fetch the file of the current track (through the source), then load it; the shell tells the result with [BackendLoaded] or [LoadFailed].
class FetchAndLoad extends Effect {
  const FetchAndLoad(this.key, this.trackIndex);
  final String key;
  final int trackIndex;
  @override
  List<Object?> get props => [key, trackIndex];
}

class StartPlayback extends Effect {
  const StartPlayback();
  @override
  List<Object?> get props => [];
}

class PausePlayback extends Effect {
  const PausePlayback();
  @override
  List<Object?> get props => [];
}

class SeekBackend extends Effect {
  const SeekBackend(this.positionMs);
  final int positionMs;
  @override
  List<Object?> get props => [positionMs];
}

class StopBackend extends Effect {
  const StopBackend();
  @override
  List<Object?> get props => [];
}

class DisposeBackend extends Effect {
  const DisposeBackend();
  @override
  List<Object?> get props => [];
}

/// Set the speed of the backend if it differs.
class EnsureSpeed extends Effect {
  const EnsureSpeed(this.speed);
  final double speed;
  @override
  List<Object?> get props => [speed];
}

/// Set the speed of the backend.
class SetBackendSpeed extends Effect {
  const SetBackendSpeed(this.speed);
  final double speed;
  @override
  List<Object?> get props => [speed];
}

class StartTick extends Effect {
  const StartTick();
  @override
  List<Object?> get props => [];
}

class StopTick extends Effect {
  const StopTick();
  @override
  List<Object?> get props => [];
}

/// Write the place down for "where I left off" (and for a playlist, its own place).
class WriteLast extends Effect {
  const WriteLast(this.bookId, this.trackIndex, this.positionMs, this.atMs, this.playlist);
  final String bookId;
  final int trackIndex;
  final int positionMs;
  final int atMs;
  final bool playlist;
  @override
  List<Object?> get props => [bookId, trackIndex, positionMs, atMs, playlist];
}

/// Tell the server where the listening is; the shell turns a network failure into a [QueueProgress].
class SaveProgress extends Effect {
  const SaveProgress(this.book, this.trackIndex, this.positionMs, this.finished);

  /// The book as it was when the progress was decided (the state may already have moved on to another one).
  final BookData book;
  String get bookId => book.id;
  final int trackIndex;
  final int positionMs;
  final bool finished;
  @override
  List<Object?> get props => [bookId, trackIndex, positionMs, finished];
}

/// Offline: the progress waits in the outbox of the UI.
class QueueProgress extends Effect {
  const QueueProgress(this.event);
  final ProgressQueued event;
  @override
  List<Object?> get props => [event.bookId, event.title, event.positionMs, event.finished, event.episodeId, event.item, event.part];
}

/// Episodes only: where the episode is, for the list of the downloaded episodes (and a heard one may go there).
class NoteEpisodeState extends Effect {
  const NoteEpisodeState(this.episodeId, this.positionMs, this.finished);
  final int episodeId;
  final int positionMs;
  final bool finished;
  @override
  List<Object?> get props => [episodeId, positionMs, finished];
}

class MarkHeard extends Effect {
  const MarkHeard(this.bookId, this.trackIndex);
  final String bookId;
  final int trackIndex;
  @override
  List<Object?> get props => [bookId, trackIndex];
}

/// A track (or episode) played to its end.
class TrackEnded extends Effect {
  const TrackEnded(this.bookId, this.trackIndex);
  final String bookId;
  final int trackIndex;
  @override
  List<Object?> get props => [bookId, trackIndex];
}

class PrefetchNext extends Effect {
  const PrefetchNext(this.trackIndex);
  final int trackIndex;
  @override
  List<Object?> get props => [trackIndex];
}

/// Publish the coarse snapshot (the state changed, or a command was handled).
class EmitSnapshot extends Effect {
  const EmitSnapshot();
  @override
  List<Object?> get props => [];
}

/// Publish only the position (a tick that changed nothing else).
class EmitPosition extends Effect {
  const EmitPosition();
  @override
  List<Object?> get props => [];
}

// ---- The transition function ----------------------------------------------------------------------

typedef Transition = ({EngineState state, List<Effect> effects});

/// What a widget that does not show the position depends on; if it changes, a snapshot is published, else a tick sends the position only.
List<Object?> coarseView(EngineState s) => [
      s.runtimeType,
      s.core.book?.id,
      s.core.trackIndex,
      s.wantsPlay,
      s.core.speed,
      s.core.streamTitle,
      s.core.streamDetails,
      s.core.heard.length,
      s is Failed ? s.message : null,
      s is Failed ? s.kind : null,
      s is Unplayable ? s.index : null,
    ];

bool _sameView(List<Object?> a, List<Object?> b) => _same(a, b);

/// The machine. An event that is not allowed in the state returns the same state (identical) and no effects.
Transition transition(EngineState state, EngineEvent event) {
  final r = _step(state, event);
  if (identical(r.state, state) && r.effects.isEmpty) return r;
  if (event is PrefetchFailed) return r;
  final effects = [...r.effects];
  if (event is Tick) {
    effects.add(_sameView(coarseView(state), coarseView(r.state)) ? const EmitPosition() : const EmitSnapshot());
  } else {
    effects.add(const EmitSnapshot());
  }
  return (state: r.state, effects: effects);
}

Transition _noop(EngineState s) => (state: s, effects: const []);
Transition _to(EngineState s, [List<Effect> e = const []]) => (state: s, effects: e);

Transition _step(EngineState s, EngineEvent e) {
  if (s is Closed) return _noop(s);
  final c = s.core;
  final now = e.nowMs;
  switch (e) {
    case UserOpen():
      var core = c;
      final eff = <Effect>[];
      final rep = _report(core, now);
      core = rep.$1;
      eff.addAll(rep.$2);
      final track = e.trackIndex ?? 0;
      core = core.copyWith(
        book: e.book,
        trackIndex: track,
        positionMs: e.startMs ?? 0,
        loadedKey: e.forceReload ? null : c.loadedKey,
      );
      return _setPlaying(s, core, true, LoadReason.open, now, eff);
    case UserToggle():
      if (c.book == null || s is Idle) return _noop(s);
      return _setPlaying(s, c, !s.wantsPlay, s is Loading ? s.reason : LoadReason.resume, now, const []);
    case UserPlay():
      if (s.wantsPlay || c.book == null) return _noop(s);
      return _setPlaying(s, c, true, LoadReason.resume, now, const []);
    case UserSeek():
      final t = c.track;
      if (t == null || c.kind == MediaKind.live) return _noop(s);
      return _seek(s, c, e.toMs);
    case UserSelectTrack():
      if (c.book == null) return _noop(s);
      final rep = _report(c, now);
      final core = rep.$1.copyWith(trackIndex: e.trackIndex, positionMs: e.positionMs);
      return _sync(s, core, s.wantsPlay, LoadReason.select, rep.$2);
    case UserSkip():
      final b = c.book;
      if (b == null || c.track == null || c.kind == MediaKind.live) return _noop(s);
      final last = b.tracks.length - 1;
      var idx = c.trackIndex;
      var p = c.positionMs + e.deltaMs;
      while (p >= b.tracks[idx].lengthMs && idx < last) {
        p -= b.tracks[idx].lengthMs;
        idx++;
      }
      while (p < 0 && idx > 0) {
        idx--;
        p += b.tracks[idx].lengthMs;
      }
      final synced = _sync(s, c.copyWith(trackIndex: idx, positionMs: p), s.wantsPlay, LoadReason.skip, const []);
      final sealed = _seek(synced.state, synced.state.core, p);
      return _to(sealed.state, [...synced.effects, ...sealed.effects]);
    case UserNext():
      final b = c.book;
      if (b == null || c.trackIndex >= b.tracks.length - 1) return _noop(s);
      final rep = _report(c, now);
      return _sync(s, rep.$1.copyWith(trackIndex: c.trackIndex + 1, positionMs: 0), s.wantsPlay, LoadReason.next, rep.$2);
    case UserPrevious():
      if (c.book == null) return _noop(s);
      final rep = _report(c, now);
      final back = c.positionMs > 3000 || c.trackIndex == 0;
      return _sync(s, rep.$1.copyWith(trackIndex: back ? c.trackIndex : c.trackIndex - 1, positionMs: 0), s.wantsPlay, LoadReason.previous, rep.$2);
    case SetSpeed():
      return _to(s.withCore(c.copyWith(speed: e.speed)), [if (c.useBackend) SetBackendSpeed(e.speed)]);
    case SetPlace():
      var core = c.copyWith(
        book: e.clearBook ? null : (e.book ?? c.book),
        trackIndex: e.trackIndex,
        positionMs: e.positionMs,
      );
      if (core.book == null) return _to(Idle(core.copyWith(loadedKey: null)));
      if (s is Idle) return _to(Paused(core));
      return _to(s.withCore(core));
    case Halt():
      final core = c.copyWith(loadedKey: null);
      return _to(core.book == null ? Idle(core) : Paused(core), [const StopTick(), if (c.env.hasBackend) const StopBackend()]);
    case ClearHeard():
      return _to(s.withCore(c.copyWith(heard: const {})));
    case EnvChanged():
      return _to(s.withCore(c.copyWith(
        env: c.env.copyWith(hasBackend: e.hasBackend, hasSource: e.hasSource, offlineActive: e.offlineActive, writesLast: e.writesLast),
      )));
    case Flush():
      final first = _report(c, now);
      final second = _writeLast(first.$1, now, true);
      return _to(s.withCore(second.$1), [...first.$2, ...second.$2]);
    case Dispose():
      final rep = _report(c, now);
      return _to(Closed(rep.$1), [...rep.$2, const StopTick(), if (c.env.hasBackend) const DisposeBackend()]);
    case Tick():
      return _tick(s, e);
    case ExternalChange():
      if (c.book == null || c.loadedKey == null || c.loadedKey != c.key || s is Idle || s is Loading) return _noop(s);
      final core = c.copyWith(positionMs: e.observation.positionMs);
      final st = e.observation.state;
      if (st == BackendState.paused && s.wantsPlay) {
        final rep = _report(core, now);
        return _to(Paused(rep.$1), [const StopTick(), ...rep.$2]);
      }
      if (st == BackendState.playing && !s.wantsPlay) return _to(Playing(core), const [StopTick(), StartTick()]);
      return _to(s.withCore(core));
    case BackendLoaded():
      if (s is! Loading || e.key != c.key) return _noop(s);
      final core = c.copyWith(loadedKey: e.key);
      return _to(s.wantPlay ? Playing(core) : Paused(core));
    case LoadFailed():
      if (s is! Loading || e.key != c.key) return _noop(s);
      if (e.transcodeFailed) return _to(Unplayable(c, c.trackIndex), const [StopTick()]);
      return _to(Failed(c, e.message, kind: e.kind), const [StopTick()]);
    case BackendFailed():
      if (s is Idle) return _noop(s);
      return _to(Failed(e.unload ? c.copyWith(loadedKey: null) : c, e.message, kind: e.kind), const [StopTick()]);
    case PrefetchFailed():
      return _to(s.withCore(c.copyWith(prefetchedKey: null)));
  }
}

/// The new state for [wantPlay], after the backend has been told (see [_sync]), plus the clock.
Transition _setPlaying(EngineState s, Core c, bool value, LoadReason reason, int now, List<Effect> before) {
  var core = c;
  final eff = <Effect>[const StopTick(), ...before];
  if (!value) {
    final rep = _report(core, now);
    core = rep.$1;
    eff.addAll(rep.$2);
  }
  final synced = _sync(s, core, value, reason, const []);
  eff.addAll(synced.effects);
  if (synced.state.wantsPlay) eff.add(const StartTick());
  return _to(synced.state, eff);
}

/// Makes the backend follow the open track: loads it if it is not the loaded one, then plays or pauses.
Transition _sync(EngineState s, Core c, bool wantPlay, LoadReason reason, List<Effect> before) {
  final eff = <Effect>[...before];
  final t = c.track;
  EngineState ready(Core k) => wantPlay ? Playing(k) : Paused(k);
  if (c.book == null || t == null) return _to(c.book == null ? Idle(c) : ready(c), eff);
  if (!c.useBackend) {
    if (c.loadedKey != null && c.env.hasBackend) eff.add(const StopBackend());
    return _to(ready(c.copyWith(loadedKey: null)), eff);
  }
  final key = c.key;
  if (key != c.loadedKey && !trackPlayableHere(t.format, t.transcode)) {
    if (c.loadedKey != null) eff.add(const StopBackend());
    return _to(Unplayable(c.copyWith(loadedKey: null), c.trackIndex), eff);
  }
  eff.add(EnsureSpeed(c.speed));
  if (key != c.loadedKey) {
    if (t.path == null) {
      eff.add(FetchAndLoad(key!, c.trackIndex));
      return _to(Loading(c, wantPlay: wantPlay, reason: reason), eff);
    }
    eff.add(LoadTrack(t.path!, c.positionMs, wantPlay, t.title));
    return _to(ready(c.copyWith(loadedKey: key)), eff);
  }
  eff.add(wantPlay ? const StartPlayback() : const PausePlayback());
  return _to(ready(c), eff);
}

Transition _seek(EngineState s, Core c, int toMs) {
  final t = c.track;
  if (t == null) return _noop(s);
  final p = toMs < 0 ? 0 : (toMs > t.lengthMs ? t.lengthMs : toMs);
  final core = c.copyWith(positionMs: p);
  final eff = <Effect>[if (c.useBackend && c.loadedKey == c.key) SeekBackend(p)];
  return _to(s is Ended ? Paused(core) : s.withCore(core), eff);
}

Transition _tick(EngineState s, Tick e) {
  if (s is! Playing && s is! Buffering) return _noop(s);
  final c = s.core;
  final t = c.track;
  final b = c.book;
  if (t == null || b == null) return _noop(s);
  final eff = <Effect>[];
  final last = b.tracks.length - 1;
  if (c.useBackend) {
    if (c.loadedKey != c.key) return _noop(s); // still fetching
    final o = e.observation;
    var core = c.copyWith(positionMs: o.positionMs, streamTitle: o.streamTitle, streamDetails: o.streamDetails);
    var r = _markHeard(core, ended: o.state == BackendState.ended);
    core = r.$1;
    eff.addAll(r.$2);
    r = _report(core, e.nowMs, force: false);
    core = r.$1;
    eff.addAll(r.$2);
    r = _prefetch(core);
    core = r.$1;
    eff.addAll(r.$2);
    if (o.state == BackendState.ended) {
      core = core.copyWith(positionMs: t.lengthMs);
      r = _report(core, e.nowMs, finished: true);
      core = r.$1;
      eff.addAll(r.$2);
      eff.add(TrackEnded(b.id, c.trackIndex));
      if (c.trackIndex < last) {
        // The end of every track is reported, not only the last one: the server counts a play from it.
        final synced = _sync(s, core.copyWith(trackIndex: c.trackIndex + 1, positionMs: 0), true, LoadReason.next, eff);
        return synced;
      }
      return _to(Ended(core), [...eff, const StopTick()]);
    }
    if (o.state == BackendState.failed) {
      return _to(Failed(core, o.errorMessage ?? 'playback failed', kind: o.errorKind), [...eff, const StopTick()]);
    }
    return _to(o.state == BackendState.buffering ? Buffering(core) : Playing(core), eff);
  }
  // No backend (a demo, a track without a file): the clock only runs on.
  var core = c.copyWith(positionMs: c.positionMs + (_tickMs * c.speed).round());
  final r = _markHeard(core);
  core = r.$1;
  eff.addAll(r.$2);
  if (core.positionMs >= t.lengthMs) {
    if (c.trackIndex < last) return _to(Playing(core.copyWith(trackIndex: c.trackIndex + 1, positionMs: 0)), eff);
    return _to(Ended(core.copyWith(positionMs: t.lengthMs)), [...eff, const StopTick()]);
  }
  return _to(s.withCore(core), eff);
}

(Core, List<Effect>) _markHeard(Core c, {bool ended = false}) {
  final b = c.book, t = c.track;
  if (b == null || t == null || t.live || b.kind == 'radio') return (c, const []);
  if (!(ended || (t.lengthMs > 0 && c.positionMs >= t.lengthMs - _heardTailMs))) return (c, const []);
  final k = '${b.id}:${c.trackIndex}';
  if (c.heard.contains(k)) return (c, const []);
  return (c.copyWith(heard: {...c.heard, k}), [MarkHeard(b.id, c.trackIndex)]);
}

(Core, List<Effect>) _prefetch(Core c) {
  final b = c.book, t = c.track;
  if (b == null || t == null || !c.env.hasSource || b.kind == 'radio' || t.live) return (c, const []);
  if (c.trackIndex >= b.tracks.length - 1 || t.lengthMs - c.positionMs > _prefetchWithinMs) return (c, const []);
  final next = b.tracks[c.trackIndex + 1];
  final key = '${b.id}:${c.trackIndex + 1}';
  if (next.path != null || c.prefetchedKey == key) return (c, const []);
  return (c.copyWith(prefetchedKey: key), [PrefetchNext(c.trackIndex + 1)]);
}

(Core, List<Effect>) _writeLast(Core c, int now, bool force) {
  final b = c.book;
  if (!c.env.writesLast || b == null || b.tracks.isEmpty) return (c, const []);
  if (!force && now - c.lastWrittenMs < _writeEveryMs) return (c, const []);
  return (c.copyWith(lastWrittenMs: now), [WriteLast(b.id, c.trackIndex, c.positionMs, now, b.kind == kPlaylistKind)]);
}

/// Writes the place down and tells the server where the listening is (at most every 10 s unless [force]d).
(Core, List<Effect>) _report(Core c, int now, {bool finished = false, bool force = true}) {
  final w = _writeLast(c, now, force);
  var core = w.$1;
  final eff = <Effect>[...w.$2];
  final b = core.book, t = core.track;
  final episode = b == null ? null : episodeIdOf(b);
  if (b == null || t == null || !core.env.hasSource || (t.partId == null && episode == null)) return (core, eff);
  if (!force && now - core.lastReportMs < _reportEveryMs) return (core, eff);
  core = core.copyWith(lastReportMs: now);
  if (episode != null) eff.add(NoteEpisodeState(episode, core.positionMs, finished));
  if (core.env.offlineActive) {
    final q = progressQueue(b, t, core.positionMs, finished);
    if (q != null) eff.add(QueueProgress(q));
  } else {
    eff.add(SaveProgress(b, core.trackIndex, core.positionMs, finished));
  }
  return (core, eff);
}

/// The progress as the outbox keeps it, or null for what has no place to keep (a station, a podcast that is not an episode, no ids).
ProgressQueued? progressQueue(BookData b, TrackData t, int positionMs, bool finished) {
  if (b.kind == 'radio') return null;
  final episode = episodeIdOf(b);
  if (episode != null) {
    return ProgressQueued(bookId: b.id, title: b.title, positionMs: positionMs, finished: finished, episodeId: episode);
  }
  final part = t.partId;
  final item = int.tryParse(t.itemId ?? b.id);
  if (part == null || item == null || b.kind == 'podcast') return null;
  return ProgressQueued(bookId: b.id, title: b.title, positionMs: positionMs, finished: finished, item: item, part: part);
}
