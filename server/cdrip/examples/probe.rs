// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads one track of the disc several times and tells where the readings differ: `probe <device> <track> [reads]`.
//! A tool for finding out what a drive delivers (jitter, flaws, stalls). It writes nothing, unless the environment names a folder in
//! `PROBE_DUMP`: the raw readings are then written there as `track<N>_read<K>.pcm`.

use std::time::Instant;

use cdrip::cd_da_reader::{CdReader, open_track_stream};

const SECTOR: usize = 2352;

fn read_track(reader: &CdReader, toc: &cdrip::cd_da_reader::Toc, track: u8) -> (Vec<u8>, Vec<(u32, f64)>, Option<String>) {
    let mut stream = match open_track_stream(reader, toc, track) {
        Ok(s) => s,
        Err(e) => return (Vec::new(), Vec::new(), Some(format!("open: {e}"))),
    };
    let mut pcm = Vec::new();
    let mut slow = Vec::new();
    let mut err = None;
    loop {
        let at = stream.current_sector();
        let t = Instant::now();
        match stream.next_chunk() {
            Ok(Some(c)) => {
                let s = t.elapsed().as_secs_f64();
                if s > 1.5 {
                    slow.push((at, s));
                }
                pcm.extend(c);
            }
            Ok(None) => break,
            Err(e) => {
                err = Some(format!("sector {at}: {e}"));
                break;
            }
        }
    }
    (pcm, slow, err)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (device, track, reads) = (args.get(1).expect("device"), args.get(2).expect("track").parse::<u8>().expect("track"), args.get(3).map_or(3, |r| r.parse::<usize>().unwrap()));
    let reader = CdReader::open_path(device).expect("open");
    let toc = reader.read_toc().expect("toc");
    let far = toc.tracks.iter().filter(|t| t.number != track).max_by_key(|t| t.start_lba.abs_diff(toc.tracks.iter().find(|x| x.number == track).unwrap().start_lba)).map(|t| t.start_lba).unwrap_or(0);
    let mut all: Vec<Vec<u8>> = Vec::new();
    for i in 0..reads {
        // Somewhere else first, so that the cache of the drive does not serve the reading.
        let _ = cdrip::rip::read_far(&reader, far);
        let t = Instant::now();
        let (pcm, slow, err) = read_track(&reader, &toc, track);
        println!("reading {}: {} sectors in {:.1} s{}{}", i + 1, pcm.len() / SECTOR, t.elapsed().as_secs_f64(), if slow.is_empty() { String::new() } else { format!(", slow commands (sector, s): {slow:?}") }, err.as_ref().map(|e| format!(", ERROR {e}")).unwrap_or_default());
        if let Some(dir) = std::env::var_os("PROBE_DUMP") {
            let _ = std::fs::write(std::path::Path::new(&dir).join(format!("track{track}_read{}.pcm", i + 1)), &pcm);
        }
        all.push(pcm);
    }
    for a in 0..all.len() {
        for b in a + 1..all.len() {
            let (x, y) = (&all[a], &all[b]);
            let n = x.len().min(y.len()) / SECTOR;
            let mut runs: Vec<(usize, usize)> = Vec::new();
            for s in 0..n {
                if x[s * SECTOR..(s + 1) * SECTOR] != y[s * SECTOR..(s + 1) * SECTOR] {
                    match runs.last_mut() {
                        Some(r) if r.0 + r.1 == s => r.1 += 1,
                        _ => runs.push((s, 1)),
                    }
                }
            }
            let differing: usize = runs.iter().map(|r| r.1).sum();
            println!("readings {} and {}: identical: {}; differing sectors: {differing} of {n} in {} runs {:?}; same music with shifts allowed: {:?}", a + 1, b + 1, x == y, runs.len(), &runs[..runs.len().min(12)], cdrip::align::agree(x, y));
        }
    }
}
