// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import '../l10n/app_localizations.dart';
import '../models.dart';
import '../offline/offline_controller.dart';
import '../player_controller.dart';
import '../profile.dart';
import '../screens/confirm_dialog.dart';
import '../widgets.dart';
import 'inbox_controller.dart';
import 'inbox_machine.dart';
import 'inbox_models.dart';
import 'inbox_text.dart';

/// Gives the pages below the shell the inbox, and rebuilds them when it changes.
class InboxScope extends InheritedNotifier<InboxController> {
  const InboxScope({
    super.key,
    required InboxController controller,
    required super.child,
  }) : super(notifier: controller);

  static InboxController? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<InboxScope>()?.notifier;
}

/// Does the inbox show at all? Only for a profile with a CD right (a child and a guest have none and see nothing), and in the demo.
bool inboxAllowed(BuildContext context, InboxController c) =>
    c.demo ||
    (ProfileScope.maybeOf(context)?.profile?.cdRights.hasInbox ?? false);

/// The unread count as a badge on [child] (the profile entry); nothing when there is none or the person has no inbox.
class InboxBadge extends StatelessWidget {
  const InboxBadge({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    final c = InboxScope.maybeOf(context);
    if (c == null || !inboxAllowed(context, c) || c.state.unread <= 0) {
      return child;
    }
    return Badge.count(
      key: const ValueKey('inbox-badge'),
      count: c.state.unread,
      child: child,
    );
  }
}

/// "2 CDs wait for approval · 3 unread" at the top of the library; opens the inbox. Hidden when there is nothing or no right.
class InboxCard extends StatelessWidget {
  const InboxCard({super.key});

  @override
  Widget build(BuildContext context) {
    final c = InboxScope.maybeOf(context);
    if (c == null || !inboxAllowed(context, c)) return const SizedBox.shrink();
    final s = c.state;
    if (s.tasksOpen <= 0 && s.unread <= 0) return const SizedBox.shrink();
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
      child: Card(
        key: const ValueKey('inbox-card'),
        margin: EdgeInsets.zero,
        color: s.tasksOpen > 0 ? cs.secondaryContainer : null,
        clipBehavior: Clip.antiAlias,
        child: ListTile(
          leading: Icon(
            s.tasksOpen > 0 ? Icons.album : Icons.mail_outline,
            color: s.tasksOpen > 0 ? cs.onSecondaryContainer : null,
          ),
          title: Text(
            s.tasksOpen > 0
                ? l.inboxCardWaiting(s.tasksOpen)
                : l.inboxCardUnread(s.unread),
          ),
          subtitle: s.tasksOpen > 0 && s.unread > 0
              ? Text(l.inboxCardUnread(s.unread))
              : null,
          trailing: const Icon(Icons.chevron_right),
          onTap: () => openInbox(context, c),
        ),
      ),
    );
  }
}

/// The entry of the settings page under the profile: opens the inbox, with the unread count.
class InboxTile extends StatelessWidget {
  const InboxTile({super.key});

  @override
  Widget build(BuildContext context) {
    final c = InboxScope.maybeOf(context);
    if (c == null || !inboxAllowed(context, c)) return const SizedBox.shrink();
    final l = AppLocalizations.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: 24),
      child: Card(
        margin: EdgeInsets.zero,
        clipBehavior: Clip.antiAlias,
        child: ListTile(
          key: const ValueKey('open-inbox'),
          leading: const InboxBadge(child: Icon(Icons.inbox_outlined)),
          title: Text(l.inboxTitle),
          trailing: const Icon(Icons.chevron_right),
          onTap: () => openInbox(context, c),
        ),
      ),
    );
  }
}

/// Opens the inbox page. The offline state is read here because the page is a route above the shell.
void openInbox(
  BuildContext context,
  InboxController c, {
  OfflineController? offline,
}) {
  offline ??= OfflineScope.maybeOf(context);
  final player = PlayerScope.maybeOf(context);
  Navigator.of(context).push(
    MaterialPageRoute<void>(
      builder: (_) =>
          InboxScreen(controller: c, offline: offline, player: player),
    ),
  );
}

/// The inbox: newest first, a card per message with the buttons the server allows. It only shows [InboxController.state].
class InboxScreen extends StatefulWidget {
  const InboxScreen({
    super.key,
    required this.controller,
    this.offline,
    this.player,
  });

  final InboxController controller;

  /// Offline mode: the inbox needs the server, so the page says so.
  final OfflineController? offline;

  /// For "Open album" (the albums of the library).
  final PlayerController? player;

  @override
  State<InboxScreen> createState() => _InboxScreenState();
}

class _InboxScreenState extends State<InboxScreen> {
  InboxController get _c => widget.controller;
  final _scroll = ScrollController();

  @override
  void initState() {
    super.initState();
    _scroll.addListener(() {
      if (_scroll.hasClients && _scroll.position.extentAfter < 300) _c.more();
    });
    // Opening the page reads the list again: the server is the truth. (After the frame: the state changes, and a build is going on.)
    if (!(widget.offline?.active ?? false)) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _c.dispatch(const InboxRefresh());
      });
    }
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _reject(InboxItem item) async {
    final l = AppLocalizations.of(context);
    final yes = await confirmRemoval(
      context,
      title: l.inboxRejectTitle,
      text: l.inboxRejectText,
      action: l.inboxReject,
    );
    if (yes) _c.reject(item.id);
  }

  void _openAlbum(InboxItem item) {
    final l = AppLocalizations.of(context);
    final byId = {
      for (final b in widget.player?.fullLibrary ?? const <Book>[]) b.id: b,
    };
    final book = byId['${item.albumId}'];
    if (book == null) {
      ScaffoldMessenger.of(context)
          .showSnackBar(SnackBar(content: Text(l.inboxAlbumNotFound)));
      return;
    }
    openBook(context, book);
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return ListenableBuilder(
      listenable: Listenable.merge([
        _c,
        if (widget.offline != null) widget.offline!,
      ]),
      builder: (context, _) {
        final s = _c.state;
        final offline = widget.offline?.active ?? false;
        return Scaffold(
          appBar: AppBar(
            title: Text(l.inboxTitle),
            actions: [
              if (!offline && s.unread > 0)
                IconButton(
                  key: const ValueKey('inbox-mark-all'),
                  tooltip: l.inboxMarkAllRead,
                  icon: const Icon(Icons.done_all),
                  onPressed: _c.markAllRead,
                ),
            ],
          ),
          body: offline
              ? _message(
                  context,
                  Icons.cloud_off_outlined,
                  l.inboxNeedsServer,
                  const ValueKey('inbox-needs-server'),
                )
              : _body(context, s),
        );
      },
    );
  }

  Widget _message(
    BuildContext context,
    IconData icon,
    String text,
    Key key, {
    Widget? action,
  }) => Center(
    child: Padding(
      padding: const EdgeInsets.all(32),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 48),
          const SizedBox(height: 12),
          Text(
            text,
            key: key,
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.bodyLarge,
          ),
          if (action != null) ...[const SizedBox(height: 12), action],
        ],
      ),
    ),
  );

  Widget _body(BuildContext context, InboxState s) {
    final l = AppLocalizations.of(context);
    if (s.phase == InboxPhase.idle ||
        (s.phase == InboxPhase.loading && s.items.isEmpty)) {
      return const Center(
        child: CircularProgressIndicator(key: ValueKey('inbox-loading')),
      );
    }
    if (s.phase == InboxPhase.error && s.items.isEmpty) {
      return _message(
        context,
        Icons.error_outline,
        l.inboxLoadFailed,
        const ValueKey('inbox-error'),
        action: FilledButton(
          key: const ValueKey('inbox-reload'),
          onPressed: () => _c.dispatch(const InboxRefresh()),
          child: Text(l.inboxRetry),
        ),
      );
    }
    return RefreshIndicator(
      onRefresh: _c.refresh,
      child: s.items.isEmpty
          ? ListView(
              physics: const AlwaysScrollableScrollPhysics(),
              children: [
                const SizedBox(height: 120),
                Center(
                  child: Text(
                    l.inboxEmpty,
                    key: const ValueKey('inbox-empty'),
                    style: Theme.of(context).textTheme.bodyLarge,
                  ),
                ),
              ],
            )
          : ListView.builder(
              controller: _scroll,
              physics: const AlwaysScrollableScrollPhysics(),
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 24),
              itemCount: s.items.length,
              itemBuilder: (context, i) => _ItemCard(
                key: ValueKey('inbox-item-${s.items[i].id}'),
                item: s.items[i],
                deciding: s.deciding.contains(s.items[i].id),
                retrying: s.retrying.contains(s.items[i].id),
                failure: s.failures[s.items[i].id],
                onOpen: () => _c.markRead(s.items[i].id),
                onApprove: () => _c.approve(s.items[i].id),
                onReject: () => _reject(s.items[i]),
                onRetry: () => _c.retry(s.items[i].id),
                onAlbum: () => _openAlbum(s.items[i]),
              ),
            ),
    );
  }
}

String _failureText(AppLocalizations l, String code, {String? by}) =>
    switch (code) {
      'network' => l.inboxErrNetwork,
      'forbidden' => l.inboxErrForbidden,
      'busy' => l.inboxErrBusy,
      'no_disc' || 'tray_open' => l.inboxErrNoDisc,
      'no_drive' || 'drive_denied' => l.inboxErrDrive,
      'decided' => l.inboxAlreadyDecided(by ?? ''),
      _ => l.inboxErrGeneric,
    };

class _ItemCard extends StatelessWidget {
  const _ItemCard({
    super.key,
    required this.item,
    required this.deciding,
    required this.retrying,
    required this.failure,
    required this.onOpen,
    required this.onApprove,
    required this.onReject,
    required this.onRetry,
    required this.onAlbum,
  });

  final InboxItem item;
  final bool deciding;
  final bool retrying;
  final String? failure;
  final VoidCallback onOpen;
  final VoidCallback onApprove;
  final VoidCallback onReject;
  final VoidCallback onRetry;
  final VoidCallback onAlbum;

  IconData get _icon => switch (item.code) {
    'cd_awaiting_approval' => Icons.hourglass_top,
    'cd_imported' || 'cd_present' => Icons.library_music_outlined,
    'cd_decided' => Icons.how_to_vote_outlined,
    'cd_expired' => Icons.timer_off_outlined,
    'cd_read_error' => Icons.error_outline,
    'cd_no_match' || 'cd_ambiguous' => Icons.help_outline,
    'cd_heal_needed' || 'cd_no_space' || 'cd_no_target' => Icons.warning_amber,
    _ => Icons.mail_outline,
  };

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final text = Theme.of(context).textTheme;
    final cs = Theme.of(context).colorScheme;
    final resolution = inboxResolution(l, item);
    final when = MaterialLocalizations.of(context);
    final at = DateTime.fromMillisecondsSinceEpoch(item.createdMs);
    final busy = deciding || retrying;
    final showDecide = item.canDecide && !item.resolved;
    final showWaiting = item.isTask && !item.canDecide && !item.resolved;
    final showRetry = item.canRetry && !item.resolved;
    final showAlbum = item.albumId != null;
    return Card(
      margin: const EdgeInsets.only(top: 8),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onOpen,
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Icon(_icon, color: item.read ? cs.outline : cs.primary),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      inboxText(l, item),
                      key: ValueKey('inbox-text-${item.id}'),
                      style: item.read
                          ? text.bodyMedium
                          : text.bodyMedium?.copyWith(
                              fontWeight: FontWeight.w600,
                            ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      '${when.formatShortDate(at)} ${when.formatTimeOfDay(TimeOfDay.fromDateTime(at), alwaysUse24HourFormat: MediaQuery.alwaysUse24HourFormatOf(context))}',
                      style: text.bodySmall?.copyWith(color: cs.outline),
                    ),
                    if (showWaiting)
                      Text(
                        l.inboxWaitsForApproval,
                        key: ValueKey('inbox-waits-${item.id}'),
                        style: text.labelLarge,
                      ),
                    if (resolution != null)
                      Text(
                        resolution,
                        key: ValueKey('inbox-resolution-${item.id}'),
                        style: text.labelLarge,
                      ),
                    if (failure != null)
                      Text(
                        _failureText(l, failure!, by: item.by),
                        key: ValueKey('inbox-failure-${item.id}'),
                        style: text.bodySmall?.copyWith(
                          color: failure == 'decided' ? cs.primary : cs.error,
                        ),
                      ),
                    if (showDecide || showRetry || showAlbum)
                      Padding(
                        padding: const EdgeInsets.only(top: 8),
                        child: Wrap(
                          spacing: 8,
                          runSpacing: 4,
                          crossAxisAlignment: WrapCrossAlignment.center,
                          children: [
                            if (showDecide) ...[
                              FilledButton(
                                key: ValueKey('inbox-approve-${item.id}'),
                                onPressed: busy ? null : onApprove,
                                child: Text(l.inboxImport),
                              ),
                              OutlinedButton(
                                key: ValueKey('inbox-reject-${item.id}'),
                                onPressed: busy ? null : onReject,
                                child: Text(l.inboxReject),
                              ),
                            ],
                            if (showRetry)
                              FilledButton.tonal(
                                key: ValueKey('inbox-retry-${item.id}'),
                                onPressed: busy ? null : onRetry,
                                child: Text(l.inboxRetry),
                              ),
                            if (showAlbum)
                              TextButton(
                                key: ValueKey('inbox-album-${item.id}'),
                                onPressed: onAlbum,
                                child: Text(l.inboxOpenAlbum),
                              ),
                            if (busy)
                              const SizedBox.square(
                                dimension: 18,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                ),
                              ),
                          ],
                        ),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
