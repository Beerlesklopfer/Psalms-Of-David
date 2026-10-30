// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';

import 'models.dart';

/// What the podcast and radio lists need from the server. [ApiClient] is the real one; tests use a fake.
abstract class FeedApi {
  Future<List<Map<String, dynamic>>> podcasts();
  Future<Map<String, dynamic>> followPodcast(String url);
  Future<void> unfollowPodcast(int id);
  Future<List<Map<String, dynamic>>> episodes(int podcastId);
  Future<List<Map<String, dynamic>>> stations();
  Future<Map<String, dynamic>> addStation(String name, String streamUrl, {String? homepage});
  Future<void> removeStation(int id);
}

/// A podcast the account follows. [approved] is false while a request of a child waits for a release.
class Podcast {
  const Podcast({required this.id, required this.title, required this.description, required this.approved, required this.unheard, this.imageUrl, this.error, this.labels = const [], this.canLabel = false});

  factory Podcast.fromJson(Map<String, dynamic> j) => Podcast(
        id: j['id'] as int,
        title: j['title'] as String,
        description: j['description'] as String? ?? '',
        approved: j['approved'] as bool? ?? true,
        unheard: j['unheard'] as int? ?? 0,
        imageUrl: j['image_url'] as String?,
        error: j['last_error'] as String?,
        labels: [for (final l in (j['labels'] as List? ?? const [])) l as String],
        canLabel: j['can_label'] == true,
      );

  final int id;
  final String title;
  final String description;
  final bool approved;
  final int unheard;
  final String? imageUrl;
  final String? error;

  /// The labels (classic tagging) of the podcast; everybody who follows it sees them.
  final List<String> labels;

  /// This user may set the labels: the server says so (an administrator, or a member who follows it).
  final bool canLabel;
}

/// One episode with where this account stopped (the same on every device of the account).
class Episode {
  const Episode({required this.id, required this.title, required this.audioUrl, required this.published, required this.duration, required this.position, required this.finished, this.description = ''});

  factory Episode.fromJson(Map<String, dynamic> j) => Episode(
        id: j['id'] as int,
        title: j['title'] as String,
        audioUrl: j['audio_url'] as String,
        published: j['published_at'] == null || (j['published_at'] as int) == 0 ? null : DateTime.fromMillisecondsSinceEpoch(j['published_at'] as int),
        duration: Duration(seconds: j['duration_s'] as int? ?? 0),
        position: Duration(milliseconds: j['position_ms'] as int? ?? 0),
        finished: j['finished'] as bool? ?? false,
        description: j['description'] as String? ?? '',
      );

  final int id;
  final String title;
  final String audioUrl;
  final DateTime? published;
  final Duration duration;
  final Duration position;
  final bool finished;
  final String description;

  /// The episode as something the player plays: one track, from the publisher. Its id says which episode it is,
  /// so the position goes back to the server.
  Book asBook(String podcast) => Book(
        id: 'ep:$id',
        title: title,
        author: podcast,
        kind: 'podcast',
        color: const Color(0xFF2C4A3E),
        tracks: [Track(disc: 1, number: 1, title: title, length: duration, url: audioUrl)],
      );
}

/// A radio station of the favourites.
class Station {
  const Station({required this.id, required this.name, required this.streamUrl, required this.approved, this.homepage, this.logoUrl, this.labels = const [], this.canLabel = false});

  factory Station.fromJson(Map<String, dynamic> j) => Station(
        id: j['id'] as int,
        name: j['name'] as String,
        streamUrl: j['stream_url'] as String,
        approved: j['approved'] as bool? ?? true,
        homepage: j['homepage'] as String?,
        logoUrl: j['logo_url'] as String?,
        labels: [for (final l in (j['labels'] as List? ?? const [])) l as String],
        canLabel: j['can_label'] == true,
      );

  final int id;
  final String name;
  final String streamUrl;
  final bool approved;
  final String? homepage;
  final String? logoUrl;

  /// The labels (classic tagging) of the station, and whether this user may set them.
  final List<String> labels;
  final bool canLabel;

  /// The station as something the player plays: a live stream without an end.
  Book asBook({Uint8List? logo}) => Book(
        id: 'st:$id',
        title: name,
        author: '',
        kind: 'radio',
        cover: logo,
        coverUrl: logoUrl,
        color: const Color(0xFF4A3B1F),
        tracks: [Track(disc: 1, number: 1, title: name, length: Duration.zero, url: streamUrl, live: true)],
      );
}
