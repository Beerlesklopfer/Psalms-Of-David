// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
import 'dart:io';

/// File formats (as the server names them: `Track.format`) that the iPhone cannot play: Ogg Vorbis and Opus in Ogg
/// (AVPlayer does not decode them), WMA, Monkey's Audio, Musepack, WavPack and Speex. MP3, M4A/AAC, FLAC, WAV and AIFF play.
/// Android (ExoPlayer) and the Linux engine (symphonia) play Ogg, FLAC and M4A.
const kIosUnplayableFormats = {'ogg', 'opus', 'spx', 'wma', 'ape', 'mpc', 'wv'};

/// Says whether the app runs on an iPhone/iPad; a test replaces it.
bool Function() runsOnIos = () => Platform.isIOS;

/// Can this platform play a file of [format]? A format that is not known (null or empty: an old server, or a file no import has looked at
/// yet) is assumed playable.
bool formatPlayableHere(String? format) {
  if (format == null || format.isEmpty) return true;
  return !(runsOnIos() && kIosUnplayableFormats.contains(format.toLowerCase()));
}

/// Is this track playable here, by itself or after the server made an MP3 of it (`Track.transcode == 'mp3'`, a server with transcoding)?
bool trackPlayableHere(String? format, String? transcode) => formatPlayableHere(format) || transcode == 'mp3';

/// Does the part have to be asked for as an MP3 (`?as=mp3`): the platform cannot play the file and the server offers the MP3.
bool needsTranscode(String? format, String? transcode) => !formatPlayableHere(format) && transcode == 'mp3';
