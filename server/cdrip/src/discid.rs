// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The MusicBrainz disc id and the `toc` lookup parameter, from a table of contents.
//! Algorithm: <https://musicbrainz.org/doc/Disc_ID_Calculation>.

use base64::Engine;
use sha1::{Digest, Sha1};

/// Frames of lead-in that MusicBrainz adds to every sector address.
pub const LEAD_IN_FRAMES: u32 = 150;
/// The gap between the last audio track and a following data session (CD-Extra), in sectors.
pub const SESSION_GAP_SECTORS: u32 = 11_400;

/// One track of a table of contents, with its zero-based start sector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TocTrack {
    pub number: u8,
    pub start_lba: u32,
    pub is_audio: bool,
}

/// A table of contents as read from the drive (zero-based sectors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocInfo {
    pub tracks: Vec<TocTrack>,
    pub leadout_lba: u32,
}

impl TocInfo {
    /// The audio tracks only; data tracks are not part of the disc id.
    pub fn audio(&self) -> Vec<TocTrack> {
        self.tracks.iter().copied().filter(|t| t.is_audio).collect()
    }

    /// End of the audio area: the lead-out, or for a CD-Extra disc (data tracks after the audio)
    /// the lead-out of the audio session, which lies the session gap before the first data track.
    pub fn audio_leadout(&self) -> u32 {
        let last_audio = self.tracks.iter().filter(|t| t.is_audio).map(|t| t.number).max();
        let data_after = self.tracks.iter().filter(|t| !t.is_audio && last_audio.is_some_and(|a| t.number > a)).map(|t| t.start_lba).min();
        match data_after {
            Some(start) => start.saturating_sub(SESSION_GAP_SECTORS),
            None => self.leadout_lba,
        }
    }

    /// First and last audio track number, or None without audio.
    pub fn first_last(&self) -> Option<(u8, u8)> {
        let a = self.audio();
        Some((a.first()?.number, a.last()?.number))
    }
}

/// The MusicBrainz disc id (28 characters), or None for a disc without audio.
pub fn disc_id(toc: &TocInfo) -> Option<String> {
    let (first, last) = toc.first_last()?;
    let audio = toc.audio();
    let mut offsets = vec![0u32; 100];
    offsets[0] = toc.audio_leadout() + LEAD_IN_FRAMES;
    for t in &audio {
        if (1..=99).contains(&t.number) {
            offsets[t.number as usize] = t.start_lba + LEAD_IN_FRAMES;
        }
    }
    let mut text = format!("{first:02X}{last:02X}");
    for o in &offsets {
        text.push_str(&format!("{o:08X}"));
    }
    let digest = Sha1::digest(text.as_bytes());
    Some(base64::engine::general_purpose::STANDARD.encode(digest).replace('+', ".").replace('/', "_").replace('=', "-"))
}

/// The `toc` parameter of a fuzzy lookup: `first+count+leadout+offset1+offset2...`.
pub fn toc_param(toc: &TocInfo) -> Option<String> {
    let (first, last) = toc.first_last()?;
    let audio = toc.audio();
    let mut parts = vec![first.to_string(), (last - first + 1).to_string(), (toc.audio_leadout() + LEAD_IN_FRAMES).to_string()];
    parts.extend(audio.iter().map(|t| (t.start_lba + LEAD_IN_FRAMES).to_string()));
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Sturz der Titanen", CD 1 of 12: id and sector values from the MusicBrainz answer (offsets there include the 150 lead-in).
    fn sturz() -> TocInfo {
        let offsets = [150u32, 33660, 61019, 94782, 109951, 151221, 184569, 213652, 227853, 263327, 288316, 315734, 334986];
        TocInfo {
            tracks: offsets.iter().enumerate().map(|(i, o)| TocTrack { number: i as u8 + 1, start_lba: o - 150, is_audio: true }).collect(),
            leadout_lba: 358071 - 150,
        }
    }

    #[test]
    fn matches_the_musicbrainz_record() {
        assert_eq!(disc_id(&sturz()).unwrap(), "ACO.I327bDmV1GDMbqs6A8um89g-");
    }

    #[test]
    fn toc_parameter() {
        let p = toc_param(&sturz()).unwrap();
        assert!(p.starts_with("1+13+358071+150+33660+"), "{p}");
        assert_eq!(p.split('+').count(), 3 + 13);
    }

    #[test]
    fn data_session_after_the_audio_is_left_out() {
        let mut t = sturz();
        let leadout_of_audio = t.leadout_lba;
        t.tracks.push(TocTrack { number: 14, start_lba: leadout_of_audio + SESSION_GAP_SECTORS, is_audio: false });
        t.leadout_lba += 50_000;
        assert_eq!(t.audio_leadout(), leadout_of_audio);
        assert_eq!(disc_id(&t), disc_id(&sturz()));
    }

    #[test]
    fn no_audio_no_id() {
        let t = TocInfo { tracks: vec![TocTrack { number: 1, start_lba: 0, is_audio: false }], leadout_lba: 1000 };
        assert_eq!(disc_id(&t), None);
        assert_eq!(toc_param(&t), None);
    }
}
