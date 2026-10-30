// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later

// The life cycle of ONE downloaded podcast episode as a PURE function: `transition(state, event) -> (state, effects)`.
// No files, no network: the shell (`PodcastDownloads`) carries out the [DownloadEffect]s and feeds the results back as [DownloadEvent]s.
// The document is docs/spec-state-machines.md.

// ---- States -----------------------------------------------------------------------------------------

sealed class DownloadState {
  const DownloadState();
}

/// Not on the device and nothing running (the start state of every episode).
class NotDownloaded extends DownloadState {
  const NotDownloaded();
}

/// Accepted, the fetch is about to start.
class Queued extends DownloadState {
  const Queued();
}

/// Bytes are coming in. [total] is null when the publisher does not say.
class Downloading extends DownloadState {
  const Downloading({this.received = 0, this.total});
  final int received;
  final int? total;
}

/// The person asked to stop; the transfer is still winding down (it ends with an `IoError('cancelled')`).
class Cancelling extends DownloadState {
  const Cancelling({this.received = 0, this.total});
  final int received;
  final int? total;
}

/// The file is on the device.
class Downloaded extends DownloadState {
  const Downloaded();
}

/// The last try stopped for [reason] (the code of `EpisodeDownloadException`: scheme, http, too_large, network, io).
class Failed extends DownloadState {
  const Failed(this.reason);
  final String reason;
}

/// The last try was stopped by the person.
class Cancelled extends DownloadState {
  const Cancelled();
}

/// The file is being removed.
class Deleting extends DownloadState {
  const Deleting();
}

// ---- Events -----------------------------------------------------------------------------------------

sealed class DownloadEvent {
  const DownloadEvent();
}

/// The person wants the episode on the device. [webAddress]: the address is http(s). [onDisk]: the file is there already (it was played).
class Download extends DownloadEvent {
  const Download({required this.webAddress, required this.onDisk});
  final bool webAddress;
  final bool onDisk;
}

class Cancel extends DownloadEvent {
  const Cancel();
}

class Delete extends DownloadEvent {
  const Delete();
}

/// The episode was heard to the end; it goes when [enabled] (the setting "delete after listening").
class DeleteAfterListening extends DownloadEvent {
  const DeleteAfterListening({required this.enabled});
  final bool enabled;
}

/// The shell began the transfer.
class FetchStarted extends DownloadEvent {
  const FetchStarted();
}

class Progress extends DownloadEvent {
  const Progress(this.received, this.total);
  final int received;
  final int? total;
}

/// The transfer is complete.
class Done extends DownloadEvent {
  const Done(this.bytes);
  final int bytes;
}

/// The transfer stopped. [code] is `cancelled` or one of the codes of `EpisodeDownloadException`.
class IoError extends DownloadEvent {
  const IoError(this.code);
  final String code;
}

/// The file was removed from the device.
class Deleted extends DownloadEvent {
  const Deleted();
}

/// The file is on the device although this machine did not fetch it (it was played, found in a listing), or is not known any more
/// ([forgotten]: gone by hand, "delete all", or the list was forgotten). The shell keeps the data; no effects.
class FileFound extends DownloadEvent {
  const FileFound();
}

class FileGone extends DownloadEvent {
  const FileGone();
}

// ---- Effects ----------------------------------------------------------------------------------------

sealed class DownloadEffect {
  const DownloadEffect();
}

/// Open the transfer (the shell answers with [FetchStarted], then [Progress]..., then [Done] or [IoError]).
class StartFetch extends DownloadEffect {
  const StartFetch();
  @override
  bool operator ==(Object other) => other is StartFetch;
  @override
  int get hashCode => 1;
}

/// Stop the transfer through its token.
class CancelFetch extends DownloadEffect {
  const CancelFetch();
  @override
  bool operator ==(Object other) => other is CancelFetch;
  @override
  int get hashCode => 2;
}

/// Write the entry of the episode into the list and save it.
class Register extends DownloadEffect {
  const Register();
  @override
  bool operator ==(Object other) => other is Register;
  @override
  int get hashCode => 3;
}

/// Remove the file and the entry, save, then answer with [Deleted].
class DeleteFile extends DownloadEffect {
  const DeleteFile();
  @override
  bool operator ==(Object other) => other is DeleteFile;
  @override
  int get hashCode => 4;
}

/// Tell the listeners; [force] false means "throttled" (progress).
class Notify extends DownloadEffect {
  const Notify({this.force = true});
  final bool force;
  @override
  bool operator ==(Object other) => other is Notify && other.force == force;
  @override
  int get hashCode => force.hashCode;
}

typedef DownloadResult = (DownloadState state, List<DownloadEffect> effects);

/// An illegal event: the same state object and no effects.
DownloadResult _ignore(DownloadState s) => (s, const []);

bool isActive(DownloadState s) => s is Queued || s is Downloading || s is Cancelling;

DownloadResult transition(DownloadState state, DownloadEvent event) {
  switch (event) {
    case Download():
      return switch (state) {
        NotDownloaded() || Failed() || Cancelled() => _start(state, event),
        Downloaded() => event.onDisk ? (state, const [Register(), Notify()]) : (event.webAddress ? (const Queued(), const [StartFetch()]) : _ignore(state)),
        _ => _ignore(state),
      };
    case Cancel():
      return switch (state) {
        Queued() => (const Cancelled(), const [CancelFetch(), Notify()]),
        Downloading(:final received, :final total) => (Cancelling(received: received, total: total), const [CancelFetch()]),
        _ => _ignore(state),
      };
    case Delete():
      return switch (state) {
        Downloaded() => (const Deleting(), const [DeleteFile()]),
        Failed() || Cancelled() => (const NotDownloaded(), const [Notify()]),
        _ => _ignore(state),
      };
    case DeleteAfterListening(:final enabled):
      return state is Downloaded && enabled ? (const Deleting(), const [DeleteFile()]) : _ignore(state);
    case FetchStarted():
      return state is Queued ? (const Downloading(), const [Notify()]) : _ignore(state);
    case Progress(:final received, :final total):
      return switch (state) {
        Downloading() => (Downloading(received: received, total: total), const [Notify(force: false)]),
        Cancelling() => (Cancelling(received: received, total: total), const [Notify(force: false)]),
        _ => _ignore(state),
      };
    case Done():
      return isActive(state) ? (const Downloaded(), const [Register(), Notify()]) : _ignore(state);
    case IoError(:final code):
      if (!isActive(state)) return _ignore(state);
      return (code == 'cancelled' ? const Cancelled() : Failed(code), const [Notify()]);
    case Deleted():
      return state is Deleting ? (const NotDownloaded(), const [Notify()]) : _ignore(state);
    case FileFound():
      return switch (state) {
        NotDownloaded() || Failed() || Cancelled() => (const Downloaded(), const []),
        _ => _ignore(state),
      };
    case FileGone():
      return switch (state) {
        Downloaded() || Deleting() => (const NotDownloaded(), const []),
        _ => _ignore(state),
      };
  }
}

DownloadResult _start(DownloadState state, Download e) {
  if (!e.webAddress) return (const Failed('scheme'), const []);
  if (e.onDisk) return (const Downloaded(), const [Register(), Notify()]);
  return (const Queued(), const [StartFetch()]);
}
