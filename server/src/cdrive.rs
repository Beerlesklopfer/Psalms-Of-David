// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CD drive of the server (project owner, 2026-10-28: the drive sits at the server, so the server reads the
//! disc). This module finds the drives, keeps their state (tray, medium, table of contents), notices changes by the
//! kernel's uevents or by looking at the drives at an interval, and rips a disc into the library through the same
//! checking, naming and placing as an upload (`cdimport::finalize`).
//!
//! The drive itself sits behind `DriveBackend`, so tests use a fake with an open and a closed tray, a changing disc,
//! read errors and a vanishing drive. `SystemDrives` is the real one (the crate `cdrip`).
//!
//! Rules kept here:
//! * no device name is built in: the drives come from the system, the setting `cd_device` can name one;
//! * the tray is never closed by the server: an eject is sent only when the state says the tray is shut with a disc in;
//! * a drive is not read while its tray is open, nor probed while a rip runs on it;
//! * nothing half done reaches the library: the files are made in a staging folder and moved by `finalize`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cdrip::discid::TocInfo;
use cdrip::drive::{DriveStatus, EjectOutcome};
use cdrip::rip::{EncoderSpec, Event as RipEvent, JobSpec, ResumeTrack, RipError, TrackSpec};
use serde::Serialize;

use crate::cd_settings::CdSettings;
use crate::cdimport::{self, CdService, Job, JobPatch, Meta, MetaTrack, Refusal, StagedTrack};
use crate::cdresume::{self, Entry, Manifest};
use crate::musicbrainz::Toc;
use crate::store::Store;

/// The tick of the monitor: how fast a change of a setting and a uevent are noticed.
const TICK: Duration = Duration::from_millis(500);
/// With uevents on, the drives are looked at anyway this often, in case an event was lost.
const SAFETY_LOOK: Duration = Duration::from_secs(60);
/// A drive that is not ready (spinning up, closing its tray) is looked at again after this time.
const NOT_READY_RETRY: Duration = Duration::from_secs(1);
/// How many times a track is read at most when two reads must agree.
const MAX_READS: u32 = 3;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tray {
    Open,
    Closed,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Medium {
    /// No disc.
    None,
    Audio,
    /// A disc that is not an audio CD (data, blank).
    Other,
    Unknown,
}

/// The state of one track of a rip that runs (or has just ended): `waiting`, `reading`, `verifying` (the second reading),
/// `encoding`, `done` or `failed`; `fraction` is that of the reading of the track (0 to 1).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TrackState {
    pub number: u32,
    pub title: String,
    pub seconds: u32,
    pub state: &'static str,
    pub fraction: f64,
    /// For `failed`: the code of the reason.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// What the service knows of a rip while it runs and after it ended, until a new one starts or it is closed.
#[derive(Debug, Clone)]
pub struct Live {
    pub job_id: i64,
    pub device: String,
    /// The fraction of the whole job (0 to 1).
    pub fraction: f64,
    pub finished: bool,
    /// The code the rip ended with (empty if it ended well).
    pub error: String,
    pub tracks: Vec<TrackState>,
    pub verify: bool,
    /// What the ripper measures: the filling of the ring, the waits of the reader, the time the drive read.
    pub stats: Arc<cdrip::ring::RipStats>,
    /// The size of the ring in bytes.
    pub ring_bytes: u64,
}

impl Live {
    /// Seconds of audio that wait in the ring now.
    pub fn buffer_seconds(&self) -> f64 {
        cdrip::rip::bytes_to_seconds(self.stats.filled_bytes.load(Ordering::Relaxed))
    }

    /// The ring in seconds of audio, and how full it is in percent.
    pub fn buffer_max_seconds(&self) -> f64 {
        self.ring_bytes as f64 / cdrip::ring::BYTES_PER_SECOND as f64
    }

    /// The speed the drive reads at, as a multiple of 1x (75 sectors a second), from what it delivered and the time it took for it (waits of
    /// the reader are not in it): a measurement, not a guess from the buffer. `None` until enough has been read to say (2 s of reading).
    pub fn speed_x(&self) -> Option<f64> {
        let ms = self.stats.read_ms.load(Ordering::Relaxed);
        let sectors = self.stats.sectors_read.load(Ordering::Relaxed);
        (ms >= 2_000 && sectors > 0).then(|| ((sectors as f64 / (ms as f64 / 1000.0) / 75.0) * 10.0).round() / 10.0)
    }

    pub fn buffer_percent(&self) -> u32 {
        (self.stats.filled_bytes.load(Ordering::Relaxed).max(0) as f64 * 100.0 / self.ring_bytes.max(1) as f64).round().min(100.0) as u32
    }

    /// How often the reader had to wait for room in the ring, and the longest wait in seconds.
    pub fn reader_waits(&self) -> (u64, f64) {
        (self.stats.reader_waits.load(Ordering::Relaxed), self.stats.wait_ms_longest.load(Ordering::Relaxed) as f64 / 1000.0)
    }

    /// The track the rip is at, counted from 1 among the chosen tracks: the first one that is being read or checked, else the first
    /// one that is not done (the tracks that are done and the one after them when the rip is between two).
    pub fn current_position(&self) -> usize {
        let i = self.tracks.iter().position(|t| matches!(t.state, "reading" | "verifying" | "rereading")).or_else(|| self.tracks.iter().position(|t| t.state != "done"));
        i.map_or(self.tracks.len().max(1), |i| i + 1)
    }

    fn track(&mut self, number: u32) -> Option<&mut TrackState> {
        self.tracks.iter_mut().find(|t| t.number == number)
    }

    /// Takes in an event of the ripper.
    pub fn apply(&mut self, e: &RipEvent) {
        match e {
            RipEvent::TrackStart { track } => {
                // The one that was read before is being encoded now.
                for t in self.tracks.iter_mut().filter(|t| t.state == "reading") {
                    t.state = "encoding";
                    t.fraction = 1.0;
                }
                if let Some(t) = self.track(u32::from(*track)) {
                    t.state = "reading";
                    t.fraction = 0.0;
                }
            }
            RipEvent::TrackProgress { track, pass, fraction } => {
                if let Some(t) = self.track(u32::from(*track)) {
                    // 1 is the first reading, 2 the confirming one, 3 and more the repeated readings after the two did not agree.
                    t.state = match *pass {
                        1 => "reading",
                        2 => "verifying",
                        _ => "rereading",
                    };
                    t.fraction = *fraction;
                }
            }
            RipEvent::Progress { fraction, .. } => self.fraction = *fraction,
            RipEvent::Encoded { track, .. } => {
                let verify = self.verify;
                if let Some(t) = self.track(u32::from(*track))
                    && t.state != "done"
                    && t.state != "verifying"
                {
                    // Read once and written; with verification it waits for the second reading ("read"), which is not "waiting" (not read yet).
                    t.state = if verify { "read" } else { "encoding" };
                    t.fraction = 1.0;
                }
            }
            RipEvent::Unverified { track } => {
                if let Some(t) = self.track(u32::from(*track)) {
                    t.state = "unverified";
                    t.fraction = 1.0;
                }
            }
            RipEvent::TrackDone { track, .. } => {
                if let Some(t) = self.track(u32::from(*track)) {
                    // A track that was taken without a confirmation stays marked, also when it is filed.
                    if t.state != "unverified" {
                        t.state = "done";
                    }
                    t.fraction = 1.0;
                }
            }
            // A track that is left out is shown as failed at once, not "rereading" until the end of the rip.
            RipEvent::Error { kind, track: Some(n), .. } => {
                if let Some(t) = self.track(u32::from(*n)) {
                    t.state = "failed";
                    t.error = rip_failure_code(kind).to_owned();
                }
            }
            _ => {}
        }
    }
}

/// One audio track of the disc in a drive.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TrackInfo {
    /// The number on the disc.
    pub number: u32,
    pub seconds: u32,
}

/// What is known about one drive. Sent to the devices as the `drive` event and by `GET /cd/drive`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DriveState {
    pub device: String,
    pub tray: Tray,
    pub medium: Medium,
    /// Number of audio tracks.
    pub tracks: u32,
    pub disc_id: Option<String>,
    /// The table of contents as the MusicBrainz `toc` parameter, for the lookup.
    pub toc: Option<String>,
    /// Total playing time of the audio tracks in seconds.
    pub seconds: u32,
    /// The audio tracks with their numbers on the disc and their lengths, for a choice of tracks.
    pub track_list: Vec<TrackInfo>,
    /// The process may not use the drive (the group of the drive is missing).
    pub denied: bool,
    /// A rip is running on it.
    pub ripping: bool,
    #[serde(skip)]
    pub table: Option<Toc>,
}

impl DriveState {
    fn empty(device: &str) -> DriveState {
        DriveState { device: device.to_owned(), tray: Tray::Unknown, medium: Medium::Unknown, tracks: 0, disc_id: None, toc: None, seconds: 0, track_list: Vec::new(), denied: false, ripping: false, table: None }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The drive behind a trait.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Ok,
    /// The process may not open the drive.
    Denied,
    Gone,
}

/// What looking at a drive found.
#[derive(Debug, Clone)]
pub struct Probe {
    pub access: Access,
    pub status: DriveStatus,
    /// The table of contents, read only when a disc is in, the tray is shut and the disc is new.
    pub toc: Option<TocInfo>,
    /// The disc is the one that was seen before (nothing was read).
    pub unchanged: bool,
}

/// Something that counts the drive events of the kernel.
pub trait Watcher: Send {
    /// The number of relevant events since the last call.
    fn take(&self) -> u32;
}

impl Watcher for cdrip::uevent::Watch {
    fn take(&self) -> u32 {
        cdrip::uevent::Watch::take(self)
    }
}

pub trait DriveBackend: Send + Sync {
    /// The drives of the system, as device paths.
    fn devices(&self) -> Vec<String>;
    /// Looks at one drive. `known_disc`: a disc was seen in it and nothing says it changed, so the table of contents
    /// need not be read again.
    fn probe(&self, device: &str, known_disc: bool) -> Probe;
    /// Reads, verifies, encodes and tags the tracks of the spec into `spec.out_dir`.
    fn rip(&self, spec: &JobSpec, cancel: &AtomicBool, emit: &mut dyn FnMut(&RipEvent)) -> Result<(), RipError>;
    /// Opens the tray, but only when a disc is in and the tray is shut.
    fn eject(&self, device: &str) -> std::io::Result<EjectOutcome>;
    /// Starts listening to the kernel's drive events; `None` where that is not possible.
    fn watch(&self) -> Option<Box<dyn Watcher>>;
    /// Asks the drive to read at `times` x 1x for the rip that follows (0 = leave the drive's own speed).
    fn set_speed(&self, _device: &str, _times: u32) -> std::io::Result<()> {
        Ok(())
    }
    /// Gives the drive its own speed back.
    fn reset_speed(&self, _device: &str) -> std::io::Result<()> {
        Ok(())
    }
}

/// The real drives.
pub struct SystemDrives;

impl DriveBackend for SystemDrives {
    fn devices(&self) -> Vec<String> {
        cdrip::drive::devices()
    }

    fn probe(&self, device: &str, known_disc: bool) -> Probe {
        let status = match cdrip::drive::status(device) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Probe { access: Access::Denied, status: DriveStatus::Unknown, toc: None, unchanged: false },
            Err(_) => return Probe { access: Access::Gone, status: DriveStatus::Unknown, toc: None, unchanged: false },
        };
        if status != DriveStatus::DiscOk {
            // An open tray, no disc or a drive that is busy: nothing is read.
            return Probe { access: Access::Ok, status, toc: None, unchanged: false };
        }
        if known_disc && cdrip::drive::media_changed(device) == Some(false) {
            return Probe { access: Access::Ok, status, toc: None, unchanged: true };
        }
        let toc = cdrip::cd_da_reader::CdReader::open_path(device).and_then(|r| r.read_toc()).ok().map(|t| cdrip::drive::toc_info(&t));
        Probe { access: Access::Ok, status, toc, unchanged: false }
    }

    fn rip(&self, spec: &JobSpec, cancel: &AtomicBool, emit: &mut dyn FnMut(&RipEvent)) -> Result<(), RipError> {
        let fail = |e: cdrip::cd_da_reader::CdReaderError| RipError { kind: if cdrip::drive::classify(&e) == cdrip::drive::ErrorClass::DriveGone { "drive_gone" } else { "read" }, message: e.to_string(), track: None };
        let reader = cdrip::cd_da_reader::CdReader::open_path(&spec.device).map_err(fail)?;
        let toc = reader.read_toc().map_err(fail)?;
        cdrip::rip::rip_disc(&reader, &toc, spec, cancel, emit)
    }

    fn eject(&self, device: &str) -> std::io::Result<EjectOutcome> {
        cdrip::drive::eject(device)
    }

    fn watch(&self) -> Option<Box<dyn Watcher>> {
        cdrip::uevent::Watch::start().ok().map(|w| Box::new(w) as Box<dyn Watcher>)
    }

    fn set_speed(&self, device: &str, times: u32) -> std::io::Result<()> {
        cdrip::drive::set_speed(device, times)
    }

    fn reset_speed(&self, device: &str) -> std::io::Result<()> {
        cdrip::drive::reset_speed(device)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The state and the service.

/// The audio part of a table of contents as the server's own type (`None` without audio or when it is not sound).
fn audio_toc(info: &TocInfo) -> Option<Toc> {
    let audio = info.audio();
    let (first, last) = info.first_last()?;
    let toc = Toc { first_track: u32::from(first), last_track: u32::from(last), leadout_lba: info.audio_leadout(), starts_lba: audio.iter().map(|t| t.start_lba).collect() };
    toc.is_valid().then_some(toc)
}

fn state_of(device: &str, p: &Probe, before: Option<&DriveState>) -> DriveState {
    let mut s = DriveState::empty(device);
    match p.access {
        Access::Denied => {
            s.denied = true;
            return s;
        }
        Access::Gone => return s,
        Access::Ok => {}
    }
    match p.status {
        DriveStatus::TrayOpen => {
            s.tray = Tray::Open;
            s.medium = Medium::None;
        }
        DriveStatus::NoDisc => {
            s.tray = Tray::Closed;
            s.medium = Medium::None;
        }
        DriveStatus::NotReady | DriveStatus::Unknown => {}
        DriveStatus::DiscOk => {
            s.tray = Tray::Closed;
            if p.unchanged {
                if let Some(b) = before {
                    s = DriveState { ripping: false, ..b.clone() };
                }
            } else if let Some(info) = &p.toc {
                match audio_toc(info) {
                    Some(toc) => {
                        s.medium = Medium::Audio;
                        s.tracks = info.audio().len() as u32;
                        s.track_list = cdrip::drive::track_sectors(info).into_iter().map(|(number, sectors)| TrackInfo { number: u32::from(number), seconds: sectors / 75 }).collect();
                        s.disc_id = Some(crate::musicbrainz::disc_id(&toc));
                        s.toc = Some(toc_param(&toc));
                        s.seconds = toc.leadout_lba.saturating_sub(toc.starts_lba[0]) / 75;
                        s.table = Some(toc);
                    }
                    None => s.medium = if info.audio().is_empty() { Medium::Other } else { Medium::Unknown },
                }
            }
        }
    }
    s
}

/// The `1+N+leadout+offsets` form of a table of contents (with the lead-in added).
fn toc_param(t: &Toc) -> String {
    let mut parts = vec![t.first_track.to_string(), t.last_track.to_string(), (t.leadout_lba + 150).to_string()];
    parts.extend(t.starts_lba.iter().map(|s| (s + 150).to_string()));
    parts.join("+")
}

/// What `eject` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EjectResult {
    Ejected,
    /// The tray is open already or there is no disc: nothing was sent (a command could close the tray).
    NothingToEject,
}

#[derive(Default)]
struct Shared {
    states: Vec<DriveState>,
    /// `udev` while kernel events are listened to, else `poll`.
    mode: &'static str,
    /// Device -> job that rips on it.
    running: HashMap<String, i64>,
    cancels: HashMap<i64, Arc<AtomicBool>>,
    /// The service is stopping: a rip that ends by the cancel flag is `interrupted`, not `cancelled`.
    shutting_down: bool,
    /// The rip of each drive that runs or ran last.
    live: HashMap<String, Live>,
    /// Counts the tracks that rips have finished or encoded (a stop waits for the next one).
    ticks: u64,
}

/// The drives of the server: the state, the running rips and the way to look at the hardware.
#[derive(Clone)]
pub struct Drives {
    backend: Arc<dyn DriveBackend>,
    shared: Arc<Mutex<Shared>>,
}

impl Drives {
    pub fn new(backend: Arc<dyn DriveBackend>) -> Drives {
        Drives { backend, shared: Arc::new(Mutex::new(Shared { mode: "poll", ..Shared::default() })) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The state of all drives, with the mode.
    pub fn snapshot(&self) -> (Vec<DriveState>, &'static str) {
        let g = self.lock();
        (g.states.clone(), g.mode)
    }

    /// The table of contents of the audio CD in the drive named by `device` (empty = the first drive that has one).
    pub fn inserted_toc(&self, device: &str) -> Option<Toc> {
        let g = self.lock();
        g.states.iter().filter(|s| device.is_empty() || s.device == device).find_map(|s| s.table.clone().filter(|_| s.medium == Medium::Audio && s.tray == Tray::Closed))
    }

    /// Looks at the drives (blocking). `force`: read the table of contents again whatever the drive says. Returns
    /// whether the state changed, and whether a drive was not ready (look again soon).
    pub fn refresh(&self, only: &str, force: bool) -> (bool, bool) {
        let mut devices = self.backend.devices();
        if !only.is_empty() {
            devices.retain(|d| d == only);
            if devices.is_empty() {
                // The named drive is not there: shown as gone rather than silently ignored.
                devices.push(only.to_owned());
            }
        }
        let (before, busy): (Vec<DriveState>, Vec<String>) = {
            let g = self.lock();
            (g.states.clone(), g.running.keys().cloned().collect())
        };
        let mut states = Vec::new();
        let mut not_ready = false;
        for device in &devices {
            let old = before.iter().find(|s| s.device == *device);
            if busy.contains(device) {
                // A rip is running: the drive is left alone.
                if let Some(o) = old {
                    states.push(DriveState { ripping: true, ..o.clone() });
                }
                continue;
            }
            let known = !force && old.is_some_and(|o| o.tray == Tray::Closed && matches!(o.medium, Medium::Audio | Medium::Other));
            let probe = self.backend.probe(device, known);
            if probe.access == Access::Gone {
                continue;
            }
            not_ready |= matches!(probe.status, DriveStatus::NotReady | DriveStatus::Unknown) && probe.access == Access::Ok;
            states.push(state_of(device, &probe, old));
        }
        let changed = states != before;
        if changed {
            self.lock().states = states;
        }
        (changed, not_ready)
    }

    /// Opens the tray of `device` after a rip or on request, never closing it: the state is looked at first (blocking).
    pub fn eject(&self, device: &str) -> Result<EjectResult, Refusal> {
        if self.lock().running.contains_key(device) {
            return Err(Refusal(409, "busy"));
        }
        // Look at the real state, not at the last one: the tray may have been opened by hand a moment ago.
        let probe = self.backend.probe(device, false);
        match probe.access {
            Access::Denied => return Err(Refusal(503, "drive_denied")),
            Access::Gone => return Err(Refusal(404, "no_drive")),
            Access::Ok => {}
        }
        if probe.status != DriveStatus::DiscOk {
            return Ok(EjectResult::NothingToEject);
        }
        match self.backend.eject(device) {
            Ok(EjectOutcome::Ejected) => Ok(EjectResult::Ejected),
            Ok(EjectOutcome::NothingToEject) => Ok(EjectResult::NothingToEject),
            Ok(EjectOutcome::Failed) => {
                tracing::warn!(device, "the drive refused to open its tray (the kernel's eject and the SCSI commands both failed)");
                Err(Refusal(500, "eject_failed"))
            }
            Err(e) => {
                tracing::warn!(device, error = %e, "the drive could not be opened to eject");
                Err(Refusal(500, "eject_failed"))
            }
        }
    }

    /// Asks a running rip to stop. False if no rip with this job runs.
    pub fn cancel(&self, job_id: i64) -> bool {
        match self.lock().cancels.get(&job_id) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// The service is stopping: lets the rips that run finish the track they are on if that takes no longer than `grace`
    /// (a track that is finished is written to the manifest and is not read again), then asks them to end and waits up
    /// to `patience` for them to write their manifest. The jobs end as `interrupted`, the finished tracks stay.
    pub async fn stop_gracefully(&self, grace: Duration, patience: Duration) {
        let ticks = {
            let mut g = self.lock();
            if g.running.is_empty() {
                return;
            }
            g.shutting_down = true;
            g.ticks
        };
        let until = Instant::now() + grace;
        while Instant::now() < until && {
            let g = self.lock();
            !g.running.is_empty() && g.ticks == ticks
        } {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        for flag in self.lock().cancels.values() {
            flag.store(true, Ordering::Relaxed);
        }
        let until = Instant::now() + patience;
        while Instant::now() < until && !self.lock().running.is_empty() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Starts the picture of a rip (replacing what the drive had of the last one).
    pub fn live_start(&self, live: Live) {
        self.lock().live.insert(live.device.clone(), live);
    }

    /// Changes the picture of the rip with this job.
    pub fn live_update(&self, job_id: i64, f: impl FnOnce(&mut Live)) {
        if let Some(l) = self.lock().live.values_mut().find(|l| l.job_id == job_id) {
            f(l);
        }
    }

    pub fn live_of_job(&self, job_id: i64) -> Option<Live> {
        self.lock().live.values().find(|l| l.job_id == job_id).cloned()
    }

    pub fn live_of_device(&self, device: &str) -> Option<Live> {
        self.lock().live.get(device).cloned()
    }

    /// Closes the list of a rip that has ended. False if it is still running or there is none.
    pub fn dismiss(&self, device: &str) -> bool {
        let mut g = self.lock();
        if g.live.get(device).is_some_and(|l| l.finished) {
            g.live.remove(device);
            return true;
        }
        false
    }

    pub fn is_ripping(&self) -> bool {
        !self.lock().running.is_empty()
    }
}

impl CdService {
    /// Looks at the drives and pushes the new state to the devices if it changed. Returns "look again soon".
    pub async fn refresh_drives(&self, only: &str, force: bool) -> bool {
        let drives = self.drives.clone();
        let only = only.to_owned();
        let Ok((changed, not_ready)) = tokio::task::spawn_blocking(move || drives.refresh(&only, force)).await else { return false };
        if changed {
            self.publish_drive();
        }
        not_ready
    }

    /// The drives as JSON, each with what an interrupted rip of its disc left (`resume`), if anything.
    pub fn drives_json(&self, fingerprint: Option<&crate::cdresume::Fingerprint>) -> Vec<serde_json::Value> {
        let keep = self.resume_keep.load(Ordering::Relaxed);
        self.drives
            .snapshot()
            .0
            .iter()
            .map(|d| {
                let mut v = serde_json::to_value(d).unwrap_or_default();
                let info = d.disc_id.as_deref().filter(|_| d.medium == Medium::Audio).and_then(|id| cdresume::info(&cdresume::dir_of(&self.staging, id), fingerprint, keep));
                v["resume"] = serde_json::to_value(info).unwrap_or_default();
                // The rip that runs on this drive (or ran last, until it is closed): its fraction and the state of every track.
                v["rip"] = self.drives.live_of_device(&d.device).map_or(serde_json::Value::Null, |l| serde_json::json!({ "job_id": l.job_id, "finished": l.finished, "error": l.error, "fraction": l.fraction, "buffer_seconds": (l.buffer_seconds() * 10.0).round() / 10.0, "buffer_max": l.buffer_max_seconds().round() as u32, "buffer_percent": l.buffer_percent(), "reader_waits": l.reader_waits().0, "reader_wait_max_s": l.reader_waits().1, "speed_x": l.speed_x(), "track_states": l.tracks, "current": l.current_position(), "total": l.tracks.len() }));
                // What is known of the CD in the library, as last worked out (null if it is not there or not looked at).
                v["present"] = d.disc_id.as_ref().and_then(|id| self.present_cache.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()).unwrap_or_default();
                v
            })
            .collect()
    }

    /// The `drive` message for the devices.
    pub fn drive_message(&self) -> String {
        let fp = self.fingerprint.lock().unwrap_or_else(|e| e.into_inner()).clone();
        serde_json::json!({ "v": cdimport::EVENTS_VERSION, "type": "drive", "mode": self.drives.snapshot().1, "drives": self.drives_json(fp.as_ref()) }).to_string()
    }

    pub fn publish_drive(&self) {
        self.publish_text(cdimport::BROADCAST, self.drive_message());
    }
}

/// The task that keeps the state: with `cd_udev` on, kernel events (with a slow look as a safety net), else a look
/// every `cd_poll_seconds`. The settings are read all the time, so a change takes effect without a restart.
pub async fn monitor(cd: CdService, store: Store, library: Option<PathBuf>) {
    let mut watcher: Option<Box<dyn Watcher>> = None;
    let mut last_look = Instant::now() - Duration::from_secs(86_400);
    let mut retry_at: Option<Instant> = None;
    let mut last_present = String::new();
    let mut first = true;
    // The disc that was last seen in each drive (None: nothing, or the tray was opened since); a new one is an insertion.
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    let mut primed = false;
    let mut udev_off_logged = false;
    let mut settings = CdSettings::load(&store).await.ok();
    let mut settings_at = Instant::now();
    loop {
        if settings_at.elapsed() >= Duration::from_secs(1) {
            settings = CdSettings::load(&store).await.ok().or(settings);
            settings_at = Instant::now();
            if let Some(set) = &settings {
                cd.apply_settings(set);
            }
        }
        let (udev, poll, device) = settings.as_ref().map_or((false, 5, String::new()), |s| (s.udev, s.poll_seconds, s.device.clone()));
        // Events on: listen (once). Off: stop listening.
        if udev && watcher.is_none() {
            watcher = cd.drives.backend.watch();
        } else if !udev {
            watcher = None;
        }
        let mode = if watcher.is_some() { "udev" } else { "poll" };
        {
            let mut g = cd.drives.lock();
            if g.mode != mode {
                g.mode = mode;
                drop(g);
                cd.publish_drive();
            }
        }
        let events = watcher.as_ref().map_or(0, |w| w.take());
        let interval = if watcher.is_some() { SAFETY_LOOK } else { Duration::from_secs(u64::from(poll)) };
        if events > 0 {
            // The event comes before the drive is ready; look now and again a little later.
            let soon = cd.refresh_drives(&device, true).await;
            last_look = Instant::now();
            retry_at = Some(Instant::now() + if soon { NOT_READY_RETRY } else { Duration::from_millis(1500) });
        } else if first || last_look.elapsed() >= interval || retry_at.is_some_and(|t| Instant::now() >= t) {
            let force = first;
            first = false;
            let soon = cd.refresh_drives(&device, force).await;
            last_look = Instant::now();
            retry_at = soon.then(|| Instant::now() + NOT_READY_RETRY);
        }
        // A disc that is put in: the automatic import looks at it (once per insertion).
        let auto = settings.as_ref().is_some_and(|s| s.auto_import);
        let udev_on = settings.as_ref().is_some_and(|s| s.udev);
        if auto && !udev_on && !udev_off_logged {
            udev_off_logged = true;
            tracing::warn!(reason = "udev_off", "the automatic import is on but the kernel events (cd_udev) are off: nothing will be started by itself");
            log_skipped(&store, &cd, &SkipInfo::default(), "udev_off", String::new()).await;
        }
        if !(auto && !udev_on) {
            udev_off_logged = false;
        }
        for d in cd.drives.snapshot().0 {
            let key = match (d.denied, d.tray, d.medium) {
                (true, _, _) => Some(Some("denied".to_owned())),
                (_, Tray::Closed, Medium::Audio) if d.disc_id.is_some() => Some(d.disc_id.clone()),
                (_, Tray::Closed, Medium::Other) => Some(Some("other".to_owned())),
                (_, Tray::Open, _) | (_, Tray::Closed, Medium::None) => Some(None),
                _ => None,
            };
            let Some(key) = key else { continue };
            let new = key.is_some() && seen.get(&d.device) != Some(&key);
            seen.insert(d.device.clone(), key);
            if new && primed && auto && udev_on {
                let (cd2, store2, library2) = (cd.clone(), store.clone(), library.clone());
                tokio::spawn(async move { auto_import(cd2, store2, library2, d).await });
            }
        }
        if cd.drives.snapshot().0.iter().any(|d| d.medium != Medium::Unknown || d.denied) || !first {
            primed = true;
        }
        // What is in the library may have changed with the disc: the drive message tells.
        if changed_since_look(&mut last_present, &cd) && let Some(set) = &settings {
            cd.refresh_present(&store, library.as_deref(), set).await;
        }
        tokio::time::sleep(TICK).await;
    }
}

/// Tells (once per change of the drives) that the library part of the drive message should be worked out again.
fn changed_since_look(last: &mut String, cd: &CdService) -> bool {
    let now = format!("{:?}", cd.drives.snapshot().0.iter().map(|d| (&d.device, d.tray, d.medium, &d.disc_id)).collect::<Vec<_>>());
    let changed = *last != now;
    *last = now;
    changed
}

/// What is known of a disc that the automatic import turned down, for the log.
#[derive(Debug, Default, Clone)]
struct SkipInfo {
    device: String,
    disc_id: String,
    author: String,
    title: String,
    disc: u32,
}

/// Writes down that the automatic import did not start (journal and log of the tab): one entry, with the reason as a code
/// (`cd.err.<code>` has the text in both languages) and the details that make it clear (book, number of releases ...).
async fn log_skipped(store: &Store, cd: &CdService, info: &SkipInfo, reason: &'static str, detail: String) {
    tracing::info!(device = %info.device, disc_id = %info.disc_id, reason, detail = %detail, author = %info.author, book = %info.title, disc = info.disc, "automatic import: nothing started");
    let patch = JobPatch { phase: Some("skipped".into()), author: Some(info.author.clone()), album: Some(info.title.clone()), disc: Some(i64::from(info.disc.max(1))), ..JobPatch::default() };
    match store.create_cd_job(cdimport::SYSTEM_USER, "auto", &patch).await {
        Ok(job) => {
            let _ = store.update_cd_job(cdimport::SYSTEM_USER, job.id, &JobPatch { error: Some(reason.to_owned()), message: Some(detail.clone()), ..JobPatch::default() }).await;
        }
        Err(e) => tracing::warn!(error = %e, "the log of the import could not be written"),
    }
    if matches!(reason, "no_match" | "ambiguous_match" | "heal_needed" | "no_space" | "no_target") {
        let detail = if reason == "ambiguous_match" { detail } else { String::new() };
        crate::cdflow::notify(store, cd, &crate::cdflow::CdEvent::NotStarted { disc_id: info.disc_id.clone(), reason: reason.to_owned(), author: info.author.clone(), title: info.title.clone(), disc: i64::from(info.disc.max(1)), detail }).await;
    }
}

/// A disc was put in and the setting `cd_auto_import` is on: imports it if MusicBrainz knows exactly one release for it and it is not in
/// the library yet; every other case is turned down with a reason and written down (`log_skipped`), never an error.
pub async fn auto_import(cd: CdService, store: Store, library: Option<PathBuf>, d: DriveState) {
    let Ok(set) = CdSettings::load(&store).await else { return };
    let mut info = SkipInfo { device: d.device.clone(), disc_id: d.disc_id.clone().unwrap_or_default(), ..SkipInfo::default() };
    macro_rules! skip {
        ($reason:expr, $detail:expr) => {{
            log_skipped(&store, &cd, &info, $reason, $detail).await;
            return;
        }};
    }
    if d.denied {
        skip!("drive_denied", String::new());
    }
    if d.medium != Medium::Audio {
        skip!("not_audio", String::new());
    }
    if cd.drives.lock().running.contains_key(&d.device) {
        skip!("busy", String::new());
    }
    let release = match d.table.as_ref() {
        Some(toc) => unique_release(&store, &cd, &set, toc).await,
        None => Err(("no_match", String::new())),
    };
    let hint = release.as_ref().ok().and_then(|c| c.disc.map(|disc| crate::cdpresent::Hint { author: c.author.clone(), title: c.title.clone(), disc }));
    if let Ok(c) = &release {
        (info.author, info.title, info.disc) = (c.author.clone(), c.title.clone(), c.disc.unwrap_or(1));
    }
    // In the library already? Then nothing is done; a CD with tracks missing or damaged is for a person to repair.
    let lengths: HashMap<u32, u32> = d.track_list.iter().map(|t| (t.number, t.seconds)).collect();
    if let Some(p) = crate::cdpresent::find(&store, library.as_deref(), &set, &info.disc_id, &lengths, hint.as_ref()).await {
        (info.author, info.title, info.disc) = (p.author.clone(), p.book.clone(), p.disc);
        let book = format!("{}: {}, CD {}", p.author, p.book, p.disc);
        if p.needs_healing() {
            skip!("heal_needed", format!("{book} ({} of {})", p.tracks_in_library, p.tracks_on_disc));
        }
        // Whole in the library: nothing is read, the job is `present`, and the disc comes out at once.
        tracing::info!(device = %d.device, disc_id = %info.disc_id, reason = "disc_present", detail = %book, "automatic import: nothing started, the disc is ejected");
        finish_present(&store, &cd, cdimport::SYSTEM_USER, "auto", "", &p, &d.device, true, &set.device).await;
        return;
    }
    let candidate = match release {
        Ok(c) => c,
        Err((reason, detail)) => skip!(reason, detail),
    };
    // An interrupted rip of this disc is not resumed by itself.
    let dir = cdresume::dir_of(&cd.staging, &info.disc_id);
    if let Some(r) = cdresume::info(&dir, Some(&cdresume::Fingerprint::of(&set)), u64::from(set.resume_keep_hours)).filter(|i| !i.tracks_done.is_empty()) {
        skip!("resume_pending", format!("{} of {}", r.tracks_done.len(), r.tracks_total));
    }
    let kind = if candidate.audiobook { "audiobook".to_owned() } else { set.target_kind.clone() };
    match cdimport::import_root(&store, library.as_deref(), &kind).await {
        Ok(root) if !crate::cdimport::writable_dir(&root) => skip!("no_target", root.display().to_string()),
        Ok(root) => {
            let need = estimated_bytes(d.table.as_ref().unwrap_or(&Toc { first_track: 1, last_track: 1, leadout_lba: 0, starts_lba: vec![0] }), &set);
            if !crate::diskspace::fits(&root, need, crate::diskspace::reserve(&store).await) {
                skip!("no_space", String::new());
            }
        }
        Err(_) => skip!("no_target", String::new()),
    }
    let req = RipRequest {
        kind: Some(kind),
        author: candidate.author.clone(),
        title: candidate.title.clone(),
        disc: candidate.disc,
        discs_total: Some(candidate.discs_total),
        narrator: candidate.narrators.first().cloned(),
        year: candidate.year.clone(),
        mbid: Some(candidate.release_id.clone()),
        tracks: candidate.tracks.iter().map(|t| t.title.clone()).collect(),
        device: Some(d.device.clone()),
        auto: true,
        ..RipRequest::default()
    };
    match start_rip(&store, &cd, library, cdimport::SYSTEM_USER, req).await {
        Ok(st) => tracing::info!(device = %d.device, disc_id = %info.disc_id, author = %info.author, book = %info.title, disc = info.disc, job = st.job.id, "automatic import started"),
        Err(r) => {
            let reason = match r.refusal.1 {
                "busy" => "busy",
                "drive_denied" => "drive_denied",
                "no_import_folder" => "no_target",
                _ => "start_failed",
            };
            skip!(reason, r.refusal.1.to_owned());
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Ripping.

/// What the caller says about the disc.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RipRequest {
    pub kind: Option<String>,
    pub author: String,
    pub title: String,
    pub disc: Option<u32>,
    pub discs_total: Option<u32>,
    pub narrator: Option<String>,
    pub year: Option<String>,
    /// The MusicBrainz release (for the cover).
    #[serde(alias = "release_id")]
    pub mbid: Option<String>,
    /// The titles of the tracks, in order, if known.
    pub tracks: Vec<String>,
    /// Eject when done; absent = the setting `cd_eject_after`.
    pub eject: Option<bool>,
    /// The drive; absent = the setting `cd_device`, else the first one with an audio CD.
    pub device: Option<String>,
    /// The numbers (on the disc) of the tracks to read; absent = all. The files keep these numbers.
    pub only: Option<Vec<u32>>,
    /// Take over the finished tracks of an interrupted rip of the same disc (default yes); `false` reads everything again.
    pub resume: Option<bool>,
    /// `import` (default) or `heal`: read only the tracks of a CD that is in the library but has tracks missing or damaged.
    pub mode: Option<String>,
    /// Import although the CD is in the library already (it is then a new copy; nothing is overwritten).
    pub force: bool,
    /// Started by the service itself (automatic import): the job belongs to `system`.
    #[serde(skip)]
    pub auto: bool,
    /// The device an app started the rip with (the label of its token; never read from the request body). Empty: the web, MCP or the drive.
    #[serde(skip)]
    pub starter_device: String,
}

fn rip_failure_code(kind: &str) -> &'static str {
    match kind {
        "read" => "read",
        "verify" => "verify",
        "drive_gone" => "drive_gone",
        "encode" => "encode",
        "disk_full" => "no_space",
        "unsupported" => "unsupported",
        "cancelled" => "cancelled",
        _ => "io",
    }
}

/// Why a rip was not started: the refusal, and for `disc_exists` the numbers of the tracks that are in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RipRefusal {
    pub refusal: Refusal,
    pub taken: Vec<u32>,
    /// For `disc_present`: what is known of the CD in the library (see `present_json`).
    pub present: Option<serde_json::Value>,
    /// For `disc_present`: the tray was opened because the request said `"eject": true` and the CD is whole in the library.
    pub ejected: bool,
}

impl From<Refusal> for RipRefusal {
    fn from(refusal: Refusal) -> Self {
        RipRefusal { refusal, taken: Vec::new(), present: None, ejected: false }
    }
}

/// Starts ripping the disc that is in the drive. Returns the job at once; the work goes on in the background and is
/// reported by events. Refusals: `no_drive`, `drive_denied`, `tray_open`, `no_disc`, `busy`, `invalid_meta`.
/// A rip that was started: the job, and what became of an interrupted rip of the same disc: `resumed` (its finished tracks
/// were taken over), `mismatch` (the settings or the choice of tracks changed: it was removed and the disc is read again)
/// or `fresh` (there was none, or the caller asked for a new reading).
#[derive(Debug, Clone)]
pub struct Started {
    pub job: Job,
    pub resume: &'static str,
}

/// What is told about a CD that is in the library: the book, the CD number, how many of its tracks are there, which are missing and
/// which are damaged (with the lengths), and whether it is whole.
pub fn present_json(p: &crate::cdpresent::Present) -> serde_json::Value {
    serde_json::json!({
        "book": p.book, "author": p.author, "disc": p.disc, "via": p.via,
        "tracks_in_library": p.tracks_in_library, "tracks_on_disc": p.tracks_on_disc,
        "missing": p.missing, "damaged": p.damaged, "complete": !p.needs_healing(),
    })
}

/// The one release MusicBrainz knows for a disc, as the details of the book (author, title, CD number). Not a guess: with none or
/// several the answer is why not.
pub async fn unique_release(store: &Store, cd: &CdService, set: &CdSettings, toc: &crate::musicbrainz::Toc) -> Result<crate::musicbrainz::Candidate, (&'static str, String)> {
    if !set.lookup_ready {
        return Err(("no_contact", String::new()));
    }
    match cdimport::lookup(store, cd, set, toc).await {
        Err(r) => Err(("lookup_failed", r.1.to_owned())),
        Ok((_, mut found)) => {
            // A release whose disc has exactly this Disc ID is the answer; the near ones (another pressing) count only when there is none.
            if found.iter().any(|c| c.disc_id_matched) {
                found.retain(|c| c.disc_id_matched);
            }
            match found.len() {
                0 => Err(("no_match", String::new())),
                1 => found.pop().ok_or(("no_match", String::new())),
                n => Err(("ambiguous_match", n.to_string())),
            }
        }
    }
}

/// Is the audio CD in this drive in the library? The Disc ID that an import stored, or else the book that the one release of
/// MusicBrainz names. `None` for a drive without an audio CD or a CD that is not there.
pub async fn present_for(store: &Store, cd: &CdService, library: Option<&Path>, set: &CdSettings, d: &DriveState) -> Option<crate::cdpresent::Present> {
    let disc_id = d.disc_id.as_deref().filter(|_| d.medium == Medium::Audio && d.tray == Tray::Closed)?;
    let lengths: HashMap<u32, u32> = d.track_list.iter().map(|t| (t.number, t.seconds)).collect();
    let hint = match d.table.as_ref() {
        Some(toc) => unique_release(store, cd, set, toc).await.ok().and_then(|c| c.disc.map(|disc| crate::cdpresent::Hint { author: c.author, title: c.title, disc })),
        None => None,
    };
    crate::cdpresent::find(store, library, set, disc_id, &lengths, hint.as_ref()).await
}

impl CdService {
    /// The drives as JSON, each with `resume` and a freshly worked out `present`.
    pub async fn drives_json_full(&self, store: &Store, library: Option<&Path>, set: &CdSettings) -> Vec<serde_json::Value> {
        self.refresh_present(store, library, set).await;
        let fp = cdresume::Fingerprint::of(set);
        self.drives_json(Some(&fp))
    }

    /// Works out which of the CDs in the drives are in the library and remembers it for the drive message. Pushes the state if it changed.
    pub async fn refresh_present(&self, store: &Store, library: Option<&Path>, set: &CdSettings) {
        let (drives, _) = self.drives.snapshot();
        let mut cache: HashMap<String, serde_json::Value> = HashMap::new();
        for d in &drives {
            if let (Some(id), Some(p)) = (d.disc_id.clone(), present_for(store, self, library, set, d).await) {
                cache.insert(id, present_json(&p));
            }
        }
        let changed = {
            let mut g = self.present_cache.lock().unwrap_or_else(|e| e.into_inner());
            let changed = *g != cache;
            *g = cache;
            changed
        };
        if changed {
            self.publish_drive();
        }
    }
}

/// The error codes after which the finished tracks of a rip are kept for a resume.
const RESUMABLE: &[&str] = &["read", "verify", "drive_gone", "cancelled", "interrupted", "unreadable_track"];
/// A rip that is stopped by the service stopping may finish the track it is on if that takes no longer than this.
pub const STOP_GRACE: Duration = Duration::from_secs(10);
/// And may take this long to write its manifest and end.
pub const STOP_PATIENCE: Duration = Duration::from_secs(10);

pub async fn start_rip(store: &Store, cd: &CdService, library: Option<PathBuf>, user_id: i64, req: RipRequest) -> Result<Started, RipRefusal> {
    match rip_inner(store, cd, library, user_id, req, false, None).await? {
        RipOutcome::Started(started) => Ok(*started),
        RipOutcome::Preview(_) => Err(Refusal(500, "internal").into()),
    }
}

/// The drive a rip uses: the one named (`wanted`, empty = any), the first with an audio CD in it and the tray shut, else the first.
pub fn pick_drive(states: &[DriveState], wanted: &str) -> Option<DriveState> {
    let candidates: Vec<&DriveState> = states.iter().filter(|s| wanted.is_empty() || s.device == wanted).collect();
    candidates.iter().find(|s| s.medium == Medium::Audio && s.tray == Tray::Closed).or(candidates.first()).map(|s| (*s).clone())
}

/// What `start_rip` would do for this request, worked out by the same code and without starting anything: no job, no
/// staging folder, no command to the drive. Refusals are the same as those of `start_rip`, except `disc_present` and
/// `disc_exists`, which are reported inside the answer (`would_start: false`, `refusal`) together with what they are about.
pub async fn preview_rip(store: &Store, cd: &CdService, library: Option<PathBuf>, req: RipRequest) -> Result<serde_json::Value, RipRefusal> {
    match rip_inner(store, cd, library, 0, req, true, None).await? {
        RipOutcome::Preview(plan) => Ok(plan),
        RipOutcome::Started(_) => Err(Refusal(500, "internal").into()),
    }
}

/// The result of `rip_inner`: a rip that runs, or (dry run) the plan of one.
enum RipOutcome {
    Started(Box<Started>),
    Preview(serde_json::Value),
}

/// The one code path behind the web form, the API and the MCP tool. With `dry` it stops before anything is created.
async fn rip_inner(store: &Store, cd: &CdService, library: Option<PathBuf>, user_id: i64, req: RipRequest, dry: bool, again: Option<(i64, crate::cdflow::Actor)>) -> Result<RipOutcome, RipRefusal> {
    let set = CdSettings::load(store).await.map_err(|_| Refusal(500, "internal"))?;
    let kind = req.kind.clone().unwrap_or_else(|| set.target_kind.clone());
    let wanted = req.device.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| set.device.clone());
    // Look at the drives now, not at the last look: the disc may have been changed a second ago.
    cd.refresh_drives(&wanted, true).await;
    let (states, _) = cd.drives.snapshot();
    let chosen = pick_drive(&states, &wanted).ok_or(Refusal(409, "no_drive"))?;
    if chosen.denied {
        return Err(Refusal(503, "drive_denied").into());
    }
    if chosen.tray == Tray::Open {
        return Err(Refusal(409, "tray_open").into());
    }
    if chosen.medium != Medium::Audio {
        return Err(Refusal(409, "no_disc").into());
    }
    let toc = chosen.table.clone().ok_or(Refusal(409, "no_disc"))?;
    // One rip per drive: said before anything is worked out (the claim at the end stays as the guard against a race).
    if cd.drives.lock().running.contains_key(&chosen.device) {
        return Err(Refusal(409, "busy").into());
    }
    // The choice of tracks: numbers on the disc, all of them if none is named.
    let on_disc: Vec<u32> = chosen.track_list.iter().map(|t| t.number).collect();
    let selected: Vec<u32> = match &req.only {
        None => on_disc.clone(),
        Some(list) => {
            if list.is_empty() {
                return Err(Refusal(422, "empty_selection").into());
            }
            let mut sorted = list.clone();
            sorted.sort_unstable();
            if sorted.windows(2).any(|w| w[0] == w[1]) || sorted.iter().any(|n| !on_disc.contains(n)) {
                return Err(Refusal(422, "invalid_selection").into());
            }
            sorted
        }
    };
    let lengths: HashMap<u32, u32> = chosen.track_list.iter().map(|t| (t.number, t.seconds)).collect();
    let disc_id = chosen.disc_id.clone().unwrap_or_default();
    let healing = req.mode.as_deref() == Some("heal");
    // Is the CD in the library already? Typed-in details (author, title, CD number) and the stored Disc ID tell.
    let hint = (!req.author.trim().is_empty() && !req.title.trim().is_empty()).then(|| crate::cdpresent::Hint { author: req.author.clone(), title: req.title.clone(), disc: req.disc.unwrap_or(1) });
    let mut present = crate::cdpresent::find(store, library.as_deref(), &set, &disc_id, &lengths, hint.as_ref()).await;
    if present.is_none() && healing {
        // No details were typed: the one release that MusicBrainz names tells which book the CD is.
        present = present_for(store, cd, library.as_deref(), &set, &chosen).await;
    }
    let mut heal_plan: Option<cdimport::HealPlan> = None;
    let (author, title, disc, discs_total, kind, selected) = if healing {
        let p = present.as_ref().ok_or(Refusal(409, "not_present"))?;
        let targets = p.to_heal();
        if targets.is_empty() {
            return Err(Refusal(409, "nothing_to_heal").into());
        }
        let quarantine_folder = store.quarantine_folder().await.map_err(|_| Refusal(500, "internal"))?;
        let names: HashMap<u32, String> = targets
            .iter()
            .filter_map(|n| crate::cdpresent::healed_name(&p.files, *n).map(|name| (*n, name)))
            .collect();
        if !p.dir.as_os_str().is_empty() {
            heal_plan = Some(cdimport::HealPlan { dest_dir: p.dir.clone(), names, quarantine_folder, existing: p.files.clone() });
        }
        (p.author.clone(), p.book.clone(), p.disc, req.discs_total.or(p.discs_total.filter(|n| *n >= p.disc)), p.kind.clone(), targets)
    } else {
        if present.is_some() && !req.force && !req.auto && !dry && again.is_none() {
            let p = present.as_ref().ok_or(Refusal(500, "internal"))?;
            // A CD that is whole in the library goes to `present`, and is ejected at once when the request asks for it. One that needs a repair
            // is for a person to decide (repair it or import it as a copy), so it stays in the drive.
            let mut ejected = false;
            if !p.needs_healing() {
                ejected = finish_present(store, cd, user_id, "server", &req.starter_device, p, &chosen.device, req.eject == Some(true), &wanted).await;
            }
            return Err(RipRefusal { refusal: Refusal(409, "disc_present"), taken: Vec::new(), present: Some(present_json(p)), ejected });
        }
        (req.author.clone(), req.title.clone(), req.disc.unwrap_or(1), req.discs_total, kind.clone(), selected)
    };
    let meta = Meta {
        author: author.clone(),
        album: title.clone(),
        narrator: req.narrator.clone(),
        year: req.year.clone(),
        disc,
        discs_total,
        kind: Some(kind.clone()),
        mbid: req.mbid.clone(),
        disc_id: chosen.disc_id.clone(),
        genre: None,
        job_id: None,
        // The files keep their numbers on the disc; a choice has gaps.
        barcode: None,
        allow_gaps: true,
        tracks: selected
            .iter()
            .map(|&n| MetaTrack {
                number: n,
                title: on_disc.iter().position(|d| *d == n).and_then(|i| req.tracks.get(i)).filter(|t| !t.trim().is_empty()).cloned(),
                file: format!("{n:02}.{}", set.extension),
            })
            .collect(),
    };
    cdimport::check_meta(&meta)?;
    let tracks_total = meta.tracks.len();
    let root = cdimport::import_root(store, library.as_deref(), &kind).await?;
    // Nothing is overwritten: the tracks that are in the library already are named before the disc is read.
    let taken = if healing { Vec::new() } else { cdimport::taken_numbers(&meta, &set, &root) };
    if dry {
        return Ok(RipOutcome::Preview(rip_plan_json(&RipPlanView { req: &req, set: &set, meta: &meta, root: &root, chosen: &chosen, kind: &kind, taken: &taken, present: present.as_ref(), healing, eject: req.eject.unwrap_or(set.eject_after), staging: &cd.staging })));
    }
    if !taken.is_empty() && again.is_some() {
        return Err(RipRefusal { refusal: Refusal(409, "disc_exists"), taken, present: None, ejected: false });
    }
    if !taken.is_empty() {
        let list = taken.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
        let job = store
            .create_cd_job(user_id, "server", &JobPatch { phase: Some("failed".into()), author: Some(meta.author.trim().to_owned()), album: Some(meta.album.trim().to_owned()), disc: Some(i64::from(disc)), discs_total: Some(i64::from(meta.discs_total.unwrap_or(1))), tracks_total: Some(tracks_total as i64), ..JobPatch::default() })
            .await
            .map_err(|_| Refusal(500, "internal"))?;
        if let Ok(Some(failed)) = store.update_cd_job(user_id, job.id, &JobPatch { error: Some("disc_exists".into()), message: Some(format!("tracks already in the library: {list}")), ..JobPatch::default() }).await {
            cd.publish(user_id, "failed", &failed);
        }
        return Err(RipRefusal { refusal: Refusal(409, "disc_exists"), taken, present: None, ejected: false });
    }
    let eject = req.eject.unwrap_or(set.eject_after);
    let reserve = crate::diskspace::reserve(store).await;

    // One rip per drive.
    let device = chosen.device.clone();
    let flag = Arc::new(AtomicBool::new(false));
    let job = match &again {
        // A new try of a job that waits in `retry_wait`: the same job, the same request.
        Some((id, _)) => store.cd_job_any(*id).await.ok().flatten().ok_or(Refusal(404, "not_found"))?,
        None => store
            .create_cd_job(
                user_id,
                if req.auto { "auto" } else { "server" },
                &JobPatch {
                    phase: Some("ripping".into()),
                    tracks_total: Some(tracks_total as i64),
                    author: Some(meta.author.trim().to_owned()),
                    album: Some(meta.album.trim().to_owned()),
                    disc: Some(i64::from(disc)),
                    discs_total: Some(i64::from(meta.discs_total.unwrap_or(1))),
                    ..JobPatch::default()
                },
            )
            .await
            .map_err(|_| Refusal(500, "internal"))?,
    };
    if again.is_none() {
        if !req.starter_device.is_empty() {
            let _ = store.set_cd_starter_device(job.id, &req.starter_device).await;
        }
        // What `retry` needs to start the same rip again.
        match serde_json::to_string(&req) {
            Ok(text) => {
                let _ = store.set_cd_request(job.id, &text).await;
            }
            Err(e) => tracing::warn!(error = %e, "the request of a rip could not be written down"),
        }
    }
    let taken = {
        let mut g = cd.drives.lock();
        if g.running.contains_key(&device) {
            false
        } else {
            g.running.insert(device.clone(), job.id);
            g.cancels.insert(job.id, flag.clone());
            true
        }
    };
    if !taken {
        if again.is_none() {
            let _ = store.cd_move(job.id, crate::cdflow::Move::Fail("busy".into()), &crate::cdflow::Actor::Server).await;
        }
        return Err(Refusal(409, "busy").into());
    }
    let job = match &again {
        Some((id, actor)) => match store.cd_move(*id, crate::cdflow::Move::Retry, actor).await {
            Ok(moved) => moved.job,
            Err(e) => {
                let mut g = cd.drives.lock();
                g.running.remove(&device);
                g.cancels.remove(&job.id);
                return Err(Refusal(e.status(), e.code()).into());
            }
        },
        None => job,
    };
    // An interrupted rip of this disc: take over what checks out, or remove it when it cannot be used. What is decided here is logged only in
    // a development build (`cfg(debug_assertions)`): a release build does not contain these lines at all (project owner, 2026-10-29).
    let fingerprint = cdresume::Fingerprint::of(&set);
    let dir = cdresume::dir_of(&cd.staging, &disc_id);
    let (mut kept, mut mode, mut started_ms): (Vec<Entry>, &'static str, i64) = (Vec::new(), "fresh", crate::store::now_ms());
    if dir.exists() {
        match cdresume::read(&dir) {
            // The tracks that are chosen again are taken over, whatever the choice was then: one track more or less does not throw away what was
            // read (project owner, 2026-10-29). Only another disc or other settings for the audio do. A repair (`heal`) reads other files for
            // another purpose and takes over only a rip with the same choice.
            Some(m) if req.resume.unwrap_or(true) && m.disc_id == disc_id && m.fingerprint == fingerprint && (!healing || m.only == selected) => {
                kept = cdresume::valid_entries(&dir, &m, &lengths).into_iter().filter(|e| selected.contains(&e.number)).collect();
                // What does not check out, or is not chosen any more, is read again or left out; its files go.
                for e in m.tracks.iter().filter(|e| !kept.contains(e)) {
                    #[cfg(debug_assertions)]
                    {
                        let why = if selected.contains(&e.number) { cdresume::check_entry(&dir, e, &lengths).err().unwrap_or("unknown") } else { "not_chosen" };
                        tracing::debug!(device = %device, track = e.number, state = %e.state, why, "resume: a track of the interrupted rip cannot be taken over and is read again");
                    }
                    let _ = std::fs::remove_file(cdresume::file_of(&dir, e));
                }
                #[cfg(debug_assertions)]
                tracing::debug!(device = %device, disc = %disc_id, in_manifest = m.tracks.len(), kept = kept.len(), "resume: an interrupted rip of this disc was found");
                started_ms = m.started_ms;
                mode = if kept.is_empty() { "fresh" } else { "resumed" };
            }
            Some(m) if req.resume.unwrap_or(true) => {
                #[cfg(debug_assertions)]
                tracing::debug!(device = %device, disc = %disc_id, in_manifest = m.tracks.len(), same_disc = m.disc_id == disc_id, same_settings = m.fingerprint == fingerprint, same_choice = m.only == selected, healing, manifest_only = ?m.only, selected = ?selected, "resume: an interrupted rip does not fit this start (other disc or settings, or a repair with another choice), it is removed");
                let _ = &m;
                let _ = std::fs::remove_dir_all(&dir);
                mode = "mismatch";
            }
            other => {
                #[cfg(debug_assertions)]
                tracing::debug!(device = %device, disc = %disc_id, readable = other.is_some(), asked_for_resume = req.resume.unwrap_or(true), "resume: the staging folder of this disc is removed, the rip starts from the beginning");
                let _ = &other;
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }
    let fail = |code: &'static str| {
        let mut g = cd.drives.lock();
        g.running.remove(&device);
        g.cancels.remove(&job.id);
        RipRefusal::from(Refusal(500, code))
    };
    std::fs::create_dir_all(&dir).map_err(|_| fail("internal"))?;
    let manifest = Manifest {
        v: cdresume::VERSION,
        disc_id: disc_id.clone(),
        started_ms,
        updated_ms: crate::store::now_ms(),
        fingerprint,
        only: selected.clone(),
        author: meta.author.trim().to_owned(),
        title: meta.album.trim().to_owned(),
        disc,
        discs_total: meta.discs_total,
        kind: kind.clone(),
        tracks: kept.clone(),
    };
    cdresume::write(&dir, &manifest).map_err(|_| fail("internal"))?;
    let resume: Vec<ResumeTrack> = kept.iter().map(|e| ResumeTrack { number: e.number as u8, sha1: e.sha1.clone(), verified: e.state == "done" }).collect();
    let done_before = kept.iter().filter(|e| e.state == "done").count() as i64;
    let job = if done_before > 0 { store.update_cd_job(user_id, job.id, &JobPatch { track: Some(done_before), ..JobPatch::default() }).await.ok().flatten().unwrap_or(job) } else { job };
    cd.publish(user_id, "job", &job);
    cd.refresh_drives(&wanted, false).await;
    cd.publish_drive();

    // The picture of the rip for the pages and the apps: every chosen track with its state.
    let stats = Arc::new(cdrip::ring::RipStats::default());
    let ring_bytes = ring_bytes(&set, selected.iter().filter_map(|n| lengths.get(n)).copied().max().unwrap_or(0));
    cd.drives.live_start(Live {
        job_id: job.id,
        device: device.clone(),
        fraction: 0.0,
        finished: false,
        error: String::new(),
        tracks: selected
            .iter()
            .map(|&n| {
                let kept_entry = kept.iter().find(|e| e.number == n);
                TrackState {
                    number: n,
                    title: on_disc.iter().position(|d| *d == n).and_then(|i| req.tracks.get(i)).cloned().unwrap_or_default(),
                    seconds: lengths.get(&n).copied().unwrap_or(0),
                    // A track that an earlier run read and wrote (not yet checked) is read: it waits for the check, it does not wait to be read.
                    state: match kept_entry.map(|e| e.state.as_str()) {
                        Some("done") => "done",
                        Some("encoded") if set.verify_read => "read",
                        Some("encoded") => "encoding",
                        _ => "waiting",
                    },
                    fraction: if kept_entry.is_some() { 1.0 } else { 0.0 },
                    error: String::new(),
                }
            })
            .collect(),
        verify: set.verify_read,
        stats: stats.clone(),
        ring_bytes,
    });
    let (store2, cd2) = (store.clone(), cd.clone());
    let library_for_task = library.clone();
    let job_id = job.id;
    let plan = RipPlan { dir: dir.clone(), manifest: Arc::new(Mutex::new(manifest)), resume, lengths, done_before, heal: heal_plan, stats: stats.clone(), ring_bytes };
    tokio::spawn(async move {
        let outcome = run_rip(&store2, &cd2, user_id, job_id, &device, &toc, meta, set, root, kind, reserve, flag, &plan, &disc_id).await;
        let shutting_down = cd2.drives.lock().shutting_down;
        // A stop of the service is not a cancel by a person.
        let outcome = match outcome {
            Err("cancelled") if shutting_down => Err("interrupted"),
            other => other,
        };
        // The list stays with the result until a new rip starts or it is closed.
        cd2.drives.live_update(job_id, |l| {
            l.finished = true;
            match &outcome {
                Ok(()) => {
                    l.fraction = 1.0;
                    for t in l.tracks.iter_mut().filter(|t| t.state != "failed" && t.state != "unverified") {
                        t.state = "done";
                        t.fraction = 1.0;
                    }
                }
                Err(code) => l.error = (*code).to_owned(),
            }
        });
        if let Err(code) = &outcome {
            // A track that cannot be read or confirmed: the CD stays in the drive and the job waits for a new try (`retry_wait`); the finished
            // tracks stay in the resume folder. A cancel and a stop of the server have their own moves; everything else is a failure.
            let mv = match *code {
                "cancelled" => crate::cdflow::Move::Cancel,
                "interrupted" => crate::cdflow::Move::Interrupted,
                "read" | "verify" | "unreadable_track" => crate::cdflow::Move::ReadFailed((*code).to_owned()),
                other => crate::cdflow::Move::Fail(other.to_owned()),
            };
            let waits = matches!(mv, crate::cdflow::Move::ReadFailed(_));
            match store2.cd_move(job_id, mv, &crate::cdflow::Actor::Server).await {
                Ok(moved) => {
                    cd2.publish(user_id, "failed", &moved.job);
                    if waits {
                        let tracks = cd2.drives.live_of_job(job_id).map(|l| l.tracks.iter().filter(|t| t.state == "failed").map(|t| t.number).collect()).unwrap_or_default();
                        crate::cdflow::notify(&store2, &cd2, &crate::cdflow::CdEvent::ReadError { job: moved.job, tracks }).await;
                    }
                }
                Err(e) => tracing::warn!(job = job_id, error = e.code(), "the end of a rip could not be written to the job"),
            }
        }
        // The finished tracks of a rip that stopped for a reason that a new try can mend stay for a resume; the rest goes.
        match &outcome {
            Err(code) if RESUMABLE.contains(code) => plan.touch(),
            _ => {
                let _ = std::fs::remove_dir_all(&plan.dir);
            }
        }
        {
            let mut g = cd2.drives.lock();
            g.running.remove(&device);
            g.cancels.remove(&job_id);
        }
        // Open the tray after a good rip, if asked for and the tray is shut with the disc in.
        if outcome.is_ok() && eject {
            let drives = cd2.drives.clone();
            let d = device.clone();
            let _ = tokio::task::spawn_blocking(move || drives.eject(&d)).await;
        }
        cd2.refresh_drives(&wanted, true).await;
        // The CD may be in the library now (or whole): the drive message tells.
        if let Ok(set) = CdSettings::load(&store2).await {
            cd2.refresh_present(&store2, library_for_task.as_deref(), &set).await;
        }
        cd2.publish_drive();
    });
    Ok(RipOutcome::Started(Box::new(Started { job, resume: mode })))
}

/// What `rip_plan_json` describes.
struct RipPlanView<'a> {
    req: &'a RipRequest,
    set: &'a CdSettings,
    meta: &'a Meta,
    root: &'a Path,
    chosen: &'a DriveState,
    kind: &'a str,
    taken: &'a [u32],
    present: Option<&'a crate::cdpresent::Present>,
    healing: bool,
    eject: bool,
    staging: &'a Path,
}

/// The plan of a rip for a dry run: the folder, the file names, the tags and the warnings. Built from the same naming
/// functions that `finalize` uses when it places the files, so it says what the rip will really do.
fn rip_plan_json(v: &RipPlanView) -> serde_json::Value {
    let (m, set) = (v.meta, v.set);
    let (book, dest) = cdimport::layout(m, set, v.root);
    let lengths: HashMap<u32, u32> = v.chosen.track_list.iter().map(|t| (t.number, t.seconds)).collect();
    let genre = if v.kind == "audiobook" { Some(set.genre.clone()) } else { m.genre.clone().filter(|g| !g.trim().is_empty()) };
    let files: Vec<serde_json::Value> = m
        .tracks
        .iter()
        .map(|t| {
            let title = t.title.clone().unwrap_or_default();
            let file = cdimport::file_name_of(m, set, &StagedTrack { number: t.number, title: title.clone(), path: PathBuf::new() });
            let stem = file.strip_suffix(&format!(".{}", set.extension)).unwrap_or(&file).to_owned();
            let title_tag = cdimport::tag_title(set, &title, t.number, &stem);
            serde_json::json!({
                "number": t.number, "title": title, "file": file, "seconds": lengths.get(&t.number).copied().unwrap_or(0),
                "tag_title": title_tag, "tag_track": set.tag_track(m.disc, t.number), "exists": v.taken.contains(&t.number),
            })
        })
        .collect();
    let mut warnings: Vec<String> = Vec::new();
    let mut refusal: Option<&str> = None;
    if let Some(p) = v.present
        && !v.healing
    {
        warnings.push(format!("an album with this author and title (or this disc) is in the library already: \"{}\" by {}, CD {}, {} of {} tracks{}", p.book, p.author, p.disc, p.tracks_in_library, p.tracks_on_disc, if !p.needs_healing() { ", complete" } else { "" }));
        if !v.req.force {
            refusal = Some("disc_present");
        }
    }
    if !v.taken.is_empty() {
        warnings.push(format!("files of these tracks exist already and are never overwritten: {}", v.taken.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")));
        refusal = Some("disc_exists");
    }
    if book.exists() && v.taken.is_empty() && v.present.is_none() {
        warnings.push(format!("the folder {} exists already; the new files are added to it", book.display()));
    }
    if v.chosen.disc_id.as_deref().is_some_and(|id| cdresume::dir_of(v.staging, id).exists()) {
        warnings.push("an interrupted rip of this disc is waiting; its finished tracks are taken over".to_owned());
    }
    if m.mbid.is_some() && !set.lookup_ready {
        warnings.push("the cover cannot be fetched: no MusicBrainz contact address is set".to_owned());
    }
    if m.mbid.is_none() {
        warnings.push("no mbid: the files get no cover".to_owned());
    }
    serde_json::json!({
        "would_start": refusal.is_none(),
        "refusal": refusal,
        "device": v.chosen.device,
        "disc_id": v.chosen.disc_id,
        "kind": v.kind,
        "folder": dest.display().to_string(),
        "book_folder": book.display().to_string(),
        "files": files,
        "tags": {
            "artist": m.author.trim(), "album_artist": m.author.trim(), "album": m.album.trim(), "narrator": m.narrator, "year": m.year,
            "disc": m.disc, "discs_total": m.discs_total.unwrap_or(1), "genre": genre, "tag_version": set.id3_version,
        },
        "format": { "format": set.format, "bitrate_kbps": set.bitrate_kbps, "sample_rate_hz": set.sample_rate_hz, "channel_mode": set.channel_mode },
        "eject": v.eject,
        "present": v.present.map(present_json),
        "warnings": warnings,
    })
}

/// What a rip needs to keep its folder and manifest.
struct RipPlan {
    dir: PathBuf,
    manifest: Arc<Mutex<Manifest>>,
    resume: Vec<ResumeTrack>,
    lengths: HashMap<u32, u32>,
    /// Tracks that were done before this run.
    done_before: i64,
    /// Healing: where the files go (and that a track that cannot be read is left out, not a reason to stop).
    heal: Option<cdimport::HealPlan>,
    /// What the ripper measures (shared with the live picture).
    stats: Arc<cdrip::ring::RipStats>,
    /// The size of the ring in bytes.
    ring_bytes: u64,
}

impl RipPlan {
    /// Records a track in the manifest (replacing an older entry of it) and writes the manifest in one piece.
    fn record(&self, number: u32, file: &str, size: u64, sha1: &str, state: &str) {
        let mut m = self.manifest.lock().unwrap_or_else(|e| e.into_inner());
        let seconds = self.lengths.get(&number).copied().unwrap_or(0);
        m.tracks.retain(|e| e.number != number);
        m.tracks.push(Entry { number, file: file.to_owned(), size, sha1: sha1.to_owned(), seconds, state: state.to_owned() });
        m.tracks.sort_by_key(|e| e.number);
        m.updated_ms = crate::store::now_ms();
        if let Err(e) = cdresume::write(&self.dir, &m) {
            tracing::warn!(error = %e, "the manifest of a rip could not be written");
        }
    }

    /// Writes the manifest with the time of the stop.
    fn touch(&self) {
        let mut m = self.manifest.lock().unwrap_or_else(|e| e.into_inner());
        m.updated_ms = crate::store::now_ms();
        let _ = cdresume::write(&self.dir, &m);
    }
}

/// A CD that is whole in the library: the job (a log entry of the flow, state `present`) is written, nothing is read, and the disc is ejected
/// at once when `eject` says so (the automatic path always does; a request asks with `"eject": true`). Returns whether the tray was opened.
#[allow(clippy::too_many_arguments)]
pub async fn finish_present(store: &Store, cd: &CdService, user_id: i64, device_label: &str, starter_device: &str, p: &crate::cdpresent::Present, device: &str, eject: bool, wanted: &str) -> bool {
    let patch = JobPatch { phase: Some("ripping".into()), author: Some(p.author.clone()), album: Some(p.book.clone()), disc: Some(i64::from(p.disc)), tracks_total: Some(p.tracks_on_disc as i64), ..JobPatch::default() };
    let Ok(job) = store.create_cd_job(user_id, device_label, &patch).await else { return false };
    if !starter_device.is_empty() {
        let _ = store.set_cd_starter_device(job.id, starter_device).await;
    }
    if store.cd_move(job.id, crate::cdflow::Move::Present, &crate::cdflow::Actor::Server).await.is_err() {
        return false;
    }
    let mut ejected = false;
    if eject {
        let drives = cd.drives.clone();
        let d = device.to_owned();
        ejected = matches!(tokio::task::spawn_blocking(move || drives.eject(&d)).await, Ok(Ok(EjectResult::Ejected)));
        cd.refresh_drives(wanted, true).await;
    }
    let message = format!("already in the library: {}: {}, CD {}{}", p.author, p.book, p.disc, if ejected { "; the disc was ejected" } else { "" });
    if let Ok(Some(done)) = store.update_cd_job(user_id, job.id, &JobPatch { message: Some(message), ..JobPatch::default() }).await {
        cd.publish(user_id, "done", &done);
        crate::cdflow::notify(store, cd, &crate::cdflow::CdEvent::Present { job: done }).await;
    }
    cd.publish_drive();
    ejected
}

/// A new try of a job that waits in `retry_wait`: the same request is started again on the same job; what was read is taken over from the
/// resume folder. The caller has checked with `Store::cd_check` that `actor` may retry; a refusal of the start itself (`busy`, `no_disc`,
/// `tray_open` ...) leaves the job in `retry_wait`.
pub async fn retry_rip(store: &Store, cd: &CdService, library: Option<PathBuf>, job_id: i64, actor: crate::cdflow::Actor) -> Result<Started, RipRefusal> {
    let job = store.cd_job_any(job_id).await.ok().flatten().ok_or(Refusal(404, "not_found"))?;
    let text = store.cd_request(job_id).await.map_err(|_| Refusal(500, "internal"))?;
    // A job without the request of a rip of the server (an upload of an app) cannot be read again here.
    let mut req: RipRequest = serde_json::from_str(&text).map_err(|_| Refusal(409, "invalid_move"))?;
    req.resume = Some(true);
    req.auto = job.device == "auto";
    match rip_inner(store, cd, library, job.user_id, req, false, Some((job_id, actor))).await? {
        RipOutcome::Started(started) => Ok(*started),
        RipOutcome::Preview(_) => Err(Refusal(500, "internal").into()),
    }
}

/// Opens the tray of the drive named, else of the one named in the settings, else of the first one that has a disc.
/// Never closes a tray (see `Drives::eject`); refused while a rip runs on the drive.
pub async fn eject(store: &Store, cd: &CdService, device: Option<String>) -> Result<EjectResult, Refusal> {
    let set = CdSettings::load(store).await.map_err(|_| Refusal(500, "internal"))?;
    let wanted = device.filter(|d| !d.is_empty()).unwrap_or(set.device);
    let (states, _) = cd.drives.snapshot();
    let pick = states
        .iter()
        .filter(|s| wanted.is_empty() || s.device == wanted)
        .find(|s| s.tray == Tray::Closed && matches!(s.medium, Medium::Audio | Medium::Other))
        .or_else(|| states.iter().find(|s| wanted.is_empty() || s.device == wanted))
        .map(|s| s.device.clone())
        .ok_or(Refusal(404, "no_drive"))?;
    let drives = cd.drives.clone();
    let d = pick.clone();
    let result = tokio::task::spawn_blocking(move || drives.eject(&d)).await.map_err(|_| Refusal(500, "internal"))??;
    cd.refresh_drives(&wanted, true).await;
    Ok(result)
}

/// The rip itself. The error is the code the job fails with. The staging folder is removed in every case.
#[allow(clippy::too_many_arguments)]
async fn run_rip(
    store: &Store,
    cd: &CdService,
    user_id: i64,
    job_id: i64,
    device: &str,
    toc: &Toc,
    meta: Meta,
    set: CdSettings,
    root: PathBuf,
    kind: String,
    reserve: u64,
    cancel: Arc<AtomicBool>,
    plan: &RipPlan,
    disc_id: &str,
) -> Result<(), &'static str> {
    // The folder of the disc stays after a stop that a new try can mend (see `RESUMABLE`); the caller decides.
    let staging_dir = plan.dir.clone();
    if !crate::diskspace::fits(&staging_dir, estimated_bytes(toc, &set), reserve) {
        return Err("no_space");
    }
    let spec = JobSpec {
        device: device.to_owned(),
        out_dir: staging_dir.clone(),
        tracks: meta.tracks.iter().map(|t| TrackSpec { number: t.number as u8, file: t.file.clone(), title: t.title.clone().unwrap_or_default(), tag_track: set.tag_track(meta.disc, t.number) }).collect(),
        album: meta.album.trim().to_owned(),
        artist: meta.author.trim().to_owned(),
        genre: if kind == "audiobook" { set.genre.clone() } else { String::new() },
        disc: meta.disc,
        discs_total: meta.discs_total,
        cover: None,
        verify: set.verify_read,
        max_reads: MAX_READS,
        // The files are tagged again by `finalize` (version and all); the ripper always writes ID3v2.4.
        encoder: EncoderSpec { format: set.format.clone(), bitrate_kbps: set.bitrate_kbps as u16, sample_rate_hz: set.sample_rate_hz, channel_mode: set.channel_mode.clone(), quality: set.quality, tag_version: "2.4".into() },
        resume: plan.resume.clone(),
        buffer_bytes: plan.ring_bytes,
        encoder_nice: Some(set.encoder_nice),
        verify_shift_sectors: set.verify_shift,
        accept_unverified: set.accept_unverified,
        verify_tolerant: true,
        stats: Some(plan.stats.clone()),
    };

    // The events of the worker thread go to the async side through a channel.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RipEvent>();
    let backend = cd.drives.backend.clone();
    let flag = cancel.clone();
    let speed = set.read_speed;
    let drive_path = device.to_owned();
    // A track that cannot be read or confirmed does not end the rip, whether it is an import or a repair: it is left out, reported, and the
    // rest is imported (project owner, 2026-10-29). "Check and repair" reads the missing ones again.
    let skip_unreadable = true;
    let worker = tokio::task::spawn_blocking(move || {
        // A steady, moderate speed for the whole rip (a drive at full speed that waits for a slower consumer brakes and
        // speeds up again); the drive gets its own speed back afterwards, whatever the end was.
        let changed = speed > 0
            && backend
                .set_speed(&drive_path, speed)
                .inspect_err(|e| tracing::warn!(error = %e, device = %drive_path, "the read speed could not be set; the drive keeps its own"))
                .is_ok();
        let mut spec = spec;
        let mut skipped: Vec<u32> = Vec::new();
        // Tracks with a scratch: the drive guessed some sectors, differently at each reading (track, sectors).
        let mut concealed: Vec<(u32, u32)> = Vec::new();
        // What this run has finished, to take over if the rip is started again without a track that cannot be read.
        let mut finished: Vec<ResumeTrack> = spec.resume.clone();
        let result = loop {
            let r = backend.rip(&spec, &flag, &mut |e| {
                match e {
                    RipEvent::Encoded { track, sha1, .. } if !finished.iter().any(|f| f.number == *track) => finished.push(ResumeTrack { number: *track, sha1: sha1.clone(), verified: false }),
                    RipEvent::Concealed { track, sectors } => concealed.push((u32::from(*track), *sectors)),
                    RipEvent::TrackDone { track, sha1, .. } => {
                        finished.retain(|f| f.number != *track);
                        finished.push(ResumeTrack { number: *track, sha1: sha1.clone(), verified: true });
                    }
                    _ => {}
                }
                drop(tx.send(e.clone()));
            });
            match r {
                // Healing: a track that cannot be read is told at the end, the others are healed all the same.
                Err(e) if skip_unreadable && matches!(e.kind, "read" | "verify") && e.track.is_some() => {
                    let number = e.track.unwrap_or_default();
                    drop(tx.send(RipEvent::Error { kind: e.kind, message: e.message.clone(), track: e.track }));
                    skipped.push(u32::from(number));
                    spec.tracks.retain(|t| t.number != number);
                    finished.retain(|f| f.number != number);
                    if spec.tracks.is_empty() {
                        break Ok(());
                    }
                    spec.resume = finished.clone();
                }
                other => break other,
            }
        };
        if changed && let Err(e) = backend.reset_speed(&drive_path) {
            tracing::warn!(error = %e, device = %drive_path, "the read speed could not be reset");
        }
        (result, skipped, concealed)
    });
    let mut done = plan.done_before;
    // The row of the job says "alive" by its time of change; the first pass can take longer than `cdimport::STALE`
    // without a track being verified, so every event renews it, at most once a minute.
    let mut last_beat = std::time::Instant::now();
    while let Some(event) = rx.recv().await {
        cd.drives.live_update(job_id, |l| l.apply(&event));
        if last_beat.elapsed() >= std::time::Duration::from_secs(60) {
            last_beat = std::time::Instant::now();
            let _ = store.update_cd_job(user_id, job_id, &JobPatch::default()).await;
        }
        match event {
            RipEvent::Encoded { track, sha1, size } => {
                let file = format!("{track:02}.{}", set.extension);
                // A track that is done already is not made 'encoded' again.
                let has_done = plan.manifest.lock().unwrap_or_else(|e| e.into_inner()).tracks.iter().any(|e| e.number == u32::from(track) && e.state == "done");
                if !has_done {
                    plan.record(u32::from(track), &file, size, &sha1, "encoded");
                }
                cd.drives.lock().ticks += 1;
                if let Ok(Some(j)) = store.cd_job(user_id, job_id).await {
                    cd.publish(user_id, "progress", &j);
                }
            }
            RipEvent::TrackDone { track, file, sha1, size } => {
                plan.record(u32::from(track), &file, size, &sha1, "done");
                cd.drives.lock().ticks += 1;
                done += 1;
                if let Ok(Some(j)) = store.update_cd_job(user_id, job_id, &JobPatch { track: Some(done), ..JobPatch::default() }).await {
                    cd.publish(user_id, "progress", &j);
                }
            }
            RipEvent::TrackStart { .. } | RipEvent::TrackProgress { .. } | RipEvent::Progress { .. } => {
                // The fraction and the states of the tracks live in memory (`Live`); the database knows whole tracks.
                if let Ok(Some(j)) = store.cd_job(user_id, job_id).await {
                    cd.publish(user_id, "progress", &j);
                }
            }
            _ => {}
        }
    }
    let concealed_tracks: Vec<(u32, u32)>;
    let skipped = match worker.await {
        Ok((Ok(()), skipped, concealed)) => {
            concealed_tracks = concealed;
            skipped
        }
        Ok((Err(e), ..)) => {
            let code = rip_failure_code(e.kind);
            // The track the drive failed on is told in the list.
            cd.drives.live_update(job_id, |l| {
                if let Some(t) = e.track.and_then(|n| l.track(u32::from(n))) {
                    t.state = "failed";
                    t.error = code.to_owned();
                }
            });
            return Err(code);
        }
        Err(_) => return Err("internal"),
    };
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled");
    }
    // The tracks that could not be read are left out of the placing.
    cd.drives.live_update(job_id, |l| {
        for n in &skipped {
            if let Some(t) = l.track(*n) {
                t.state = "failed";
                t.error = "unreadable_track".to_owned();
            }
        }
    });
    let mut meta = meta;
    meta.tracks.retain(|t| !skipped.contains(&t.number));
    if meta.tracks.is_empty() {
        return Err("unreadable_track");
    }
    let cover = match (&meta.mbid, set.lookup_ready) {
        (Some(mbid), true) => cdimport::fetch_cover(store, cd, &set, mbid).await.ok().flatten(),
        _ => None,
    };
    // The EAN of the chosen release goes into the tags (best effort: a rip does not fail because MusicBrainz is not reachable).
    if meta.barcode.is_none()
        && set.lookup_ready
        && let Some(mbid) = &meta.mbid
        && let Ok(Some(release)) = cdimport::release_for(store, cd, &set, mbid, meta.tracks.len()).await
    {
        meta.barcode = release.barcode;
    }
    let staged: Vec<StagedTrack> = meta.tracks.iter().map(|t| StagedTrack { number: t.number, title: t.title.clone().unwrap_or_default(), path: staging_dir.join(&t.file) }).collect();
    // What the disc is made of is written down once the files are placed (SHA-1 of the PCM as the manifest has it).
    let info: HashMap<u32, (String, u32)> = plan.manifest.lock().unwrap_or_else(|e| e.into_inner()).tracks.iter().map(|e| (e.number, (e.sha1.clone(), e.seconds))).collect();
    let record = (!disc_id.is_empty()).then(|| crate::cdpresent::DiscRow {
        disc_id: disc_id.to_owned(),
        author: meta.author.trim().to_owned(),
        title: meta.album.trim().to_owned(),
        disc: meta.disc,
        discs_total: meta.discs_total.unwrap_or(1),
        kind: kind.clone(),
        folder: String::new(),
    });
    // What the rip measured of itself, to see afterwards what held the drive up: the time the drive read, the waits of the reader
    // for room in the ring and how full the ring got.
    let st = &plan.stats;
    let (read_s, waited_s, waits, longest_s, peak_s, ring_s) = (
        st.read_ms.load(Ordering::Relaxed) as f64 / 1000.0,
        st.wait_ms_total.load(Ordering::Relaxed) as f64 / 1000.0,
        st.reader_waits.load(Ordering::Relaxed),
        st.wait_ms_longest.load(Ordering::Relaxed) as f64 / 1000.0,
        cdrip::rip::bytes_to_seconds(st.peak_bytes.load(Ordering::Relaxed)),
        plan.ring_bytes as f64 / cdrip::ring::BYTES_PER_SECOND as f64,
    );
    let priority = match (st.encoder_nice_state.load(Ordering::Relaxed), st.encoder_nice.load(Ordering::Relaxed)) {
        (1, n) => format!("encoder nice {n}"),
        (2, _) => "encoder priority refused by the system (LimitNICE= in the service unit?)".to_owned(),
        _ => "encoder priority left alone".to_owned(),
    };
    let measured = format!("drive read {read_s:.0} s, reader waited {waited_s:.1} s in {waits} pauses (longest {longest_s:.1} s), buffer peak {peak_s:.0} s of {ring_s:.0} s, {priority}");
    tracing::info!(job = job_id, device, read_s, waited_s, waits, longest_s, peak_s, ring_s, "rip measured: {measured}");
    let unreadable = if skipped.is_empty() { String::new() } else { format!("unreadable tracks: {}; ", skipped.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")) };
    // The tracks that were filed without a confirmation, for the log: the person who listens can look at them.
    let unverified: Vec<u32> = cd.drives.live_of_job(job_id).map(|l| l.tracks.iter().filter(|t| t.state == "unverified").map(|t| t.number).collect()).unwrap_or_default();
    let unconfirmed = if unverified.is_empty() { String::new() } else { format!("not confirmed (taken with the first reading): {}; ", unverified.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")) };
    let scratched = if concealed_tracks.is_empty() {
        String::new()
    } else {
        format!("scratched (the drive guessed sectors): {}; ", concealed_tracks.iter().map(|(t, n)| format!("{t} ({n} sectors)")).collect::<Vec<_>>().join(", "))
    };
    let extra = cdimport::FinalizeExtra {
        heal: plan.heal.clone(),
        record,
        info,
        note: format!("{unreadable}{unconfirmed}{scratched}{measured}"),
        // What is more serious is the one that shows: a track left out is worse than one that was taken without a confirmation.
        error: if !skipped.is_empty() { Some("unreadable_track".to_owned()) } else if !unverified.is_empty() { Some("unverified_track".to_owned()) } else { None },
    };
    crate::cdflow::conclude(store, cd, user_id, job_id, &meta, &set, root, kind, cover, staged, reserve, extra).await.map(|_| ()).map_err(|r| r.1)
}

/// The size of the ring between the drive and the encoder: the seconds of the setting, or (0) the longest chosen track by the table of
/// contents and a reserve, so that a whole track fits; never more than `cd_buffer_max_mb`, never less than two seconds.
pub fn ring_bytes(set: &CdSettings, longest_track_s: u32) -> u64 {
    let seconds = if set.buffer_seconds == 0 { longest_track_s + crate::cd_settings::BUFFER_AUTO_RESERVE_S } else { set.buffer_seconds };
    let wanted = u64::from(seconds) * cdrip::ring::BYTES_PER_SECOND as u64;
    let cap = u64::from(set.buffer_max_mb) * 1024 * 1024;
    wanted.min(cap).max(2 * cdrip::ring::BYTES_PER_SECOND as u64)
}

/// A generous guess of the room the staging area needs: the PCM of one track plus the encoded files.
fn estimated_bytes(toc: &Toc, set: &CdSettings) -> u64 {
    let sectors = u64::from(toc.leadout_lba.saturating_sub(toc.starts_lba.first().copied().unwrap_or(0)));
    let pcm_longest = sectors * 2352;
    let mp3 = sectors / 75 * u64::from(set.bitrate_kbps) * 125;
    pcm_longest.min(800 * 1024 * 1024) + mp3
}

/// The folder for tests that need a staging path under a temporary data folder.
pub fn staging_of(data_dir: &Path) -> PathBuf {
    data_dir.join("cd-import")
}

// ---------------------------------------------------------------------------------------------------------------
// A drive for tests.

/// A drive that is not there: the tray, the disc, the errors and the events are set by the test.
pub mod fake {
    use super::*;
    use std::sync::atomic::AtomicU32;

    pub struct Disc {
        pub toc: TocInfo,
    }

    #[derive(Default)]
    pub struct Unit {
        pub tray_open: bool,
        pub disc: Option<Disc>,
        pub denied: bool,
        pub gone: bool,
        pub not_ready: bool,
        /// A disc was put in or taken out since the last look (what the kernel's media-change flag says).
        pub changed: bool,
    }

    #[derive(Default)]
    pub struct FakeDrives {
        pub units: Mutex<Vec<(String, Unit)>>,
        /// The bytes of a valid MP3 that every ripped track gets.
        pub mp3: Mutex<Vec<u8>>,
        /// Fail the rip at this track with this kind of error.
        pub fail_at: Mutex<Option<(u8, &'static str)>>,
        /// After this track the rip waits for the cancel flag (a rip that takes a long time).
        pub hold_after: Mutex<Option<u8>>,
        /// Halfway through this track the rip waits (until the value changes or it is cancelled): a track that is being read.
        pub hold_in: Mutex<Option<u8>>,
        /// What the fake rip reports as waiting in the buffer (chunks) while it holds, and the buffer it was asked for.
        pub filled_bytes: Mutex<i64>,
        /// The size of the ring the rip was asked for, in bytes.
        pub asked_buffer: Mutex<u64>,
        pub ejects: Mutex<Vec<String>>,
        /// The speed requests in order: `set <times>` and `reset`.
        pub speeds: Mutex<Vec<String>>,
        /// The numbers of the tracks that were read, in order (a track taken over from an earlier run is not in it).
        pub reads: Mutex<Vec<u8>>,
        pub probes: AtomicU32,
        pub toc_reads: AtomicU32,
        pub events: Arc<AtomicU32>,
        pub watch_available: AtomicBool,
    }

    /// A table of contents with `n` audio tracks of about two minutes.
    pub fn disc(n: u8) -> Disc {
        disc_sectors(n, 9000)
    }

    /// A table of contents with `n` audio tracks of `sectors` sectors each (75 sectors are a second).
    pub fn disc_sectors(n: u8, sectors: u32) -> Disc {
        let tracks = (0..n).map(|i| cdrip::discid::TocTrack { number: i + 1, start_lba: u32::from(i) * sectors, is_audio: true }).collect();
        Disc { toc: TocInfo { tracks, leadout_lba: u32::from(n) * sectors } }
    }

    impl FakeDrives {
        pub fn with_drive(device: &str) -> Arc<FakeDrives> {
            let f = FakeDrives::default();
            f.watch_available.store(true, Ordering::Relaxed);
            f.units.lock().unwrap().push((device.to_owned(), Unit::default()));
            Arc::new(f)
        }

        pub fn edit(&self, device: &str, f: impl FnOnce(&mut Unit)) {
            let mut units = self.units.lock().unwrap();
            if let Some((_, u)) = units.iter_mut().find(|(d, _)| d == device) {
                f(u);
            }
            // The kernel announces tray and medium changes.
            self.events.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct EventCounter(Arc<AtomicU32>);
    impl Watcher for EventCounter {
        fn take(&self) -> u32 {
            self.0.swap(0, Ordering::Relaxed)
        }
    }

    impl DriveBackend for FakeDrives {
        fn devices(&self) -> Vec<String> {
            self.units.lock().unwrap().iter().filter(|(_, u)| !u.gone).map(|(d, _)| d.clone()).collect()
        }

        fn probe(&self, device: &str, known_disc: bool) -> Probe {
            self.probes.fetch_add(1, Ordering::Relaxed);
            let mut units = self.units.lock().unwrap();
            let Some((_, u)) = units.iter_mut().find(|(d, _)| d == device) else { return Probe { access: Access::Gone, status: DriveStatus::Unknown, toc: None, unchanged: false } };
            if u.gone {
                return Probe { access: Access::Gone, status: DriveStatus::Unknown, toc: None, unchanged: false };
            }
            if u.denied {
                return Probe { access: Access::Denied, status: DriveStatus::Unknown, toc: None, unchanged: false };
            }
            if u.not_ready {
                return Probe { access: Access::Ok, status: DriveStatus::NotReady, toc: None, unchanged: false };
            }
            if u.tray_open {
                return Probe { access: Access::Ok, status: DriveStatus::TrayOpen, toc: None, unchanged: false };
            }
            match &u.disc {
                None => Probe { access: Access::Ok, status: DriveStatus::NoDisc, toc: None, unchanged: false },
                Some(d) => {
                    if known_disc && !u.changed {
                        return Probe { access: Access::Ok, status: DriveStatus::DiscOk, toc: None, unchanged: true };
                    }
                    u.changed = false;
                    self.toc_reads.fetch_add(1, Ordering::Relaxed);
                    Probe { access: Access::Ok, status: DriveStatus::DiscOk, toc: Some(d.toc.clone()), unchanged: false }
                }
            }
        }

        fn rip(&self, spec: &JobSpec, cancel: &AtomicBool, emit: &mut dyn FnMut(&RipEvent)) -> Result<(), RipError> {
            let mp3 = self.mp3.lock().unwrap().clone();
            *self.asked_buffer.lock().unwrap() = spec.buffer_bytes;
            for t in &spec.tracks {
                if spec.resume.iter().any(|r| r.number == t.number && r.verified) {
                    continue;
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err(RipError { kind: "cancelled", message: "cancelled".into(), track: Some(t.number) });
                }
                emit(&RipEvent::TrackStart { track: t.number });
                if let Some((n, kind)) = *self.fail_at.lock().unwrap()
                    && n == t.number
                {
                    return Err(RipError { kind, message: "simulated".into(), track: Some(t.number) });
                }
                self.reads.lock().unwrap().push(t.number);
                emit(&RipEvent::TrackProgress { track: t.number, pass: 1, fraction: 0.5 });
                if let Some(st) = &spec.stats {
                    st.filled_bytes.store(*self.filled_bytes.lock().unwrap(), Ordering::Relaxed);
                    st.reader_waits.store(3, Ordering::Relaxed);
                    st.wait_ms_longest.store(2500, Ordering::Relaxed);
                    // 6000 sectors in 10 s of reading: the drive reads at 8x.
                    st.read_ms.store(10_000, Ordering::Relaxed);
                    st.sectors_read.store(6_000, Ordering::Relaxed);
                }
                for _ in 0..500 {
                    if *self.hold_in.lock().unwrap() != Some(t.number) {
                        break;
                    }
                    if cancel.load(Ordering::Relaxed) {
                        return Err(RipError { kind: "cancelled", message: "cancelled".into(), track: Some(t.number) });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                emit(&RipEvent::Progress { track: t.number, fraction: 0.5 });
                std::fs::write(spec.out_dir.join(&t.file), &mp3).map_err(|e| RipError { kind: "io", message: e.to_string(), track: Some(t.number) })?;
                emit(&RipEvent::TrackDone { track: t.number, file: t.file.clone(), sha1: format!("sha-{}", t.number), size: mp3.len() as u64 });
                if *self.hold_after.lock().unwrap() == Some(t.number) {
                    for _ in 0..500 {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            emit(&RipEvent::Done);
            Ok(())
        }

        fn eject(&self, device: &str) -> std::io::Result<EjectOutcome> {
            let mut units = self.units.lock().unwrap();
            let Some((_, u)) = units.iter_mut().find(|(d, _)| d == device) else { return Err(std::io::ErrorKind::NotFound.into()) };
            // The real function sends nothing when the tray is open or no disc is in.
            if u.tray_open || u.disc.is_none() {
                return Ok(EjectOutcome::NothingToEject);
            }
            u.tray_open = true;
            u.changed = true;
            self.ejects.lock().unwrap().push(device.to_owned());
            Ok(EjectOutcome::Ejected)
        }

        fn watch(&self) -> Option<Box<dyn Watcher>> {
            self.watch_available.load(Ordering::Relaxed).then(|| Box::new(EventCounter(self.events.clone())) as Box<dyn Watcher>)
        }

        fn set_speed(&self, device: &str, times: u32) -> std::io::Result<()> {
            self.speeds.lock().unwrap().push(format!("set {times} {device}"));
            Ok(())
        }

        fn reset_speed(&self, device: &str) -> std::io::Result<()> {
            self.speeds.lock().unwrap().push(format!("reset {device}"));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_speed_of_the_drive_is_measured_from_the_sectors_and_the_time_it_read() {
        use std::sync::atomic::Ordering::Relaxed;
        let live = Live { job_id: 1, device: "d".into(), fraction: 0.0, finished: false, error: String::new(), tracks: Vec::new(), verify: true, stats: Arc::new(cdrip::ring::RipStats::default()), ring_bytes: 1 };
        assert_eq!(live.speed_x(), None, "nothing read yet");
        live.stats.read_ms.store(1_000, Relaxed);
        live.stats.sectors_read.store(600, Relaxed);
        assert_eq!(live.speed_x(), None, "too little to say");
        // 6000 sectors in 10 s of reading are 600 a second, which is 8x (1x is 75).
        live.stats.read_ms.store(10_000, Relaxed);
        live.stats.sectors_read.store(6_000, Relaxed);
        assert_eq!(live.speed_x(), Some(8.0));
        live.stats.sectors_read.store(2_625, Relaxed);
        assert_eq!(live.speed_x(), Some(3.5));
    }

    #[test]
    fn a_track_that_is_read_once_waits_for_the_check_and_the_rip_is_where_it_reads() {
        let track = |n: u32| TrackState { number: n, title: String::new(), seconds: 100, state: "waiting", fraction: 0.0, error: String::new() };
        let mut live = Live {
            job_id: 1,
            device: "/dev/sr0".into(),
            fraction: 0.0,
            finished: false,
            error: String::new(),
            tracks: vec![track(1), track(2), track(3)],
            verify: true,
            stats: Arc::new(cdrip::ring::RipStats::default()),
            ring_bytes: 1,
        };
        let states = |l: &Live| l.tracks.iter().map(|t| t.state).collect::<Vec<_>>();
        assert_eq!(live.current_position(), 1, "nothing read yet: the first");
        live.apply(&RipEvent::TrackStart { track: 1 });
        live.apply(&RipEvent::Encoded { track: 1, sha1: "a".into(), size: 1 });
        // Read once: it waits for the second reading, which is not the same as not being read yet.
        assert_eq!(states(&live), ["read", "waiting", "waiting"]);
        live.apply(&RipEvent::TrackStart { track: 2 });
        live.apply(&RipEvent::TrackProgress { track: 2, pass: 1, fraction: 0.3 });
        assert_eq!(states(&live), ["read", "reading", "waiting"]);
        assert_eq!(live.current_position(), 2, "the rip is at the track that is read, though none is done");
        // The second reading starts at the first track.
        live.apply(&RipEvent::TrackProgress { track: 1, pass: 2, fraction: 0.5 });
        assert_eq!(live.current_position(), 1);
        live.apply(&RipEvent::TrackDone { track: 1, file: "01.mp3".into(), sha1: "a".into(), size: 1 });
        assert_eq!(states(&live)[0], "done");
        // A third reading (the first two did not agree) is its own state, with its progress, and the rip is still at that track.
        live.apply(&RipEvent::TrackProgress { track: 2, pass: 3, fraction: 0.4 });
        assert_eq!((states(&live)[1], live.tracks[1].fraction), ("rereading", 0.4));
        assert_eq!(live.current_position(), 2);
        // A track that was taken without a confirmation stays marked when it is filed, and the end of the rip does not make it "done".
        live.apply(&RipEvent::Unverified { track: 3 });
        live.apply(&RipEvent::TrackDone { track: 3, file: "03.mp3".into(), sha1: "c".into(), size: 1 });
        assert_eq!(states(&live)[2], "unverified");
        // Without verification a written track is converted, not "read".
        live.verify = false;
        live.apply(&RipEvent::Encoded { track: 3, sha1: "c".into(), size: 1 });
        assert_eq!(states(&live)[2], "encoding");
        // A track that is left out is failed at once, with its code, while the rip goes on with the others.
        live.apply(&RipEvent::Error { kind: "verify", message: "x".into(), track: Some(2) });
        assert_eq!(states(&live)[1], "failed");
        assert_eq!(live.tracks[1].error, rip_failure_code("verify"));
    }

    use super::fake::*;
    use super::*;

    fn drives() -> (Arc<FakeDrives>, Drives) {
        let fake = FakeDrives::with_drive("/dev/fake0");
        (fake.clone(), Drives::new(fake))
    }

    fn only(d: &Drives) -> DriveState {
        d.snapshot().0.into_iter().next().expect("one drive")
    }

    #[test]
    fn the_state_follows_the_tray_and_the_disc_and_reads_the_table_only_for_a_new_disc() {
        let (fake, d) = drives();
        assert_eq!(d.refresh("", true), (true, false));
        let s = only(&d);
        assert_eq!((s.tray, s.medium, s.tracks), (Tray::Closed, Medium::None, 0));
        // A disc goes in: the table is read, the disc id is computed.
        fake.edit("/dev/fake0", |u| {
            u.disc = Some(disc(3));
            u.changed = true;
        });
        assert!(d.refresh("", false).0);
        let s = only(&d);
        assert_eq!((s.tray, s.medium, s.tracks), (Tray::Closed, Medium::Audio, 3));
        assert!(s.disc_id.as_deref().is_some_and(|i| i.len() == 28) && s.toc.as_deref().is_some_and(|t| t.starts_with("1+3+")), "{s:?}");
        assert_eq!(fake.toc_reads.load(Ordering::Relaxed), 1);
        // Looking again does not read it again and changes nothing.
        assert_eq!(d.refresh("", false), (false, false));
        assert_eq!(fake.toc_reads.load(Ordering::Relaxed), 1);
        // The tray opens: no disc is shown and nothing is read.
        fake.edit("/dev/fake0", |u| u.tray_open = true);
        assert!(d.refresh("", false).0);
        let s = only(&d);
        assert_eq!((s.tray, s.medium, s.disc_id), (Tray::Open, Medium::None, None));
        assert_eq!(fake.toc_reads.load(Ordering::Relaxed), 1);
        // Another disc, the tray shut: read anew.
        fake.edit("/dev/fake0", |u| {
            u.tray_open = false;
            u.disc = Some(disc(5));
            u.changed = true;
        });
        d.refresh("", false);
        assert_eq!((only(&d).medium, only(&d).tracks), (Medium::Audio, 5));
        assert_eq!(fake.toc_reads.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_drive_that_is_not_ready_is_asked_again_soon_and_a_denied_or_vanished_one_is_told() {
        let (fake, d) = drives();
        fake.edit("/dev/fake0", |u| u.not_ready = true);
        assert_eq!(d.refresh("", false), (true, true));
        assert_eq!((only(&d).tray, only(&d).medium), (Tray::Unknown, Medium::Unknown));
        fake.edit("/dev/fake0", |u| {
            u.not_ready = false;
            u.denied = true;
        });
        d.refresh("", false);
        assert!(only(&d).denied);
        fake.edit("/dev/fake0", |u| {
            u.denied = false;
            u.gone = true;
        });
        d.refresh("", false);
        assert!(d.snapshot().0.is_empty(), "a vanished drive is not listed");
        // A drive named in the settings that is not there is shown as nothing, not invented.
        assert!(d.refresh("/dev/other", false).0 || d.snapshot().0.is_empty());
    }

    #[test]
    fn eject_never_closes_a_tray_and_sends_nothing_without_a_disc() {
        let (fake, d) = drives();
        // No disc: nothing is sent.
        assert_eq!(d.eject("/dev/fake0").unwrap(), EjectResult::NothingToEject);
        fake.edit("/dev/fake0", |u| u.disc = Some(disc(2)));
        assert_eq!(d.eject("/dev/fake0").unwrap(), EjectResult::Ejected);
        assert_eq!(fake.ejects.lock().unwrap().len(), 1);
        // The tray is open now: a second eject must not toggle it shut.
        assert_eq!(d.eject("/dev/fake0").unwrap(), EjectResult::NothingToEject);
        assert_eq!(fake.ejects.lock().unwrap().len(), 1);
        assert!(fake.units.lock().unwrap()[0].1.tray_open);
        // A drive that is gone or may not be used.
        fake.edit("/dev/fake0", |u| u.denied = true);
        assert_eq!(d.eject("/dev/fake0").unwrap_err(), Refusal(503, "drive_denied"));
        assert_eq!(d.eject("/dev/none").unwrap_err(), Refusal(404, "no_drive"));
    }

    #[test]
    fn the_toc_for_a_lookup_comes_from_the_disc_in_the_drive() {
        let (fake, d) = drives();
        assert!(d.inserted_toc("").is_none());
        fake.edit("/dev/fake0", |u| {
            u.disc = Some(disc(4));
            u.changed = true;
        });
        d.refresh("", false);
        let toc = d.inserted_toc("").unwrap();
        assert_eq!((toc.first_track, toc.last_track, toc.starts_lba.len()), (1, 4, 4));
        assert!(d.inserted_toc("/dev/other").is_none());
        fake.edit("/dev/fake0", |u| u.tray_open = true);
        d.refresh("", false);
        assert!(d.inserted_toc("").is_none(), "no table while the tray is open");
    }

    #[test]
    fn a_table_with_a_data_session_counts_its_audio_part_only() {
        let info = TocInfo {
            tracks: vec![
                cdrip::discid::TocTrack { number: 1, start_lba: 0, is_audio: true },
                cdrip::discid::TocTrack { number: 2, start_lba: 20_000, is_audio: true },
                cdrip::discid::TocTrack { number: 3, start_lba: 60_000, is_audio: false },
            ],
            leadout_lba: 70_000,
        };
        let t = audio_toc(&info).unwrap();
        assert_eq!((t.first_track, t.last_track, t.leadout_lba), (1, 2, 60_000 - 11_400));
        assert!(audio_toc(&TocInfo { tracks: vec![cdrip::discid::TocTrack { number: 1, start_lba: 0, is_audio: false }], leadout_lba: 100 }).is_none());
    }
}
