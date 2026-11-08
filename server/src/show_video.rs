// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The slideshow as a video for a television or a USB stick (docs/spec-pictures.md, 7; project owner, 2026-11-08): Full HD (1920×1080,
//! 30 frames, H.264 and AAC in MP4), made in the background from the same choice as a project, one after the other, kept until the person
//! deletes it. It looks like the slideshow: every picture its seconds, cross-fade, cross-fade with a slow zoom, cut, or one of them at
//! random for every change; the date and the place below on the left; the videos play through with their sound, and the music of the
//! playlist goes on behind it all, quieter while a video plays.
//!
//! How it is made, with ffmpeg: every picture and every video becomes a piece of its own (`piece-0001.mp4`, the zoom and the caption in
//! it, no sound); twenty pieces at a time are joined with their transitions (`xfade`), then the groups the same way; then the sound is
//! laid under it (the music again and again from the start, quieter where a video is, and the sound of each video at its place). The
//! pieces overlap by the length of a transition, so the times of the whole are known beforehand ([`timeline`]).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;
use tokio::sync::Notify;

use crate::admin::i18n::{Catalog, Lang};
use crate::roles::Role;
use crate::show_project::{music_of, piece};
use crate::error::Result;
use crate::store::Store;

/// The size of the picture of the video.
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FPS: u32 = 30;
/// A cross-fade, in seconds; a cut is a little more than one frame (`xfade` needs a length, and with exactly one frame it drops the
/// next piece).
const FADE: f64 = 1.0;
const CUT: f64 = 0.04;
/// How many pieces are joined in one go.
const GROUP: usize = 20;
/// How quiet the music is while a video plays.
const DUCK: f64 = 0.15;

/// A video of a person, as the list of `/pictures` shows it.
#[derive(Debug, Clone, Serialize)]
pub struct ShowVideo {
    pub id: i64,
    pub title: String,
    pub state: String,
    /// 0 to 1000.
    pub progress: i64,
    pub error: Option<String>,
    pub pictures: i64,
    pub size_bytes: i64,
    pub duration_ms: i64,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

/// The file of a finished video.
pub fn file_of(data_dir: &Path, id: i64) -> PathBuf {
    data_dir.join("show-videos").join(format!("{id}.mp4"))
}

fn work_dir(data_dir: &Path, id: i64) -> PathBuf {
    data_dir.join("show-videos").join(format!("work-{id}"))
}

fn row(r: &sea_orm::QueryResult) -> std::result::Result<ShowVideo, sea_orm::DbErr> {
    Ok(ShowVideo {
        id: r.try_get("", "id")?,
        title: r.try_get("", "title")?,
        state: r.try_get("", "state")?,
        progress: r.try_get("", "progress")?,
        error: r.try_get("", "error")?,
        pictures: r.try_get("", "pictures")?,
        size_bytes: r.try_get("", "size_bytes")?,
        duration_ms: r.try_get("", "duration_ms")?,
        created_at: r.try_get("", "created_at")?,
        finished_at: r.try_get("", "finished_at")?,
    })
}

const COLUMNS: &str = "id, title, state, progress, error, pictures, size_bytes, duration_ms, created_at, finished_at";

impl Store {
    /// Asks for a video: the choice is the query of a project (`show_project`), made later by the worker.
    pub async fn create_show_video(&self, user: i64, title: &str, query: &str, lang: Lang) -> Result<i64> {
        let values: [Value; 4] = [user.into(), title.into(), query.into(), lang.code().into()];
        let r = self
            .db
            .writer
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO show_video (user_id, title, query, lang) VALUES (?, ?, ?, ?) RETURNING id",
                values,
            ))
            .await?;
        Ok(r.map(|r| r.try_get("", "id")).transpose()?.unwrap_or_default())
    }

    /// The videos of a person, the newest first.
    pub async fn show_videos(&self, user: i64) -> Result<Vec<ShowVideo>> {
        let rows = self
            .db
            .reader
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT {COLUMNS} FROM show_video WHERE user_id = ? ORDER BY id DESC"),
                [user.into()],
            ))
            .await?;
        Ok(rows.iter().map(row).collect::<std::result::Result<_, _>>()?)
    }

    pub async fn show_video(&self, user: i64, id: i64) -> Result<Option<ShowVideo>> {
        let r = self
            .db
            .reader
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT {COLUMNS} FROM show_video WHERE user_id = ? AND id = ?"),
                [user.into(), id.into()],
            ))
            .await?;
        Ok(r.as_ref().map(row).transpose()?)
    }

    /// Takes a video away, its file too; one that is being made stops at the next piece. False when the person has no such video.
    pub async fn delete_show_video(&self, user: i64, id: i64) -> Result<bool> {
        let n = self
            .db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "DELETE FROM show_video WHERE user_id = ? AND id = ?", [user.into(), id.into()]))
            .await?
            .rows_affected();
        if n > 0 {
            let _ = std::fs::remove_file(file_of(&self.data_dir, id));
        }
        Ok(n > 0)
    }

    async fn set_video(&self, id: i64, sql: &str, mut values: Vec<Value>) -> Result<()> {
        values.push(id.into());
        self.db
            .writer
            .execute_raw(Statement::from_sql_and_values(DbBackend::Sqlite, format!("UPDATE show_video SET {sql} WHERE id = ?"), values))
            .await?;
        Ok(())
    }
}

/// What wakes the worker when a video is asked for.
fn bell() -> &'static Arc<Notify> {
    static BELL: std::sync::OnceLock<Arc<Notify>> = std::sync::OnceLock::new();
    BELL.get_or_init(|| Arc::new(Notify::new()))
}

/// A new video waits: the worker looks at once (without a worker, as in some tests, nothing happens).
pub fn wake() {
    bell().notify_one();
}

/// The worker: makes the videos asked for, one after the other.
pub struct Videos;

impl Videos {
    /// Starts the worker; a video that was being made when the server stopped is made again from the start.
    pub fn start(store: Store, library: Option<PathBuf>) {
        let wake = bell().clone();
        tokio::spawn(async move {
            let _ = store
                .db
                .writer
                .execute_raw(Statement::from_string(DbBackend::Sqlite, "UPDATE show_video SET state = 'queued', progress = 0 WHERE state = 'running'"))
                .await;
            loop {
                match next_queued(&store).await {
                    Some((id, user, query, lang)) => {
                        let lang = Lang::from_code(&lang).unwrap_or(Lang::En);
                        run(&store, library.as_deref(), id, user, &query, lang).await;
                    }
                    None => {
                        // Woken by a new video, and every minute anyway.
                        let _ = tokio::time::timeout(std::time::Duration::from_secs(60), wake.notified()).await;
                    }
                }
            }
        });
    }
}

async fn next_queued(store: &Store) -> Option<(i64, i64, String, String)> {
    let r = store
        .db
        .reader
        .query_one_raw(Statement::from_string(DbBackend::Sqlite, "SELECT id, user_id, query, lang FROM show_video WHERE state = 'queued' ORDER BY id LIMIT 1"))
        .await
        .ok()
        .flatten()?;
    Some((r.try_get("", "id").ok()?, r.try_get("", "user_id").ok()?, r.try_get("", "query").ok()?, r.try_get("", "lang").ok()?))
}

/// Whether the row of the video is still there (the person may have deleted it meanwhile).
async fn wanted(store: &Store, id: i64) -> bool {
    store
        .db
        .reader
        .query_one_raw(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT 1 AS x FROM show_video WHERE id = ?", [id.into()]))
        .await
        .ok()
        .flatten()
        .is_some()
}

async fn run(store: &Store, library: Option<&Path>, id: i64, user: i64, query: &str, lang: Lang) {
    let _ = store.set_video(id, "state = 'running', progress = 0, error = NULL", vec![]).await;
    tracing::info!(video = id, user, "making the video of a slideshow");
    let work = work_dir(&store.data_dir, id);
    let made = make(store, library, id, user, query, lang, &work).await;
    let _ = std::fs::remove_dir_all(&work);
    match made {
        Ok(Made { pictures, duration_ms, size }) => {
            let now = (jiff::Timestamp::now().as_millisecond()).into();
            let _ = store
                .set_video(
                    id,
                    "state = 'done', progress = 1000, pictures = ?, duration_ms = ?, size_bytes = ?, finished_at = ?",
                    vec![(pictures as i64).into(), duration_ms.into(), (size as i64).into(), now],
                )
                .await;
            tracing::info!(video = id, pictures, duration_ms, size, "the video of a slideshow is made");
            // Deleted while it was made: the file goes too.
            if !wanted(store, id).await {
                let _ = std::fs::remove_file(file_of(&store.data_dir, id));
            }
        }
        Err(Failed::Gone) => {
            let _ = std::fs::remove_file(file_of(&store.data_dir, id));
            tracing::info!(video = id, "the video was deleted while it was made");
        }
        Err(Failed::Because(code)) => {
            tracing::warn!(video = id, error = %code, "the video of a slideshow could not be made");
            let _ = store.set_video(id, "state = 'failed', error = ?", vec![code.into()]).await;
        }
    }
}

struct Made {
    pictures: usize,
    duration_ms: i64,
    size: u64,
}

enum Failed {
    /// The person deleted the video meanwhile.
    Gone,
    /// A code for the page: `empty` (no pictures), `no_ffmpeg`, `disk_full`, `ffmpeg` (it failed).
    Because(String),
}

fn because(code: &str) -> Failed {
    Failed::Because(code.to_owned())
}

/// One piece of the video: a picture, or a video, with its caption and the transition that brings it.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub image: PathBuf,
    pub video: Option<PathBuf>,
    pub caption: Vec<String>,
    /// `fade`, `zoom` or `cut`: how this piece comes in (the first one too, from black is not done: it simply starts).
    pub transition: String,
    /// Seconds: a picture its seconds and a fade; a video its length.
    pub length: f64,
    /// For the zoom: where the picture comes closer to (0 to 1, from the left and the top).
    pub focus: (f64, f64),
}

/// The length of the transition that brings a piece in.
fn overlap(transition: &str) -> f64 {
    if transition == "cut" { CUT } else { FADE }
}

/// When every piece starts, and how long the whole is: a piece starts where the one before it ends, less the transition between them.
pub fn timeline(segments: &[Segment]) -> (Vec<f64>, f64) {
    let mut starts = Vec::with_capacity(segments.len());
    let mut end: f64 = 0.0;
    for (i, s) in segments.iter().enumerate() {
        let start = if i == 0 { 0.0 } else { (end - overlap(&s.transition)).max(0.0) };
        starts.push(start);
        end = start + s.length;
    }
    (starts, end)
}

/// A small random source for the order and the transitions (shuffled once, when the video is made).
struct Dice(u64);

impl Dice {
    fn new() -> Dice {
        Dice((uuid::Uuid::new_v4().as_u128() as u64) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn unit(&mut self) -> f64 {
        (self.next() % 10_000) as f64 / 10_000.0
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

/// The date of a picture as the slideshow says it: "1. Juli 2017", "July 1, 2017", in the time zone of the server.
pub fn date_text(lang: Lang, ms: i64, zone: Option<&str>) -> String {
    let Ok(ts) = jiff::Timestamp::from_millisecond(ms) else { return String::new() };
    let z = match zone.map(|z| ts.in_tz(z)) {
        Some(Ok(z)) => z,
        _ => ts.to_zoned(jiff::tz::TimeZone::system()),
    };
    let m = z.month() as usize - 1;
    match lang {
        Lang::De => {
            const M: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
            format!("{}. {} {}", z.day(), M[m], z.year())
        }
        Lang::En => {
            const M: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
            format!("{} {}, {}", M[m], z.day(), z.year())
        }
    }
}

/// A font for the caption, where the system has one (Debian `fonts-dejavu-core`, Fedora `dejavu-sans-fonts`).
fn font() -> Option<PathBuf> {
    [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/Library/Fonts/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
}

/// Whether the ffmpeg of the system can write text (`drawtext`, with libfreetype).
fn has_drawtext() -> bool {
    static FOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FOUND.get_or_init(|| {
        Command::new("ffmpeg")
            .args(["-hide_banner", "-filters"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(" drawtext "))
    })
}

/// Runs ffmpeg (or ffprobe); the last lines of its errors on failure.
fn ffmpeg(program: &str, args: &[String]) -> std::result::Result<Vec<u8>, String> {
    let out = Command::new(program).args(args).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().rev().take(3).collect::<Vec<_>>().join(" / "))
    }
}

fn s(x: impl ToString) -> String {
    x.to_string()
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// The length of a video in seconds, and whether it has sound.
fn probe(file: &Path) -> Option<(f64, bool)> {
    let out = ffmpeg("ffprobe", &[s("-v"), s("error"), s("-show_entries"), s("format=duration:stream=codec_type"), s("-of"), s("default=nw=1"), path(file)]).ok()?;
    let text = String::from_utf8_lossy(&out);
    let length = text.lines().find_map(|l| l.strip_prefix("duration=")).and_then(|d| d.trim().parse::<f64>().ok())?;
    Some((length, text.lines().any(|l| l == "codec_type=audio")))
}

/// The filters that put a picture (or a frame of a video) on the screen: as large as it fits, black around it.
fn fit() -> String {
    format!("scale={WIDTH}:{HEIGHT}:force_original_aspect_ratio=decrease,pad={WIDTH}:{HEIGHT}:(ow-iw)/2:(oh-ih)/2:black,setsar=1")
}

/// The caption: the date, and the place smaller below it, at the bottom on the left, white with a shadow (the files keep the text, so
/// nothing in it needs escaping).
fn caption_filters(font: &Path, files: &[PathBuf]) -> String {
    let font = path(font).replace('\\', "/").replace(':', "\\:").replace('\'', "\\'");
    let mut out = String::new();
    let lines = files.len();
    for (i, f) in files.iter().enumerate() {
        let file = path(f).replace('\\', "/").replace(':', "\\:").replace('\'', "\\'");
        let (size, alpha) = if i == 0 { (44, "1") } else { (34, "0.85") };
        // From the bottom: the last line 68 px above the edge, the one above it 52 px higher.
        let y = 68 + 52 * (lines - 1 - i);
        out.push_str(&format!(
            ",drawtext=fontfile='{font}':textfile='{file}':x=48:y=h-{y}-th/2:fontsize={size}:fontcolor=white@{alpha}:shadowcolor=black@0.8:shadowx=2:shadowy=2"
        ));
    }
    out
}

/// Makes one piece: a picture held for its length (slowly closer with the zoom), or a video; the caption on it; no sound.
fn render_segment(seg: &Segment, font: Option<&Path>, dir: &Path, n: usize, out: &Path) -> std::result::Result<(), String> {
    let mut files = Vec::new();
    if font.is_some() {
        for (i, line) in seg.caption.iter().enumerate() {
            let f = dir.join(format!("caption-{n:04}-{i}.txt"));
            std::fs::write(&f, line).map_err(|e| e.to_string())?;
            files.push(f);
        }
    }
    let caption = font.map(|f| caption_filters(f, &files)).unwrap_or_default();
    let encode = [s("-c:v"), s("libx264"), s("-preset"), s("veryfast"), s("-crf"), s("18"), s("-r"), s(FPS), s("-pix_fmt"), s("yuv420p"), s("-an")];
    let mut args = vec![s("-nostdin"), s("-v"), s("error"), s("-y")];
    match &seg.video {
        Some(video) => {
            args.extend([s("-i"), path(video), s("-vf"), format!("{},fps={FPS}{caption},format=yuv420p", fit())]);
        }
        None => {
            let frames = (seg.length * FPS as f64).round().max(1.0);
            let zoom = if seg.transition == "zoom" {
                // Twice the size first, so the slow zoom moves in steps of half a pixel.
                format!(
                    ",scale={}:{},zoompan=z='1+0.12*on/{frames}':x='(iw-iw/zoom)*{:.3}':y='(ih-ih/zoom)*{:.3}':d=1:s={WIDTH}x{HEIGHT}:fps={FPS}",
                    WIDTH * 2,
                    HEIGHT * 2,
                    seg.focus.0,
                    seg.focus.1
                )
            } else {
                String::new()
            };
            args.extend([
                s("-loop"),
                s("1"),
                s("-framerate"),
                s(FPS),
                s("-t"),
                format!("{:.3}", seg.length),
                s("-i"),
                path(&seg.image),
                s("-vf"),
                format!("{}{zoom}{caption},format=yuv420p", fit()),
            ]);
        }
    }
    args.extend(encode);
    args.push(path(out));
    ffmpeg("ffmpeg", &args).map(|_| ())
}

/// Joins pieces with the transitions between them (`into[i]` brings piece `i` in; `into[0]` is not used) into one video without sound.
fn join(parts: &[(PathBuf, f64)], into: &[String], out: &Path) -> std::result::Result<f64, String> {
    if parts.len() == 1 {
        std::fs::copy(&parts[0].0, out).map_err(|e| e.to_string())?;
        return Ok(parts[0].1);
    }
    let mut args = vec![s("-nostdin"), s("-v"), s("error"), s("-y")];
    for (p, _) in parts {
        args.extend([s("-i"), path(p)]);
    }
    let mut graph = String::new();
    let mut end = parts[0].1;
    let mut last = s("0:v");
    for (i, (_, length)) in parts.iter().enumerate().skip(1) {
        let d = overlap(&into[i]);
        let offset = (end - d).max(0.0);
        let label = if i + 1 == parts.len() { s("v") } else { format!("x{i}") };
        graph.push_str(&format!("[{last}][{i}:v]xfade=transition=fade:duration={d:.4}:offset={offset:.4}[{label}];"));
        last = label;
        end = offset + length;
    }
    graph.pop();
    args.extend([s("-filter_complex"), graph, s("-map"), s("[v]")]);
    args.extend([s("-c:v"), s("libx264"), s("-preset"), s("veryfast"), s("-crf"), s("18"), s("-r"), s(FPS), s("-pix_fmt"), s("yuv420p"), path(out)]);
    ffmpeg("ffmpeg", &args)?;
    Ok(end)
}

/// Lays the sound under the picture: the music again and again (quieter where a video plays, fading out at the end) and the sound of every
/// video at its place; without either the video has no sound.
fn mux(picture: &Path, length: f64, music: &[PathBuf], videos: &[(PathBuf, f64, f64)], dir: &Path, out: &Path) -> std::result::Result<(), String> {
    let mut args = vec![s("-nostdin"), s("-v"), s("error"), s("-y"), s("-i"), path(picture)];
    let mut inputs = 1;
    let mut graph = String::new();
    let mut mix = Vec::new();
    if !music.is_empty() {
        let list = dir.join("music.txt");
        let text: String = music.iter().map(|m| format!("file '{}'\n", path(m).replace('\'', "'\\''"))).collect();
        std::fs::write(&list, text).map_err(|e| e.to_string())?;
        args.extend([s("-stream_loop"), s("-1"), s("-f"), s("concat"), s("-safe"), s("0"), s("-i"), path(&list)]);
        let quiet: Vec<String> = videos.iter().map(|(_, at, len)| format!("between(t,{at:.3},{:.3})", at + len)).collect();
        let volume = if quiet.is_empty() { s("1") } else { format!("if({},{DUCK},1)", quiet.join("+")) };
        let fade_at = (length - 2.0).max(0.0);
        graph.push_str(&format!(
            "[{inputs}:a]atrim=0:{length:.3},asetpts=PTS-STARTPTS,aresample=48000,volume='{volume}':eval=frame,afade=t=out:st={fade_at:.3}:d=2[m];"
        ));
        mix.push(s("[m]"));
        inputs += 1;
    }
    for (i, (video, at, _)) in videos.iter().enumerate() {
        args.extend([s("-i"), path(video)]);
        let ms = (at * 1000.0).round() as i64;
        graph.push_str(&format!("[{inputs}:a]aresample=48000,adelay={ms}:all=1[v{i}];"));
        mix.push(format!("[v{i}]"));
        inputs += 1;
    }
    if mix.is_empty() {
        args.extend([s("-map"), s("0:v"), s("-c:v"), s("copy"), s("-movflags"), s("+faststart"), path(out)]);
        return ffmpeg("ffmpeg", &args).map(|_| ());
    }
    graph.push_str(&format!("{}amix=inputs={}:duration=longest:normalize=0,atrim=0:{length:.3}[a]", mix.join(""), mix.len()));
    args.extend([s("-filter_complex"), graph, s("-map"), s("0:v"), s("-map"), s("[a]")]);
    args.extend([s("-c:v"), s("copy"), s("-c:a"), s("aac"), s("-b:a"), s("192k"), s("-movflags"), s("+faststart"), s("-t"), format!("{length:.3}"), path(out)]);
    ffmpeg("ffmpeg", &args).map(|_| ())
}

/// What a step of ffmpeg did wrong, as a code for the page: a full disk is said as such.
fn failed(e: String) -> Failed {
    tracing::warn!(error = %e, "ffmpeg");
    if e.contains("No space left") { because("disk_full") } else { because("ffmpeg") }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> std::result::Result<T, String> + Send + 'static) -> std::result::Result<T, Failed> {
    tokio::task::spawn_blocking(f).await.map_err(|_| because("ffmpeg"))?.map_err(failed)
}

#[allow(clippy::too_many_arguments)]
async fn make(store: &Store, library: Option<&Path>, id: i64, user: i64, query: &str, lang: Lang, work: &Path) -> std::result::Result<Made, Failed> {
    if !crate::picture_render::has_ffmpeg() {
        return Err(because("no_ffmpeg"));
    }
    let role: Role = match store.profile_by_id(user).await {
        Ok(Some(p)) => p.role.parse().unwrap_or(Role::Guest),
        _ => return Err(Failed::Gone),
    };
    if role == Role::Guest {
        return Err(Failed::Gone);
    }
    let q = crate::api_pictures::parse_query(Some(query));
    let (settings, playlist) = crate::show_project::settings(Some(query));
    let mut items = store.pictures(user, role, &q).await.map_err(|_| because("ffmpeg"))?;
    if items.is_empty() {
        return Err(because("empty"));
    }
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).map_err(|e| failed(e.to_string()))?;
    let mut dice = Dice::new();
    if settings.order == "random" {
        dice.shuffle(&mut items);
    }
    let zone = store.setting("time_zone").await.ok().flatten();
    let font = if settings.info != "none" && has_drawtext() { font() } else { None };
    if settings.info != "none" && font.is_none() {
        tracing::warn!("no font or no drawtext in ffmpeg: the video has no date and place");
    }

    // 1. What the pieces are (this asks for the pictures, the MP4s of the videos and the places: the first part of the progress).
    let total = items.len();
    let mut segments = Vec::with_capacity(total);
    for (i, item) in items.iter().enumerate() {
        if i % 10 == 0 && !wanted(store, id).await {
            return Err(Failed::Gone);
        }
        let Some(p) = piece(store, user, role, lang, &settings.info, item).await else { continue };
        let mut caption = Vec::new();
        if settings.info != "none" {
            caption.push(date_text(lang, p.taken_at, zone.as_deref()));
            if let Some(place) = p.place.filter(|_| settings.info == "both") {
                caption.push(place);
            }
        }
        let transition = match settings.transition.as_str() {
            "random" => ["fade", "zoom", "cut"][dice.below(3)].to_owned(),
            t => t.to_owned(),
        };
        let length = match &p.video {
            Some(v) => {
                let v = v.clone();
                match tokio::task::spawn_blocking(move || probe(&v)).await.ok().flatten() {
                    Some((len, _)) if len > 0.2 => len,
                    _ => settings.seconds as f64 + FADE,
                }
            }
            None => settings.seconds as f64 + FADE,
        };
        // A video that cannot be read is held as its picture.
        let video = p.video.filter(|_| length > 0.2);
        segments.push(Segment { image: p.image, video, caption, transition, length, focus: (0.2 + dice.unit() * 0.6, 0.2 + dice.unit() * 0.6) });
        progress(store, id, i + 1, total, 0, 100).await;
    }
    if segments.is_empty() {
        return Err(because("empty"));
    }
    let music: Vec<PathBuf> = {
        let mut m: Vec<PathBuf> = music_of(store, library, user, playlist).await.into_iter().map(|(p, _, _)| p).collect();
        if settings.order == "random" {
            dice.shuffle(&mut m);
        }
        m
    };

    // 2. The pieces (most of the time).
    let mut parts = Vec::with_capacity(segments.len());
    for (n, seg) in segments.iter().enumerate() {
        if !wanted(store, id).await {
            return Err(Failed::Gone);
        }
        let out = work.join(format!("piece-{n:04}.mp4"));
        let (seg2, dir, font2, out2) = (seg.clone(), work.to_path_buf(), font.clone(), out.clone());
        blocking(move || render_segment(&seg2, font2.as_deref(), &dir, n, &out2)).await?;
        parts.push((out, seg.length));
        progress(store, id, n + 1, segments.len(), 100, 800).await;
    }

    // 3. The groups, then the whole, with the transitions.
    let into: Vec<String> = segments.iter().map(|s| s.transition.clone()).collect();
    let mut groups = Vec::new();
    let chunks: Vec<_> = parts.chunks(GROUP).map(<[_]>::to_vec).collect();
    let count = chunks.len();
    for (g, chunk) in chunks.into_iter().enumerate() {
        if !wanted(store, id).await {
            return Err(Failed::Gone);
        }
        let first = g * GROUP;
        let into_g: Vec<String> = into[first..first + chunk.len()].to_vec();
        let out = work.join(format!("group-{g:03}.mp4"));
        let out2 = out.clone();
        let length = blocking(move || join(&chunk, &into_g, &out2)).await?;
        groups.push((out, length));
        progress(store, id, g + 1, count, 800, 920).await;
    }
    for p in &parts {
        let _ = std::fs::remove_file(&p.0);
    }
    // The transition that brings each group in is the one of its first piece.
    let into_groups: Vec<String> = (0..groups.len()).map(|g| into[g * GROUP].clone()).collect();
    let whole = work.join("picture.mp4");
    let (groups2, whole2) = (groups.clone(), whole.clone());
    let length = blocking(move || join(&groups2, &into_groups, &whole2)).await?;
    progress(store, id, 1, 1, 920, 960).await;

    // 4. The sound.
    let (starts, planned) = timeline(&segments);
    let videos: Vec<(PathBuf, f64, f64)> = segments
        .iter()
        .zip(&starts)
        .filter_map(|(s, at)| s.video.as_ref().map(|v| (v.clone(), *at, s.length)))
        .filter(|(v, _, _)| probe(v).is_some_and(|(_, sound)| sound))
        .collect();
    tracing::debug!(video = id, planned, length, "the length of the video");
    let target = file_of(&store.data_dir, id);
    let temp = work.join("video.mp4");
    let (w, t, d) = (whole.clone(), temp.clone(), work.to_path_buf());
    blocking(move || mux(&w, length, &music, &videos, &d, &t)).await?;
    if !wanted(store, id).await {
        return Err(Failed::Gone);
    }
    std::fs::rename(&temp, &target).or_else(|_| std::fs::copy(&temp, &target).map(|_| ())).map_err(|e| failed(e.to_string()))?;
    let size = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    Ok(Made { pictures: segments.len(), duration_ms: (length * 1000.0).round() as i64, size })
}

/// The progress of a step that runs from `from` to `to` (of 1000), `done` of `total` done.
async fn progress(store: &Store, id: i64, done: usize, total: usize, from: i64, to: i64) {
    let p = from + ((to - from) as f64 * done as f64 / total.max(1) as f64) as i64;
    let _ = store.set_video(id, "progress = ?", vec![p.clamp(0, 1000).into()]).await;
}

/// The title of a video: the title of the project of the same choice.
pub fn title(cat: &Catalog, items: &[crate::pictures::Item]) -> String {
    crate::show_project::title_of(cat, items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(transition: &str, length: f64) -> Segment {
        Segment { image: PathBuf::new(), video: None, caption: vec![], transition: transition.into(), length, focus: (0.5, 0.5) }
    }

    #[test]
    fn the_pieces_overlap_by_their_transition() {
        let (starts, end) = timeline(&[seg("fade", 9.0), seg("zoom", 9.0), seg("cut", 3.0), seg("fade", 9.0)]);
        assert_eq!(starts.len(), 4);
        // The second comes with a fade (one second over the first), the third with a cut (about one frame), the fourth with a fade.
        assert!((starts[1] - 8.0).abs() < 1e-9);
        assert!((starts[2] - (17.0 - CUT)).abs() < 1e-9, "a cut is about one frame");
        assert!((starts[3] - (19.0 - CUT)).abs() < 1e-9);
        assert!((end - (28.0 - CUT)).abs() < 1e-9);
    }

    #[test]
    fn joined_pieces_keep_every_piece_with_fades_and_cuts() {
        if !crate::picture_render::has_ffmpeg() {
            eprintln!("no ffmpeg here: skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut parts = Vec::new();
        for (i, len) in [2.0, 2.0, 1.5, 2.0, 2.0].iter().enumerate() {
            let out = dir.path().join(format!("p{i}.mp4"));
            let ok = Command::new("ffmpeg")
                .args(["-v", "error", "-y", "-f", "lavfi", "-i", &format!("color=c=0x{:02x}4080:size={WIDTH}x{HEIGHT}:rate={FPS}", i * 40), "-t", &len.to_string()])
                .args(["-c:v", "libx264", "-preset", "ultrafast", "-r", &FPS.to_string(), "-pix_fmt", "yuv420p"])
                .arg(&out)
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok);
            parts.push((out, *len));
        }
        let into: Vec<String> = ["fade", "cut", "fade", "cut", "cut"].iter().map(|s| s.to_string()).collect();
        let segs: Vec<Segment> = parts.iter().zip(&into).map(|((_, l), t)| seg(t, *l)).collect();
        let (_, planned) = timeline(&segs);
        let out = dir.path().join("all.mp4");
        let length = join(&parts, &into, &out).unwrap();
        assert!((length - planned).abs() < 1e-6);
        let (real, _) = probe(&out).unwrap();
        assert!((real - planned).abs() < 0.1, "{real} s made, {planned} s planned");
        // The groups are joined the same way: two groups of the whole again.
        let again = dir.path().join("again.mp4");
        let length2 = join(&[(out.clone(), length), (out.clone(), length)], &["fade".into(), "cut".into()], &again).unwrap();
        let (real2, _) = probe(&again).unwrap();
        assert!((real2 - length2).abs() < 0.1 && (length2 - (2.0 * length - CUT)).abs() < 1e-6, "{real2} for {length2}");
    }

    #[test]
    fn the_date_is_said_as_the_slideshow_says_it() {
        let ms = jiff::civil::date(2017, 7, 1).at(12, 0, 0, 0).in_tz("Europe/Berlin").unwrap().timestamp().as_millisecond();
        assert_eq!(date_text(Lang::De, ms, Some("Europe/Berlin")), "1. Juli 2017");
        assert_eq!(date_text(Lang::En, ms, Some("Europe/Berlin")), "July 1, 2017");
    }

    #[test]
    fn the_dice_shuffle_keeps_every_item_once() {
        let mut d = Dice::new();
        let mut v: Vec<u32> = (0..50).collect();
        d.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
    }
}
