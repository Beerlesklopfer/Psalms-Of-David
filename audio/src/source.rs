// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads an audio file and yields decoded interleaved f32 samples (symphonia).

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

use crate::Error;

pub struct Source {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::audio::AudioDecoder>,
    track_id: u32,
    scratch: Vec<f32>,
    /// Sample rate and channel count of the file, as the first block reported them.
    pub rate: u32,
    pub channels: u32,
    /// Length of the file if the container says so (an MP3 without a header may not).
    pub duration: Option<Duration>,
}

impl Source {
    /// Opens `path` and starts at `start`. Fails if the file cannot be read or has
    /// no audio. The first block is already decoded, so rate and channels are known.
    pub fn open(path: &Path, start: Duration) -> Result<(Source, Vec<f32>), Error> {
        let file = Box::new(File::open(path)?);
        let mss = MediaSourceStream::new(file, Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let mut format = symphonia::default::get_probe()
            .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
            .map_err(|e| Error::Decode(format!("{}: {e}", path.display())))?;
        let track = format.default_track(TrackType::Audio).ok_or_else(|| Error::Decode("no audio track".into()))?;
        let track_id = track.id;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| Error::Decode("no audio parameters".into()))?;
        let header_rate = params.sample_rate;
        let num_frames = track.num_frames;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|e| Error::Decode(format!("no decoder: {e}")))?;
        let duration = match (num_frames, header_rate) {
            (Some(frames), Some(rate)) if rate > 0 => Some(Duration::from_secs_f64(frames as f64 / rate as f64)),
            _ => None,
        };

        if !start.is_zero() {
            let time = Time::try_from_secs_f64(start.as_secs_f64())
                .ok_or_else(|| Error::Decode("cannot seek to that time".into()))?;
            format
                .seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(track_id) })
                .map_err(|e| Error::Decode(format!("seek failed: {e}")))?;
        }

        let mut source = Source { format, decoder, track_id, scratch: Vec::new(), rate: 0, channels: 0, duration };
        if !start.is_zero() {
            source.decoder.reset();
        }
        let first = source.next_samples()?.ok_or_else(|| Error::Decode("no audio at that position".into()))?;
        Ok((source, first))
    }

    /// Next block of interleaved samples, or `None` at the end of the file.
    pub fn next_samples(&mut self) -> Result<Option<Vec<f32>>, Error> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(e) => return Err(Error::Decode(e.to_string())),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    self.rate = buf.spec().rate();
                    self.channels = buf.spec().channels().count() as u32;
                    self.scratch.resize(buf.samples_interleaved(), f32::MID);
                    buf.copy_to_slice_interleaved(&mut self.scratch);
                    if self.scratch.is_empty() {
                        continue;
                    }
                    return Ok(Some(self.scratch.clone()));
                }
                // A damaged frame is skipped, like most players do.
                Err(DecodeError::DecodeError(_)) => continue,
                Err(e) => return Err(Error::Decode(e.to_string())),
            }
        }
    }
}
