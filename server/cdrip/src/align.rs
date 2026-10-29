// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Comparing two readings of a track that may be off by a few frames.
//!
//! Some drives (Blu-ray and DVD drives above all) do not deliver the audio of a CD bit-exact: a reading may lose or repeat a few
//! frames at the start of a command, so that two readings of the same track never have the same hash although they carry the same
//! music. `cd-paranoia` handles that by comparing overlapping pieces of the readings. This is the same idea for whole readings:
//! walk both in step; where they differ, look for a small shift after which they agree again for a whole window. A difference that
//! no shift explains (a flipped sample, a burst of garbage) is a real disagreement.

use crate::encode::SECTOR_BYTES;

/// Bytes of one frame (a sample of both channels, 16 bit).
const FRAME: usize = 4;
/// How many frames a reading may be shifted against the other at one place: two sectors.
pub const MAX_SLIP: usize = 2 * SECTOR_BYTES / FRAME;
/// How many frames must agree after a shift before it counts (not an accident of silence or a flat tone).
const WINDOW: usize = 128;
/// How many shifts a track may need before it is not trusted any more.
pub const MAX_SLIPS: u32 = 400;

fn equal_run(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// The shift after the difference at the start of `a` and `b` (frames to skip in each), if one makes them agree for a window.
fn resync(a: &[u8], b: &[u8]) -> Option<(usize, usize)> {
    let (fa, fb) = (a.len() / FRAME, b.len() / FRAME);
    for d in 1..=MAX_SLIP {
        for (da, db) in [(d, 0), (0, d)] {
            if fa < da + WINDOW || fb < db + WINDOW {
                continue;
            }
            if a[da * FRAME..(da + WINDOW) * FRAME] == b[db * FRAME..(db + WINDOW) * FRAME] {
                return Some((da, db));
            }
        }
    }
    None
}

/// Whether two readings are the same music: equal, or equal but for shifts of at most [`MAX_SLIP`] frames. The number of shifts
/// that were needed (0 for identical readings), or `None` when they really differ.
pub fn agree(a: &[u8], b: &[u8]) -> Option<u32> {
    let (mut i, mut j, mut slips) = (0usize, 0usize, 0u32);
    loop {
        let run = equal_run(&a[i..], &b[j..]) / FRAME * FRAME;
        i += run;
        j += run;
        let (ra, rb) = (a.len() - i, b.len() - j);
        if ra == 0 && rb == 0 {
            return Some(slips);
        }
        // The end: a rest too short to be compared may be what a shift left over.
        if ra <= MAX_SLIP * FRAME + WINDOW * FRAME && rb <= MAX_SLIP * FRAME + WINDOW * FRAME && (ra < WINDOW * FRAME || rb < WINDOW * FRAME) {
            return Some(slips);
        }
        let (da, db) = resync(&a[i..], &b[j..])?;
        i += da * FRAME;
        j += db * FRAME;
        slips += 1;
        if slips > MAX_SLIPS {
            return None;
        }
    }
}

/// Frames of a sector.
const SECTOR_FRAMES: usize = SECTOR_BYTES / FRAME;
/// How many frames of a sector may differ for a sector to count as "concealed" (about two thirds).
pub const CONCEALED_MAX_FRAMES: usize = 420;
/// The largest difference of a sample (of 32767) in a concealed sector: the drive guessed, it did not make noise.
pub const CONCEALED_MAX_STEP: i32 = 8192;
/// The share of the sectors of a track that may be concealed, in thousandths (and at least [`CONCEALED_MIN_SECTORS`] sectors).
pub const CONCEALED_MAX_SHARE: usize = 50;
pub const CONCEALED_MIN_SECTORS: usize = 16;

/// Two readings of a place that the drive could not read (a scratch): its error concealment guesses the samples, differently each
/// time, so the readings differ in a few sectors by small amounts and are the same everywhere else. They count as the same music when
/// the differing sectors are few (at most 5 % of the track), each differs in at most [`CONCEALED_MAX_FRAMES`] frames, and no sample
/// differs by more than [`CONCEALED_MAX_STEP`]. The number of sectors that differ (0 for identical readings), or `None` when the
/// readings differ more than that (or have another length).
pub fn concealed_agree(a: &[u8], b: &[u8]) -> Option<usize> {
    if a.len() != b.len() {
        return None;
    }
    let sectors = a.len() / SECTOR_BYTES;
    let limit = (sectors * CONCEALED_MAX_SHARE / 1000).max(CONCEALED_MIN_SECTORS);
    let mut differing = 0;
    for (x, y) in a.chunks(SECTOR_BYTES).zip(b.chunks(SECTOR_BYTES)) {
        if x == y {
            continue;
        }
        differing += 1;
        if differing > limit || x.len() != SECTOR_BYTES {
            return None;
        }
        let mut frames = 0;
        for f in 0..SECTOR_FRAMES {
            let (fx, fy) = (&x[f * FRAME..(f + 1) * FRAME], &y[f * FRAME..(f + 1) * FRAME]);
            if fx != fy {
                frames += 1;
                for ch in 0..2 {
                    let (sx, sy) = (i16::from_le_bytes([fx[ch * 2], fx[ch * 2 + 1]]), i16::from_le_bytes([fy[ch * 2], fy[ch * 2 + 1]]));
                    if (i32::from(sx) - i32::from(sy)).abs() > CONCEALED_MAX_STEP {
                        return None;
                    }
                }
            }
        }
        if frames > CONCEALED_MAX_FRAMES {
            return None;
        }
    }
    Some(differing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn music(frames: usize) -> Vec<u8> {
        // Not periodic within a window, so a wrong shift never matches by accident.
        let mut x = 12345u32;
        (0..frames * FRAME)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn identical_readings_agree_without_a_shift() {
        let a = music(20_000);
        assert_eq!(agree(&a, &a.clone()), Some(0));
    }

    #[test]
    fn a_frame_lost_or_repeated_in_one_reading_is_a_shift_and_not_a_disagreement() {
        let a = music(20_000);
        let mut lost = a.clone();
        lost.drain(5_000 * FRAME..5_003 * FRAME);
        assert_eq!(agree(&a, &lost), Some(1));
        let mut repeated = a.clone();
        let dup: Vec<u8> = a[9_000 * FRAME..9_002 * FRAME].to_vec();
        repeated.splice(9_000 * FRAME..9_000 * FRAME, dup);
        assert_eq!(agree(&a, &repeated), Some(1));
        let mut both = lost.clone();
        both.drain(15_000 * FRAME..15_001 * FRAME);
        assert_eq!(agree(&both, &repeated), Some(3));
    }

    #[test]
    fn a_changed_sample_is_a_disagreement_that_no_shift_explains() {
        let a = music(20_000);
        let mut bad = a.clone();
        bad[40_000] ^= 0x55;
        assert_eq!(agree(&a, &bad), None);
        let mut burst = a.clone();
        for b in &mut burst[30_000..30_800] {
            *b = 0;
        }
        assert_eq!(agree(&a, &burst), None);
    }

    #[test]
    fn a_shift_larger_than_two_sectors_is_not_trusted() {
        let a = music(20_000);
        let mut far = a.clone();
        far.drain(5_000 * FRAME..(5_000 + MAX_SLIP + 1) * FRAME);
        assert_eq!(agree(&a, &far), None);
    }

    #[test]
    fn readings_that_differ_in_the_last_frames_agree() {
        let a = music(20_000);
        assert_eq!(agree(&a, &a[..a.len() - 5 * FRAME]), Some(0));
    }

    fn nudge(data: &mut [u8], sector: usize, frames: usize, by: i16) {
        for f in 0..frames {
            let at = sector * SECTOR_BYTES + f * FRAME;
            let v = i16::from_le_bytes([data[at], data[at + 1]]).saturating_add(by);
            data[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
    }

    #[test]
    fn readings_that_differ_by_small_guesses_in_a_few_sectors_are_the_same_music_with_concealed_errors() {
        let a = music(588 * 400);
        let mut b = a.clone();
        // The drive guessed: 30 frames of a sector by 60, every 17th sector, 20 sectors in all.
        for k in 0..20 {
            nudge(&mut b, 15 + k * 17, 30, 60);
        }
        assert_eq!(concealed_agree(&a, &a.clone()), Some(0));
        assert_eq!(concealed_agree(&a, &b), Some(20));
    }

    #[test]
    fn garbage_a_big_step_many_sectors_or_another_length_are_no_agreement() {
        let a = music(588 * 400);
        let mut big = a.clone();
        nudge(&mut big, 100, 5, 12000);
        assert_eq!(concealed_agree(&a, &big), None, "a sample that moves by more than a guess");
        let mut wide = a.clone();
        nudge(&mut wide, 100, 500, 40);
        assert_eq!(concealed_agree(&a, &wide), None, "most of a sector differs");
        let mut many = a.clone();
        for k in 0..60 {
            nudge(&mut many, k * 6, 10, 20);
        }
        assert_eq!(concealed_agree(&a, &many), None, "more than 5 % of the track");
        assert_eq!(concealed_agree(&a, &a[..a.len() - SECTOR_BYTES]), None);
    }
}
