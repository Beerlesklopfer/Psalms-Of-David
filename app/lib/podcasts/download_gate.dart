// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import '../podcasts.dart' show Episode;

/// The gate in front of "download the whole collection" of a podcast (project owner, 2026-10-30: a collection can take several gigabytes, so it
/// never starts by itself and never exceeds the limit the person set). Pure: it only counts; the dialog and the downloads are elsewhere.

/// The limits of one bulk download that the settings offer, in megabytes (up to 20 GB).
const kBulkSizeChoices = [250, 500, 1000, 2000, 5000, 10000, 20000];

/// The limit of one bulk download until the person changes it: 1 GB.
const kBulkDefaultMegabytes = 1000;

/// The bytes per second of a typical podcast MP3 (128 kbit/s): the size of an episode is estimated from its duration, the list does not carry sizes.
const int kEstimatedBytesPerSecond = 16000;

/// The size assumed for an episode that has no duration either.
const int kAssumedEpisodeBytes = 40 * 1000 * 1000;

/// About how large [e] is on the device.
int estimatedEpisodeBytes(Episode e) => e.duration > Duration.zero ? e.duration.inSeconds * kEstimatedBytesPerSecond : kAssumedEpisodeBytes;

/// The room that always stays free on the device, whatever a bulk download would still fit: 1 GB.
const int kFreeSpaceReserveBytes = 1000 * 1000 * 1000;

/// What a bulk download would do: [all] is everything that is missing on the device (the newest first), [fitting] the newest of them that fit in
/// the limit. The two are the same when everything fits. [freeBytes] is the room the device has (null: not known); when it, minus
/// [kFreeSpaceReserveBytes], is less than the limit of the person, the room is what cuts the plan ([spaceBound]).
class BulkPlan {
  const BulkPlan({required this.all, required this.fitting, required this.limitBytes, this.freeBytes, this.spaceBound = false});

  final List<Episode> all;
  final List<Episode> fitting;
  final int limitBytes;
  final int? freeBytes;
  final bool spaceBound;

  int get count => all.length;
  int get fittingCount => fitting.length;
  int get totalBytes => all.fold(0, (sum, e) => sum + estimatedEpisodeBytes(e));
  int get fittingBytes => fitting.fold(0, (sum, e) => sum + estimatedEpisodeBytes(e));

  /// More is missing than the limit allows.
  bool get exceedsLimit => fitting.length < all.length;

  /// Not even the newest episode fits: nothing can be loaded with this limit.
  bool get blocked => all.isNotEmpty && fitting.isEmpty;
}

/// Counts [todo] (the newest first) against [limitMegabytes]: the newest episodes are taken as long as their estimated sizes add up to at most the limit.
BulkPlan planBulk(List<Episode> todo, {required int limitMegabytes, int? freeBytes}) {
  var limit = limitMegabytes * 1000 * 1000;
  var spaceBound = false;
  if (freeBytes != null) {
    final room = freeBytes - kFreeSpaceReserveBytes;
    if (room < limit) {
      limit = room < 0 ? 0 : room;
      spaceBound = true;
    }
  }
  final fitting = <Episode>[];
  var used = 0;
  for (final e in todo) {
    final next = used + estimatedEpisodeBytes(e);
    if (next > limit) break;
    used = next;
    fitting.add(e);
  }
  return BulkPlan(all: List.unmodifiable(todo), fitting: List.unmodifiable(fitting), limitBytes: limit, freeBytes: freeBytes, spaceBound: spaceBound);
}
