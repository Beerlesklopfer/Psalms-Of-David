// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! CD-Text: the titles and names that the maker of a CD wrote onto it (the lead-in), read with `READ TOC/PMA/ATIP` format 5
//! (MMC). `cd-da-reader` does not read it, so the command goes through `SG_IO` here.
//!
//! The answer is a header of 4 bytes and packs of 18 bytes: type, track, sequence number, block and character position,
//! 12 bytes of text, and a CRC. The texts of one type (title, performer ...) run through the packs one after another, each
//! ending with a NUL; the first belongs to the track named in the first pack (0 is the album), the next to the track after
//! it. A text of one TAB means "the same as the one before". Only block 0 is read (the first language), and only the
//! one-byte character sets (ISO 8859-1, ASCII); a disc with only Japanese text (MS-JIS) has none for us.

use serde::Serialize;

/// The text of a disc.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CdText {
    pub album_title: Option<String>,
    pub album_performer: Option<String>,
    pub tracks: Vec<CdTextTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CdTextTrack {
    pub number: u8,
    pub title: Option<String>,
    pub performer: Option<String>,
}

impl CdText {
    /// The title of a track, if the disc names it.
    pub fn title_of(&self, number: u8) -> Option<&str> {
        self.tracks.iter().find(|t| t.number == number).and_then(|t| t.title.as_deref())
    }

    /// Album, performer and a title for every one of `numbers`: enough to file the CD without asking anybody else.
    pub fn is_complete(&self, numbers: &[u8]) -> bool {
        self.album_title.is_some() && self.album_performer.is_some() && !numbers.is_empty() && numbers.iter().all(|n| self.title_of(*n).is_some())
    }
}

const PACK: usize = 18;
const TITLE: u8 = 0x80;
const PERFORMER: u8 = 0x81;
const SIZE_INFO: u8 = 0x8F;

/// CRC-16/CCITT (polynomial 0x1021, start 0) over the first 16 bytes, stored inverted. Drives that leave it 0 are believed.
fn crc_ok(p: &[u8]) -> bool {
    let stored = u16::from_be_bytes([p[16], p[17]]);
    if stored == 0 {
        return true;
    }
    let mut crc: u16 = 0;
    for &b in &p[..16] {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    !crc == stored
}

fn block_of(p: &[u8]) -> u8 {
    (p[3] >> 4) & 0x07
}

/// The texts of one type in block 0, by track number.
fn texts(packs: &[&[u8]], kind: u8, latin1: bool) -> Vec<(u8, String)> {
    let of_kind: Vec<&&[u8]> = packs.iter().filter(|p| p[0] == kind && block_of(p) == 0 && p[3] & 0x80 == 0).collect();
    let Some(first) = of_kind.first() else { return Vec::new() };
    let mut bytes: Vec<u8> = Vec::with_capacity(of_kind.len() * 12);
    for p in &of_kind {
        bytes.extend_from_slice(&p[4..16]);
    }
    let start = first[1] & 0x7F;
    let mut out: Vec<(u8, String)> = Vec::new();
    // The last pack is filled up with NULs: they end the last text and are no texts of their own.
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    for (i, raw) in bytes.split(|b| *b == 0).enumerate() {
        let Some(track) = u8::try_from(usize::from(start) + i).ok().filter(|t| *t <= 99) else { break };
        let text = if raw == b"\t" {
            out.last().map(|(_, t)| t.clone()).unwrap_or_default()
        } else if latin1 {
            raw.iter().map(|b| char::from(*b)).collect()
        } else {
            raw.iter().filter(|b| b.is_ascii()).map(|b| char::from(*b)).collect()
        };
        out.push((track, text));
    }
    out
}

fn clean(s: &str) -> Option<String> {
    let t: String = s.chars().filter(|c| !c.is_control()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    (!t.is_empty()).then_some(t)
}

/// Reads the answer of `READ TOC/PMA/ATIP` format 5 (with its header of 4 bytes). `None` if it holds no text we can read.
pub fn parse(answer: &[u8]) -> Option<CdText> {
    if answer.len() < 4 + PACK {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([answer[0], answer[1]])) + 2;
    let body = &answer[4..len.min(answer.len())];
    let packs: Vec<&[u8]> = body.chunks_exact(PACK).filter(|p| crc_ok(p)).collect();
    // The character set of block 0: the first byte of the text of the first size pack (0 = ISO 8859-1, 1 = ASCII, 0x80 = MS-JIS).
    let charset = packs.iter().find(|p| p[0] == SIZE_INFO && block_of(p) == 0 && p[1] == 0).map_or(0, |p| p[4]);
    if !matches!(charset, 0x00 | 0x01) {
        return None;
    }
    let latin1 = charset == 0;
    let titles = texts(&packs, TITLE, latin1);
    let performers = texts(&packs, PERFORMER, latin1);
    let find = |list: &[(u8, String)], n: u8| list.iter().find(|(t, _)| *t == n).and_then(|(_, s)| clean(s));
    let mut numbers: Vec<u8> = titles.iter().chain(performers.iter()).map(|(n, _)| *n).filter(|n| *n > 0).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let text = CdText {
        album_title: find(&titles, 0),
        album_performer: find(&performers, 0),
        tracks: numbers.into_iter().map(|n| CdTextTrack { number: n, title: find(&titles, n), performer: find(&performers, n) }).collect(),
    };
    (text.album_title.is_some() || text.tracks.iter().any(|t| t.title.is_some())).then_some(text)
}

/// The CD-Text of the disc in `device`; `None` if it has none or the drive cannot give it.
#[cfg(target_os = "linux")]
pub fn read(device: &str) -> Option<CdText> {
    use std::os::fd::AsRawFd;
    let f = crate::drive::open_sg(device).ok()?;
    let fd = f.as_raw_fd();
    let cdb = |len: u16| {
        let [hi, lo] = len.to_be_bytes();
        [0x43, 0x00, 0x05, 0, 0, 0, 0, hi, lo, 0]
    };
    let mut head = [0u8; 4];
    crate::drive::sg_io(fd, &cdb(4), Some(&mut head))?;
    let len = usize::from(u16::from_be_bytes([head[0], head[1]])) + 2;
    if len < 4 + PACK {
        return None;
    }
    let mut all = vec![0u8; len.min(usize::from(u16::MAX))];
    let got = crate::drive::sg_io(fd, &cdb(all.len() as u16), Some(&mut all))?;
    all.truncate(got);
    parse(&all)
}

#[cfg(not(target_os = "linux"))]
pub fn read(_device: &str) -> Option<CdText> {
    None
}

/// Builds the answer of a drive for tests: packs of the given texts (album first, then the tracks), block 0, ISO 8859-1.
pub fn encode_for_tests(titles: &[&str], performers: &[&str]) -> Vec<u8> {
    let mut packs: Vec<[u8; PACK]> = Vec::new();
    let mut seq: u8 = 0;
    for (kind, list) in [(TITLE, titles), (PERFORMER, performers)] {
        let mut bytes: Vec<u8> = Vec::new();
        for t in list {
            bytes.extend(t.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')));
            bytes.push(0);
        }
        // Where each text starts, to name the track whose text starts in (or runs into) each 12-byte piece.
        let mut track_at = Vec::new();
        let mut pos = 0usize;
        for (i, t) in list.iter().enumerate() {
            track_at.push((pos, i as u8));
            pos += t.chars().count() + 1;
        }
        for (k, chunk) in bytes.chunks(12).enumerate() {
            let at = k * 12;
            let (start, track) = track_at.iter().rev().find(|(p, _)| *p <= at).copied().unwrap_or((0, 0));
            let mut p = [0u8; PACK];
            p[0] = kind;
            p[1] = track;
            p[2] = seq;
            p[3] = ((at - start).min(15)) as u8;
            p[4..4 + chunk.len()].copy_from_slice(chunk);
            packs.push(p);
            seq = seq.wrapping_add(1);
        }
    }
    let mut size = [0u8; PACK];
    size[0] = SIZE_INFO;
    size[2] = seq;
    packs.push(size);
    let mut out = Vec::new();
    out.extend_from_slice(&((packs.len() * PACK + 2) as u16).to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    for p in packs {
        out.extend_from_slice(&p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_album_and_the_tracks_are_read_with_umlauts() {
        let raw = encode_for_tests(&["Er hört dein Gebet", "Lied eins", "Zwei", "Drei, ein längerer Titel über zwölf"], &["Christoph Zehendner & Manfred Staiger", "\t", "\t", "\t"]);
        let t = parse(&raw).unwrap();
        assert_eq!(t.album_title.as_deref(), Some("Er hört dein Gebet"));
        assert_eq!(t.album_performer.as_deref(), Some("Christoph Zehendner & Manfred Staiger"));
        assert_eq!(t.tracks.len(), 3);
        assert_eq!((t.title_of(1), t.title_of(3)), (Some("Lied eins"), Some("Drei, ein längerer Titel über zwölf")));
        assert_eq!(t.tracks[1].performer.as_deref(), Some("Christoph Zehendner & Manfred Staiger"), "a TAB repeats the text before");
        assert!(t.is_complete(&[1, 2, 3]) && !t.is_complete(&[1, 2, 3, 4]) && !t.is_complete(&[]));
    }

    #[test]
    fn a_pack_with_a_wrong_crc_is_left_out_and_a_good_crc_is_believed() {
        let mut raw = encode_for_tests(&["Album", "A"], &["P", "P"]);
        let first = &mut raw[4..4 + PACK];
        let mut crc: u16 = 0;
        for &b in &first[..16] {
            crc ^= u16::from(b) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
            }
        }
        first[16..18].copy_from_slice(&(!crc).to_be_bytes());
        assert_eq!(parse(&raw).unwrap().album_title.as_deref(), Some("Album"));
        raw[4 + 16] ^= 0xFF;
        assert!(parse(&raw).is_none(), "the only pack of titles is dropped, and performers alone are no text to file a CD with");
    }

    #[test]
    fn nothing_readable_is_none() {
        assert!(parse(&[]).is_none());
        assert!(parse(&[0, 2, 0, 0]).is_none());
        let mut jis = encode_for_tests(&["X", "Y"], &["Z", "Z"]);
        let n = jis.len();
        jis[n - PACK + 4] = 0x80; // the size pack says MS-JIS
        assert!(parse(&jis).is_none());
    }
}
