// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

/// File formats (as the server names them: `Track.format`) that the player of the Apple platforms (AVPlayer: iPhone, iPad and Mac) cannot play: Ogg Vorbis and Opus in Ogg
/// (AVPlayer does not decode them), WMA, Monkey's Audio, Musepack, WavPack and Speex. MP3, M4A/AAC, FLAC, WAV and AIFF play.
/// Android (ExoPlayer) and the Linux engine (symphonia) play Ogg, FLAC and M4A.
const kIosUnplayableFormats = {'ogg', 'opus', 'spx', 'wma', 'ape', 'mpc', 'wv'};

/// Says whether the app runs on an Apple platform (iOS, iPadOS, macOS: AVPlayer decodes there); a test replaces it.
bool Function() runsOnApple = () => Platform.isIOS || Platform.isMacOS;

/// Can this platform play a file of [format]? A format that is not known (null or empty: an old server, or a file no import has looked at
/// yet) is assumed playable.
bool formatPlayableHere(String? format) {
  if (format == null || format.isEmpty) return true;
  return !(runsOnApple() && kIosUnplayableFormats.contains(format.toLowerCase()));
}

/// Is this track playable here, by itself or after the server made an MP3 of it (`Track.transcode == 'mp3'`, a server with transcoding)?
bool trackPlayableHere(String? format, String? transcode) => formatPlayableHere(format) || transcode == 'mp3';

/// The ending of the file of a downloaded part. AVPlayer (iPhone, iPad, Mac) decides how to read a local file by its ending: a file called `17.audio`
/// is not opened at all (only radio, which plays from an address, worked on the iPhone). So the file gets the ending of its format, `mp3` for what the
/// server made into an MP3; for a part whose format is not known (an old server) it stays `audio`.
String audioFileEnding({String? format, String? mime, bool mp3 = false}) {
  if (mp3) return 'mp3';
  const byFormat = {
    'mp3': 'mp3', 'mp2': 'mp2', 'm4a': 'm4a', 'mp4': 'm4a', 'm4b': 'm4a', 'aac': 'aac', 'ogg': 'ogg', 'oga': 'ogg', 'opus': 'opus',
    'flac': 'flac', 'wav': 'wav', 'aiff': 'aiff', 'aif': 'aiff', 'wma': 'wma',
  };
  final f = byFormat[(format ?? '').toLowerCase()];
  if (f != null) return f;
  const byMime = {
    'audio/mpeg': 'mp3', 'audio/mp3': 'mp3', 'audio/mp4': 'm4a', 'audio/x-m4a': 'm4a', 'audio/aac': 'aac', 'audio/ogg': 'ogg',
    'audio/flac': 'flac', 'audio/x-flac': 'flac', 'audio/wav': 'wav', 'audio/x-wav': 'wav',
  };
  return byMime[(mime ?? '').toLowerCase().split(';').first.trim()] ?? 'audio';
}

/// The ending of an episode file: that of the address (`.../folge-3.m4a?x=1`), else `mp3` (what nearly every publisher sends).
String episodeFileEnding(String url) {
  final path = Uri.tryParse(url.trim())?.path.toLowerCase() ?? '';
  final dot = path.lastIndexOf('.');
  final ext = dot < 0 ? '' : path.substring(dot + 1);
  const known = {'mp3', 'm4a', 'aac', 'ogg', 'opus', 'flac', 'wav', 'mp4'};
  return known.contains(ext) ? (ext == 'mp4' ? 'm4a' : ext) : 'mp3';
}

/// Does the part have to be asked for as an MP3 (`?as=mp3`): the platform cannot play the file and the server offers the MP3.
bool needsTranscode(String? format, String? transcode) => !formatPlayableHere(format) && transcode == 'mp3';
