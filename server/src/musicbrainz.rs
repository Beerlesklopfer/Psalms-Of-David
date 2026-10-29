// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! MusicBrainz and the Cover Art Archive for the CD import (docs/spec-cd-ripping.md, section 4).
//!
//! * `disc_id` computes the MusicBrainz Disc ID from the table of contents of a CD,
//! * `parse_release` / `parse_discid` turn the JSON of MusicBrainz into candidates (pure functions),
//! * `Lookup` is the one place where the internet is touched, so that tests put a fake there,
//! * `HttpLookup` is the real one: one request per second at most, a User-Agent with a contact address,
//!   public addresses only (`fetch::get_bytes`).
//!
//! The two services are named by constants as defaults; the settings `cd_musicbrainz_url` and `cd_coverart_url`
//! replace them (a mirror, a test server).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};

/// The public MusicBrainz web service (documented at https://musicbrainz.org/doc/MusicBrainz_API).
pub const DEFAULT_MUSICBRAINZ_URL: &str = "https://musicbrainz.org";
/// The public Cover Art Archive (https://musicbrainz.org/doc/Cover_Art_Archive/API).
pub const DEFAULT_COVERART_URL: &str = "https://coverartarchive.org";
/// MusicBrainz asks for at most one request per second and address; a little more is kept.
pub const MIN_GAP: Duration = Duration::from_millis(1100);
/// The largest answer of MusicBrainz that is read, and the largest cover.
pub const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_COVER_BYTES: usize = 4 * 1024 * 1024;
/// The sizes of a cover that the Cover Art Archive makes.
pub const COVER_SIZES: &[u32] = &[250, 500, 1200];

/// The table of contents of an audio CD as `cd-da-reader` gives it: sector numbers counted from the start of the
/// disc (no lead-in of 150 sectors added). Only audio tracks; `last_track - first_track + 1 == starts_lba.len()`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Toc {
    pub first_track: u32,
    pub last_track: u32,
    pub leadout_lba: u32,
    pub starts_lba: Vec<u32>,
}

/// The lead-in that the MusicBrainz offsets count in addition to the sector numbers of the drive.
const LEAD_IN: u32 = 150;

impl Toc {
    /// Is the table believable?
    pub fn is_valid(&self) -> bool {
        let n = self.starts_lba.len() as u32;
        (1..=99).contains(&self.first_track)
            && self.last_track >= self.first_track
            && self.last_track <= 99
            && n == self.last_track - self.first_track + 1
            && self.starts_lba.windows(2).all(|w| w[0] < w[1])
            && self.starts_lba.last().is_some_and(|last| *last < self.leadout_lba)
            && self.leadout_lba < 1_000_000
    }

    /// The table of contents in the form of the MusicBrainz `toc` parameter, `1+14+334409+150+16910+...` (first
    /// track, last track, lead-out, then one offset per track, all counted with the lead-in of 150 sectors).
    pub fn from_param(param: &str) -> Option<Toc> {
        let numbers: Vec<u32> = param.trim().split(['+', ' ']).filter(|p| !p.is_empty()).map(str::parse).collect::<Result<_, _>>().ok()?;
        let [first, last, leadout, offsets @ ..] = numbers.as_slice() else { return None };
        let starts: Vec<u32> = offsets.iter().map(|o| o.checked_sub(LEAD_IN)).collect::<Option<_>>()?;
        let toc = Toc { first_track: *first, last_track: *last, leadout_lba: leadout.checked_sub(LEAD_IN)?, starts_lba: starts };
        toc.is_valid().then_some(toc)
    }

    /// The offsets as MusicBrainz counts them (+150), for the `toc=` form of the lookup.
    fn offsets(&self) -> Vec<u32> {
        self.starts_lba.iter().map(|s| s + LEAD_IN).collect()
    }
}

/// The MusicBrainz Disc ID (https://musicbrainz.org/doc/Disc_ID_Calculation): SHA-1 over the first and the last
/// track (two hex digits each), then 100 offsets of eight hex digits each (the lead-out, then track 1 to 99, a
/// track that does not exist counts 0), base64 with `+ / =` replaced by `. _ -`.
pub fn disc_id(toc: &Toc) -> String {
    let mut text = format!("{:02X}{:02X}", toc.first_track, toc.last_track);
    let mut offsets = [0u32; 100];
    offsets[0] = toc.leadout_lba + LEAD_IN;
    for (i, start) in toc.starts_lba.iter().enumerate() {
        offsets[(toc.first_track as usize) + i] = start + LEAD_IN;
    }
    for o in offsets {
        text.push_str(&format!("{o:08X}"));
    }
    let digest = Sha1::digest(text.as_bytes());
    base64(&digest).replace('+', ".").replace('/', "_").replace('=', "-")
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Is this the form of a MusicBrainz identifier (a UUID)? It goes into a URL, so nothing else is let through.
pub fn is_mbid(s: &str) -> bool {
    s.len() == 36 && s.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// The address of the lookup by Disc ID. The exact form first; `toc` gives the form for a disc whose ID is not
/// known but whose table of contents is near one that is.
pub fn discid_url(base: &str, id: &str, toc: Option<&Toc>) -> String {
    match toc {
        None => format!("{base}/ws/2/discid/{id}?inc=artist-credits+recordings+release-groups&fmt=json"),
        Some(t) => {
            let offsets: Vec<String> = t.offsets().iter().map(u32::to_string).collect();
            format!(
                "{base}/ws/2/discid/-?toc={}+{}+{}+{}&cdstubs=no&inc=artist-credits+recordings+release-groups&fmt=json",
                t.first_track,
                t.last_track,
                t.leadout_lba + LEAD_IN,
                offsets.join("+")
            )
        }
    }
}

/// Percent-encodes `s` for a query string.
fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The address of a search for releases by words (title and artist): what a person typed when the disc is not known or known wrongly.
pub fn search_url(base: &str, words: &str) -> String {
    // The characters of the search syntax are taken out; the words go into the title and the artist alike.
    let clean: String = words.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '\'' || c == '-' { c } else { ' ' }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let query = format!("release:({clean}) OR artist:({clean})");
    format!("{base}/ws/2/release/?query={}&limit=15&fmt=json", encode(&query))
}

/// The address of one release with its tracks.
pub fn release_url(base: &str, mbid: &str) -> String {
    format!("{base}/ws/2/release/{mbid}?inc=artist-credits+recordings+release-groups&fmt=json")
}

/// The answer of a search: releases without a disc that matches (no tracks).
pub fn parse_search(json: &str) -> Result<Vec<Candidate>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("not JSON: {e}"))?;
    let list = v.get("releases").and_then(Value::as_array).ok_or("no list of releases in the answer")?;
    Ok(list.iter().filter_map(|r| parse_release_of(r, None, None)).collect())
}

pub fn cover_url(base: &str, mbid: &str, px: u32) -> String {
    let size = if COVER_SIZES.contains(&px) { px } else { 500 };
    format!("{base}/release/{mbid}/front-{size}")
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Credit {
    pub name: String,
    /// The text that joins this name to the next (" gelesen von ", " & ", empty).
    pub join: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CandTrack {
    pub number: u32,
    pub title: String,
    pub length_ms: Option<u64>,
}

/// One release that MusicBrainz offers for a disc.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Candidate {
    pub release_id: String,
    pub title: String,
    /// The first name of the credit: the author of a book.
    pub author: String,
    /// The names after the first: the speakers of a book.
    pub narrators: Vec<String>,
    /// All names with their joining text, for a person to decide when the guess above is wrong.
    pub credits: Vec<Credit>,
    pub date: Option<String>,
    pub year: Option<String>,
    /// ISO 639-3 as MusicBrainz writes it (`deu`).
    pub language: Option<String>,
    /// Which disc of the release this is; `None` if the disc was not in the answer.
    pub disc: Option<u32>,
    pub discs_total: u32,
    /// Is the disc with the asked ID in this release (else only the table of contents was alike)?
    pub disc_id_matched: bool,
    pub tracks: Vec<CandTrack>,
    pub cover_available: bool,
    /// MusicBrainz calls the release an audiobook (secondary type of its release group), or it has a speaker
    /// besides the author: a book to be filed as an audiobook.
    pub audiobook: bool,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn credits(v: &Value) -> Vec<Credit> {
    v.get("artist-credit")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|c| {
                    let name = text(c, "name").or_else(|| c.get("artist").and_then(|a| text(a, "name")))?;
                    Some(Credit { name, join: c.get("joinphrase").and_then(Value::as_str).unwrap_or("").to_owned() })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The secondary types of a release group that make a release a spoken work (MusicBrainz's own names).
const SPOKEN_TYPES: &[&str] = &["audiobook", "audio drama", "spokenword"];

/// One release object of MusicBrainz (from a Disc ID lookup or from `/release/<id>`). `disc_id` picks the medium
/// that carries it; without a match the medium is unknown and `disc` and `tracks` stay empty.
pub fn parse_release(release: &Value, disc_id: Option<&str>) -> Option<Candidate> {
    parse_release_of(release, disc_id, None)
}

/// A release with the medium chosen by its Disc ID, else (when `track_count` is given) the first one that has that many tracks, else the first.
pub fn parse_release_of(release: &Value, disc_id: Option<&str>, track_count: Option<usize>) -> Option<Candidate> {
    let release_id = text(release, "id")?;
    let title = text(release, "title")?;
    let credits = credits(release);
    let author = credits.first().map(|c| c.name.clone()).unwrap_or_default();
    let narrators: Vec<String> = credits.iter().skip(1).map(|c| c.name.clone()).collect();
    let media = release.get("media").and_then(Value::as_array).cloned().unwrap_or_default();
    let matched = disc_id
        .and_then(|id| media.iter().find(|m| m.get("discs").and_then(Value::as_array).is_some_and(|d| d.iter().any(|d| d.get("id").and_then(Value::as_str) == Some(id)))))
        .or_else(|| {
            let n = track_count?;
            let count = |m: &Value| m.get("tracks").and_then(Value::as_array).map(Vec::len).or_else(|| m.get("track-count").and_then(Value::as_u64).map(|c| c as usize));
            media.iter().find(|m| count(m) == Some(n) && m.get("tracks").is_some())
        });
    let (disc, tracks) = match matched {
        Some(m) => {
            let pos = m.get("position").and_then(Value::as_u64).map(|p| p as u32);
            let tracks = m
                .get("tracks")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .enumerate()
                        .map(|(i, t)| CandTrack {
                            number: t.get("position").and_then(Value::as_u64).map_or(i as u32 + 1, |p| p as u32),
                            title: text(t, "title").or_else(|| t.get("recording").and_then(|r| text(r, "title"))).unwrap_or_default(),
                            length_ms: t.get("length").and_then(Value::as_u64).or_else(|| t.get("recording").and_then(|r| r.get("length")).and_then(Value::as_u64)),
                        })
                        .collect()
                })
                .unwrap_or_default();
            (pos, tracks)
        }
        None => (None, Vec::new()),
    };
    let date = text(release, "date");
    let spoken = release
        .get("release-group")
        .and_then(|g| g.get("secondary-types"))
        .and_then(Value::as_array)
        .is_some_and(|t| t.iter().filter_map(Value::as_str).any(|x| SPOKEN_TYPES.contains(&x.to_ascii_lowercase().as_str())));
    Some(Candidate {
        audiobook: spoken || !narrators.is_empty(),
        release_id,
        title,
        author,
        narrators,
        credits,
        year: date.as_ref().map(|d| d.chars().take(4).collect()),
        date,
        language: release.get("text-representation").and_then(|t| text(t, "language")),
        disc,
        discs_total: media.len().max(1) as u32,
        disc_id_matched: disc_id.is_some_and(|id| matched.is_some_and(|m| m.get("discs").and_then(Value::as_array).is_some_and(|d| d.iter().any(|d| d.get("id").and_then(Value::as_str) == Some(id))))),
        tracks,
        cover_available: release.get("cover-art-archive").and_then(|c| c.get("front")).and_then(Value::as_bool).unwrap_or(false),
    })
}

/// The answer of `discid/<id>`: a list of releases (`releases`), or for a stub-less miss nothing.
pub fn parse_discid(json: &str, disc_id: &str) -> Result<Vec<Candidate>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("not JSON: {e}"))?;
    let list = v.get("releases").and_then(Value::as_array).ok_or("no list of releases in the answer")?;
    Ok(list.iter().filter_map(|r| parse_release(r, Some(disc_id))).collect())
}

/// Why a request to a service failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError {
    /// No address to send from (the lookup is off).
    Failed(String),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let LookupError::Failed(why) = self;
        f.write_str(why)
    }
}

/// What kind of service answers (the limit of one request per second applies to MusicBrainz only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    MusicBrainz,
    CoverArt,
}

/// The one place where the internet is touched. Blocking: call it from a blocking task. `Ok(None)` is a plain
/// "not there" (404).
pub trait Lookup: Send + Sync {
    fn get(&self, service: Service, url: &str, user_agent: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, LookupError>;
}

/// Lets requests through at least `gap` apart, one at a time.
pub struct RateGate {
    gap: Duration,
    last: Mutex<Option<Instant>>,
}

impl RateGate {
    pub fn new(gap: Duration) -> RateGate {
        RateGate { gap, last: Mutex::new(None) }
    }

    /// Waits until a request may go, and holds the gate while `f` runs, so the next one waits for the end of this
    /// one plus the gap.
    pub fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = *last {
            let since = at.elapsed();
            if since < self.gap {
                std::thread::sleep(self.gap - since);
            }
        }
        let out = f();
        *last = Some(Instant::now());
        out
    }
}

/// The real thing.
pub struct HttpLookup {
    gate: RateGate,
}

impl HttpLookup {
    pub fn new() -> HttpLookup {
        HttpLookup { gate: RateGate::new(MIN_GAP) }
    }
}

impl Default for HttpLookup {
    fn default() -> Self {
        HttpLookup::new()
    }
}

impl Lookup for HttpLookup {
    fn get(&self, service: Service, url: &str, user_agent: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, LookupError> {
        let fetch = || crate::fetch::get_bytes(url, max_bytes, user_agent).map_err(LookupError::Failed);
        match service {
            Service::MusicBrainz => self.gate.run(fetch),
            Service::CoverArt => fetch(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISCID: &str = include_str!("../tests/data/mb-discid.json");
    const TOC_LOOKUP: &str = include_str!("../tests/data/mb-toc-lookup.json");
    const RELEASE: &str = include_str!("../tests/data/mb-release.json");

    /// "Sturz der Titanen" CD 2 of 12 (Ken Follett, gelesen von Johannes Steck), as `rd toc` read it from the disc.
    pub(crate) fn spike_toc() -> Toc {
        Toc {
            first_track: 1,
            last_track: 14,
            leadout_lba: 334259,
            starts_lba: vec![0, 16760, 48475, 78673, 103647, 134004, 149048, 162914, 192909, 213458, 248099, 271069, 291260, 306741],
        }
    }

    #[test]
    fn the_disc_id_of_the_spike_cd_is_the_one_musicbrainz_knows() {
        assert_eq!(disc_id(&spike_toc()), "AzCi4YfdjUrdmGcBX7URtQixDmE-");
    }

    #[test]
    fn every_disc_of_the_recorded_answer_gets_its_own_id_back() {
        // The answer lists the offsets of all 12 discs; the ID computed from them must be the ID listed.
        let v: Value = serde_json::from_str(DISCID).unwrap();
        let mut checked = 0;
        for medium in v["releases"][0]["media"].as_array().unwrap() {
            for d in medium["discs"].as_array().unwrap() {
                let offsets: Vec<u32> = d["offsets"].as_array().unwrap().iter().map(|o| o.as_u64().unwrap() as u32 - 150).collect();
                let toc = Toc { first_track: 1, last_track: offsets.len() as u32, leadout_lba: d["sectors"].as_u64().unwrap() as u32 - 150, starts_lba: offsets };
                assert!(toc.is_valid());
                assert_eq!(disc_id(&toc), d["id"].as_str().unwrap());
                checked += 1;
            }
        }
        assert_eq!(checked, 12);
    }

    #[test]
    fn the_toc_parameter_of_the_apps_is_read_back() {
        let toc = spike_toc();
        let url = discid_url("https://x", "-", Some(&toc));
        let param = url.split("toc=").nth(1).unwrap().split('&').next().unwrap();
        assert_eq!(Toc::from_param(param), Some(toc));
        assert_eq!(Toc::from_param("1+2"), None);
        assert_eq!(Toc::from_param("1+2+1000+100+700"), None, "an offset below the lead-in");
        assert_eq!(Toc::from_param("x+y+z"), None);
    }

    #[test]
    fn a_toc_must_make_sense() {
        let ok = spike_toc();
        assert!(ok.is_valid());
        let mut t = ok.clone();
        t.starts_lba.pop();
        assert!(!t.is_valid());
        let mut t = ok.clone();
        t.starts_lba[3] = t.starts_lba[2];
        assert!(!t.is_valid());
        let mut t = ok.clone();
        t.leadout_lba = 100;
        assert!(!t.is_valid());
        let mut t = ok;
        t.first_track = 0;
        assert!(!t.is_valid());
    }

    #[test]
    fn the_answer_for_a_disc_id_becomes_a_candidate_with_disc_2_of_12() {
        let found = parse_discid(DISCID, "AzCi4YfdjUrdmGcBX7URtQixDmE-").unwrap();
        assert_eq!(found.len(), 1);
        let c = &found[0];
        assert_eq!(c.title, "Sturz der Titanen");
        assert_eq!(c.author, "Ken Follett");
        assert_eq!(c.narrators, ["Johannes Steck"]);
        assert_eq!(c.credits[0].join, " gelesen von ");
        assert_eq!((c.disc, c.discs_total, c.disc_id_matched), (Some(2), 12, true));
        assert_eq!(c.language.as_deref(), Some("deu"));
        assert_eq!((c.date.as_deref(), c.year.as_deref()), (Some("2010-09-28"), Some("2010")));
        assert!(c.cover_available);
        assert!(c.audiobook, "a speaker besides the author");
        assert!(is_mbid(&c.release_id));
        assert_eq!(c.tracks.len(), 14);
        assert_eq!((c.tracks[0].number, c.tracks[0].title.as_str(), c.tracks[0].length_ms), (1, "Teil 014", Some(223000)));
        assert_eq!(c.tracks[1].length_ms, Some(423000));
        // An ID that is not among the media: the release is still offered, without a disc.
        let other = parse_discid(DISCID, "nonsense-").unwrap();
        assert_eq!((other[0].disc, other[0].disc_id_matched, other[0].tracks.len()), (None, false, 0));
    }

    #[test]
    fn the_other_recorded_answers_parse_too() {
        let a = parse_discid(TOC_LOOKUP, "AzCi4YfdjUrdmGcBX7URtQixDmE-").unwrap();
        assert!(!a.is_empty() && a[0].discs_total >= 1);
        let v: Value = serde_json::from_str(RELEASE).unwrap();
        // A release fetched by its own identifier does not list the discs, so no medium can be picked from it.
        let c = parse_release(&v, Some("AzCi4YfdjUrdmGcBX7URtQixDmE-")).unwrap();
        assert_eq!((c.title.as_str(), c.author.as_str(), c.discs_total, c.disc, c.tracks.len()), ("Sturz der Titanen", "Ken Follett", 12, None, 0));
    }

    #[test]
    fn a_release_group_of_the_type_audiobook_makes_an_audiobook() {
        let v: Value = serde_json::from_str(DISCID).unwrap();
        let mut release = v["releases"][0].clone();
        // Without a speaker and without the type it is music.
        release["artist-credit"] = serde_json::json!([{ "name": "Some Band", "joinphrase": "" }]);
        assert!(!parse_release(&release, None).unwrap().audiobook);
        release["release-group"] = serde_json::json!({ "primary-type": "Other", "secondary-types": ["Audiobook"] });
        assert!(parse_release(&release, None).unwrap().audiobook);
        release["release-group"] = serde_json::json!({ "primary-type": "Album", "secondary-types": ["Live"] });
        assert!(!parse_release(&release, None).unwrap().audiobook);
    }

    #[test]
    fn the_answer_of_a_search_by_words_becomes_candidates_without_tracks() {
        // Recorded from the search for "Bach".
        let found = parse_search(include_str!("../tests/data/mb-search.json")).unwrap();
        assert_eq!(found.len(), 15);
        assert!(found.iter().all(|c| !c.release_id.is_empty() && !c.title.is_empty() && c.tracks.is_empty() && !c.disc_id_matched), "{found:?}");
        assert_eq!((found[0].title.as_str(), found[0].author.as_str()), ("Bach", "Bach"));
        assert!(found[0].discs_total >= 1);
    }

    /// Against the real MusicBrainz (`cargo test -- --ignored`): the address of a search is accepted and the answer is read.
    #[test]
    #[ignore = "needs the internet"]
    fn the_real_musicbrainz_answers_a_search_by_words() {
        let url = search_url(DEFAULT_MUSICBRAINZ_URL, "Bach Matthäus-Passion");
        let body = HttpLookup::new().get(Service::MusicBrainz, &url, "david-server/test ( test@example.org )", MAX_JSON_BYTES).unwrap().expect("an answer");
        let found = parse_search(&String::from_utf8_lossy(&body)).unwrap();
        assert!(!found.is_empty(), "{url}");
    }

    #[test]
    fn rubbish_is_an_error_not_a_panic() {
        assert!(parse_discid("not json", "x").is_err());
        assert!(parse_discid("{}", "x").is_err());
        assert_eq!(parse_discid(r#"{"releases":[]}"#, "x").unwrap(), vec![]);
        assert!(parse_release(&serde_json::json!({"id": "x"}), None).is_none());
    }

    #[test]
    fn addresses_are_built_from_the_base() {
        assert_eq!(discid_url("https://mb.example", "AzCi-", None), "https://mb.example/ws/2/discid/AzCi-?inc=artist-credits+recordings+release-groups&fmt=json");
        let u = discid_url("https://mb.example", "-", Some(&Toc { first_track: 1, last_track: 2, leadout_lba: 1000, starts_lba: vec![0, 500] }));
        assert!(u.starts_with("https://mb.example/ws/2/discid/-?toc=1+2+1150+150+650&cdstubs=no"), "{u}");
        assert_eq!(cover_url("https://c.example", "x", 500), "https://c.example/release/x/front-500");
        assert_eq!(cover_url("https://c.example", "x", 1200), "https://c.example/release/x/front-1200");
        assert_eq!(cover_url("https://c.example", "x", 333), "https://c.example/release/x/front-500");
        assert!(is_mbid("58537de9-45ed-4b6b-9f67-e9571c63bc03") && !is_mbid("../etc/passwd") && !is_mbid("58537de9-45ed-4b6b-9f67-e9571c63bc0"));
    }

    #[test]
    fn the_gate_keeps_requests_apart() {
        let gate = RateGate::new(Duration::from_millis(120));
        let start = Instant::now();
        let mut stamps = Vec::new();
        for _ in 0..3 {
            gate.run(|| stamps.push(start.elapsed()));
        }
        assert!(stamps[1] - stamps[0] >= Duration::from_millis(115), "{stamps:?}");
        assert!(stamps[2] - stamps[1] >= Duration::from_millis(115), "{stamps:?}");
    }
}
