// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:async';
import 'dart:io';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import 'package:video_player/video_player.dart';
import 'package:wakelock_plus/wakelock_plus.dart';

import '../audio/audio_backend.dart';
import '../audio/open_backend.dart';
import '../l10n/app_localizations.dart';
import '../player_controller.dart';
import 'show_project.dart';

/// Whether this device plays the videos of a project (video_player: Android, iOS, macOS); elsewhere a video shows as its picture.
bool get showVideosHere => Platform.isAndroid || Platform.isIOS || Platform.isMacOS;

/// The slideshow of a project on this device, without the server: the model is the slideshow of the browser (`server/static/slideshow.js`,
/// project owner 2026-11-08). The whole screen; a tap shows the controls for three seconds, a swipe goes on or back, a swipe down ends it,
/// the keys of a computer do the same (arrows, space, Escape). Videos play through and the music is quieter meanwhile.
class ShowPlayerScreen extends StatefulWidget {
  const ShowPlayerScreen({super.key, required this.project, this.musicEngine = openAudioBackend, bool? videos, this.random})
      : videos = videos ?? false;

  final ShowProject project;

  /// The engine for the music; a second one beside the player of the audiobooks.
  final AudioBackend? Function() musicEngine;

  /// Whether videos play here ([showVideosHere]; tests set it).
  final bool videos;

  /// For the order "shuffled" and the transition "random" (tests give a seeded one).
  final Random? random;

  static Future<void> open(BuildContext context, ShowProject project) => Navigator.of(context).push(MaterialPageRoute<void>(
        fullscreenDialog: true,
        builder: (_) => ShowPlayerScreen(project: project, videos: showVideosHere),
      ));

  @override
  State<ShowPlayerScreen> createState() => _ShowPlayerScreenState();
}

class _ShowPlayerScreenState extends State<ShowPlayerScreen> {
  static const _fade = Duration(milliseconds: 1000);

  late final Random _random = widget.random ?? Random();
  late final List<ShowPicture> _order = [...widget.project.pictures];
  final FocusNode _focus = FocusNode();
  int _at = 0;

  /// Counts the changes; the key of the shown slide, and the guard of what a late answer may still change.
  int _step = 0;
  String _transition = 'fade';

  /// Where the picture comes closer to, chosen once per picture.
  Alignment _zoomAt = Alignment.center;
  bool _paused = false;
  bool _ui = true;
  Timer? _timer;
  Timer? _hideUi;
  VideoPlayerController? _video;
  bool _videoDone = false;

  AudioBackend? _music;
  List<ShowTrack> _tracks = const [];
  int _track = 0;
  int _musicFailures = 0;
  Timer? _musicPoll;
  Timer? _duckTimer;
  double _volume = 1;

  ShowSettings get _settings => widget.project.settings;

  @override
  void initState() {
    super.initState();
    if (_settings.order == 'random') _order.shuffle(_random);
    SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersiveSticky);
    WakelockPlus.enable().catchError((Object _) {});
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      // The audiobook stops while the pictures have the sound.
      final player = PlayerScope.maybeOf(context);
      if (player != null && player.playing) player.toggle();
      _startMusic();
      _show(0, 1);
      _flashUi();
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    _hideUi?.cancel();
    _musicPoll?.cancel();
    _duckTimer?.cancel();
    _video?.dispose();
    _music?.dispose();
    _focus.dispose();
    SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
    WakelockPlus.disable().catchError((Object _) {});
    super.dispose();
  }

  // ---- The pictures ------------------------------------------------------------------------------------------------------------------

  /// How a picture comes in: its own transition (a project put together by hand on the server), else the one of the project.
  String _pick(ShowPicture p) {
    if (MediaQuery.maybeDisableAnimationsOf(context) ?? false) return 'cut';
    final t = p.transition ?? _settings.transition;
    return t == 'random' ? const ['fade', 'zoom', 'cut'][_random.nextInt(3)] : t;
  }

  /// How long the picture shown stays: its own seconds, else those of the project.
  int get _seconds => (_order.isEmpty ? null : _order[_at].seconds) ?? _settings.seconds;

  void _go(int dir) {
    if (_order.isEmpty) return;
    _show((_at + dir) % _order.length, dir);
  }

  Future<void> _show(int i, int dir) async {
    if (_order.isEmpty) return;
    final mine = ++_step;
    _timer?.cancel();
    _endVideo();
    final p = _order[i];
    final transition = _pick(p);
    _zoomAt = Alignment(_random.nextDouble() * 1.2 - 0.6, _random.nextDouble() * 1.2 - 0.6);
    if (p.video != null && widget.videos) {
      final c = VideoPlayerController.file(File(widget.project.path(p.video!)));
      try {
        await c.initialize();
      } on Object {
        await c.dispose();
        if (mounted && mine == _step) _go(dir);
        return;
      }
      if (!mounted || mine != _step) {
        await c.dispose();
        return;
      }
      _videoDone = false;
      c.addListener(() => _videoTick(c, mine));
      setState(() {
        _video = c;
        _at = i;
        _transition = transition;
      });
      _duck(true);
      if (!_paused) await c.play();
      return;
    }
    setState(() {
      _at = i;
      _transition = transition;
    });
    _precache((i + 1) % _order.length);
    _schedule();
  }

  void _schedule() {
    _timer?.cancel();
    if (_paused) return;
    _timer = Timer(Duration(seconds: _seconds), () => _go(1));
  }

  void _precache(int i) {
    final p = _order[i];
    precacheImage(FileImage(File(widget.project.path(p.image))), context).catchError((Object _) {});
  }

  /// A picture that cannot be read: on to the next one.
  void _skipBroken(int step) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && step == _step) _go(1);
    });
  }

  // ---- Videos ------------------------------------------------------------------------------------------------------------------------

  void _videoTick(VideoPlayerController c, int step) {
    if (step != _step || _videoDone) return;
    final v = c.value;
    final ended = v.isInitialized && v.duration > Duration.zero && !v.isPlaying && v.position >= v.duration - const Duration(milliseconds: 250);
    if (v.hasError || ended) {
      _videoDone = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && step == _step) _go(1);
      });
    }
  }

  void _endVideo() {
    final c = _video;
    if (c == null) return;
    _video = null;
    c.pause().whenComplete(c.dispose);
    _duck(false);
  }

  // ---- The music ---------------------------------------------------------------------------------------------------------------------

  void _startMusic() {
    if (widget.project.music.isEmpty) return;
    _music = widget.musicEngine();
    if (_music == null) return;
    _tracks = [...widget.project.music];
    if (_settings.order == 'random') _tracks.shuffle(_random);
    _playTrack();
    _musicPoll = Timer.periodic(const Duration(milliseconds: 500), (_) {
      final m = _music;
      if (m == null) return;
      if (m.state == BackendState.ended || m.state == BackendState.failed) _nextTrack();
    });
  }

  void _playTrack() {
    final m = _music;
    if (m == null || _tracks.isEmpty) return;
    try {
      m.load(widget.project.path(_tracks[_track].file), play: !_paused, title: _tracks[_track].title);
      m.setVolume(_volume);
      _musicFailures = 0;
    } on AudioException {
      // A track that does not open: the next one, but not round and round when none opens.
      if (++_musicFailures < _tracks.length) _nextTrack();
    }
  }

  void _nextTrack() {
    _track = (_track + 1) % _tracks.length;
    _playTrack();
  }

  /// The music quieter while a video plays, and back afterwards, in small steps.
  void _duck(bool down) {
    _duckTimer?.cancel();
    final target = down ? 0.15 : 1.0;
    _duckTimer = Timer.periodic(const Duration(milliseconds: 40), (t) {
      final diff = target - _volume;
      _volume = diff.abs() <= 0.05 ? target : _volume + (diff > 0 ? 0.05 : -0.05);
      _music?.setVolume(_volume);
      if (_volume == target) t.cancel();
    });
  }

  // ---- Controls ----------------------------------------------------------------------------------------------------------------------

  void _flashUi() {
    setState(() => _ui = true);
    _hideUi?.cancel();
    _hideUi = Timer(const Duration(seconds: 3), () {
      if (mounted) setState(() => _ui = false);
    });
  }

  void _togglePause() {
    setState(() => _paused = !_paused);
    if (_paused) {
      _timer?.cancel();
      _video?.pause();
      _music?.pause();
    } else {
      _music?.play();
      if (_video != null) {
        _video!.play();
      } else {
        _schedule();
      }
    }
    _flashUi();
  }

  void _close() => Navigator.of(context).maybePop();

  KeyEventResult _key(FocusNode _, KeyEvent e) {
    if (e is! KeyDownEvent) return KeyEventResult.ignored;
    switch (e.logicalKey) {
      case LogicalKeyboardKey.arrowRight:
        _go(1);
      case LogicalKeyboardKey.arrowLeft:
        _go(-1);
      case LogicalKeyboardKey.space:
        _togglePause();
        return KeyEventResult.handled;
      case LogicalKeyboardKey.escape:
        _close();
        return KeyEventResult.handled;
      default:
        return KeyEventResult.ignored;
    }
    _flashUi();
    return KeyEventResult.handled;
  }

  // ---- The screen --------------------------------------------------------------------------------------------------------------------

  Widget _slide() {
    if (_order.isEmpty) return const SizedBox.expand(key: ValueKey('empty'));
    final p = _order[_at];
    final step = _step;
    final c = _video;
    if (c != null && c.value.isInitialized) {
      return Center(
        key: ValueKey('slide-$step'),
        child: AspectRatio(aspectRatio: c.value.aspectRatio, child: VideoPlayer(c)),
      );
    }
    final image = Image.file(
      File(widget.project.path(p.image)),
      fit: BoxFit.contain,
      width: double.infinity,
      height: double.infinity,
      gaplessPlayback: true,
      errorBuilder: (_, _, _) {
        _skipBroken(step);
        return const SizedBox.expand();
      },
    );
    if (_transition != 'zoom') return KeyedSubtree(key: ValueKey('slide-$step'), child: image);
    return _SlowZoom(
      key: ValueKey('slide-$step'),
      duration: Duration(seconds: _seconds) + _fade * 2,
      alignment: _zoomAt,
      child: image,
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final p = _order.isEmpty ? null : _order[_at];
    final showUi = _ui || _paused;
    final locale = Localizations.localeOf(context).toLanguageTag();
    final shadow = const [Shadow(blurRadius: 4, color: Colors.black), Shadow(blurRadius: 12, color: Colors.black54)];
    final caption = p == null || _settings.info == 'none'
        ? null
        : [
            DateFormat.yMMMMd(locale).format(p.takenAt),
            if (_settings.info == 'both' && p.video == null && p.place != null) p.place!,
          ];
    return Focus(
      focusNode: _focus,
      autofocus: true,
      onKeyEvent: _key,
      child: Scaffold(
        backgroundColor: Colors.black,
        body: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: _flashUi,
          onHorizontalDragEnd: (d) {
            final v = d.primaryVelocity ?? 0;
            if (v.abs() < 150) return;
            _go(v < 0 ? 1 : -1);
            _flashUi();
          },
          // A swipe down ends the show, as in the photo apps of the phones.
          onVerticalDragEnd: (d) {
            if ((d.primaryVelocity ?? 0) > 300) _close();
          },
          child: Stack(
            fit: StackFit.expand,
            children: [
              AnimatedSwitcher(
                duration: _transition == 'cut' ? Duration.zero : _fade,
                layoutBuilder: (current, previous) => Stack(fit: StackFit.expand, children: [...previous, ?current]),
                child: _slide(),
              ),
              if (caption != null)
                Positioned(
                  left: 16,
                  right: 96,
                  bottom: 16,
                  child: SafeArea(
                    top: false,
                    child: DefaultTextStyle(
                      style: TextStyle(color: Colors.white, fontSize: 18, shadows: shadow),
                      child: Column(
                        key: const ValueKey('show-caption'),
                        crossAxisAlignment: CrossAxisAlignment.start,
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Text(caption.first, key: const ValueKey('show-date')),
                          if (caption.length > 1)
                            Text(caption[1], key: const ValueKey('show-place'), style: const TextStyle(fontSize: 15, color: Colors.white70)),
                        ],
                      ),
                    ),
                  ),
                ),
              // Above the caption, so the date stays readable while the controls show.
              Positioned(
                left: 0,
                right: 0,
                bottom: 80,
                child: SafeArea(
                  top: false,
                  child: IgnorePointer(
                    ignoring: !showUi,
                    child: AnimatedOpacity(
                      opacity: showUi ? 1 : 0,
                      duration: const Duration(milliseconds: 300),
                      child: Row(
                        mainAxisAlignment: MainAxisAlignment.center,
                        children: [
                          _RoundButton(key: const ValueKey('show-prev'), icon: Icons.chevron_left, label: l.showPrev, onPressed: () {
                            _go(-1);
                            _flashUi();
                          }),
                          const SizedBox(width: 16),
                          _RoundButton(
                            key: const ValueKey('show-pause'),
                            icon: _paused ? Icons.play_arrow : Icons.pause,
                            label: l.showPause,
                            onPressed: _togglePause,
                          ),
                          const SizedBox(width: 16),
                          _RoundButton(key: const ValueKey('show-next'), icon: Icons.chevron_right, label: l.showNext, onPressed: () {
                            _go(1);
                            _flashUi();
                          }),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
              Positioned(
                top: 0,
                right: 0,
                child: SafeArea(
                  child: Padding(
                    padding: const EdgeInsets.all(12),
                    child: IgnorePointer(
                      ignoring: !showUi,
                      child: AnimatedOpacity(
                        opacity: showUi ? 1 : 0,
                        duration: const Duration(milliseconds: 300),
                        child: _RoundButton(key: const ValueKey('show-close'), icon: Icons.close, label: l.showClose, onPressed: _close),
                      ),
                    ),
                  ),
                ),
              ),
              if (_paused)
                Positioned(
                  top: 0,
                  left: 0,
                  right: 0,
                  child: SafeArea(
                    child: Center(
                      child: Container(
                        margin: const EdgeInsets.only(top: 16),
                        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
                        decoration: BoxDecoration(color: Colors.black54, borderRadius: BorderRadius.circular(16)),
                        child: Text(l.showPaused, style: const TextStyle(color: Colors.white)),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _RoundButton extends StatelessWidget {
  const _RoundButton({super.key, required this.icon, required this.label, required this.onPressed});

  final IconData icon;
  final String label;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) => Semantics(
        button: true,
        label: label,
        child: Material(
          color: Colors.black54,
          shape: const CircleBorder(),
          clipBehavior: Clip.antiAlias,
          child: IconButton(
            tooltip: label,
            iconSize: 30,
            color: Colors.white,
            padding: const EdgeInsets.all(12),
            onPressed: onPressed,
            icon: Icon(icon),
          ),
        ),
      );
}

/// The picture comes slowly closer while it is shown (the transition "zoom").
class _SlowZoom extends StatefulWidget {
  const _SlowZoom({super.key, required this.duration, required this.alignment, required this.child});

  final Duration duration;
  final Alignment alignment;
  final Widget child;

  @override
  State<_SlowZoom> createState() => _SlowZoomState();
}

class _SlowZoomState extends State<_SlowZoom> with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(vsync: this, duration: widget.duration)..forward();

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => ClipRect(
        child: ScaleTransition(
          scale: Tween<double>(begin: 1, end: 1.12).animate(_c),
          alignment: widget.alignment,
          child: widget.child,
        ),
      );
}
