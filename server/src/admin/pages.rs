// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pages. Templates are Jinja files in `templates/`, parsed once at start and
//! embedded in the program (MiniJinja). A page gets plain data from the `Store`
//! and renders it for one language.

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
    ("schedule.html", include_str!("../../templates/schedule.html")),
    ("pairing.html", include_str!("../../templates/pairing.html")),
    // Sub-templates. A list is a frame (`*_list`) around rows (`*_row`). The page
    // includes them, and a procedure can answer with the same fragment alone.
    ("fragments/message.html", include_str!("../../templates/fragments/message.html")),
    ("fragments/user_list.html", include_str!("../../templates/fragments/user_list.html")),
    ("fragments/user_row.html", include_str!("../../templates/fragments/user_row.html")),
    ("fragments/book_list.html", include_str!("../../templates/fragments/book_list.html")),
    ("fragments/book_row.html", include_str!("../../templates/fragments/book_row.html")),
    ("fragments/pairing_result.html", include_str!("../../templates/fragments/pairing_result.html")),
    ("fragments/device_list.html", include_str!("../../templates/fragments/device_list.html")),
    ("fragments/device_row.html", include_str!("../../templates/fragments/device_row.html")),
    ("fragments/schedule_panel.html", include_str!("../../templates/fragments/schedule_panel.html")),
    ("fragments/book_tags.html", include_str!("../../templates/fragments/book_tags.html")),
    ("fragments/upload_section.html", include_str!("../../templates/fragments/upload_section.html")),
    ("fragments/upload_queue.html", include_str!("../../templates/fragments/upload_queue.html")),
    ("fragments/import_panel.html", include_str!("../../templates/fragments/import_panel.html")),
    ("fragments/settings_values.html", include_str!("../../templates/fragments/settings_values.html")),
    ("fragments/settings_roles.html", include_str!("../../templates/fragments/settings_roles.html")),
    ("fragments/import_settings.html", include_str!("../../templates/fragments/import_settings.html")),
    ("profile.html", include_str!("../../templates/profile.html")),
    ("fragments/ui.html", include_str!("../../templates/fragments/ui.html")),
    ("fragments/avatar.html", include_str!("../../templates/fragments/avatar.html")),
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
}

impl<'a> View<'a> {
    pub fn new(lang: Lang) -> View<'static> {
        View { lang, csrf: "", message: None, me: None }
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

    fn context(&self) -> minijinja::Value {
        let catalog = Catalog::new(self.lang);
        context! {
            lang => self.lang.code(),
            csrf => self.csrf,
            me => self.me.map(Serde),
            message => self.message.map(|(_, key)| catalog.t(key).to_owned()),
            message_kind => match self.message {
                Some((MessageKind::Error, _)) => "error",
                _ => "ok",
            },
        }
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
}

impl SettingView {
    fn from_row(s: crate::entities::setting::Model) -> SettingView {
        let minutes = |v: &str| v.parse::<i64>().map_or_else(|_| v.to_owned(), |n| (n / 60).to_string());
        let (group, unit, shown) = match s.key.as_str() {
            "pin_max_failures" => ("signin", "unit.attempts", s.value.clone()),
            "pin_lock_s" => ("signin", "unit.minutes", minutes(&s.value)),
            "enrollment_code_ttl_s" => ("pairing", "unit.minutes", minutes(&s.value)),
            "device_inactive_days" => ("pairing", "unit.days", s.value.clone()),
            "guest_expire_days" => ("guests", "unit.days", s.value.clone()),
            "import_interval_min" => ("library", "unit.minutes", s.value.clone()),
            "min_free_gb" => ("library", "unit.gb", s.value.clone()),
            "time_zone" => ("general", "", s.value.clone()),
            _ => ("other", "", s.value.clone()),
        };
        SettingView { key: s.key, value: s.value, shown, unit, group, confirmed: s.confirmed }
    }

    /// The import has its own tab; its values are not listed again as plain values.
    fn is_import(&self) -> bool {
        (self.key.starts_with("import_") && self.key != "import_interval_min") || self.key == "rating_rule"
    }
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
    Ok(count + 4)
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
        Ok(AdminUi { env, dirs, overrides })
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
        self.render("overview.html", context! { data => Serde(data), ..view.into().context() })
    }

    pub async fn users<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let users = store.user_rows().await?;
        self.render("users.html", context! { users => Serde(users), ..view.into().context() })
    }

    /// The whole list of users, without the page around it. This is what a
    /// procedure answers when the list has to be replaced.
    pub async fn user_list_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let users = store.user_rows().await?;
        self.render("fragments/user_list.html", context! { users => Serde(users), ..view.into().context() })
    }

    /// One row of the list, for replacing a single user after a change. `None` if
    /// the user does not exist (any more).
    pub async fn user_row_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, id: i64) -> Result<Option<String>> {
        let Some(user) = store.user_row(id).await? else { return Ok(None) };
        self.render("fragments/user_row.html", context! { user => Serde(user), ..view.into().context() }).map(Some)
    }

    /// The list of albums of one kind: `audiobook` or `music`.
    pub async fn books<'a>(&self, store: &Store, view: impl Into<View<'a>>, kind: &str) -> Result<String> {
        let books: Vec<_> = store.admin_book_rows().await?.into_iter().filter(|b| b.kind == kind).collect();
        self.render("books.html", context! { books => Serde(books), kind => kind, ..view.into().context() })
    }

    pub async fn book_row_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, id: i64) -> Result<Option<String>> {
        let Some(book) = store.admin_book_row(id).await? else { return Ok(None) };
        self.render("fragments/book_row.html", context! { book => Serde(book), ..view.into().context() }).map(Some)
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
                ..view.into().context()
            },
        )
    }

    pub async fn settings<'a>(&self, store: &Store, view: impl Into<View<'a>>, tab: Option<&str>) -> Result<String> {
        let tab = tab.filter(|t| matches!(*t, "roles" | "values")).unwrap_or("import");
        let policies = self.policy_views(store).await?;
        let settings = self.setting_views(store).await?;
        let state = store.import_state().await?;
        self.render(
            "settings.html",
            context! { tab => tab, saved => false, zones => Self::zone_names(), state => Serde(state), policies => Serde(policies), settings => Serde(settings), ..view.into().context() },
        )
    }

    /// The pairing page: the form for a new code, and the devices.
    ///
    /// `result` is the fragment of a code that was just made. A browser without
    /// JavaScript gets it inside the page, because the code is shown only once.
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
        self.render(
            "pairing.html",
            context! {
                users => Serde(users),
                devices => Serde(devices),
                address => default_address,
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

    /// The import page.
    pub async fn import<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let state = store.import_state().await?;
        let jobs = store.upload_jobs(15).await?;
        let busy = jobs.iter().any(|j| j.state == "queued" || j.state == "working");
        self.render(
            "import.html",
            context! { state => Serde(state), report => Option::<Serde<ScanReport>>::None, jobs => Serde(jobs), busy => busy, ..view.into().context() },
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
            context! { state => Serde(state), report => report.map(Serde), ..view.into().context() },
        )
    }

    /// The import tab of the settings alone: folders, rule and the lock.
    pub async fn import_settings_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>) -> Result<String> {
        let state = store.import_state().await?;
        self.render("fragments/import_settings.html", context! { state => Serde(state), ..view.into().context() })
    }

    /// The form with the plain values of the settings page.
    pub async fn settings_values_fragment<'a>(&self, store: &Store, view: impl Into<View<'a>>, saved: bool) -> Result<String> {
        let settings = self.setting_views(store).await?;
        self.render("fragments/settings_values.html", context! { settings => Serde(settings), saved => saved, zones => Self::zone_names(), ..view.into().context() })
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
        Ok(store.setting_rows().await?.into_iter().map(SettingView::from_row).filter(|s| !s.is_import()).collect())
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

#[cfg(test)]
mod tests {
    use super::*;

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
