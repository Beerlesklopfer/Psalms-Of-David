// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pages. Templates are Jinja files in `templates/`, parsed once at start and
//! embedded in the program (MiniJinja). A page gets plain data from the `Store`
//! and renders it for one language.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use minijinja::value::Serde;
use minijinja::{Environment, State, UndefinedBehavior, context};
use serde::Serialize;

use super::i18n::{Catalog, Lang};
use crate::error::{Error, Result};
use crate::roles::Schedule;
use crate::scan::ScanReport;
use crate::store::Store;

/// The templates, embedded so that the program is a single file.
const TEMPLATES: &[(&str, &str)] = &[
    ("base.html", include_str!("../../templates/base.html")),
    ("login.html", include_str!("../../templates/login.html")),
    ("overview.html", include_str!("../../templates/overview.html")),
    ("users.html", include_str!("../../templates/users.html")),
    ("books.html", include_str!("../../templates/books.html")),
    ("settings.html", include_str!("../../templates/settings.html")),
    ("import.html", include_str!("../../templates/import.html")),
    ("cd.html", include_str!("../../templates/cd.html")),
    ("schedule.html", include_str!("../../templates/schedule.html")),
    ("pairing.html", include_str!("../../templates/pairing.html")),
    ("genres.html", include_str!("../../templates/genres.html")),
    ("releases.html", include_str!("../../templates/releases.html")),
    ("podcasts.html", include_str!("../../templates/podcasts.html")),
    ("fragments/podcast_actions.html", include_str!("../../templates/fragments/podcast_actions.html")),
    ("fragments/waiting_requests.html", include_str!("../../templates/fragments/waiting_requests.html")),
    ("podcast.html", include_str!("../../templates/podcast.html")),
    ("radio.html", include_str!("../../templates/radio.html")),
    // Sub-templates. A list is a frame (`*_list`) around rows (`*_row`). The page
    // includes them, and a procedure can answer with the same fragment alone.
    ("fragments/message.html", include_str!("../../templates/fragments/message.html")),
    ("fragments/user_list.html", include_str!("../../templates/fragments/user_list.html")),
    ("fragments/user_row.html", include_str!("../../templates/fragments/user_row.html")),
    ("fragments/book_tracks.html", include_str!("../../templates/fragments/book_tracks.html")),
    ("fragments/book_track_labels.html", include_str!("../../templates/fragments/book_track_labels.html")),
    ("fragments/book_list.html", include_str!("../../templates/fragments/book_list.html")),
    ("fragments/book_row.html", include_str!("../../templates/fragments/book_row.html")),
    ("fragments/pairing_result.html", include_str!("../../templates/fragments/pairing_result.html")),
    ("fragments/pairing_codes.html", include_str!("../../templates/fragments/pairing_codes.html")),
    ("fragments/pairing_badge.html", include_str!("../../templates/fragments/pairing_badge.html")),
    ("fragments/device_list.html", include_str!("../../templates/fragments/device_list.html")),
    ("fragments/device_row.html", include_str!("../../templates/fragments/device_row.html")),
    ("fragments/schedule_panel.html", include_str!("../../templates/fragments/schedule_panel.html")),
    ("fragments/book_tags.html", include_str!("../../templates/fragments/book_tags.html")),
    ("fragments/upload_section.html", include_str!("../../templates/fragments/upload_section.html")),
    ("fragments/upload_queue.html", include_str!("../../templates/fragments/upload_queue.html")),
    ("fragments/import_panel.html", include_str!("../../templates/fragments/import_panel.html")),
    ("fragments/settings_values.html", include_str!("../../templates/fragments/settings_values.html")),
    ("fragments/cd_drive.html", include_str!("../../templates/fragments/cd_drive.html")),
    ("fragments/folder_picker.html", include_str!("../../templates/fragments/folder_picker.html")),
    ("fragments/pager.html", include_str!("../../templates/fragments/pager.html")),
    ("fragments/cd_import.html", include_str!("../../templates/fragments/cd_import.html")),
    ("fragments/settings_cdimport.html", include_str!("../../templates/fragments/settings_cdimport.html")),
    ("fragments/settings_llm.html", include_str!("../../templates/fragments/settings_llm.html")),
    ("fragments/access_warning.html", include_str!("../../templates/fragments/access_warning.html")),
    ("fragments/settings_roles.html", include_str!("../../templates/fragments/settings_roles.html")),
    ("fragments/import_settings.html", include_str!("../../templates/fragments/import_settings.html")),
    ("fragments/root_row.html", include_str!("../../templates/fragments/root_row.html")),
    ("fragments/book_filing.html", include_str!("../../templates/fragments/book_filing.html")),
    ("profile.html", include_str!("../../templates/profile.html")),
    ("fragments/ui.html", include_str!("../../templates/fragments/ui.html")),
    ("fragments/avatar.html", include_str!("../../templates/fragments/avatar.html")),
    ("blacklist.html", include_str!("../../templates/blacklist.html")),
    ("blacklist_hits.html", include_str!("../../templates/blacklist_hits.html")),
    ("fragments/blacklist_form.html", include_str!("../../templates/fragments/blacklist_form.html")),
    ("fragments/blacklist_preview.html", include_str!("../../templates/fragments/blacklist_preview.html")),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Ok,
    Error,
}

/// What every page needs besides its data: the language, the CSRF token of the
/// session (empty when nobody is signed in) and an optional message. The message
/// is given as a catalog key, so it is shown in the language of the page.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    pub lang: Lang,
    pub csrf: &'a str,
    pub message: Option<(MessageKind, &'a str)>,
    /// Who is signed in, for the header.
    pub me: Option<&'a crate::web::Me>,
    /// Seconds until the next try after wrong PINs, for the countdown of the sign-in page.
    pub wait_s: Option<i64>,
}

impl<'a> View<'a> {
    pub fn new(lang: Lang) -> View<'static> {
        View { lang, csrf: "", message: None, me: None, wait_s: None }
    }

    pub fn csrf(mut self, csrf: &'a str) -> Self {
        self.csrf = csrf;
        self
    }

    pub fn me(mut self, me: &'a crate::web::Me) -> Self {
        self.me = Some(me);
        self
    }

    pub fn message(mut self, kind: MessageKind, key: &'a str) -> Self {
        self.message = Some((kind, key));
        self
    }

    /// The wait until the next try, shown as a countdown.
    pub fn wait(mut self, wait_s: Option<i64>) -> Self {
        self.wait_s = wait_s.filter(|&s| s > 0);
        self
    }

    fn context(&self) -> minijinja::Value {
        let catalog = Catalog::new(self.lang);
        context! {
            lang => self.lang.code(),
            csrf => self.csrf,
            me => self.me.map(Serde),
            wait_s => self.wait_s,
            wait_shown => self.wait_s.map(countdown_text),
            message => self.message.map(|(_, key)| catalog.t(key).to_owned()),
            message_kind => match self.message {
                Some((MessageKind::Error, _)) => "error",
                _ => "ok",
            },
        }
    }
}

/// A wait as the countdown shows it: `0:08`, `16:40`, `9:06:08`, or with days `2 d 03:15:00`.
pub fn countdown_text(seconds: i64) -> String {
    let (d, h, m, s) = (seconds / 86_400, seconds / 3_600 % 24, seconds / 60 % 60, seconds % 60);
    match (d, h) {
        (0, 0) => format!("{m}:{s:02}"),
        (0, _) => format!("{h}:{m:02}:{s:02}"),
        _ => format!("{d} d {h:02}:{m:02}:{s:02}"),
    }
}

impl From<Lang> for View<'static> {
    fn from(lang: Lang) -> Self {
        View::new(lang)
    }
}

/// One quarter of an hour in the planner grid: its label and, for each day, whether it is allowed.
#[derive(Serialize)]
struct SlotRow {
    index: usize,
    /// "07:00" on the full hour, empty on the others, so the grid stays readable.
    label: String,
    days: Vec<bool>,
}

#[derive(Serialize)]
struct ScheduleView {
    id: i64,
    name: String,
    role: String,
    has_schedule: bool,
    hours: String,
    rows: Vec<SlotRow>,
}

fn schedule_view(id: i64, name: String, role: String, schedule: Option<Schedule>) -> ScheduleView {
    let planner = schedule.clone().unwrap_or_else(Schedule::none);
    let rows = (0..96usize)
        .map(|slot| SlotRow {
            index: slot,
            label: if slot % 4 == 0 { format!("{:02}:00", slot / 4) } else { String::new() },
            days: (0..7u8).map(|day| planner.slot(day, slot)).collect(),
        })
        .collect();
    let quarters = planner.allowed_slots();
    ScheduleView {
        id,
        name,
        role,
        has_schedule: schedule.is_some(),
        hours: format!("{}.{:02}", quarters / 4, (quarters % 4) * 25),
        rows,
    }
}

#[derive(Serialize)]
struct PolicyView {
    /// The session length in whole days, at least one.
    days: i64,
    role: String,
    max_devices: i64,
    session_ttl_s: i64,
    step_up_s: i64,
    confirmed: bool,
}

#[derive(Serialize)]
struct SettingView {
    key: String,
    value: String,
    /// The value as people read it: seconds as minutes.
    shown: String,
    /// Message key of the unit, or empty.
    unit: &'static str,
    /// Which block of the settings page the value belongs to.
    group: &'static str,
    confirmed: bool,
    /// The level the start fixed for `log_level` (it wins at the next start), else nothing.
    fixed: Option<String>,
    /// The placeholders a template setting offers, by their names (the page shows them in plain words).
    vars: Vec<&'static str>,
}

impl SettingView {
    fn from_row(s: crate::entities::setting::Model) -> SettingView {
        let minutes = |v: &str| v.parse::<i64>().map_or_else(|_| v.to_owned(), |n| (n / 60).to_string());
        let (group, unit, shown) = match s.key.as_str() {
            "pin_backoff_s" => ("signin", "unit.seconds", s.value.clone()),
            "enrollment_code_ttl_s" => ("pairing", "unit.minutes", minutes(&s.value)),
            "device_inactive_days" => ("pairing", "unit.days", s.value.clone()),
            "guest_expire_days" => ("guests", "unit.days", s.value.clone()),
            "import_interval_min" => ("library", "unit.minutes", s.value.clone()),
            "new_days" => ("library", "unit.days", s.value.clone()),
            "list_page_size" => ("library", "unit.items", s.value.clone()),
            "min_free_gb" => ("library", "unit.gb", s.value.clone()),
            "time_zone" => ("general", "", s.value.clone()),
            "log_level" => ("general", "", s.value.clone()),
            "tree_audiobook" | "tree_music" => ("library", "", s.value.clone()),
            "quarantine_folder" | "audiobook_genres" | "placeholder_tags" => ("library", "", s.value.clone()),
            "transcode_kbps" => ("library", "unit.kbps", s.value.clone()),
            "transcode_cache_mb" => ("library", "unit.mb", s.value.clone()),
            "transcode_parallel" => ("library", "", s.value.clone()),
            "feed_refresh_min" => ("podcasts", "unit.minutes", s.value.clone()),
            "feed_max_bytes" => ("podcasts", "unit.mb", s.value.parse::<i64>().map_or_else(|_| s.value.clone(), |n| (n / 1_048_576).to_string())),
            "radio_directory_url" => ("podcasts", "", s.value.clone()),
            "cd_bitrate_kbps" => ("cdimport", "unit.kbps", s.value.clone()),
            "cd_sample_rate_hz" => ("cdimport", "unit.hz", s.value.clone()),
            "cd_cover_px" => ("cdimport", "unit.px", s.value.clone()),
            "cd_poll_seconds" => ("cdimport", "unit.seconds", s.value.clone()),
            "cd_read_speed" => ("cdimport", "unit.speed_x", s.value.clone()),
            "cd_buffer_seconds" => ("cdimport", "unit.seconds", s.value.clone()),
            "cd_buffer_max_mb" => ("cdimport", "unit.mb", s.value.clone()),
            "cd_resume_keep_hours" => ("cdimport", "unit.hours", s.value.clone()),
            "cd_heal_tolerance_s" => ("cdimport", "unit.seconds", s.value.clone()),
            "cd_held_keep_days" => ("cdimport", "unit.days", s.value.clone()),
            k if crate::cd_settings::is_cd_key(k) => ("cdimport", "", s.value.clone()),
            "tls_cert" | "tls_key" | "tls_host" => ("tls", "", s.value.clone()),
            "tls_reload_s" => ("tls", "unit.seconds", s.value.clone()),
            _ => ("other", "", s.value.clone()),
        };
        let fixed = (s.key == "log_level").then(|| crate::logging::fixed().map(|l| l.to_string().to_lowercase())).flatten();
        let vars = crate::cd_settings::vars_of(&s.key).to_vec();
        SettingView { key: s.key, value: s.value, shown, unit, group, confirmed: s.confirmed, fixed, vars }
    }

    /// The import has its own tab; its values are not listed again as plain values.
    fn is_import(&self) -> bool {
        (self.key.starts_with("import_") && self.key != "import_interval_min") || self.key == "rating_rule"
    }
}

/// A job under way, in the drive panel.
#[derive(Serialize)]
struct ActiveJob {
    id: i64,
    book: String,
    author: String,
    disc: i64,
    discs_total: i64,
    phase: String,
    track: i64,
    tracks_total: i64,
    percent: i64,
    /// The server rips it (it can be cancelled here).
    by_server: bool,
}

/// What the page sends back when it asks for the panel again: the release chosen, the choice of tracks and the
/// typed text, so that a new rendering does not lose them.
#[derive(Default, Debug, Clone)]
pub struct PanelQuery {
    pub pick: Option<usize>,
    /// "Cancel": start afresh.
    pub reset: bool,
    /// The form was sent along (the choice of tracks counts, even if no box is ticked).
    pub sel: bool,
    pub only: Vec<u32>,
    pub disc_id: Option<String>,
    /// Show the form for a new import although the CD is in the library.
    pub force: bool,
    pub fields: HashMap<String, String>,
    /// The profile that looks at the panel (its rights decide which buttons are shown); 0 = nobody known.
    pub viewer: i64,
}

impl PanelQuery {
    pub fn with_viewer(mut self, viewer: i64) -> PanelQuery {
        self.viewer = viewer;
        self
    }

    /// From the pairs of a query string or a form (`only` may repeat).
    pub fn from_pairs(pairs: Vec<(String, String)>) -> PanelQuery {
        let mut q = PanelQuery::default();
        for (k, v) in pairs {
            match k.as_str() {
                "pick" => q.pick = v.parse().ok(),
                "reset" => q.reset = v == "1",
                "force" => q.force = v == "1",
                "sel" => q.sel = v == "1",
                "only" => q.only.extend(v.parse::<u32>().ok()),
                "disc_id" => q.disc_id = Some(v),
                "csrf" | "sig" => {}
                _ => {
                    q.fields.insert(k, v);
                }
            }
        }
        q
    }
}

/// What an interrupted rip of the disc in the drive left, for the page.
#[derive(Serialize)]
struct ResumePanel {
    done: usize,
    total: usize,
    when: String,
    keep_until: String,
    hours: u64,
    /// The settings that decide the audio are the same as then.
    settings_match: bool,
    /// Of the tracks that are chosen now: how many were read then (taken over), how many are read now, and how many that were read then are
    /// not chosen any more (dropped).
    kept: usize,
    rest: usize,
    dropped: usize,
}

/// A line of the list of tracks of a rip, with its state.
#[derive(Serialize)]
struct LiveRow {
    number: u32,
    nn: String,
    title: String,
    length: String,
    state: &'static str,
    percent: i64,
    error: String,
    error_key: Option<String>,
}

/// The rip that runs on the drive (or ran last, until it is closed), for the page.
#[derive(Serialize)]
struct RipPanel {
    device: String,
    active: bool,
    percent: i64,
    /// "Track `current` of `total`".
    current: usize,
    total: usize,
    /// The catalog key suffix of the phase (`cd.phase.<phase>`).
    phase: &'static str,
    book: String,
    author: String,
    disc: i64,
    discs_total: i64,
    rows: Vec<LiveRow>,
    error: String,
    error_key: Option<String>,
    can_cancel: bool,
    job_id: i64,
    /// Seconds waiting in the buffer, the most it holds, and whether it is full or empty (what brakes the rip).
    buffer: Option<BufferView>,
}

#[derive(Serialize)]
struct BufferView {
    seconds: String,
    max: u32,
    percent: u32,
    waits: u64,
    longest: String,
    state: &'static str,
    /// The measured speed of the drive as text ("7.8"), empty while it is not known yet.
    speed: String,
}

/// A CD that is in the library, for the page.
#[derive(Serialize)]
struct PresentPanel {
    book: String,
    author: String,
    disc: u32,
    in_library: usize,
    on_disc: usize,
    complete: bool,
    missing: String,
    damaged: Vec<DamageRow>,
}

#[derive(Serialize)]
struct DamageRow {
    number: u32,
    reason: &'static str,
    have: String,
    want: String,
}

/// One line of the list of tracks.
#[derive(Serialize)]
struct TrackRow {
    number: u32,
    nn: String,
    title: String,
    seconds: u32,
    length: String,
    checked: bool,
    /// Read already by an interrupted rip of this disc that the settings still fit: this track is taken over if it stays chosen.
    read: bool,
}

#[derive(Serialize)]
struct CandView {
    index: usize,
    release_id: String,
    title: String,
    author: String,
    narrator: String,
    audiobook: bool,
    disc: Option<u32>,
    discs_total: u32,
    tracks: Vec<String>,
}

/// The fields of the form that starts a rip.
#[derive(Serialize)]
struct RipForm {
    author: String,
    title: String,
    disc: String,
    discs_total: String,
    mbid: String,
    tracks: String,
    narrator: String,
    eject: bool,
    kind: String,
}

/// What the drive panel of the tab "CD import" shows.
#[derive(Serialize)]
struct DrivePanel {
    drives: Vec<crate::cdrive::DriveState>,
    mode: &'static str,
    sig: String,
    jobs: Vec<ActiveJob>,
    candidates: Vec<CandView>,
    /// The words of a search by hand (kept while the panel is drawn again).
    mbq: String,
    /// The same as a part of a query string (`mbq=Bach%20Passion`), for the addresses of the links; empty without a search.
    mbq_param: String,
    form: RipForm,
    can_rip: bool,
    /// A disc is being read: nothing may reach the drive or the MusicBrainz lookup meanwhile.
    busy: bool,
    device: String,
    lookup_error: Option<&'static str>,
    rip_error: Option<String>,
    pick: Option<usize>,
    /// A disc is in a drive whose tray is shut and nothing runs on it: it may be ejected.
    can_eject: bool,
    /// The numbers that are in the library already, when a start was refused with `disc_exists`.
    taken: String,
    disc_id: String,
    rows: Vec<TrackRow>,
    total_count: usize,
    selected_count: usize,
    selected_time: String,
    resume: Option<ResumePanel>,
    /// The CD is in the library: the page offers checking and repairing instead of a new import.
    present: Option<PresentPanel>,
    /// The rip that runs, or ran last.
    rip: Option<RipPanel>,
    /// The form for a new import is shown (no CD in the library, or the person asked for it anyway).
    show_import: bool,
    /// CDs that wait for approval or for a new try, and what the viewer may do with them.
    waiting: Vec<WaitingView>,
    can_insert: bool,
}

/// A CD that waits for a decision or for a new try, in the drive panel.
#[derive(Serialize)]
struct WaitingView {
    id: i64,
    /// `awaiting_approval` or `retry_wait`.
    state: String,
    book: String,
    author: String,
    disc: i64,
    discs_total: i64,
    tracks: i64,
    starter: String,
    since: String,
    /// The code of the failure that made it wait, and the catalog key of its text.
    error: String,
    error_key: Option<String>,
    /// The viewer may approve or reject it (shown as buttons); else only the fact is shown.
    can_decide: bool,
    /// The viewer may read it again or give it up.
    can_retry: bool,
}

/// One line of the import log on the tab "CD import".
#[derive(Serialize)]
struct CdJobView {
    book: String,
    author: String,
    disc: i64,
    discs_total: i64,
    phase: String,
    /// The state of the job in the state machine, and the decision (who) and the reason it was rejected.
    cd_state: String,
    decided_by: String,
    reason: String,
    when: String,
    /// The code of the failure, and the catalog key of its text.
    error: String,
    error_key: Option<String>,
    /// Finished, but the import of the library did not run (it is locked).
    locked: bool,
    /// What the job says besides its state (for a turned-down automatic import: the book, the number of releases ...).
    message: String,
    /// The server started it by itself.
    auto: bool,
}

#[derive(Serialize)]
struct DeviceDash {
    name: String,
    shared: bool,
    last_seen: Option<String>,
}

#[derive(Serialize)]
struct ListeningDash {
    title: String,
    author: String,
    percent: i64,
    last: String,
}

#[derive(Serialize)]
struct HoursDash {
    /// Allowed hours per week, written like the planner does (12.25).
    per_week: String,
    allowed_now: bool,
}

/// What the profile page shows about the user themselves.
#[derive(Serialize)]
struct PersonalDashboard {
    language: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    role: String,
    age_limit: Option<i64>,
    device_count: usize,
    device_limit: i64,
    devices: Vec<DeviceDash>,
    listening: Vec<ListeningDash>,
    hours: Option<HoursDash>,
}

/// "2026-10-27 22:41" in the time zone of the server.
fn stamp(ms: i64, zone: Option<&str>) -> String {
    let Ok(ts) = jiff::Timestamp::from_millisecond(ms) else { return String::new() };
    let zoned = match zone.map(|z| ts.in_tz(z)) {
        Some(Ok(z)) => z,
        _ => ts.to_zoned(jiff::tz::TimeZone::system()),
    };
    zoned.strftime("%Y-%m-%d %H:%M").to_string()
}

async fn personal_dashboard(store: &Store, user_id: i64) -> Result<PersonalDashboard> {
    let profile = store.profile_by_id(user_id).await?.ok_or(Error::WrongPin)?;
    let zone = store.setting("time_zone").await?;
    let policy_limit = store
        .role_policies()
        .await?
        .into_iter()
        .find(|p| p.role == profile.role)
        .map_or(0, |p| p.max_devices);
    let devices: Vec<_> = store.devices_of(user_id).await?.into_iter().filter(|d| d.revoked_at.is_none()).collect();
    let device_limit = profile.max_devices.unwrap_or(policy_limit);

    let mut listening = Vec::new();
    for entry in store.continue_listening(user_id, 6).await? {
        let tracks = store.tracks_of(entry.book_id).await?;
        let total: i64 = tracks.iter().map(|t| t.duration_ms).sum();
        let before: i64 = tracks.iter().take_while(|t| t.id != entry.track_id).map(|t| t.duration_ms).sum();
        let percent = if total > 0 { ((before + entry.position_ms) * 100 / total).clamp(0, 100) } else { 0 };
        listening.push(ListeningDash { title: entry.title, author: entry.author, percent, last: stamp(entry.updated_at, zone.as_deref()) });
    }

    let hours = profile.schedule.as_deref().and_then(Schedule::from_bytes).map(|planner| {
        let (weekday, minute) = crate::api::local_time_now(zone.as_deref());
        let quarters = planner.allowed_slots();
        HoursDash { per_week: if quarters % 4 == 0 { (quarters / 4).to_string() } else { format!("{}.{:02}", quarters / 4, (quarters % 4) * 25) }, allowed_now: planner.is_allowed(weekday, minute) }
    });

    Ok(PersonalDashboard {
        language: profile.language,
        first_name: profile.first_name,
        last_name: profile.last_name,
        role: profile.role,
        age_limit: profile.max_age,
        device_count: devices.len(),
        device_limit,
        devices: devices
            .into_iter()
            .map(|d| DeviceDash { name: d.name, shared: d.shared, last_seen: d.last_seen.map(|ms| stamp(ms, zone.as_deref())) })
            .collect(),
        listening,
        hours,
    })
}

/// One device as the pairing page shows it.
#[derive(Serialize)]
struct DeviceView {
    id: i64,
    user_name: String,
    name: String,
    tunnel_ip: String,
    shared: bool,
    revoked: bool,
    last_seen: Option<String>,
}

fn device_view(d: crate::pairing::DeviceRow, zone: Option<&str>) -> DeviceView {
    DeviceView {
        id: d.id,
        user_name: d.user_name,
        name: d.name,
        tunnel_ip: d.tunnel_ip,
        shared: d.shared,
        revoked: d.revoked_at.is_some(),
        last_seen: d.last_seen.map(|ms| stamp(ms, zone)),
    }
}

/// One pairing code as the pairing page lists it. Never the code itself.
#[derive(Serialize)]
struct CodeView {
    id: i64,
    user_name: String,
    created_by: String,
    shared: bool,
    created: String,
    until: String,
    /// `open`, `used`, `revoked` or `expired`.
    state: &'static str,
    device: String,
    used: String,
}

/// Something that happened with a code in the last minutes: a device paired with it, or it ran out unused.
#[derive(Serialize)]
struct CodeEvent {
    /// `paired` or `expired`.
    kind: &'static str,
    device: String,
    user_name: String,
    minutes: i64,
}

/// How far back the messages of the pairing page go.
const EVENT_WINDOW_MS: i64 = 15 * 60_000;

/// Parses the templates once and renders pages.
pub struct AdminUi {
    env: Environment<'static>,
    /// Folders that may hold templates and `static/admin.css` of their own, in
    /// order of preference.
    dirs: Vec<PathBuf>,
    /// The files of those folders that are in use instead of the built-in ones.
    pub overrides: Vec<PathBuf>,
}

/// Where templates of a distribution or of the administrator are looked for: the
/// folder in `DAVID_TEMPLATES_DIR`, then the machine's own (`/etc/david/templates`),
/// then the distribution's (`/usr/share/david/templates`). A file found there is
/// used instead of the built-in template of the same name; folders that do not
/// exist are skipped.
pub fn default_template_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("DAVID_TEMPLATES_DIR").map(PathBuf::from).into_iter().collect();
    #[cfg(unix)]
    dirs.extend([PathBuf::from("/etc/david/templates"), PathBuf::from("/usr/share/david/templates")]);
    #[cfg(windows)]
    dirs.extend(std::env::var_os("PROGRAMDATA").map(|p| PathBuf::from(p).join("David").join("templates")));
    dirs
}

/// Writes the built-in templates and stylesheet into `dir`, as a starting point for
/// a distribution's own. Returns how many files were written.
pub fn export_templates(dir: &Path) -> std::io::Result<usize> {
    let mut count = 0;
    for (name, source) in TEMPLATES {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, source)?;
        count += 1;
    }
    std::fs::create_dir_all(dir.join("static"))?;
    std::fs::write(dir.join("static/admin.css"), include_str!("../../static/admin.css"))?;
    std::fs::write(dir.join("static/logo.svg"), include_str!("../../static/logo.svg"))?;
    std::fs::write(dir.join("static/favicon.svg"), include_str!("../../static/favicon.svg"))?;
    std::fs::write(dir.join("static/ui.js"), include_str!("../../static/ui.js"))?;
    std::fs::write(dir.join("static/crop.js"), include_str!("../../static/crop.js"))?;
    std::fs::write(dir.join("static/chips.js"), include_str!("../../static/chips.js"))?;
    std::fs::write(dir.join("static/fields.js"), include_str!("../../static/fields.js"))?;
    Ok(count + 7)
}

impl AdminUi {
    /// Fails if a template has a syntax error, so a broken template is found at
    /// start (and by the tests) and not when somebody opens the page.
    pub fn new() -> Result<AdminUi> {
        AdminUi::with_override_dirs(Vec::new())
    }

    /// Like `new`, but a template that exists in one of `dirs` replaces the built-in
    /// one. A template of a distribution that does not parse stops the start too,
    /// with the name of the file.
    pub fn with_override_dirs(dirs: Vec<PathBuf>) -> Result<AdminUi> {
        let mut env = Environment::new();
        let mut overrides = Vec::new();
        // Using a variable that does not exist is an error, not an empty string.
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        for (name, source) in TEMPLATES {
            match dirs.iter().map(|d| d.join(name)).find(|p| p.is_file()) {
                Some(path) => {
                    let text = std::fs::read_to_string(&path).map_err(|e| Error::Template(format!("{}: {e}", path.display())))?;
                    env.add_template_owned((*name).to_owned(), text)
                        .map_err(|e| Error::Template(format!("{}: {e}", path.display())))?;
                    overrides.push(path);
                }
                None => env.add_template(name, source).map_err(template_error)?,
            }
        }
        // `t("key")` looks the text up in the language of the page being rendered.
        env.add_function("t", |state: &State, key: String| -> String {
            let lang = match state.lookup("lang").as_ref().and_then(|v| v.as_str()) {
                Some("de") => Lang::De,
                _ => Lang::En,
            };
            Catalog::new(lang).t(&key).to_owned()
        });
        // Hidden features are off in the templates until `with_features` switches them on; a strict environment needs the name defined.
        env.add_global("downloads", false);
        Ok(AdminUi { env, dirs, overrides })
    }

    /// Tells the templates which hidden features are on (`features.rs`). Without this call none is: the pages render as they always did.
    pub fn with_features(mut self, features: crate::features::Features) -> AdminUi {
        self.env.add_global("downloads", features.download);
        self
    }

    /// The stylesheet or another static file of a distribution, if a folder has one.
    pub fn static_file(&self, name: &str) -> Option<Vec<u8>> {
        self.dirs.iter().find_map(|d| std::fs::read(d.join("static").join(name)).ok())
    }

    fn render(&self, template: &str, ctx: minijinja::Value) -> Result<String> {
        self.env
            .get_template(template)
            .and_then(|t| t.render(ctx))
            .map_err(template_error)
    }

    /// The sign-in page.
    pub fn login<'a>(&self, view: impl Into<View<'a>>) -> Result<String> {
        self.render("login.html", view.into().context())
    }

    /// A message on its own: what a procedure answers when something went wrong.
    pub fn message_fragment<'a>(&self, view: impl Into<View<'a>>) -> Result<String> {
        self.render("fragments/message.html", view.into().context())
    }

    /// The page of a user about themselves: their own dashboard (role, devices,
    /// what they are listening to, their hours), the picture and the PIN.
    pub async fn profile<'a>(&self, store: &Store, view: impl Into<View<'a>>, user_id: i64) -> Result<String> {
        let dash = personal_dashboard(store, user_id).await?;
        self.render(
            "profile.html",
            context! { dash => Serde(dash), max_kib => crate::profile::MAX_AVATAR_BYTES / 1024, ..view.into().context() },
        )
    }

    pub async fn overview<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let data = store.overview().await?;
        let (open_codes, open_until) = store.open_enrollments().await?;
        let zone = store.setting("time_zone").await?;
        let open_until = open_until.map(|ms| stamp(ms, zone.as_deref())).unwrap_or_default();
        self.render("overview.html", context! { data => Serde(data), open_codes => open_codes, open_until => open_until, access => crate::access::current(store).await.map(Serde), ..view.into().context() })
    }

    pub async fn users<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let users = store.user_rows().await?;
        let folders = store.all_profile_folders().await?;
        self.render("users.html", context! { users => Serde(users), folders => Serde(folders), ..view.into().context() })
    }

    /// The whole list of users, without the page around it. This is what a
    /// procedure answers when the list has to be replaced.
    pub async fn user_list_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let users = store.user_rows().await?;
        let folders = store.all_profile_folders().await?;
        self.render("fragments/user_list.html", context! { users => Serde(users), folders => Serde(folders), ..view.into().context() })
    }

    /// One row of the list, for replacing a single user after a change. `None` if
    /// the user does not exist (any more).
    pub async fn user_row_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, id: i64) -> Result<Option<String>> {
        let Some(user) = store.user_row(id).await? else { return Ok(None) };
        let folders = store.all_profile_folders().await?;
        self.render("fragments/user_row.html", context! { user => Serde(user), folders => Serde(folders), ..view.into().context() }).map(Some)
    }

    /// The list of albums of one kind: `audiobook` or `music`.
    #[allow(clippy::too_many_arguments)]
    pub async fn books<'a>(&self, store: &Store, view: impl Into<View<'a>>, library: Option<&std::path::Path>, kind: &str, query: Option<&str>, only_problems: bool, label: Option<&str>, page: Option<usize>, per: Option<usize>) -> Result<String> {
        let all: Vec<_> = store.admin_book_rows().await?.into_iter().filter(|b| b.kind == kind).collect();
        // The completions: every title, author and narrator once, sorted.
        let mut words: Vec<String> = all.iter().flat_map(|b| [Some(b.title.clone()), Some(b.author.clone()), b.narrator.clone()]).flatten().filter(|w| !w.trim().is_empty()).collect();
        words.sort_by_key(|w| w.to_lowercase());
        words.dedup();
        // Books that look like a copy of another one: id -> the others, for a badge in the row.
        let mut dupes: std::collections::HashMap<String, Vec<crate::dupes::DupeBook>> = std::collections::HashMap::new();
        let rules = store.ignore_rules().await?;
        for group in store.duplicate_groups().await?.into_iter().filter(|g| g.kind != "parts") {
            for b in &group.books {
                dupes.insert(b.id.to_string(), group.books.iter().filter(|o| o.id != b.id).cloned().collect());
            }
        }
        // Books with missing tracks (a gap in the numbers of a CD): a red mark in the row, and a filter. A rule to ignore it hides the mark.
        let mut gaps = store.track_gaps().await?;
        let by_id: std::collections::HashMap<i64, &crate::store::AdminBookRow> = all.iter().map(|b| (b.id, b)).collect();
        gaps.retain(|id, _| by_id.get(id).is_some_and(|row| !crate::ignore::ignored(&rules, row, "track_number_gaps")));
        // Books without a cover (neither a picture file nor a picture in the first file).
        let ids: Vec<i64> = all.iter().filter(|b| !crate::ignore::ignored(&rules, b, "missing_cover")).map(|b| b.id).collect();
        let nocover = missing_covers(store, library, &ids).await;
        let problem_count = all.iter().filter(|b| gaps.contains_key(&b.id) || nocover.contains(&b.id)).count();
        // The search: words, wildcards (`*`, `?`) or a regular expression between slashes (see `search.rs`). A broken expression finds nothing and says why.
        let needle = query.map(str::trim).filter(|q| !q.is_empty());
        let (matcher, search_error) = match crate::search::compile(needle.unwrap_or("")) {
            Ok(m) => (m, None),
            Err(e) => (crate::search::Matcher::Contains("\u{0}".to_owned()), Some(e)),
        };
        let all: Vec<_> = if only_problems { all.into_iter().filter(|b| gaps.contains_key(&b.id) || nocover.contains(&b.id)).collect() } else { all };
        let nocover: Vec<String> = nocover.iter().map(i64::to_string).collect();
        let books: Vec<_> = match needle {
            Some(_) => all.into_iter().filter(|b| matcher.matches_book(&b.title, &b.author, b.narrator.as_deref()) || matcher.matches_barcode(b.barcode.as_deref())).collect(),
            None => all,
        };
        // Labels: only the books with the chosen one, the names for the filter and the labels of every book for the rows.
        let label_map = store.book_labels_with_mark().await?;
        // A book is found by the labels of its titles as well.
        let mut found_by = label_map.clone();
        for (id, names) in store.track_labels_by_book().await? {
            found_by.entry(id).or_default().extend(names);
        }
        let books: Vec<_> = match label {
            // Several labels (comma separated; a label has no comma): the books with any of them.
            Some(l) => books.into_iter().filter(|b| found_by.get(&b.id).is_some_and(|names| names.iter().any(|n| l.split(',').any(|c| n.eq_ignore_ascii_case(c.trim()))))).collect(),
            None => books,
        };
        let label_texts: std::collections::HashMap<String, Vec<String>> = label_map.iter().map(|(id, names)| (id.to_string(), names.clone())).collect();
        // The names for the filter with the number of books (the book or one of its titles carries the label).
        let mut label_names = store.labels_with_mark().await?;
        for row in &mut label_names {
            row.books = found_by.values().filter(|names| names.iter().any(|n| n.eq_ignore_ascii_case(&row.name))).count() as i64;
        }
        // The matrix: a column for every person (administrators see everything), a mark where the person sees the book, and the owner box.
        let (members, seen) = visibility_columns(store, None).await?;
        let mode = store.setting(if kind == "music" { "tree_music" } else { "tree_audiobook" }).await?.filter(|m| crate::tree::valid(m)).unwrap_or_else(|| (if kind == "music" { "folder" } else { "author" }).to_owned());
        // Pages: the books of the list (after the search) in pages of `per`; the tree is built from all books and cut into the pages.
        let default_per = store.setting("list_page_size").await?.and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(50);
        let per = per.unwrap_or(default_per).clamp(10, 500);
        let total = books.len();
        let pages = total.div_ceil(per).max(1);
        let page = page.unwrap_or(1).clamp(1, pages);
        let (from, to) = ((page - 1) * per, (page * per).min(total));
        // The tree is built from ALL books and then cut into pages, so a group (an author, a folder) is whole whichever page shows it.
        let arranged = crate::tree::build(books.clone(), &mode);
        let items = crate::tree::page(arranged, from, per);
        let books: Vec<_> = items.iter().filter_map(|i| if let crate::tree::Item::Book { book, .. } = i { Some((**book).clone()) } else { None }).collect();
        let base = {
            let mut pairs: Vec<(&str, String)> = Vec::new();
            if let Some(q) = needle {
                pairs.push(("q", q.to_owned()));
            }
            if only_problems {
                pairs.push(("problems", "1".to_owned()));
            }
            if let Some(l) = label {
                pairs.push(("label", l.to_owned()));
            }
            if per != default_per {
                pairs.push(("per", per.to_string()));
            }
            serde_urlencoded::to_string(pairs).unwrap_or_default()
        };
        // The numbers of the pages to link: the first, the last and a few around the current one.
        let shown: Vec<usize> = (1..=pages).filter(|n| *n == 1 || *n == pages || n.abs_diff(page) <= 2).collect();
        let numbers: Vec<serde_json::Value> = shown.iter().enumerate().map(|(i, n)| serde_json::json!({ "n": n, "gap": i > 0 && n - shown[i - 1] > 1 })).collect();
        let pager = serde_json::json!({ "page": page, "pages": pages, "per": per, "total": total, "from": if total == 0 { 0 } else { from + 1 }, "to": to, "base": base, "numbers": numbers, "prev": page.saturating_sub(1).max(1), "next": (page + 1).min(pages), "default_per": default_per, "sizes": [10, 25, 50, 100, 250, 500], "q": needle.unwrap_or(""), "problems": only_problems, "label": label.unwrap_or("") });
        self.render("books.html", context! { books => Serde(books), items => Serde(items), tree => mode != "flat", kind => kind, words => words, q => query.unwrap_or_default(), searched => needle.is_some() || only_problems || label.is_some(), label => label.unwrap_or_default(), label_selected => label.map(|l| l.split(',').map(|c| c.trim().to_lowercase()).collect::<Vec<_>>()).unwrap_or_default(), label_names => Serde(label_names), label_texts => Serde(label_texts), search_error => search_error, only_problems => only_problems, problem_count => problem_count, pager => Serde(pager), members => Serde(members), seen => Serde(seen), owners => Serde(owners_list(store).await?), gaps => Serde(gaps), nocover => Serde(nocover), dupes => Serde(dupes), ignores => Serde(rules), access => crate::access::current(store).await.map(Serde), ..view.into().context() })
    }

    /// The titles of one album, for the list that opens under it.
    pub async fn book_tracks_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, id: i64) -> Result<String> {
        #[derive(serde::Serialize)]
        struct Line {
            id: i64,
            disc: i64,
            number: i64,
            title: String,
            length: String,
            /// The labels of its own.
            labels: Vec<String>,
            /// The file format (`mp3`, `ogg`, ...); empty until an import has looked at the file; `ios_ok` is false for one an iPhone cannot play.
            format: String,
            ios_ok: bool,
        }
        let own = store.track_labels().await?;
        let lines: Vec<Line> = store
            .tracks_of(id)
            .await?
            .into_iter()
            .map(|t| {
                let s = t.duration_ms / 1000;
                Line {
                    id: t.id,
                    disc: t.disc,
                    number: t.number,
                    title: t.title,
                    length: if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) },
                    labels: own.get(&t.id).cloned().unwrap_or_default(),
                    ios_ok: crate::audioformat::ios_playable(&t.format),
                    format: t.format,
                }
            })
            .collect();
        self.render("fragments/book_tracks.html", context! { tracks => Serde(lines), book_id => id, ..view.into().context() })
    }

    /// The titles of an album with an entry of labels for each (in the tab "Edit"); the form of the window sends them with its Save.
    pub async fn book_track_labels_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, id: i64) -> Result<String> {
        #[derive(serde::Serialize)]
        struct Line {
            id: i64,
            disc: i64,
            number: i64,
            title: String,
            labels: String,
        }
        let own = store.track_labels().await?;
        let lines: Vec<Line> = store
            .tracks_of(id)
            .await?
            .into_iter()
            .map(|t| Line { id: t.id, disc: t.disc, number: t.number, title: t.title, labels: own.get(&t.id).map(|l| l.join(", ")).unwrap_or_default() })
            .collect();
        self.render("fragments/book_track_labels.html", context! { tracks => Serde(lines), ..view.into().context() })
    }

    /// The tab "Filing" of a book: the complete place on the disk and what the database knows (also the fields the triggers fill in).
    pub async fn book_filing_fragment<'a>(&self, store: &Store, library: Option<&std::path::Path>, view: impl Into<View<'a>>, id: i64) -> Result<Option<String>> {
        let Some(book) = store.admin_book_row(id).await? else { return Ok(None) };
        let Some(meta) = store.book_meta(id).await? else { return Ok(None) };
        let zone = store.setting("time_zone").await?;
        let when = |ms: i64| if ms > 0 { stamp(ms, zone.as_deref()) } else { String::new() };
        let place = crate::relocate::book_dir(store, library, id).await.map(|(dir, _, _)| dir.display().to_string());
        let megabytes = format!("{:.1}", meta.files_bytes as f64 / 1_048_576.0);
        // The file formats of the tracks, one line per distinct combination with the number of tracks (`ogg/vorbis 44.1 kHz stereo`).
        let mut combos: std::collections::BTreeMap<(String, String, i64, i64), i64> = std::collections::BTreeMap::new();
        for t in store.tracks_of(id).await? {
            *combos.entry((t.format, t.codec, t.sample_rate, t.channels)).or_default() += 1;
        }
        let formats: Vec<serde_json::Value> = combos
            .into_iter()
            .map(|((format, codec, rate, channels), count)| {
                serde_json::json!({ "line": crate::audioformat::describe_line(&format, &codec, rate, channels), "count": count, "known": !format.is_empty(), "ios_ok": crate::audioformat::ios_playable(&format) })
            })
            .collect();
        let info = serde_json::json!({
            "place": place,
            "added": when(meta.added_at),
            "released": meta.released_at.map(when).unwrap_or_default(),
            "created": when(meta.created_at),
            "created_by": meta.created_by,
            "updated": when(meta.updated_at),
            "updated_by": meta.updated_by,
            "tracks_updated": meta.tracks_updated_at.map(when).unwrap_or_default(),
            "megabytes": megabytes,
            "genre_tag": meta.genre_tag,
            "formats": formats,
        });
        self.render("fragments/book_filing.html", context! { book => Serde(book), info => Serde(info), ..view.into().context() }).map(Some)
    }

    pub async fn book_row_fragment<'a>(&self, store: &Store, library: Option<&std::path::Path>, view: impl Into<View<'a>>, id: i64) -> Result<Option<String>> {
        let Some(book) = store.admin_book_row(id).await? else { return Ok(None) };
        // The red mark of a book with missing tracks stays when the row is drawn again.
        let rules = store.ignore_rules().await?;
        let mut gaps = store.track_gaps().await?;
        gaps.retain(|b, _| *b == id && !crate::ignore::ignored(&rules, &book, "track_number_gaps"));
        let nocover: Vec<String> = if crate::ignore::ignored(&rules, &book, "missing_cover") || crate::api::find_cover(store, library, id).await.is_some() { Vec::new() } else { vec![id.to_string()] };
        let (members, seen) = visibility_columns(store, Some(id)).await?;
        self.render("fragments/book_row.html", context! { book => Serde(book), gaps => Serde(gaps), nocover => Serde(nocover), members => Serde(members), seen => Serde(seen), owners => Serde(owners_list(store).await?), label_list => Serde(store.book_labels_with_mark().await?.remove(&id).unwrap_or_default()), label_names => Serde(store.labels_with_mark().await?), ..view.into().context() }).map(Some)
    }

    /// The upload queue of the import page: the newest jobs, with a timer while one is still open.
    pub async fn upload_queue_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let jobs = store.upload_jobs(15).await?;
        let busy = jobs.iter().any(|j| j.state == "queued" || j.state == "working");
        self.render("fragments/upload_queue.html", context! { jobs => Serde(jobs), busy => busy, ..view.into().context() })
    }

    /// The content of the tags window of an album, filled in from its first file.
    pub fn book_tags_fragment<'a>(&self, view: impl Into<View<'a>>, id: i64, kind: &str, tags: &crate::tags::AlbumTags) -> Result<String> {
        self.render(
            "fragments/book_tags.html",
            context! {
                id => id,
                kind => kind,
                narrator => tags.narrator.clone().unwrap_or_default(),
                album => tags.album.clone().unwrap_or_default(),
                artist => tags.artist.clone().unwrap_or_default(),
                year => tags.year.clone().unwrap_or_default(),
                genre => tags.genre.clone().unwrap_or_default(),
                disc => tags.disc,
                discs => tags.discs,
                catalog => serde_json::to_string(&crate::tags::field_catalog().iter().map(|(n, k)| serde_json::json!({ "name": n, "kind": k })).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".to_owned()),
                ..view.into().context()
            },
        )
    }

    pub async fn settings<'a>(&self, store: &Store, cd: &crate::cdimport::CdService, view: impl Into<View<'a>>, tab: Option<&str>, mcp: Option<&str>) -> Result<String> {
        let tab = tab.filter(|t| matches!(*t, "roles" | "values" | "llm" | "cdimport")).unwrap_or("import");
        let llm = if tab == "llm" { self.llm_context(store, mcp, None).await? } else { self.llm_context(store, None, None).await.unwrap_or_default() };
        let policies = self.policy_views(store).await?;
        let settings = self.setting_views(store).await?;
        let state = store.import_state().await?;
        // The tab holds the values of the CD import; the import itself (drive, choice, progress, log) is the page "CD import".
        let cd = self.cd_context(store, cd, false, &HashMap::new(), 0).await?;
        let ctx = context! { tab => tab, saved => false, zones => Self::zone_names(), kinds => Self::root_kinds(&state), state => Serde(state), policies => Serde(policies), settings => Serde(settings), ..view.into().context() };
        let ctx = context! { ..ctx, ..llm, ..cd };
        self.render("settings.html", ctx)
    }

    /// What the tab "LLM access" shows: the endpoint (if the server runs with `--mcp`), the
    /// tokens, the newest tag changes, and a token that was just made (shown only once).
    async fn llm_context(&self, store: &Store, endpoint: Option<&str>, new_token: Option<&str>) -> Result<minijinja::Value> {
        let zone = store.setting("time_zone").await?;
        let tokens: Vec<_> = store
            .mcp_tokens()
            .await?
            .into_iter()
            .map(|t| context! { id => t.id, name => t.name, can_write => t.can_write, created => stamp(t.created_at, zone.as_deref()), last_used => t.last_used_at.map(|ms| stamp(ms, zone.as_deref())) })
            .collect();
        let changes: Vec<_> = store
            .tag_changes(0, 20)
            .await?
            .into_iter()
            .map(|c| context! { at => stamp(c.at, zone.as_deref()), token => c.token_name, book_id => c.book_id, file => c.file, field => c.field, old => c.old_value, new => c.new_value })
            .collect();
        // The entry for a client's MCP configuration (the form `claude mcp add-json` takes).
        let mcp_json = endpoint.zip(new_token).map(|(url, token)| {
            serde_json::to_string_pretty(&serde_json::json!({ "type": "http", "url": url, "headers": { "Authorization": format!("Bearer {token}") } }))
                .unwrap_or_default()
        });
        Ok(context! { mcp_endpoint => endpoint, mcp_tokens => tokens, mcp_changes => changes, new_token => new_token, mcp_json => mcp_json, mcp_prompt => store.mcp_prompt().await?.unwrap_or_else(|| crate::mcp::DEFAULT_PROMPT.to_owned()) })
    }

    /// The whole settings page on the tab "LLM access", with a token that was just made.
    pub async fn settings_with_token<'a>(&self, store: &Store, view: impl Into<View<'a>>, endpoint: Option<&str>, token: &str) -> Result<String> {
        let policies = self.policy_views(store).await?;
        let settings = self.setting_views(store).await?;
        let state = store.import_state().await?;
        let llm = self.llm_context(store, endpoint, Some(token)).await?;
        let ctx = context! { tab => "llm", saved => false, zones => Self::zone_names(), kinds => Self::root_kinds(&state), state => Serde(state), policies => Serde(policies), settings => Serde(settings), ..view.into().context() };
        let ctx = context! { ..ctx, ..llm };
        self.render("settings.html", ctx)
    }

    /// The tab "LLM access" alone, as the answer to making or revoking a token.
    pub async fn settings_llm_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, endpoint: Option<&str>, new_token: Option<&str>) -> Result<String> {
        let llm = self.llm_context(store, endpoint, new_token).await?;
        self.render("fragments/settings_llm.html", context! { ..llm, ..view.into().context() })
    }

    /// The warning about missing rights, alone, as the answer to "check again".
    pub async fn access_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let report = crate::access::refresh(store).await;
        self.render("fragments/access_warning.html", context! { access => Serde(report), ..view.into().context() })
    }

    /// What children asked for (podcasts, stations) and what waits for a release.
    pub async fn releases<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        self.render("releases.html", context! { pending => Serde(store.pending_releases().await?), ..view.into().context() })
    }

    /// The genres: the list that the apps show, and the tag texts that have no genre yet.
    pub async fn genres<'a>(&self, store: &Store, view: impl Into<View<'a>>, filter: Option<&str>) -> Result<String> {
        let needle = filter.map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty());
        let loose: Vec<_> = store.loose_genre_tags().await?.into_iter().filter(|t| needle.as_ref().is_none_or(|n| t.text.to_lowercase().contains(n))).collect();
        self.render(
            "genres.html",
            context! {
                genres => Serde(store.genres().await?),
                loose => Serde(loose),
                filter => filter.unwrap_or_default(),
                // The names of the ID3 list: what a tag text is given.
                standard => lofty::id3::v1::GENRES.to_vec(),
                // The names of the ID3 list, offered when a genre is named.
                standard => lofty::id3::v1::GENRES.to_vec(),
                without => store.albums_without_genre().await?,
                ..view.into().context()
            },
        )
    }

    /// The pairing page: the form for a new code, and the devices.
    ///
    /// `result` is the fragment of a code that was just made. A browser without
    /// JavaScript gets it inside the page, because the code is shown only once.
    /// The codes of the last days, what happened with them in the last quarter of an hour, and how many are open.
    async fn pairing_codes_data(&self, store: &Store) -> Result<(Vec<CodeView>, Vec<CodeEvent>, usize)> {
        let zone = store.setting("time_zone").await?;
        let now = crate::store::now_ms();
        let rows = store.enrollment_rows().await?;
        let mut events = Vec::new();
        let mut open = 0;
        let mut codes = Vec::new();
        for r in rows {
            let state = r.state(now);
            if state == crate::pairing::CodeState::Open {
                open += 1;
            }
            match (state, r.used_at) {
                (crate::pairing::CodeState::Used, Some(at)) if now - at <= EVENT_WINDOW_MS => {
                    events.push(CodeEvent { kind: "paired", device: r.device_name.clone().unwrap_or_default(), user_name: r.user_name.clone(), minutes: ((now - at) / 60_000).max(0) });
                }
                (crate::pairing::CodeState::Expired, _) if now - r.expires_at <= EVENT_WINDOW_MS => {
                    events.push(CodeEvent { kind: "expired", device: String::new(), user_name: r.user_name.clone(), minutes: ((now - r.expires_at) / 60_000).max(0) });
                }
                _ => {}
            }
            codes.push(CodeView {
                id: r.id,
                user_name: r.user_name,
                created_by: r.created_by_name.unwrap_or_default(),
                shared: r.shared,
                created: stamp(r.created_at, zone.as_deref()),
                until: stamp(r.expires_at, zone.as_deref()),
                state: match state {
                    crate::pairing::CodeState::Open => "open",
                    crate::pairing::CodeState::Used => "used",
                    crate::pairing::CodeState::Revoked => "revoked",
                    crate::pairing::CodeState::Expired => "expired",
                },
                device: r.device_name.unwrap_or_default(),
                used: r.used_at.map(|ms| stamp(ms, zone.as_deref())).unwrap_or_default(),
            });
        }
        events.sort_by_key(|e| e.minutes);
        events.truncate(5);
        Ok((codes, events, open))
    }

    /// The section "Pairing codes" without the page around it: what the page polls and what the buttons answer.
    pub async fn pairing_codes_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let (codes, events, open) = self.pairing_codes_data(store).await?;
        self.render("fragments/pairing_codes.html", context! { codes => Serde(codes), events => Serde(events), open_codes => open, ..view.into().context() })
    }

    /// The small number at the menu item "Pair a device": the open codes; nothing (hidden) when there are none.
    pub async fn pairing_badge_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let (count, _) = store.open_enrollments().await?;
        self.render("fragments/pairing_badge.html", context! { count => count, ..view.into().context() })
    }

    pub async fn pairing<'a>(
        &self,
        store: &Store,
        view: impl Into<View<'a>>,
        default_address: &str,
        result: Option<String>,
        tls: bool,
    ) -> Result<String> {
        let users = store.user_rows().await?;
        let zone = store.setting("time_zone").await?;
        let devices: Vec<DeviceView> = store.device_rows().await?.into_iter().map(|d| device_view(d, zone.as_deref())).collect();
        let (codes, events, open_codes) = self.pairing_codes_data(store).await?;
        self.render(
            "pairing.html",
            context! {
                users => Serde(users),
                devices => Serde(devices),
                codes => Serde(codes),
                events => Serde(events),
                open_codes => open_codes,
                address => default_address,
                // Only an address a device cannot reach (127.0.0.1) needs the warning.
                loopback => default_address.parse::<std::net::SocketAddr>().is_ok_and(|a| a.ip().is_loopback()),
                tls => tls,
                // Our own fragment, already escaped where it needs to be.
                result => minijinja::Value::from_safe_string(result.unwrap_or_default()),
                ..view.into().context()
            },
        )
    }

    /// The QR code and the text of a freshly made pairing code. This is shown once;
    /// the code itself is not stored.
    pub fn pairing_result_fragment<'a>(
        &self,
        view: impl Into<View<'a>>,
        user_name: &str,
        uri: &str,
        minutes: i64,
        in_window: bool,
    ) -> Result<String> {
        let svg = crate::pairing::qr_svg(uri)?;
        self.render(
            "fragments/pairing_result.html",
            context! {
                user_name => user_name,
                uri => uri,
                minutes => minutes,
                // In the pairing window the code replaces the form, and the foot of the window becomes "Close".
                in_window => in_window,
                // Our own renderer made this markup from the address, so it is not escaped again.
                qr => minijinja::Value::from_safe_string(svg),
                ..view.into().context()
            },
        )
    }

    pub async fn device_row_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, id: i64) -> Result<Option<String>> {
        let Some(device) = store.device_row(id).await? else { return Ok(None) };
        let zone = store.setting("time_zone").await?;
        self.render("fragments/device_row.html", context! { device => Serde(device_view(device, zone.as_deref())), ..view.into().context() }).map(Some)
    }

    /// The weekly planner of one user. `None` if the user does not exist.
    pub async fn schedule<'a>(&self, store: &Store, view: impl Into<View<'a>>, user: i64) -> Result<Option<String>> {
        let Some(p) = store.profile_by_id(user).await? else { return Ok(None) };
        let planner = p.schedule.as_deref().and_then(Schedule::from_bytes);
        let data = schedule_view(p.id, p.name, p.role, planner);
        self.render("schedule.html", context! { s => Serde(data), ..view.into().context() }).map(Some)
    }

    /// The planner panel alone: what a procedure answers after a change.
    pub async fn schedule_panel_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, user: i64) -> Result<Option<String>> {
        let Some(p) = store.profile_by_id(user).await? else { return Ok(None) };
        let planner = p.schedule.as_deref().and_then(Schedule::from_bytes);
        let data = schedule_view(p.id, p.name, p.role, planner);
        self.render("fragments/schedule_panel.html", context! { s => Serde(data), ..view.into().context() }).map(Some)
    }

    /// The page "CD import" (a menu item of its own): what keeps the import from working, the drive of the server with the choice of tracks and the
    /// progress, and the log. The values (format, names, tags) stay in the settings, tab "CD import".
    pub async fn cd_page<'a>(&self, store: &Store, cd: &crate::cdimport::CdService, view: impl Into<View<'a>>) -> Result<String> {
        let view: View<'a> = view.into();
        let viewer = view.me.map_or(0, |m| m.id);
        let cd = self.cd_context(store, cd, true, &HashMap::new(), viewer).await?;
        self.render("cd.html", context! { ..view.context(), ..cd })
    }

    /// The import page.
    pub async fn import<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let state = store.import_state().await?;
        let zone = store.setting("time_zone").await?;
        let last = store.last_import().await;
        let jobs = store.upload_jobs(15).await?;
        let busy = jobs.iter().any(|j| j.state == "queued" || j.state == "working");
        self.render(
            "import.html",
            context! {
                state => Serde(state),
                access => crate::access::current(store).await.map(Serde),
                report => Option::<Serde<ScanReport>>::None,
                last_report => last.as_ref().map(|(_, r)| Serde(r.clone())),
                last_at => last.as_ref().map(|(at, _)| stamp(*at, zone.as_deref())),
                jobs => Serde(jobs),
                busy => busy,
                ..view.into().context()
            },
        )
    }

    /// The import panel alone, with the result of an import if one just ran.
    pub async fn import_panel_fragment<'a>(
        &self,
        store: &Store,
        view: impl Into<View<'a>>,
        report: Option<ScanReport>,
    ) -> Result<String> {
        let state = store.import_state().await?;
        self.render(
            "fragments/import_panel.html",
            context! {
                state => Serde(state),
                report => report.map(Serde),
                last_report => Option::<Serde<ScanReport>>::None,
                last_at => Option::<String>::None,
                ..view.into().context()
            },
        )
    }

    /// The import tab of the settings alone: folders, rule and the lock.
    pub async fn import_settings_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let state = store.import_state().await?;
        let kinds = Self::root_kinds(&state);
        self.render("fragments/import_settings.html", context! { state => Serde(state), kinds => kinds, ..view.into().context() })
    }

    /// The four kinds of medium with their folders and what they leave out, in the order the settings page shows them.
    fn root_kinds(state: &crate::importer::ImportState) -> Serde<Vec<serde_json::Value>> {
        let kinds = [
            ("audiobook", "folder", &state.roots, &state.excludes),
            ("music", "music", &state.music_roots, &state.music_excludes),
            ("photo", "media", &state.photo_roots, &state.photo_excludes),
            ("ebook", "books", &state.ebook_roots, &state.ebook_excludes),
        ];
        Serde(kinds.iter().map(|(kind, icon, roots, excludes)| serde_json::json!({ "kind": kind, "icon": icon, "roots": roots, "excludes": excludes })).collect())
    }

    /// One empty row of folders for the dialog of the import settings.
    pub async fn import_row_fragment<'a>(&self, view: impl Into<View<'a>>, kind: &str, n: i64) -> Result<String> {
        self.render("fragments/root_row.html", context! { kind => kind, n => n, path => "", excluded => false, ..view.into().context() })
    }

    /// The form with the plain values of the settings page.
    pub async fn settings_values_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, saved: bool) -> Result<String> {
        let settings = self.setting_views(store).await?;
        let tls_problem = Self::tls_problem(store).await;
        self.render("fragments/settings_values.html", context! { settings => Serde(settings), saved => saved, zones => Self::zone_names(), tls_problem => tls_problem, ..view.into().context() })
    }

    /// The tab "CD import" alone. `refused` holds the values that were refused, by key, with the code of the
    /// reason; they stay in the fields and the reasons are shown next to them and at the top.
    pub async fn settings_cd_fragment<'a>(&self, store: &Store, cd: &crate::cdimport::CdService, view: impl Into<View<'a>>, saved: bool, refused: &[(String, String, &'static str)]) -> Result<String> {
        let mut settings = self.setting_views(store).await?;
        let view: View<'a> = view.into();
        let lang = view.lang;
        let mut errors: HashMap<String, String> = HashMap::new();
        for (key, value, code) in refused {
            errors.insert(key.clone(), verr_text(lang, key, value, code));
            if let Some(v) = settings.iter_mut().find(|v| v.key == *key) {
                v.value = value.clone();
                v.shown = value.clone();
            }
        }
        let cd = self.cd_context(store, cd, false, &errors, 0).await?;
        let ctx = context! { settings => Serde(settings), saved => saved, ..view.context() };
        self.render("fragments/settings_cdimport.html", context! { ..ctx, ..cd })
    }

    /// What the tab "CD import" shows besides the values: what keeps the import from working, the last entries
    /// of the log, and the reasons a save was refused.
    async fn cd_context(&self, store: &Store, cd: &crate::cdimport::CdService, wanted: bool, errors: &HashMap<String, String>, viewer: i64) -> Result<minijinja::Value> {
        let (problems, jobs) = if wanted {
            let zone = store.setting("time_zone").await?;
            let keep = crate::cd_settings::CdSettings::load(store).await?.log_keep;
            let jobs: Vec<CdJobView> = store
                .cd_log(keep)
                .await?
                .into_iter()
                .map(|j| CdJobView {
                    when: stamp(j.updated_ms, zone.as_deref()),
                    error_key: (!j.error.is_empty()).then(|| crate::cdimport::error_key(&j.error)),
                    locked: j.phase == "done" && !j.import_ran,
                    auto: j.device == "auto",
                    message: j.message,
                    book: j.album,
                    author: j.author,
                    disc: j.disc,
                    discs_total: j.discs_total,
                    phase: j.phase,
                    cd_state: j.cd_state,
                    decided_by: j.decided_by,
                    reason: j.reason,
                    error: j.error,
                })
                .collect();
            (crate::cdimport::readiness(store, cd, None).await, jobs)
        } else {
            (Vec::new(), Vec::new())
        };
        let drive = if wanted { Some(self.cd_drive_panel(store, cd, &PanelQuery::default().with_viewer(viewer), None, &[]).await?) } else { None };
        Ok(context! { cd_problems => Serde(problems), cd_jobs => Serde(jobs), errors => Serde(errors.clone()), drive => Serde(drive) })
    }

    /// The panel with the drive of the server: its state, the releases MusicBrainz offers for the disc, the list of
    /// tracks with the choice, the form to start the import, the jobs under way. Returns `None` when `sig` is the
    /// signature of what would be shown (nothing changed: the page keeps what it has, with the choice of tracks and
    /// the typed text).
    pub async fn cd_drive_fragment<'a>(&self, store: &Store, cd: &crate::cdimport::CdService, view: impl Into<View<'a>>, q: &PanelQuery, sig: Option<&str>, refused: Option<(&str, &[u32])>) -> Result<Option<String>> {
        let (rip_error, taken) = refused.map_or((None, &[][..]), |(code, taken)| (Some(code), taken));
        let panel = self.cd_drive_panel(store, cd, q, rip_error, taken).await?;
        if rip_error.is_none() && sig == Some(panel.sig.as_str()) {
            return Ok(None);
        }
        let ctx = context! { drive => Serde(panel) };
        Ok(Some(self.render("fragments/cd_drive.html", context! { ..ctx, ..view.into().context() })?))
    }

    async fn cd_drive_panel(&self, store: &Store, cd: &crate::cdimport::CdService, q: &PanelQuery, rip_error: Option<&str>, taken: &[u32]) -> Result<DrivePanel> {
        use crate::cdrive::{Medium, Tray};
        let set = crate::cd_settings::CdSettings::load(store).await?;
        let (drives, mode) = cd.drives.snapshot();
        let pick = q.pick;
        let live: Option<crate::cdrive::Live> = drives.iter().find_map(|d| cd.drives.live_of_device(&d.device));
        let log = store.cd_log(50).await?;
        let all_active: Vec<crate::cdimport::Job> = log.iter().filter(|j| j.active).cloned().collect();
        // The jobs of a rip on this server are shown by the list of tracks, the others (an upload under way) as a line.
        let jobs: Vec<ActiveJob> = all_active
            .iter()
            .filter(|j| live.as_ref().is_none_or(|l| l.job_id != j.id))
            .map(|j| ActiveJob { id: j.id, book: j.album.clone(), author: j.author.clone(), disc: j.disc, discs_total: j.discs_total, phase: j.phase.clone(), track: j.track, tracks_total: j.tracks_total, percent: (j.fraction.unwrap_or(0.0) * 100.0).round() as i64, by_server: j.device == "server" })
            .collect();
        let rights = store.cd_rights(q.viewer).await.unwrap_or_default();
        let zone_for_waiting = store.setting("time_zone").await?;
        let waiting: Vec<WaitingView> = store
            .cd_jobs_waiting()
            .await?
            .into_iter()
            // Somebody who may approve sees every CD that waits; the starter sees their own (without the buttons for a decision).
            .filter(|j| rights.may_approve || (j.user_id == q.viewer && q.viewer != 0))
            .map(|j| WaitingView {
                can_decide: rights.may_approve && j.cd_state == "awaiting_approval",
                can_retry: j.cd_state == "retry_wait" && (rights.may_approve || (j.user_id == q.viewer && q.viewer != 0)),
                since: stamp(j.held_ms.unwrap_or(j.updated_ms), zone_for_waiting.as_deref()),
                error_key: (!j.error.is_empty()).then(|| crate::cdimport::error_key(&j.error)),
                id: j.id,
                state: j.cd_state,
                book: j.album,
                author: j.author,
                disc: j.disc,
                discs_total: j.discs_total,
                tracks: j.tracks_total,
                starter: j.starter,
                error: j.error,
            })
            .collect();
        let rip = match &live {
            Some(l) => {
                let job = log.iter().find(|j| j.id == l.job_id).cloned();
                let active = job.as_ref().is_some_and(|j| j.active) && !l.finished;
                let mmss = |s: u32| format!("{}:{:02}", s / 60, s % 60);
                let count = |st: &str| l.tracks.iter().filter(|t| t.state == st).count();

                let job_phase = job.as_ref().map(|j| j.phase.as_str()).unwrap_or("ripping");
                let phase = match job_phase {
                    "placing" => "placing",
                    "importing" => "importing",
                    _ if count("verifying") + count("rereading") > 0 => "verifying",
                    _ if count("reading") > 0 => "ripping",
                    _ if l.verify && (count("waiting") > 0 || count("read") > 0) => "verifying",
                    _ => "encoding",
                };
                let error = l.error.clone();
                Some(RipPanel {
                    device: l.device.clone(),
                    active,
                    percent: if l.finished && error.is_empty() { 100 } else { (l.fraction.clamp(0.0, 1.0) * 100.0).round() as i64 },
                    current: l.current_position(),
                    total: l.tracks.len(),
                    phase,
                    book: job.as_ref().map(|j| j.album.clone()).unwrap_or_default(),
                    author: job.as_ref().map(|j| j.author.clone()).unwrap_or_default(),
                    disc: job.as_ref().map_or(1, |j| j.disc),
                    discs_total: job.as_ref().map_or(1, |j| j.discs_total),
                    rows: l
                        .tracks
                        .iter()
                        .map(|t| LiveRow {
                            number: t.number,
                            nn: format!("{:02}", t.number),
                            title: t.title.clone(),
                            length: mmss(t.seconds),
                            state: t.state,
                            percent: (t.fraction.clamp(0.0, 1.0) * 100.0).round() as i64,
                            error_key: (!t.error.is_empty()).then(|| crate::cdimport::error_key(&t.error)),
                            error: t.error.clone(),
                        })
                        .collect(),
                    error_key: (!error.is_empty()).then(|| crate::cdimport::error_key(&error)),
                    error,
                    can_cancel: active && job_phase == "ripping",
                    job_id: l.job_id,
                    buffer: (active && job_phase == "ripping" && count("reading") + count("verifying") + count("rereading") > 0).then(|| {
                        let secs = l.buffer_seconds();
                        BufferView {
                            seconds: format!("{secs:.0}"),
                            max: l.buffer_max_seconds().round() as u32,
                            percent: l.buffer_percent(),
                            waits: l.reader_waits().0,
                            longest: format!("{:.1}", l.reader_waits().1),
                            state: if l.buffer_percent() >= 95 { "full" } else if secs < 0.5 { "empty" } else { "between" },
                            speed: l.speed_x().map(|x| format!("{x:.1}")).unwrap_or_default(),
                        }
                    }),
                })
            }
            None => None,
        };
        let ready = drives.iter().find(|d| d.medium == Medium::Audio && d.tray == Tray::Closed).cloned();
        // The signature covers what is shown that changes: the drives, the jobs, the choice of a release.
        let mut sig_text = serde_json::to_string(&drives).unwrap_or_default();
        for j in &jobs {
            sig_text.push_str(&format!("|{}:{}:{}:{}", j.id, j.phase, j.track, j.percent));
        }
        for w in &waiting {
            sig_text.push_str(&format!("|wait:{}:{}:{}:{}", w.id, w.state, w.can_decide, w.can_retry));
        }
        sig_text.push_str(&format!("|{}", set.lookup_ready));
        if let Some(r) = &rip {
            // The bar and the list change while a rip runs: their state is part of the signature.
            sig_text.push_str(&format!("|rip:{}:{}:{}:{}:{}:{}", r.job_id, r.percent, r.phase, r.active, r.error, r.rows.iter().map(|x| format!("{}{}{}", x.state, x.percent, x.error)).collect::<Vec<_>>().join(",")));
            if let Some(b) = &r.buffer {
                sig_text.push_str(&format!("|buf:{}", b.seconds));
            }
        }
        if let Some(id) = ready.as_ref().and_then(|d| d.disc_id.as_deref()) {
            sig_text.push_str(&format!("|{:?}", crate::cdresume::info(&crate::cdresume::dir_of(&cd.staging, id), None, 0).map(|i| (i.tracks_done, i.interrupted_at))));
        }
        let sig = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            sig_text.hash(&mut h);
            format!("{:016x}", h.finish())
        };
        let mbq = q.fields.get("mbq").map(|v| v.trim().to_owned()).unwrap_or_default();
        let (mut candidates, mut lookup_error) = (Vec::new(), None);
        if let Some(d) = &ready
            && let Some(toc) = &d.table
        {
            if set.lookup_ready {
                match crate::cdimport::lookup(store, cd, &set, toc).await {
                    Ok((_, found)) => {
                        candidates = found
                            .into_iter()
                            .enumerate()
                            .map(|(index, c)| CandView {
                                index,
                                release_id: c.release_id,
                                title: c.title,
                                author: c.author,
                                narrator: c.narrators.first().cloned().unwrap_or_default(),
                                audiobook: c.audiobook,
                                disc: c.disc,
                                discs_total: c.discs_total,
                                tracks: c.tracks.into_iter().map(|t| t.title).collect(),
                            })
                            .collect()
                    }
                    Err(r) => lookup_error = Some(r.1),
                }
                // Words typed by hand: the releases that MusicBrainz finds for them follow those that the disc found.
                if !mbq.is_empty() {
                    match crate::cdimport::search(store, cd, &set, &mbq).await {
                        Ok(found) => {
                            for c in found {
                                if !candidates.iter().any(|k: &CandView| k.release_id == c.release_id) {
                                    let index = candidates.len();
                                    candidates.push(CandView {
                                        index,
                                        release_id: c.release_id,
                                        title: c.title,
                                        author: c.author,
                                        narrator: c.narrators.first().cloned().unwrap_or_default(),
                                        audiobook: c.audiobook,
                                        disc: c.disc,
                                        discs_total: c.discs_total,
                                        tracks: Vec::new(),
                                    });
                                }
                            }
                        }
                        Err(r) => lookup_error = Some(r.1),
                    }
                }
            } else {
                lookup_error = Some("no_contact");
            }
        }
        // A release is taken over when it is chosen, or when MusicBrainz knows only one.
        let chosen = pick.and_then(|i| candidates.get(i)).or(if candidates.len() == 1 { candidates.first() } else { None });
        // A release that came from a search has no tracks yet: they are fetched when it is chosen (the disc with as many tracks as this CD).
        let fetched: Option<CandView> = match (chosen, &ready) {
            (Some(c), Some(d)) if c.tracks.is_empty() && !c.release_id.is_empty() && set.lookup_ready => {
                match crate::cdimport::release_for(store, cd, &set, &c.release_id, d.track_list.len()).await {
                    Ok(Some(full)) => Some(CandView {
                        index: c.index,
                        release_id: full.release_id,
                        title: full.title,
                        author: full.author,
                        narrator: full.narrators.first().cloned().unwrap_or_default(),
                        audiobook: full.audiobook,
                        disc: full.disc,
                        discs_total: full.discs_total,
                        tracks: full.tracks.into_iter().map(|t| t.title).collect(),
                    }),
                    Ok(None) => None,
                    Err(r) => {
                        lookup_error = Some(r.1);
                        None
                    }
                }
            }
            _ => None,
        };
        let chosen = fetched.as_ref().or(chosen);
        let disc_id = ready.as_ref().and_then(|d| d.disc_id.clone()).unwrap_or_default();
        // What was typed is kept while the same disc is in; choosing a release or "Cancel" starts afresh.
        let typed = q.sel && !q.reset && pick.is_none() && q.disc_id.as_deref() == Some(disc_id.as_str());
        let field = |key: &str, from_release: String| -> String { if typed { q.fields.get(key).cloned().unwrap_or(from_release) } else { from_release } };
        let form = RipForm {
            author: field("author", chosen.map(|c| c.author.clone()).unwrap_or_default()),
            title: field("title", chosen.map(|c| c.title.clone()).unwrap_or_default()),
            disc: field("disc", chosen.and_then(|c| c.disc).unwrap_or(1).to_string()),
            discs_total: field("discs_total", chosen.map(|c| c.discs_total.to_string()).unwrap_or_default()),
            mbid: field("mbid", chosen.map(|c| c.release_id.clone()).unwrap_or_default()),
            tracks: field("tracks", chosen.map(|c| c.tracks.join("\n")).unwrap_or_default()),
            narrator: field("narrator", chosen.map(|c| c.narrator.clone()).unwrap_or_default()),
            eject: if typed { q.fields.contains_key("eject") } else { set.eject_after },
            // The setting, unless MusicBrainz says the release is an audiobook (or a person chose otherwise).
            kind: field("kind", if chosen.is_some_and(|c| c.audiobook) { "audiobook".to_owned() } else { set.target_kind.clone() }),
        };
        // The list of tracks: titles from the release (by position on the disc), lengths from the table of contents.
        let titles: Vec<String> = form.tracks.lines().map(|l| l.trim().to_owned()).collect();
        let selection = (q.sel && !q.reset && q.disc_id.as_deref() == Some(disc_id.as_str())).then_some(&q.only);
        // An interrupted rip of this disc: which tracks it has read (they are taken over when they stay chosen and the settings fit).
        let keep_hours = u64::from(set.resume_keep_hours);
        let fingerprint = crate::cdresume::Fingerprint::of(&set);
        let interrupted = (!disc_id.is_empty())
            .then(|| crate::cdresume::info(&crate::cdresume::dir_of(&cd.staging, &disc_id), Some(&fingerprint), keep_hours))
            .flatten()
            .filter(|i| !i.tracks_done.is_empty());
        let read_numbers: Vec<u32> = interrupted.as_ref().filter(|i| i.settings_match).map(|i| i.tracks_done.clone()).unwrap_or_default();
        let rows: Vec<TrackRow> = ready
            .as_ref()
            .map(|d| {
                d.track_list
                    .iter()
                    .enumerate()
                    .map(|(i, t)| TrackRow {
                        number: t.number,
                        nn: format!("{:02}", t.number),
                        title: titles.get(i).cloned().unwrap_or_default(),
                        seconds: t.seconds,
                        length: format!("{}:{:02}", t.seconds / 60, t.seconds % 60),
                        checked: selection.is_none_or(|s| s.contains(&t.number)),
                        read: read_numbers.contains(&t.number),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (selected_count, selected_seconds) = rows.iter().filter(|r| r.checked).fold((0usize, 0u32), |(n, s), r| (n + 1, s + r.seconds));
        let device = ready.as_ref().map(|d| d.device.clone()).unwrap_or_default();
        let running = !jobs.is_empty() || rip.as_ref().is_some_and(|r| r.active);
        let can_eject = !running && drives.iter().any(|d| d.tray == Tray::Closed && matches!(d.medium, Medium::Audio | Medium::Other) && !d.ripping && !d.denied);
        let zone = store.setting("time_zone").await?;
        let chosen_numbers: Vec<u32> = rows.iter().filter(|r| r.checked).map(|r| r.number).collect();
        let resume = interrupted.map(|i| {
            // Of the chosen tracks, these are taken over and these are read again; a track that was read and is not chosen now is dropped.
            let kept = chosen_numbers.iter().filter(|n| i.tracks_done.contains(n)).count();
            ResumePanel {
                done: i.tracks_done.len(),
                total: i.tracks_total,
                when: stamp(i.interrupted_at, zone.as_deref()),
                keep_until: stamp(i.keep_until, zone.as_deref()),
                hours: keep_hours,
                settings_match: i.settings_match,
                kept,
                rest: chosen_numbers.len() - kept,
                dropped: i.tracks_done.len() - kept,
            }
        });
        let present = match &ready {
            Some(d) => crate::cdrive::present_for(store, cd, cd.library.as_deref(), &set, d).await,
            None => None,
        };
        let mmss = |s: u32| format!("{}:{:02}", s / 60, s % 60);
        let present = present.map(|p| PresentPanel {
            complete: !p.needs_healing(),
            missing: p.missing.iter().map(u32::to_string).collect::<Vec<_>>().join(", "),
            damaged: p.damaged.iter().map(|d| DamageRow { number: d.number, reason: d.reason, have: mmss(d.have_s), want: mmss(d.want_s) }).collect(),
            book: p.book,
            author: p.author,
            disc: p.disc,
            in_library: p.tracks_in_library,
            on_disc: p.tracks_on_disc,
        });
        let show_import = present.is_none() || q.force;
        let busy = running || drives.iter().any(|d| d.ripping);
        Ok(DrivePanel {
            rip,
            waiting,
            can_insert: rights.may_insert,
            present,
            show_import,
            resume,
            drives,
            mode,
            sig,
            jobs,
            candidates,
            mbq_param: if mbq.is_empty() { String::new() } else { serde_urlencoded::to_string([("mbq", mbq.as_str())]).unwrap_or_default() },
            mbq,
            form,
            can_rip: ready.is_some() && !running,
            busy,
            device,
            lookup_error,
            rip_error: rip_error.map(str::to_owned),
            taken: taken.iter().map(u32::to_string).collect::<Vec<_>>().join(", "),
            pick,
            can_eject,
            disc_id,
            total_count: rows.len(),
            selected_count,
            selected_time: format!("{}:{:02}", selected_seconds / 60, selected_seconds % 60),
            rows,
        })
    }


    /// What is wrong with the certificate that the settings name, if anything: a file that cannot be read,
    /// a key that does not fit. `None` if none is named or it can be read.
    async fn tls_problem(store: &Store) -> Option<String> {
        let ext = store.external_tls().await.ok()?;
        if !ext.is_set() {
            return None;
        }
        crate::tls::load_external(&ext).err().map(|e| e.to_string())
    }

    /// The form with the device limit and session length of the roles.
    pub async fn settings_roles_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, saved: bool) -> Result<String> {
        let policies = self.policy_views(store).await?;
        self.render("fragments/settings_roles.html", context! { policies => Serde(policies), saved => saved, ..view.into().context() })
    }

    /// The time zone names the system knows, sorted, for the search list of the time zone field.
    fn zone_names() -> Vec<String> {
        let mut names: Vec<String> = jiff::tz::db().available().map(|n| n.as_str().to_owned()).collect();
        names.sort();
        names
    }

    async fn setting_views(&self, store: &Store) -> Result<Vec<SettingView>> {
        let mut views: Vec<SettingView> = store.setting_rows().await?.into_iter().map(SettingView::from_row).filter(|s| !s.is_import()).collect();
        // The CD import values in the order of the group, the others as they were.
        views.sort_by_key(|v| crate::cd_settings::KEYS.iter().position(|k| *k == v.key).unwrap_or(usize::MAX));
        Ok(views)
    }

    async fn policy_views(&self, store: &Store) -> Result<Vec<PolicyView>> {
        Ok(store
            .role_policies()
            .await?
            .into_iter()
            .map(|p| PolicyView {
                days: if p.session_ttl_s % 86_400 == 0 { p.session_ttl_s / 86_400 } else { (p.session_ttl_s / 86_400).max(1) },
                role: p.role,
                max_devices: p.max_devices,
                session_ttl_s: p.session_ttl_s,
                step_up_s: p.step_up_s,
                confirmed: p.confirmed,
            })
            .collect())
    }
}

fn template_error(e: minijinja::Error) -> Error {
    // The detailed form names the template, the line and the cause.
    Error::Template(format!("{e:#}"))
}

/// The text for a refused value of the tab "CD import", in plain words: the reason, and for a template the
/// placeholder that is wrong and the ones that are allowed, with their names (`Author {author}`).
pub fn verr_text(lang: Lang, key: &str, raw: &str, code: &str) -> String {
    let cat = Catalog::new(lang);
    let named = |vars: &[String]| vars.iter().map(|v| format!("{} {{{v}}}", cat.t(&format!("cd.ph.{v}")))).collect::<Vec<_>>().join(", ");
    let shown: Vec<String> = crate::cd_settings::vars_of(key).iter().map(|v| (*v).to_owned()).collect();
    let text = cat.t(&format!("cd.verr.{code}")).to_owned();
    match code {
        "template_unknown_placeholder" => {
            let bad = crate::cd_settings::unknown_placeholder(key, raw).unwrap_or_default();
            text.replace("{x}", &format!("{{{bad}}}")).replace("{allowed}", &named(&shown))
        }
        "template_missing_placeholder" => text.replace("{missing}", &named(&crate::cd_settings::missing_placeholders(key, raw))),
        _ => text,
    }
}

/// The example behind a template field: the real naming, filled with sample values from the catalog, or the reason
/// why the template is refused. Plain text.
pub async fn cd_preview(store: &Store, lang: Lang, field: &str, pattern: &str) -> String {
    use crate::cdimport::{Meta, MetaTrack, StagedTrack};
    let cat = Catalog::new(lang);
    if crate::cd_settings::vars_of(field).is_empty() {
        return String::new();
    }
    let value = match crate::cd_settings::check(field, pattern) {
        Ok(v) => v,
        Err(code) => return verr_text(lang, field, pattern, code),
    };
    let Ok(mut set) = crate::cd_settings::CdSettings::load(store).await else { return String::new() };
    let sample = |k: &str| cat.t(k).to_owned();
    let meta = Meta {
        author: sample("cd.sample.author"),
        album: sample("cd.sample.title"),
        narrator: Some(sample("cd.sample.narrator")),
        year: Some(sample("cd.sample.year")),
        disc: 3,
        discs_total: Some(12),
        kind: None,
        mbid: None,
        disc_id: None,
        genre: None,
        job_id: None,
        barcode: None,
        allow_gaps: false,
        tracks: vec![MetaTrack { number: 5, title: Some(sample("cd.sample.track_title")), file: String::new() }],
    };
    let track = StagedTrack { number: 5, title: sample("cd.sample.track_title"), path: PathBuf::new() };
    let example = match field {
        "cd_file_pattern" => {
            set.file_pattern = value;
            crate::cdimport::file_name_of(&meta, &set, &track)
        }
        "cd_folder_pattern" => {
            set.folder_pattern = value;
            crate::cdimport::layout(&meta, &set, Path::new("")).0.to_string_lossy().trim_start_matches('/').to_owned()
        }
        "cd_disc_folder_pattern" => {
            set.disc_folder_pattern = value;
            let (book, dest) = crate::cdimport::layout(&meta, &set, Path::new(""));
            dest.strip_prefix(&book).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
        }
        _ => return String::new(),
    };
    let example = if example.is_empty() { cat.t("cd.preview.flat").to_owned() } else { example };
    format!("{}: {example}", cat.t("cd.preview.example"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_countdown_reads_like_a_clock() {
        assert_eq!(countdown_text(2), "0:02");
        assert_eq!(countdown_text(1024), "17:04");
        assert_eq!(countdown_text(32_768), "9:06:08");
        assert_eq!(countdown_text(2 * 86_400 + 3 * 3_600 + 15 * 60), "2 d 03:15:00");
    }

    #[test]
    fn all_templates_parse() {
        AdminUi::new().expect("a template has a syntax error");
    }

    #[test]
    fn a_missing_variable_is_an_error_and_not_an_empty_string() {
        let ui = AdminUi::new().unwrap();
        let r = ui.render("overview.html", View::new(Lang::En).context());
        // Debug builds name the variable; release builds name the template and the line.
        let message = r.unwrap_err().to_string();
        assert!(message.contains("undefined") && message.contains("overview.html"), "{message}");
    }

    #[test]
    fn a_message_is_shown_in_the_language_of_the_page() {
        let ui = AdminUi::new().unwrap();
        let en = ui.message_fragment(View::new(Lang::En).message(MessageKind::Error, "err.last_admin")).unwrap();
        assert!(en.contains("class=\"message error\"") && en.contains("last administrator"));
        let de = ui.message_fragment(View::new(Lang::De).message(MessageKind::Error, "err.last_admin")).unwrap();
        assert!(de.contains("letzte Administrator"));
        assert!(ui.message_fragment(View::new(Lang::En)).unwrap().trim().is_empty(), "no message, no markup");
    }

    #[test]
    fn the_login_page_has_no_navigation() {
        let ui = AdminUi::new().unwrap();
        let page = ui.login(Lang::En).unwrap();
        assert!(page.contains("action=\"/admin/login\"") && !page.contains("/admin/logout"));
        assert!(!page.contains("hx-headers"), "no CSRF header without a session");
    }
}


/// The books of `ids` that have no cover. Looking costs a read of the picture, so an answer is kept for a few minutes (the page asks for
/// every book at once).
async fn missing_covers(store: &Store, library: Option<&std::path::Path>, ids: &[i64]) -> std::collections::HashSet<i64> {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    static KNOWN: OnceLock<Mutex<std::collections::HashMap<i64, (Instant, bool)>>> = OnceLock::new();
    let cache = KNOWN.get_or_init(Default::default);
    let mut missing = std::collections::HashSet::new();
    let mut ask = Vec::new();
    {
        let known = cache.lock().unwrap_or_else(|e| e.into_inner());
        for id in ids {
            match known.get(id) {
                Some((at, has)) if at.elapsed() < Duration::from_secs(300) => {
                    if !*has {
                        missing.insert(*id);
                    }
                }
                _ => ask.push(*id),
            }
        }
    }
    for id in ask {
        let has = crate::api::find_cover(store, library, id).await.is_some();
        cache.lock().unwrap_or_else(|e| e.into_inner()).insert(id, (Instant::now(), has));
        if !has {
            missing.insert(id);
        }
    }
    missing
}


/// One column of the matrix of the media list.
#[derive(serde::Serialize)]
struct Member {
    id: i64,
    name: String,
    /// Administrators see everything: their box is ticked and cannot be changed.
    admin: bool,
}

/// The columns of the matrix (the family members that are no administrators) and the marks `"<book>:<member>"` where the member sees the
/// book. With `only` the marks are given for that book only (to draw one row again).
async fn visibility_columns(store: &Store, only: Option<i64>) -> Result<(Vec<Member>, Vec<String>)> {
    let matrix = store.visibility_matrix().await?;
    let members = matrix.iter().map(|(id, name, _, admin)| Member { id: *id, name: name.clone(), admin: *admin }).collect();
    let seen = matrix.iter().flat_map(|(id, _, books, _)| books.iter().filter(move |b| only.is_none_or(|o| o == **b)).map(move |b| format!("{b}:{id}"))).collect();
    Ok((members, seen))
}


/// Everybody a book can belong to (the choices of the owner box): id and nickname.
async fn owners_list(store: &Store) -> Result<Vec<Member>> {
    Ok(store.user_rows().await?.into_iter().map(|u| Member { admin: u.role == "admin", id: u.id, name: u.name }).collect())
}


impl AdminUi {
    /// The dialog that chooses a folder: what the server can read, write and enter below `path` (the start: the places it works in). `target` is
    /// the id of the input that gets the choice.
    pub async fn folder_picker<'a>(&self, store: &Store, view: impl Into<View<'a>>, path: &str, target: &str, library: Option<&std::path::Path>, create_error: Option<&str>) -> Result<String> {
        let mut starts: Vec<std::path::PathBuf> = Vec::new();
        let state = store.import_state().await?;
        starts.extend(state.roots.iter().chain(state.music_roots.iter()).map(std::path::PathBuf::from));
        starts.extend(library.map(std::path::Path::to_path_buf));
        for per_profile in store.all_profile_folders().await?.into_values() {
            starts.extend(per_profile.into_values().map(std::path::PathBuf::from));
        }
        let browse = crate::folders::browse(path, &starts);
        let (listing, error) = match browse {
            Ok(b) => (Some(b), None),
            Err(e) => (None, Some(e.to_string())),
        };
        self.render("fragments/folder_picker.html", context! { listing => Serde(listing), error => error, create_error => create_error, target => target, ..view.into().context() })
    }
}

/// The numbers of one page of a list that is cut after it is complete.
struct Paging {
    page: usize,
    per: usize,
    default_per: usize,
    total: usize,
    pages: usize,
    from: usize,
    to: usize,
}

/// The sizes of a page that the lists offer.
const PAGE_SIZES: [usize; 6] = [10, 25, 50, 100, 250, 500];

impl Paging {
    /// `per` and `page` as asked (10 to 500 per page; the setting `list_page_size` when absent); the page is brought into range.
    async fn of(store: &Store, total: usize, page: Option<usize>, per: Option<usize>) -> Result<Paging> {
        let default_per = store.setting("list_page_size").await?.and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(50);
        let per = per.unwrap_or(default_per).clamp(10, 500);
        let pages = total.div_ceil(per).max(1);
        let page = page.unwrap_or(1).clamp(1, pages);
        Ok(Paging { page, per, default_per, total, pages, from: (page - 1) * per, to: (page * per).min(total) })
    }

    /// What `fragments/pager.html` needs: `path` is the page the links lead to, `query` the search text that is kept.
    fn json(&self, path: &str, query: Option<&str>) -> serde_json::Value {
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(q) = query {
            pairs.push(("q", q.to_owned()));
        }
        if self.per != self.default_per {
            pairs.push(("per", self.per.to_string()));
        }
        let base = serde_urlencoded::to_string(pairs).unwrap_or_default();
        let shown: Vec<usize> = (1..=self.pages).filter(|n| *n == 1 || *n == self.pages || n.abs_diff(self.page) <= 2).collect();
        let numbers: Vec<serde_json::Value> = shown.iter().enumerate().map(|(i, n)| serde_json::json!({ "n": n, "gap": i > 0 && n - shown[i - 1] > 1 })).collect();
        serde_json::json!({
            "path": path, "page": self.page, "pages": self.pages, "per": self.per, "total": self.total, "from": if self.total == 0 { 0 } else { self.from + 1 }, "to": self.to,
            "base": base, "numbers": numbers, "prev": self.page.saturating_sub(1).max(1), "next": (self.page + 1).min(self.pages), "default_per": self.default_per, "sizes": PAGE_SIZES,
            "q": query.unwrap_or(""), "problems": false, "label": "",
        })
    }
}

/// A person that asked for a podcast or a station and waits for a release.
#[derive(serde::Serialize)]
struct Waiting {
    profile_id: i64,
    name: String,
}

/// The requests of children, by what they asked for: `(kind, target id)` -> who.
async fn waiting_by_target(store: &Store, kind: &str) -> Result<std::collections::HashMap<i64, Vec<Waiting>>> {
    let mut out: std::collections::HashMap<i64, Vec<Waiting>> = std::collections::HashMap::new();
    for p in store.pending_releases().await?.into_iter().filter(|p| p.kind == kind) {
        out.entry(p.target_id).or_default().push(Waiting { profile_id: p.profile_id, name: p.profile });
    }
    Ok(out)
}

/// Every profile for the dialog "Followers" of a podcast: `{id, name, role}`, by name.
async fn follower_choices(store: &Store) -> Result<Vec<serde_json::Value>> {
    Ok(store.list_profiles().await?.into_iter().map(|p| serde_json::json!({ "id": p.id, "name": p.name, "role": p.role })).collect())
}

/// The text of a time of listening: `h:mm:ss`, or `m:ss` below an hour.
fn clock(seconds: i64) -> String {
    let s = seconds.max(0);
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

pub mod blacklist_pages;

/// "2026-10-27" in the time zone of the server.
fn day(ms: i64, zone: Option<&str>) -> String {
    stamp(ms, zone).chars().take(10).collect()
}

impl AdminUi {
    /// The podcasts: a list with a search field, cut into pages after the search; every podcast has its own page (`/admin/podcasts/{id}`).
    pub async fn podcasts<'a>(&self, store: &Store, view: impl Into<View<'a>>, query: Option<&str>, page: Option<usize>, per: Option<usize>) -> Result<String> {
        let needle = query.map(str::trim).filter(|q| !q.is_empty());
        let waiting = waiting_by_target(store, "podcast").await?;
        let labels = store.podcast_labels_with_mark().await?;
        let lower = needle.map(str::to_lowercase);
        let feeds: Vec<_> = store
            .feeds_with_followers()
            .await?
            .into_iter()
            .filter(|(f, _)| lower.as_ref().is_none_or(|n| f.title.to_lowercase().contains(n) || f.url.to_lowercase().contains(n)))
            .collect();
        let paging = Paging::of(store, feeds.len(), page, per).await?;
        // For the dialog "Followers": every profile, and who follows (ticked) or asked (waits) per podcast, by id.
        let profiles = follower_choices(store).await?;
        let by_feed = store.follow_ids_by_feed().await?;
        let items: Vec<serde_json::Value> = feeds
            .into_iter()
            .skip(paging.from)
            .take(paging.per)
            .map(|(f, followers)| {
                let followers: Vec<serde_json::Value> = followers.into_iter().filter(|(_, ok)| *ok).map(|(n, _)| serde_json::Value::String(n)).collect();
                let ids = by_feed.get(&f.id).cloned().unwrap_or_default();
                let follower_ids: Vec<i64> = ids.iter().filter(|(_, ok)| *ok).map(|(p, _)| *p).collect();
                let waiting_ids: Vec<i64> = ids.iter().filter(|(_, ok)| !*ok).map(|(p, _)| *p).collect();
                serde_json::json!({ "follower_ids": follower_ids, "waiting_ids": waiting_ids, "id": f.id, "url": f.url, "title": f.title, "last_error": f.last_error, "followers": followers, "waiting": waiting.get(&f.id).map(|w| serde_json::json!(w)).unwrap_or_else(|| serde_json::json!([])), "labels": labels.get(&f.id).cloned().unwrap_or_default() })
            })
            .collect();
        let pager = paging.json("/admin/podcasts", needle);
        let label_names = store.labels().await?;
        self.render("podcasts.html", context! { profiles => Serde(profiles), feeds => Serde(items), pager => Serde(pager), q => needle.unwrap_or_default(), searched => needle.is_some(), label_names => Serde(label_names), ..view.into().context() })
    }

    /// One podcast with its episodes, newest first, in pages. `None` if there is no such podcast.
    pub async fn podcast_page<'a>(&self, store: &Store, view: impl Into<View<'a>>, id: i64, page: Option<usize>, per: Option<usize>) -> Result<Option<String>> {
        let Some(head) = store.feed_head(id).await? else { return Ok(None) };
        let zone = store.setting("time_zone").await?;
        let followers: Vec<String> = store.feeds_with_followers().await?.into_iter().find(|(f, _)| f.id == id).map(|(_, fs)| fs.into_iter().filter(|(_, ok)| *ok).map(|(n, _)| n).collect()).unwrap_or_default();
        let waiting = waiting_by_target(store, "podcast").await?.remove(&id).unwrap_or_default();
        let labels = store.podcast_labels_with_mark().await?.remove(&id).unwrap_or_default();
        let paging = Paging::of(store, store.episode_count(id).await?, page, per).await?;
        let episodes: Vec<serde_json::Value> = store
            .admin_episodes(id, paging.per, paging.from)
            .await?
            .into_iter()
            .map(|e| serde_json::json!({ "id": e.id, "title": e.title, "published": day(e.published_at, zone.as_deref()), "duration": e.duration_s.map(clock), "finished_by": e.finished_by, "started_by": e.started_by }))
            .collect();
        let pager = paging.json(&format!("/admin/podcasts/{id}"), None);
        let label_names = store.labels().await?;
        let ids = store.follow_ids_by_feed().await?.remove(&id).unwrap_or_default();
        let follower_ids: Vec<i64> = ids.iter().filter(|(_, ok)| *ok).map(|(p, _)| *p).collect();
        let waiting_ids: Vec<i64> = ids.iter().filter(|(_, ok)| !*ok).map(|(p, _)| *p).collect();
        let profiles = follower_choices(store).await?;
        let feed = serde_json::json!({ "follower_ids": follower_ids, "waiting_ids": waiting_ids, "id": head.id, "url": head.url, "title": head.title, "last_error": head.last_error, "followers": followers, "waiting": waiting, "labels": labels });
        self.render("podcast.html", context! { profiles => Serde(profiles), feed => Serde(feed), episodes => Serde(episodes), pager => Serde(pager), label_names => Serde(label_names), ..view.into().context() }).map(Some)
    }

    /// The radio stations: a list with a search field, cut into pages after the search.
    pub async fn radio<'a>(&self, store: &Store, view: impl Into<View<'a>>, query: Option<&str>, page: Option<usize>, per: Option<usize>) -> Result<String> {
        let needle = query.map(str::trim).filter(|q| !q.is_empty());
        let waiting = waiting_by_target(store, "station").await?;
        let labels = store.station_labels_with_mark().await?;
        let lower = needle.map(str::to_lowercase);
        let stations: Vec<_> = store
            .stations_with_owners()
            .await?
            .into_iter()
            .filter(|(s, _)| lower.as_ref().is_none_or(|n| s.name.to_lowercase().contains(n) || s.stream_url.to_lowercase().contains(n)))
            .collect();
        let paging = Paging::of(store, stations.len(), page, per).await?;
        // For the dialog "Favourite of": every profile, and who has the station (ticked) or asked for it (waits), by id.
        let profiles = follower_choices(store).await?;
        let by_station = store.favourite_ids_by_station().await?;
        let items: Vec<serde_json::Value> = stations
            .into_iter()
            .skip(paging.from)
            .take(paging.per)
            .map(|(s, owners)| {
                // The names that wait for a release come marked "(waiting)" from the store; they are listed apart.
                let favourite: Vec<String> = owners.into_iter().filter(|n| !n.ends_with(" (waiting)")).collect();
                let ids = by_station.get(&s.id).cloned().unwrap_or_default();
                let favourite_ids: Vec<i64> = ids.iter().filter(|(_, ok)| *ok).map(|(p, _)| *p).collect();
                let waiting_ids: Vec<i64> = ids.iter().filter(|(_, ok)| !*ok).map(|(p, _)| *p).collect();
                serde_json::json!({ "favourite_ids": favourite_ids, "waiting_ids": waiting_ids, "id": s.id, "name": s.name, "stream_url": s.stream_url, "homepage": s.homepage, "logo_url": s.logo_url, "favourite": favourite, "waiting": waiting.get(&s.id).map(|w| serde_json::json!(w)).unwrap_or_else(|| serde_json::json!([])), "labels": labels.get(&s.id).cloned().unwrap_or_default() })
            })
            .collect();
        let pager = paging.json("/admin/radio", needle);
        let label_names = store.labels().await?;
        self.render("radio.html", context! { profiles => Serde(profiles), stations => Serde(items), pager => Serde(pager), q => needle.unwrap_or_default(), searched => needle.is_some(), label_names => Serde(label_names), ..view.into().context() })
    }
}
