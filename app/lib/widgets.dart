// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'package:flutter/material.dart';

import 'format.dart';
import 'l10n/app_localizations.dart';
import 'models.dart';
import 'player_controller.dart';
import 'screens/item_screen.dart';
import 'screens/hide_menu.dart';
import 'screens/player_screen.dart';
import 'theme.dart';

/// The cover of a book: its product picture if it has one, else a drawn one.
class Cover extends StatelessWidget {
  const Cover({super.key, required this.book});

  final LibraryItem book;

  @override
  Widget build(BuildContext context) {
    final picture = book.cover;
    if (picture != null) {
      // The product picture, cropped to the square. If the bytes are no picture after
      // all, the drawn cover below is shown instead.
      return ClipRRect(
        borderRadius: BorderRadius.circular(3),
        child: Image.memory(
          picture,
          fit: BoxFit.cover,
          gaplessPlayback: true,
          semanticLabel: book.title,
          errorBuilder: (context, error, stack) => _DrawnCover(book: book),
        ),
      );
    }
    return _DrawnCover(book: book);
  }
}

/// Square cover without artwork: a color field with a fine gold frame and the title.
class _DrawnCover extends StatelessWidget {
  const _DrawnCover({required this.book});

  final LibraryItem book;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, box) {
        final s = box.biggest.shortestSide;
        final big = s >= 120;
        return DecoratedBox(
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(3),
            gradient: LinearGradient(
              begin: Alignment.topLeft,
              end: Alignment.bottomRight,
              colors: [book.color, Color.lerp(book.color, Colors.black, 0.5)!],
            ),
          ),
          child: Padding(
            padding: EdgeInsets.all(s * 0.06),
            child: DecoratedBox(
              decoration: BoxDecoration(
                border: Border.all(
                  color: Palette.gold.withValues(alpha: 0.55),
                  width: 1,
                ),
              ),
              child: Padding(
                padding: EdgeInsets.all(s * 0.05),
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    Text(
                      book.title,
                      textAlign: TextAlign.center,
                      maxLines: big ? 5 : 3,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontFamilyFallback: serifFallback,
                        fontWeight: FontWeight.w600,
                        fontSize: s * (big ? 0.105 : 0.16),
                        height: 1.15,
                        color: Palette.parchment,
                      ),
                    ),
                    if (big) ...[
                      SizedBox(height: s * 0.05),
                      Container(
                        width: s * 0.18,
                        height: 1,
                        color: Palette.gold.withValues(alpha: 0.7),
                      ),
                      SizedBox(height: s * 0.05),
                      Text(
                        book.author,
                        textAlign: TextAlign.center,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: s * 0.065,
                          letterSpacing: 0.6,
                          color: Palette.gold,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
            ),
          ),
        );
      },
    );
  }
}

void openItem(BuildContext context, LibraryItem book) {
  Navigator.of(context).push(
    MaterialPageRoute<void>(builder: (_) => ItemScreen(book: book)),
  );
}

/// Cover with title and author below, used in rows and grids.
/// The green circle with a check mark that tells a track was heard to the end.
class PlayedMark extends StatelessWidget {
  const PlayedMark({super.key});

  @override
  Widget build(BuildContext context) {
    final label = AppLocalizations.of(context).tooltipPlayed;
    return Tooltip(
      message: label,
      child: Semantics(label: label, child: const Icon(Icons.check_circle, key: ValueKey('played-mark'), color: Palette.played, size: 20)),
    );
  }
}

class ItemCard extends StatelessWidget {
  const ItemCard({super.key, required this.book, this.badge});

  final LibraryItem book;

  /// A small label on the corner of the cover (for example "New"); null for none.
  final String? badge;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    final muted = Theme.of(context).colorScheme.onSurfaceVariant;
    final card = InkWell(
      borderRadius: BorderRadius.circular(6),
      onTap: () => openItem(context, book),
      // The choices about hiding (a book that is no item of the server has none).
      onLongPress: () => showHideSheet(context, book),
      child: Padding(
        padding: const EdgeInsets.all(6),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            AspectRatio(
              aspectRatio: 1,
              child: Stack(
                fit: StackFit.expand,
                children: [
                  Cover(book: book),
                  if (book.hidden)
                    Positioned(
                      top: 6,
                      right: 6,
                      child: Tooltip(
                        message: AppLocalizations.of(context).hiddenMark,
                        child: const Icon(Icons.visibility_off, key: ValueKey('hidden-mark'), size: 20, color: Colors.white),
                      ),
                    ),
                  if (badge != null)
                    Positioned(
                      top: 6,
                      left: 6,
                      child: DecoratedBox(
                        decoration: BoxDecoration(color: Theme.of(context).colorScheme.primary, borderRadius: BorderRadius.circular(4)),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                          child: Text(
                            badge!,
                            key: const ValueKey('book-badge'),
                            style: text.labelSmall?.copyWith(color: Theme.of(context).colorScheme.onPrimary, fontWeight: FontWeight.w700),
                          ),
                        ),
                      ),
                    ),
                ],
              ),
            ),
            const SizedBox(height: 8),
            Text(
              book.title,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: text.titleSmall?.copyWith(fontWeight: FontWeight.w600),
            ),
            Text(
              book.author,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: text.bodySmall?.copyWith(color: muted),
            ),
          ],
        ),
      ),
    );
    // A hidden book is listed only when the setting asks for it, and then it is dimmed.
    return book.hidden ? Opacity(opacity: 0.5, child: card) : card;
  }
}

/// A small window with text fields and one action (Cancel / action). Returns the entries, or null if it was cancelled
/// or a field is empty.
Future<List<String>?> askFields(BuildContext context, {required String title, required List<({String label, String? hint})> fields, required String action, List<String>? initial}) {
  final controllers = [for (var i = 0; i < fields.length; i++) TextEditingController(text: initial != null && i < initial.length ? initial[i] : null)];
  final cancel = MaterialLocalizations.of(context).cancelButtonLabel;
  return showDialog<List<String>>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(title),
      content: SizedBox(
        width: 420,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final (i, f) in fields.indexed)
              TextField(
                controller: controllers[i],
                autofocus: i == 0,
                decoration: InputDecoration(labelText: f.label, hintText: f.hint),
                onSubmitted: i == fields.length - 1 ? (_) => Navigator.of(context).pop([for (final c in controllers) c.text.trim()]) : null,
              ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(cancel)),
        FilledButton(onPressed: () => Navigator.of(context).pop([for (final c in controllers) c.text.trim()]), child: Text(action)),
      ],
    ),
  ).then((v) => v == null || v.any((s) => s.isEmpty) ? null : v);
}

/// One text field: the entry, or null.
Future<String?> askText(BuildContext context, {required String title, required String label, String? hint, required String action, String? initial}) =>
    askFields(context, title: title, fields: [(label: label, hint: hint)], action: action, initial: initial == null ? null : [initial]).then((v) => v?.first);

class SectionHeader extends StatelessWidget {
  const SectionHeader(this.title, {super.key});

  final String title;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 28, 16, 8),
      child: Text(title, style: Theme.of(context).textTheme.titleLarge),
    );
  }
}

/// The head of a group that opens and closes: the title, how many items are in it, and a chevron.
class FoldHeader extends StatelessWidget {
  const FoldHeader({
    super.key,
    required this.title,
    required this.count,
    required this.open,
    required this.onToggle,
    required this.tooltip,
  });

  final String title;
  final int count;
  final bool open;
  final VoidCallback onToggle;

  /// What a tap does ("Expand" / "Collapse"), for screen readers.
  final String tooltip;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    return Semantics(
      button: true,
      expanded: open,
      label: tooltip,
      child: InkWell(
        onTap: onToggle,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
          child: Row(
            children: [
              Icon(open ? Icons.expand_more : Icons.chevron_right),
              const SizedBox(width: 8),
              Flexible(
                child: Text(title, style: t.titleLarge, overflow: TextOverflow.ellipsis),
              ),
              const SizedBox(width: 8),
              Text('($count)', style: t.labelLarge),
            ],
          ),
        ),
      ),
    );
  }
}

/// The bar to wind through the track: tap or drag to jump. While a finger or the mouse
/// holds it, it shows where it is held and does not follow the player, which keeps
/// reporting the old position; the jump is made when it is let go. [compact] is the
/// thin one at the bottom edge of the mini player.
class SeekBar extends StatefulWidget {
  const SeekBar({super.key, this.compact = false, this.wholeBook = false});

  final bool compact;

  /// The bar covers the whole book, not only the track that is playing.
  final bool wholeBook;

  @override
  State<SeekBar> createState() => _SeekBarState();
}

class _SeekBarState extends State<SeekBar> {
  /// Where the bar is held, in milliseconds; null when nobody holds it.
  double? _held;

  @override
  Widget build(BuildContext context) {
    final p = PlayerScope.of(context);
    final track = p.track;
    if (track == null || track.live) return const SizedBox.shrink();
    final cs = Theme.of(context).colorScheme;
    final length = widget.wholeBook ? p.bookTotal : track.length;
    final max = length.inMilliseconds.toDouble().clamp(1.0, double.infinity);
    // Only the slider follows the position; the rest of the bar depends on the coarse state.
    final slider = ValueListenableBuilder<Duration>(
      valueListenable: p.positionListenable,
      builder: (context, _, _) => _slider(p, max),
    );
    if (!widget.compact) return slider;
    return SizedBox(
      height: 18,
      child: SliderTheme(
        data: SliderTheme.of(context).copyWith(
          trackHeight: 2,
          thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 5, elevation: 0),
          overlayShape: SliderComponentShape.noOverlay,
          activeTrackColor: cs.primary,
          inactiveTrackColor: cs.outlineVariant.withValues(alpha: 0.4),
          thumbColor: cs.primary,
        ),
        child: slider,
      ),
    );
  }

  Widget _slider(PlayerController p, double max) {
    final now = widget.wholeBook ? p.bookPosition : p.position;
    final value = (_held ?? now.inMilliseconds.toDouble()).clamp(0.0, max);
    return Slider(
      value: value,
      max: max,
      onChangeStart: (v) => setState(() => _held = v),
      onChanged: (v) => setState(() => _held = v),
      onChangeEnd: (v) {
        setState(() => _held = null);
        final to = Duration(milliseconds: v.round());
        widget.wholeBook ? p.seekBook(to) : p.seek(to);
      },
    );
  }
}

/// Small bar above the navigation, like on Spotify.
/// The speeds offered for playback.
const playbackSpeeds = [0.8, 1.0, 1.2, 1.5, 1.75, 2.0];

class MiniPlayer extends StatelessWidget {
  const MiniPlayer({super.key});

  @override
  Widget build(BuildContext context) {
    final p = PlayerScope.of(context);
    final book = p.book;
    if (book == null) return const SizedBox.shrink();
    return LayoutBuilder(builder: (context, box) => _frame(context, p, book, box.maxWidth));
  }

  Widget _frame(BuildContext context, PlayerController p, LibraryItem book, double width) {
    final l = AppLocalizations.of(context);
    final cs = Theme.of(context).colorScheme;
    final text = Theme.of(context).textTheme;
    final transport = width >= 640; // previous, back, forward, next
    final speedMenu = width >= 480;
    return Material(
      color: cs.surfaceContainerHigh,
      child: InkWell(
        onTap: () => Navigator.of(context).push(PlayerScreen.route()),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
              child: Row(
                children: [
                  SizedBox.square(dimension: 44, child: Cover(book: p.shownBook ?? book)),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          book.title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: text.titleSmall
                              ?.copyWith(fontWeight: FontWeight.w600),
                        ),
                        Text(
                          p.unplayable != null
                              ? l.formatNotPlayableIos
                              : p.nowPlaying ?? (p.track!.live ? p.track!.title : l.discTrack(p.track!.disc, p.track!.title)),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: text.bodySmall
                              ?.copyWith(color: cs.onSurfaceVariant),
                        ),
                      ],
                    ),
                  ),
                  // Played and total time: of the track, or (a tap switches) of the whole book (CD).
                  Tooltip(
                    message: p.showBookTime ? l.tooltipTimeBook : l.tooltipTimeTrack,
                    child: GestureDetector(
                      key: const ValueKey('time-scope'),
                      behavior: HitTestBehavior.opaque,
                      onTap: p.toggleTimeScope,
                      child: Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 8),
                        child: ValueListenableBuilder<Duration>(
                          valueListenable: p.positionListenable,
                          builder: (context, _, _) => Text(
                            p.track?.live == true
                                ? l.radioLive
                                : p.showBookTime
                                    ? '${formatDuration(p.bookPosition)} / ${formatDuration(p.bookTotal)}'
                                    : '${formatDuration(p.position)} / ${formatDuration(p.track!.length)}',
                            style: text.bodySmall?.copyWith(color: cs.onSurfaceVariant, fontFeatures: const [FontFeature.tabularFigures()]),
                          ),
                        ),
                      ),
                    ),
                  ),
                  if (speedMenu)
                    PopupMenuButton<double>(
                      tooltip: l.tooltipSpeed,
                      initialValue: p.speed,
                      onSelected: p.setSpeed,
                      itemBuilder: (context) => [
                        for (final s in playbackSpeeds)
                          PopupMenuItem(value: s, child: Text(formatSpeed(l.localeName, s))),
                      ],
                      child: Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                        child: Text(
                          formatSpeed(l.localeName, p.speed),
                          style: text.labelLarge?.copyWith(color: p.speed == 1.0 ? cs.onSurfaceVariant : cs.primary),
                        ),
                      ),
                    ),
                  if (transport) ...[
                    IconButton(tooltip: l.tooltipPrevious, onPressed: p.previous, icon: const Icon(Icons.skip_previous)),
                    IconButton(
                      tooltip: l.tooltipBack10,
                      onPressed: () => p.skip(const Duration(seconds: -10)),
                      icon: const Icon(Icons.replay_10),
                    ),
                  ],
                  IconButton(
                    tooltip: p.playing ? l.tooltipPause : l.tooltipPlay,
                    onPressed: p.toggle,
                    icon: Icon(p.playing ? Icons.pause : Icons.play_arrow),
                    iconSize: 30,
                  ),
                  if (transport) ...[
                    IconButton(
                      tooltip: l.tooltipForward30,
                      onPressed: () => p.skip(const Duration(seconds: 30)),
                      icon: const Icon(Icons.forward_30),
                    ),
                    IconButton(tooltip: l.tooltipNext, onPressed: p.next, icon: const Icon(Icons.skip_next)),
                  ],
                ],
              ),
            ),
            SeekBar(compact: true, wholeBook: p.showBookTime),
          ],
        ),
      ),
    );
  }
}


/// A page of plain content: centred, never wider than a comfortable column, with
/// the title in the app bar.
class CenteredPage extends StatelessWidget {
  const CenteredPage({super.key, required this.title, required this.children});

  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return CustomScrollView(
      slivers: [
        SliverAppBar(
          floating: true,
          backgroundColor: Theme.of(context).colorScheme.surface,
          title: Text(title, style: Theme.of(context).textTheme.headlineSmall),
        ),
        SliverToBoxAdapter(
          child: Align(
            alignment: Alignment.topCenter,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 560),
              child: Padding(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
                child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: children),
              ),
            ),
          ),
        ),
      ],
    );
  }
}


/// The lyre of the logo, drawn in a 24 x 24 box (the same shape as in the server's
/// logo.svg) and scaled to the size of the widget.
class Lyre extends StatelessWidget {
  const Lyre({super.key, required this.size, required this.color});

  final double size;
  final Color color;

  @override
  Widget build(BuildContext context) => CustomPaint(size: Size.square(size), painter: _LyrePainter(color));
}

class _LyrePainter extends CustomPainter {
  _LyrePainter(this.color);

  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final k = size.width / 24;
    Offset o(double x, double y) => Offset(x * k, y * k);
    final pen = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.5 * k
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round;
    final body = Path()
      ..moveTo(8 * k, 3.5 * k)
      ..cubicTo(5.5 * k, 6 * k, 5 * k, 10.5 * k, 7.5 * k, 14.2 * k)
      ..cubicTo(8.8 * k, 16.1 * k, 10.3 * k, 17 * k, 12 * k, 17 * k)
      ..cubicTo(13.7 * k, 17 * k, 15.2 * k, 16.1 * k, 16.5 * k, 14.2 * k)
      ..cubicTo(19 * k, 10.5 * k, 18.5 * k, 6 * k, 16 * k, 3.5 * k);
    canvas.drawPath(body, pen);
    canvas.drawLine(o(7.2, 6.2), o(16.8, 6.2), pen);
    canvas.drawLine(o(10, 6.4), o(10, 15), pen);
    canvas.drawLine(o(12, 6.4), o(12, 17.6), pen);
    canvas.drawLine(o(14, 6.4), o(14, 15), pen);
    canvas.drawLine(o(9.5, 20), o(14.5, 20), pen);
    canvas.drawLine(o(12, 17.2), o(12, 20), pen);
  }

  @override
  bool shouldRepaint(_LyrePainter old) => old.color != color;
}


/// Loads the library again from the server. Not shown without a server (demo data).
class RefreshButton extends StatelessWidget {
  const RefreshButton({super.key, this.also});

  /// More to reload together with the library (the lists of podcasts and stations). With it the button shows even
  /// where the library itself cannot refresh.
  final VoidCallback? also;

  @override
  Widget build(BuildContext context) {
    final p = PlayerScope.of(context);
    if (!p.canRefresh && also == null) return const SizedBox.shrink();
    final l = AppLocalizations.of(context);
    return p.refreshing
        ? const Padding(padding: EdgeInsets.all(14), child: SizedBox.square(dimension: 20, child: CircularProgressIndicator(strokeWidth: 2)))
        : IconButton(
            tooltip: l.tooltipRefresh,
            onPressed: () {
              if (p.canRefresh) p.refreshLibrary();
              also?.call();
            },
            icon: const Icon(Icons.refresh),
          );
  }
}
