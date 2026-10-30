// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! PCM (44.1 kHz, signed 16-bit little-endian stereo, as on the disc) to MP3 with LAME.
//!
//! The settings are those the spike showed to be bit-identical to `lame -b 64 -m j`: quality
//! `VeryNice` (3, the command line's default), CBR, joint stereo, resampled output.

use std::io::Read;
use std::num::NonZeroU32;

use mp3lame_encoder::{Bitrate, Builder, FlushGap, InterleavedPcm, Mode, Quality};

/// Sample rate of the PCM on a CD.
pub const CD_SAMPLE_RATE: u32 = 44_100;
/// Bytes of PCM in one CD sector.
pub const SECTOR_BYTES: usize = 2352;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderSettings {
    pub bitrate_kbps: u16,
    pub sample_rate_hz: u32,
    /// `joint_stereo`, `stereo`, `mono` or `dual_channel`.
    pub channel_mode: String,
    /// LAME quality 0 (best) to 9 (worst).
    pub quality: u8,
}

fn bitrate(k: u16) -> Result<Bitrate, String> {
    Ok(match k {
        8 => Bitrate::Kbps8,
        16 => Bitrate::Kbps16,
        24 => Bitrate::Kbps24,
        32 => Bitrate::Kbps32,
        40 => Bitrate::Kbps40,
        48 => Bitrate::Kbps48,
        64 => Bitrate::Kbps64,
        80 => Bitrate::Kbps80,
        96 => Bitrate::Kbps96,
        112 => Bitrate::Kbps112,
        128 => Bitrate::Kbps128,
        160 => Bitrate::Kbps160,
        192 => Bitrate::Kbps192,
        224 => Bitrate::Kbps224,
        256 => Bitrate::Kbps256,
        320 => Bitrate::Kbps320,
        other => return Err(format!("bitrate {other} kbit/s is not one LAME offers")),
    })
}

fn mode(m: &str) -> Result<Mode, String> {
    Ok(match m {
        "joint_stereo" => Mode::JointStereo,
        "stereo" => Mode::Stereo,
        "mono" => Mode::Mono,
        "dual_channel" => Mode::DaulChannel,
        other => return Err(format!("channel mode \"{other}\" is unknown")),
    })
}

fn quality(q: u8) -> Result<Quality, String> {
    Ok(match q {
        0 => Quality::Best,
        1 => Quality::SecondBest,
        2 => Quality::NearBest,
        3 => Quality::VeryNice,
        4 => Quality::Nice,
        5 => Quality::Good,
        6 => Quality::Decent,
        7 => Quality::Ok,
        8 => Quality::SecondWorst,
        9 => Quality::Worst,
        other => return Err(format!("quality {other} is outside 0 to 9")),
    })
}

impl EncoderSettings {
    /// Checks the values without encoding anything.
    pub fn validate(&self) -> Result<(), String> {
        bitrate(self.bitrate_kbps)?;
        mode(&self.channel_mode)?;
        quality(self.quality)?;
        if self.sample_rate_hz == 0 {
            return Err("sample rate 0".into());
        }
        Ok(())
    }
}

/// Encodes all PCM from [`input`] to an MP3 stream (without any ID3 tag). [`on_bytes`] is told how
/// many PCM bytes have been read so far.
pub fn encode(mut input: impl Read, s: &EncoderSettings, mut on_bytes: impl FnMut(u64)) -> Result<Vec<u8>, String> {
    let mut b = Builder::new().ok_or("the encoder could not be created")?;
    b.set_num_channels(2).map_err(|e| format!("{e:?}"))?;
    b.set_sample_rate(CD_SAMPLE_RATE).map_err(|e| format!("{e:?}"))?;
    let out_rate = (s.sample_rate_hz != CD_SAMPLE_RATE).then(|| NonZeroU32::new(s.sample_rate_hz)).flatten();
    b.set_output_sample_rate(out_rate).map_err(|e| format!("{e:?}"))?;
    b.set_brate(bitrate(s.bitrate_kbps)?).map_err(|e| format!("{e:?}"))?;
    b.set_mode(mode(&s.channel_mode)?).map_err(|e| format!("{e:?}"))?;
    b.set_quality(quality(s.quality)?).map_err(|e| format!("{e:?}"))?;
    let mut enc = b.build().map_err(|e| format!("{e:?}"))?;

    let mut out: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; SECTOR_BYTES * 27];
    let mut total = 0u64;
    loop {
        let mut filled = 0;
        while filled < buf.len() {
            let n = input.read(&mut buf[filled..]).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            break;
        }
        total += filled as u64;
        let pcm: Vec<i16> = buf[..filled - filled % 4].as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect();
        out.reserve(mp3lame_encoder::max_required_buffer_size(pcm.len() / 2));
        enc.encode_to_vec(InterleavedPcm(&pcm), &mut out).map_err(|e| format!("{e:?}"))?;
        on_bytes(total);
        if filled < buf.len() {
            break;
        }
    }
    out.reserve(7200);
    enc.flush_to_vec::<FlushGap>(&mut out).map_err(|e| format!("{e:?}"))?;
    // The Xing/Info frame belongs at the start; the stream reserved room for it.
    let mut tag = Vec::with_capacity(enc.lame_tag_size());
    let n = enc.lame_tag_encode_to_vec(&mut tag).map(|n| n.get()).unwrap_or(0);
    if n > 0 && n <= out.len() {
        out[..n].copy_from_slice(&tag[..n]);
    }
    Ok(out)
}

/// The nearest bitrate (kbit/s) LAME offers: the setting of the server may hold any number.
pub fn nearest_bitrate(kbps: u32) -> u16 {
    const OFFERED: [u16; 16] = [8, 16, 24, 32, 40, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    OFFERED.iter().copied().min_by_key(|b| (*b as i64 - kbps as i64).abs()).unwrap_or(128)
}

/// Encodes PCM of any rate and one or two channels to MP3 piece by piece, so a long file never lies in memory as a whole. The input is
/// 16-bit interleaved. A rate MP3 does not know is resampled by LAME to 44.1 kHz. Used by the server to make an MP3 of a file that a device
/// cannot play (see `docs/spec-transcoding.md`).
pub struct StreamEncoder {
    enc: mp3lame_encoder::Encoder,
    channels: u8,
}

impl StreamEncoder {
    /// A rate that MPEG audio layer III has (8 to 48 kHz).
    const MPEG_RATES: [u32; 9] = [8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000];

    pub fn new(channels: u8, sample_rate: u32, kbps: u32) -> Result<StreamEncoder, String> {
        if !(1..=2).contains(&channels) || sample_rate == 0 {
            return Err(format!("{channels} channels at {sample_rate} Hz cannot be encoded"));
        }
        let mut b = Builder::new().ok_or("the encoder could not be created")?;
        b.set_num_channels(channels).map_err(|e| format!("{e:?}"))?;
        b.set_sample_rate(sample_rate).map_err(|e| format!("{e:?}"))?;
        if !Self::MPEG_RATES.contains(&sample_rate) {
            b.set_output_sample_rate(NonZeroU32::new(CD_SAMPLE_RATE)).map_err(|e| format!("{e:?}"))?;
        }
        b.set_brate(bitrate(nearest_bitrate(kbps))?).map_err(|e| format!("{e:?}"))?;
        b.set_mode(if channels == 1 { Mode::Mono } else { Mode::JointStereo }).map_err(|e| format!("{e:?}"))?;
        b.set_quality(Quality::VeryNice).map_err(|e| format!("{e:?}"))?;
        Ok(StreamEncoder { enc: b.build().map_err(|e| format!("{e:?}"))?, channels })
    }

    /// Encodes a block of interleaved samples and appends the MP3 bytes to `out`.
    pub fn push(&mut self, pcm: &[i16], out: &mut Vec<u8>) -> Result<(), String> {
        if pcm.is_empty() {
            return Ok(());
        }
        out.reserve(mp3lame_encoder::max_required_buffer_size(pcm.len() / self.channels as usize));
        if self.channels == 1 {
            self.enc.encode_to_vec(mp3lame_encoder::MonoPcm(pcm), out).map_err(|e| format!("{e:?}"))?;
        } else {
            self.enc.encode_to_vec(InterleavedPcm(pcm), out).map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }

    /// Flushes the encoder: the last bytes are appended to `out`. Returns the Xing/Info frame that belongs at the start of the file (its room was
    /// reserved at the start of the stream; empty if the encoder made none).
    pub fn finish(mut self, out: &mut Vec<u8>) -> Result<Vec<u8>, String> {
        out.reserve(7200);
        self.enc.flush_to_vec::<FlushGap>(out).map_err(|e| format!("{e:?}"))?;
        let mut tag = Vec::with_capacity(self.enc.lame_tag_size());
        let n = self.enc.lame_tag_encode_to_vec(&mut tag).map(|n| n.get()).unwrap_or(0);
        tag.truncate(n);
        Ok(tag)
    }
}

/// Properties of the first MPEG audio frame found in [`mp3`] (after any ID3v2 tag): sample rate, bitrate in kbit/s, channel mode bits.
pub fn first_frame(mp3: &[u8]) -> Option<(u32, u32, u8)> {
    let mut p = 0;
    if mp3.len() > 10 && &mp3[..3] == b"ID3" {
        p = 10 + mp3[6..10].iter().fold(0usize, |a, b| (a << 7) | *b as usize);
    }
    while p + 4 <= mp3.len() {
        let h = u32::from_be_bytes([mp3[p], mp3[p + 1], mp3[p + 2], mp3[p + 3]]);
        if h >> 21 == 0x7ff {
            let version = (h >> 19) & 3; // 3 = MPEG1, 2 = MPEG2, 0 = MPEG2.5
            let layer = (h >> 17) & 3; // 1 = Layer III
            let br = ((h >> 12) & 15) as usize;
            let sr = ((h >> 10) & 3) as usize;
            if layer == 1 && version != 1 && br != 0 && br != 15 && sr != 3 {
                const MPEG1: [u32; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
                const MPEG2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
                let rates: [u32; 3] = match version {
                    3 => [44_100, 48_000, 32_000],
                    2 => [22_050, 24_000, 16_000],
                    _ => [11_025, 12_000, 8_000],
                };
                let kbps = if version == 3 { MPEG1[br] } else { MPEG2[br] };
                return Some((rates[sr], kbps, ((h >> 6) & 3) as u8));
            }
        }
        p += 1;
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A sine wave as CD PCM.
    pub fn sine(seconds: f32, hz: f32) -> Vec<u8> {
        let n = (seconds * CD_SAMPLE_RATE as f32) as usize;
        let mut v = Vec::with_capacity(n * 4);
        for i in 0..n {
            let s = ((i as f32 / CD_SAMPLE_RATE as f32 * hz * std::f32::consts::TAU).sin() * 12_000.0) as i16;
            v.extend_from_slice(&s.to_le_bytes());
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }

    fn settings() -> EncoderSettings {
        EncoderSettings { bitrate_kbps: 64, sample_rate_hz: 24_000, channel_mode: "joint_stereo".into(), quality: 3 }
    }

    #[test]
    fn the_library_format() {
        let mp3 = encode(&sine(3.0, 440.0)[..], &settings(), |_| {}).unwrap();
        let (rate, kbps, ch) = first_frame(&mp3).expect("a frame");
        assert_eq!((rate, kbps), (24_000, 64));
        assert_eq!(ch, 1, "joint stereo");
        // 3 s at 64 kbit/s is about 24 kB.
        assert!((20_000..30_000).contains(&mp3.len()), "{}", mp3.len());
    }

    #[test]
    fn other_settings_are_followed() {
        let s = EncoderSettings { bitrate_kbps: 128, sample_rate_hz: 44_100, channel_mode: "stereo".into(), quality: 5 };
        let mp3 = encode(&sine(2.0, 300.0)[..], &s, |_| {}).unwrap();
        let (rate, kbps, ch) = first_frame(&mp3).unwrap();
        assert_eq!((rate, kbps, ch), (44_100, 128, 0));
    }

    #[test]
    fn bad_values_are_refused() {
        let mut s = settings();
        s.bitrate_kbps = 65;
        assert!(s.validate().is_err());
        s = settings();
        s.channel_mode = "surround".into();
        assert!(s.validate().is_err());
        s = settings();
        s.quality = 10;
        assert!(s.validate().is_err());
        assert!(settings().validate().is_ok());
    }

    #[test]
    fn progress_counts_bytes() {
        let pcm = sine(1.0, 200.0);
        let mut last = 0;
        encode(&pcm[..], &settings(), |b| last = b).unwrap();
        assert_eq!(last as usize, pcm.len());
    }

    #[test]
    fn the_stream_encoder_makes_an_mp3_of_mono_and_stereo_pcm_of_any_rate() {
        for (channels, rate) in [(2u8, 44_100u32), (1, 22_050), (2, 96_000), (1, 8_000)] {
            let mut e = StreamEncoder::new(channels, rate, 128).unwrap();
            let mut out = Vec::new();
            // One second of a sine, in pieces.
            let frames = rate as usize;
            let pcm: Vec<i16> = (0..frames)
                .flat_map(|i| {
                    let v = ((i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 12_000.0) as i16;
                    std::iter::repeat_n(v, channels as usize)
                })
                .collect();
            for piece in pcm.chunks(4096 * channels as usize) {
                e.push(piece, &mut out).unwrap();
            }
            let tag = e.finish(&mut out).unwrap();
            if !tag.is_empty() {
                out[..tag.len()].copy_from_slice(&tag);
            }
            let (found_rate, _kbps, _mode) = first_frame(&out).unwrap_or_else(|| panic!("no MP3 frame for {channels} ch at {rate} Hz"));
            let expected = if StreamEncoder::MPEG_RATES.contains(&rate) { rate } else { CD_SAMPLE_RATE };
            assert_eq!(found_rate, expected, "{channels} ch at {rate} Hz");
        }
        assert!(StreamEncoder::new(3, 44_100, 128).is_err());
        assert_eq!(nearest_bitrate(130), 128);
        assert_eq!(nearest_bitrate(1000), 320);
    }
}
