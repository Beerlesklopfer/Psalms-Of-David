// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import 'audio/audio_backend.dart';
import 'l10n/app_localizations.dart';
import 'player_controller.dart';

/// What a station tells about what it plays and about itself: artist and track from the announced title, and the name, genre,
/// quality and address from the headers of the connection. Rows without a value are left out.
class StreamInfoCard extends StatelessWidget {
  const StreamInfoCard({super.key, required this.player});

  final PlayerController player;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    final now = player.nowPlaying;
    final d = player.streamDetails;
    final split = now == null ? null : splitStreamTitle(now);
    final quality = [
      if (d?.bitrateKbps != null) '${d!.bitrateKbps} kbit/s',
      if (d?.format != null) d!.format!,
    ].join(' · ');
    final rows = <(String, String)>[
      if (split?.artist != null) (l.streamArtist, split!.artist!),
      if (split != null) (l.streamTrack, split.track),
      if (d?.name != null) (l.streamStation, d!.name!),
      if (d?.genre != null) (l.streamGenre, d!.genre!),
      if (d?.description != null && d!.description != d.name) (l.streamDescription, d.description!),
      if (quality.isNotEmpty) (l.streamQuality, quality),
      if (d?.website != null) (l.streamWebsite, d!.website!),
    ];
    return Card(
      key: const Key('stream-info'),
      margin: EdgeInsets.zero,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: rows.isEmpty
            ? Text(l.streamWaiting, style: text.bodyMedium?.copyWith(color: cs.onSurfaceVariant))
            : Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (final (label, value) in rows)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 3),
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          SizedBox(
                            width: 96,
                            child: Text(
                              label,
                              style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant),
                            ),
                          ),
                          Expanded(child: SelectableText(value, style: text.bodyMedium)),
                        ],
                      ),
                    ),
                ],
              ),
      ),
    );
  }
}
