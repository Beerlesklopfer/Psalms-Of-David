// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../player_controller.dart';
import '../podcasts.dart';
import '../server/api_client.dart';
import '../station_logo.dart';
import '../widgets.dart';

/// The state of the station list; same roles as [PodcastsController].
class RadioController extends ChangeNotifier {
  RadioController(this.api) {
    load();
  }

  final FeedApi? api;
  List<Station>? stations;
  String? error;
  bool _disposed = false;

  Future<void> load() async {
    final api = this.api;
    if (api == null) return;
    try {
      final rows = await api.stations();
      stations = rows.map(Station.fromJson).toList();
      error = null;
    } on ApiException catch (e) {
      error = e.code;
    }
    if (!_disposed) notifyListeners();
  }

  Future<void> add(BuildContext context) async {
    final l = AppLocalizations.of(context);
    final entry = await askFields(
      context,
      title: l.stationAdd,
      fields: [
        (label: l.stationName, hint: null),
        (label: l.stationAddress, hint: 'https://stream.example.org/live'),
      ],
      action: l.stationSave,
    );
    if (entry == null || !context.mounted) return;
    final (name, url) = (entry[0], entry[1]);
    final messenger = ScaffoldMessenger.of(context);
    try {
      final r = await api!.addStation(name, url);
      messenger.showSnackBar(
        SnackBar(content: Text(r['approved'] == true ? l.stationAdded(name) : l.askToRelease)),
      );
      await load();
    } on ApiException catch (e) {
      messenger.showSnackBar(
        SnackBar(
          content: Text(
            e.code == 'invalid_station'
                ? l.stationInvalid
                : e.code == 'forbidden'
                ? l.notAllowed
                : l.serverUnreachableShort,
          ),
        ),
      );
    }
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }
}

/// The add and refresh buttons of the station list (for an app bar).
class RadioActions extends StatelessWidget {
  const RadioActions({super.key, required this.controller, this.withRefresh = true});

  final RadioController controller;

  /// The page of its own has a refresh button of its own; in the library the one of the library reloads the lists too.
  final bool withRefresh;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    if (controller.api == null) return const SizedBox.shrink();
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        IconButton(
          tooltip: l.stationAdd,
          icon: const Icon(Icons.add),
          onPressed: () => controller.add(context),
        ),
        if (withRefresh)
          IconButton(
            tooltip: l.tooltipRefresh,
            icon: const Icon(Icons.refresh),
            onPressed: controller.load,
          ),
      ],
    );
  }
}

/// The station list as a sliver, for the page of its own and for the library.
class RadioBody extends StatelessWidget {
  const RadioBody({super.key, required this.controller});

  final RadioController controller;

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = PlayerScope.of(context);
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final list = controller.stations;
        if (controller.api == null)
          return SliverFillRemaining(
            hasScrollBody: false,
            child: Center(child: Text(l.needsServer)),
          );
        if (controller.error != null)
          return SliverFillRemaining(
            hasScrollBody: false,
            child: Center(child: Text(l.serverUnreachableShort)),
          );
        if (list == null)
          return const SliverFillRemaining(
            hasScrollBody: false,
            child: Center(child: CircularProgressIndicator()),
          );
        if (list.isEmpty)
          return SliverFillRemaining(
            hasScrollBody: false,
            child: Center(child: Text(l.noStationsYet)),
          );
        return SliverList.builder(
          itemCount: list.length,
          itemBuilder: (context, i) {
            final s = list[i];
            return ListTile(
              leading: _StationIcon(station: s),
              title: Text(s.name),
              subtitle: s.approved ? null : Text(l.waitingForRelease),
              enabled: s.approved,
              trailing: IconButton(
                tooltip: l.stationRemove,
                icon: const Icon(Icons.delete_outline),
                onPressed: () async {
                  await controller.api!.removeStation(s.id);
                  await controller.load();
                },
              ),
              onTap: s.approved
                  ? () async {
                      // The logo is fetched once; without one the cover is drawn.
                      final logo = await StationLogos.load(s.logoUrl);
                      p.open(s.asBook(logo: logo));
                    }
                  : null,
            );
          },
        );
      },
    );
  }
}

/// The favourite radio stations of the account (the same on every device of it).
class RadioScreen extends StatefulWidget {
  const RadioScreen({super.key, required this.api});

  final FeedApi? api;

  @override
  State<RadioScreen> createState() => _RadioScreenState();
}

class _RadioScreenState extends State<RadioScreen> {
  late final RadioController _controller = RadioController(widget.api);

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return CustomScrollView(
      slivers: [
        SliverAppBar(
          floating: true,
          backgroundColor: Theme.of(context).colorScheme.surface,
          title: Text(l.navRadio, style: Theme.of(context).textTheme.headlineSmall),
          actions: [RadioActions(controller: _controller)],
        ),
        RadioBody(controller: _controller),
      ],
    );
  }
}

/// The picture of a station in the list: its logo once fetched, else the radio symbol.
class _StationIcon extends StatelessWidget {
  const _StationIcon({required this.station});

  final Station station;

  @override
  Widget build(BuildContext context) {
    if (station.logoUrl == null) return const Icon(Icons.radio);
    return SizedBox.square(
      dimension: 40,
      child: FutureBuilder<Uint8List?>(
        future: StationLogos.load(station.logoUrl),
        builder: (context, snapshot) {
          final bytes = snapshot.data;
          if (bytes == null) return const Icon(Icons.radio);
          return ClipRRect(
            borderRadius: BorderRadius.circular(3),
            child: Image.memory(
              bytes,
              fit: BoxFit.cover,
              errorBuilder: (context, error, stack) => const Icon(Icons.radio),
            ),
          );
        },
      ),
    );
  }
}
