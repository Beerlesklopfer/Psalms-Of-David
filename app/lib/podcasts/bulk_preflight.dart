// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import '../podcast_downloads.dart';
import '../podcasts.dart' show Episode;
import 'download_gate.dart';

/// What the checks in front of a bulk download found, before the person is asked.
sealed class BulkPreflight {
  const BulkPreflight();
}

/// The device is on mobile data (or offline) and the setting says collections are only loaded on Wi-Fi.
class BulkMobileData extends BulkPreflight {
  const BulkMobileData();
}

/// The checks are passed: the plan says what would be loaded (cut by the limit and by the room that is free).
class BulkReady extends BulkPreflight {
  const BulkReady(this.plan);

  final BulkPlan plan;
}

/// Is the connection allowed, and how much of [todo] fits the limit and the room of the device?
Future<BulkPreflight> preflightBulk(PodcastDownloads downloads, List<Episode> todo) async {
  if (downloads.bulkWifiOnly && await downloads.environment.unmetered() == false) return const BulkMobileData();
  final free = await downloads.environment.freeBytes(downloads.cache.path);
  return BulkReady(planBulk(todo, limitMegabytes: downloads.bulkMegabytes, freeBytes: free));
}
