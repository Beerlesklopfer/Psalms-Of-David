// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The project of a slideshow (docs/spec-pictures.md, 6): one file that holds a whole slideshow, so that it lies on a device by itself and
//! plays without the server and without a network (project owner, 2026-11-08). It is a ZIP archive:
//!
//! | Entry | What |
//! |---|---|
//! | `show.json` | the slideshow: the format, the choices, the pictures and the music, in order (read by the apps) |
//! | `show.js` | the same as a script (`window.DAVID_SHOW = …`), for a browser that opens `index.html` from the disk, where it may not load JSON |
//! | `index.html`, `slideshow.js`, `slideshow.css` | the player of the browser: the slideshow of `/pictures`, reading the project |
//! | `pictures/0001.jpg` … | every picture in the screen size, upright (the frame of a video as its poster) |
//! | `videos/0002.mp4` … | every video as H.264/AAC, which every device plays |
//! | `music/01.mp3` … | the music of the chosen playlist, as MP3 |
//!
//! The archive is written while it is sent (nothing is held in memory or on the disk beyond the cache of the pictures); `show.json` comes
//! last, so it names only what is in the archive: a picture the server cannot show is left out. The same rules of access apply as for
//! the slideshow itself.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::admin::i18n::{Catalog, Lang};
use crate::picture_render::Size;
use crate::roles::Role;
use crate::store::Store;
use crate::web::download::{Sink, content_disposition, safe_name, zip_bytes, zip_file};
use crate::zipstream::ZipWriter;

/// What `show.json` says it is.
pub const FORMAT: &str = "david-slideshow";
/// The version of the format; a player refuses a newer one.
pub const VERSION: u32 = 1;

/// The choices of the slideshow, as the form of `/pictures` names them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    pub order: String,
    pub seconds: u32,
    pub transition: String,
    pub videos: String,
    pub info: String,
}

/// The choices in a query (`order`, `seconds`, `transition`, `videos`, `info`, `music`); what is not one of the choices is the default.
pub fn settings(raw: Option<&str>) -> (Settings, Option<i64>) {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(raw.unwrap_or_default()).unwrap_or_default();
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str()).unwrap_or_default();
    let one_of = |k: &str, options: &[&str]| options.iter().find(|o| **o == get(k)).copied().unwrap_or(options[0]).to_owned();
    let seconds = get("seconds").parse().ok().filter(|s| [3, 5, 8, 15, 30].contains(s)).unwrap_or(8);
    let s = Settings {
        order: one_of("order", &["date", "random"]),
        seconds,
        transition: one_of("transition", &["fade", "zoom", "cut", "random"]),
        videos: one_of("videos", &["on", "off"]),
        info: one_of("info", &["both", "date", "none"]),
    };
    (s, get("music").parse().ok())
}

#[derive(Debug, Serialize)]
struct Picture {
    image: String,
    video: Option<String>,
    taken_at: i64,
    place: Option<String>,
    /// The transition and the seconds of this picture in a project put together by hand (`crate::slideshow`); left out: those of
    /// `settings`. A player that does not know them plays the picture with those of `settings`.
    #[serde(skip_serializing_if = "Option::is_none")]
    transition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seconds: Option<u32>,
}

#[derive(Debug, Serialize)]
struct Track {
    file: String,
    title: String,
    artist: String,
}

#[derive(Debug, Serialize)]
struct Show<'a> {
    format: &'static str,
    version: u32,
    title: &'a str,
    lang: &'static str,
    made_at: i64,
    settings: &'a Settings,
    pictures: &'a [Picture],
    music: &'a [Track],
}

/// The answer to a request for a project: the archive as a download, written while it is sent; 404 when nothing is chosen that the person may see.
pub async fn response(store: Store, library: Option<PathBuf>, user: i64, role: Role, lang: Lang, raw: Option<String>) -> Response {
    if role == Role::Guest {
        return StatusCode::NOT_FOUND.into_response();
    }
    let chosen = match crate::slideshow::chosen(&store, user, role, raw.as_deref()).await {
        Ok(Some(c)) if !c.entries.is_empty() => c,
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(error = %e, "the pictures of a project failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let cat = Catalog::new(lang);
    let title = chosen.title.clone().unwrap_or_else(|| title_of(&cat, &chosen.entries));
    let crate::slideshow::Chosen { entries, settings, music, .. } = chosen;
    let mut res = Response::new(Body::empty());
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&content_disposition(&format!("{}.zip", safe_name(&title, "slideshow").trim_end_matches('.')))) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    let (mut sink, body) = Sink::new();
    *res.body_mut() = body;
    tokio::spawn(crate::actor::run_as(crate::actor::profile(user), async move {
        let job = Job { store, library, user, role, lang, settings, music, title };
        let (complete, pictures) = job.write(&mut sink, entries).await;
        tracing::info!(user, pictures, bytes = sink.sent, complete, "project of a slideshow");
    }));
    res
}

/// "Bilder 2017–2019": the name of the page and the years the pictures were taken.
pub fn title_of(cat: &Catalog, entries: &[crate::slideshow::Entry]) -> String {
    let year = |ms: i64| time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000)).map(|t| t.year()).ok();
    let first = entries.iter().map(|e| e.item.taken_at).min().and_then(year);
    let last = entries.iter().map(|e| e.item.taken_at).max().and_then(year);
    let name = cat.t("pictures.title");
    match (first, last) {
        (Some(a), Some(b)) if a == b => format!("{name} {a}"),
        (Some(a), Some(b)) => format!("{name} {a}\u{2013}{b}"),
        _ => name.to_owned(),
    }
}

struct Job {
    store: Store,
    library: Option<PathBuf>,
    user: i64,
    role: Role,
    lang: Lang,
    settings: Settings,
    music: Option<i64>,
    title: String,
}

/// What one picture of a slideshow is made of, on the disk of the server: the screen size (for a video its frame), the MP4 of a video, the
/// time and the place. Shared by the project and the video of a slideshow (`crate::show_video`).
pub struct Piece {
    pub image: PathBuf,
    pub video: Option<PathBuf>,
    pub taken_at: i64,
    pub place: Option<String>,
}

/// The piece of one picture the person may see; `None` when the server cannot show it (it is left out then). A video without its MP4 (no
/// ffmpeg, a damaged file) is its picture. The place only with `info` = `both`.
pub async fn piece(store: &Store, user: i64, role: Role, lang: Lang, info: &str, item: &crate::pictures::Item) -> Option<Piece> {
    let found = store.picture(user, role, &item.key).await.ok().flatten()?;
    let image = match crate::picture_render::rendered(&store.data_dir, &item.key, &found.path, &found.mime, Size::Screen).await {
        Some(p) => p,
        None => match found.device_thumb.clone() {
            Some(p) => p,
            None => {
                tracing::warn!(key = %item.key, "a picture the server cannot show is left out of the slideshow");
                return None;
            }
        },
    };
    let video = if item.video { crate::picture_render::video_mp4(&store.data_dir, &item.key, &found.path).await } else { None };
    let place = if info == "both" && !item.video {
        store.picture_info(&found, lang.code()).await.ok().and_then(|v| v.get("place").and_then(|p| p.as_str()).map(str::to_owned))
    } else {
        None
    };
    Some(Piece { image, video, taken_at: found.taken_at, place })
}

/// The music of an own playlist the person may hear now, each track as an MP3 file: (path, title, artist).
pub async fn music_of(store: &Store, library: Option<&std::path::Path>, user: i64, playlist: Option<i64>) -> Vec<(PathBuf, String, String)> {
    let Some(id) = playlist else { return Vec::new() };
    let entries = match store.playlist_view(id, user).await {
        Ok(Some(v)) => v.entries.into_iter().filter(|e| e.available).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for e in entries {
        match crate::api::track_file(store, library, user, e.track_id, true).await {
            Ok((_, path, _, _)) => out.push((path, e.title, e.author)),
            Err(_) => tracing::warn!(track = e.track_id, "a track is left out of the slideshow"),
        }
    }
    out
}

impl Job {
    /// Writes the archive; true when all of it went out, and how many pictures it holds.
    async fn write(&self, sink: &mut Sink, entries: Vec<crate::slideshow::Entry>) -> (bool, usize) {
        let mut zip = ZipWriter::new();
        let mut pictures = Vec::with_capacity(entries.len());
        for entry in entries {
            let Some(p) = piece(&self.store, self.user, self.role, self.lang, &self.settings.info, &entry.item).await else { continue };
            let n = pictures.len() + 1;
            let image = format!("pictures/{n:04}.jpg");
            if !put_file(sink, &mut zip, &image, &p.image).await {
                return (false, pictures.len());
            }
            let mut video = None;
            if let Some(mp4) = &p.video {
                let name = format!("videos/{n:04}.mp4");
                if !put_file(sink, &mut zip, &name, mp4).await {
                    return (false, pictures.len());
                }
                video = Some(name);
            }
            pictures.push(Picture { image, video, taken_at: p.taken_at, place: p.place, transition: entry.transition, seconds: entry.seconds });
        }
        let mut music = Vec::new();
        for (path, title, artist) in music_of(&self.store, self.library.as_deref(), self.user, self.music).await {
            let file = format!("music/{:02}.mp3", music.len() + 1);
            if !put_file(sink, &mut zip, &file, &path).await {
                return (false, pictures.len());
            }
            music.push(Track { file, title, artist });
        }
        let show = Show {
            format: FORMAT,
            version: VERSION,
            title: &self.title,
            lang: self.lang.code(),
            made_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64,
            settings: &self.settings,
            pictures: &pictures,
            music: &music,
        };
        let json = serde_json::to_string(&show).unwrap_or_default();
        // `</` cannot end the script early when the JSON stands in one.
        let script = format!("window.DAVID_SHOW = {};\n", json.replace("</", "<\\/"));
        let texts: [(&str, Vec<u8>); 5] = [
            ("show.json", json.into_bytes()),
            ("show.js", script.into_bytes()),
            ("index.html", index_html(&Catalog::new(self.lang), &self.title).into_bytes()),
            ("slideshow.js", include_bytes!("../static/slideshow.js").to_vec()),
            ("slideshow.css", include_bytes!("../static/slideshow.css").to_vec()),
        ];
        for (name, bytes) in texts {
            if !zip_bytes(sink, &mut zip, name, bytes).await {
                return (false, pictures.len());
            }
        }
        (sink.put(zip.finish()).await, pictures.len())
    }
}

async fn put_file(sink: &mut Sink, zip: &mut ZipWriter, name: &str, path: &std::path::Path) -> bool {
    match tokio::fs::metadata(path).await {
        Ok(m) => zip_file(sink, zip, name, path, m.len()).await,
        Err(_) => {
            sink.abort("a file of the project cannot be read").await;
            false
        }
    }
}

/// The page of the player: `static/show-project.html` with the texts of the language.
pub fn index_html(cat: &Catalog, title: &str) -> String {
    let mut page = include_str!("../static/show-project.html").to_owned();
    page = page.replace("{lang}", cat.lang().code()).replace("{title}", &escape(title));
    while let Some(at) = page.find("{t:") {
        let Some(end) = page[at..].find('}') else { break };
        let key = page[at + 3..at + end].to_owned();
        page.replace_range(at..at + end + 1, &escape(cat.t(&key)));
    }
    page
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choices_take_only_what_the_form_offers() {
        let (s, music) = settings(Some("order=random&seconds=15&transition=zoom&videos=off&info=date&music=7"));
        assert_eq!(s, Settings { order: "random".into(), seconds: 15, transition: "zoom".into(), videos: "off".into(), info: "date".into() });
        assert_eq!(music, Some(7));
        let (s, music) = settings(Some("order=x&seconds=4&transition=<b>&music=no"));
        assert_eq!(s, Settings { order: "date".into(), seconds: 8, transition: "fade".into(), videos: "on".into(), info: "both".into() });
        assert_eq!(music, None);
    }

    #[test]
    fn the_page_of_the_player_has_the_texts_and_no_placeholder() {
        let page = index_html(&Catalog::new(Lang::De), "Bilder <2019>");
        assert!(page.contains("<html lang=\"de\">") && page.contains("Bilder &lt;2019&gt;"), "{page}");
        assert!(!page.contains("{t:") && !page.contains("{title}"), "{page}");
        assert!(page.contains("src=\"show.js\"") && page.contains("slideshow.js"));
    }
}
