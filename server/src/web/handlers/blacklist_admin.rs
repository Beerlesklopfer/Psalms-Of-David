// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The procedures and pages of the blacklist administration (`/admin/blacklist`). Administrators only (`AdminSession`); every change checks the media
//! again and the page it leads back to says how many marks it set and took off.

use super::*;
use crate::blacklist::{BlacklistReport, WordOutcome};

/// What the pages of the blacklist read from the address: the message, the search, the page, and the counts a change leaves for the page after it.
#[derive(Deserialize, Default)]
pub struct BlacklistQuery {
    msg: Option<String>,
    q: Option<String>,
    page: Option<usize>,
    per: Option<usize>,
    added: Option<usize>,
    removed: Option<usize>,
    marked: Option<usize>,
    /// The word the preview is asked about.
    pattern: Option<String>,
}

impl BlacklistQuery {
    fn page_query(&self) -> PageQuery {
        PageQuery { msg: self.msg.clone(), q: self.q.clone(), page: self.page, per: self.per, ..PageQuery::default() }
    }
}

/// What the forms of the blacklist send.
#[derive(Deserialize, Default)]
pub struct BlacklistForm {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    pattern: String,
    #[serde(default)]
    note: String,
    /// `book`, `podcast` or `station`, for approving a medium.
    #[serde(default)]
    kind: String,
    /// The page the form sits on, and where the answer leads back to (only pages of the blacklist are accepted).
    #[serde(default)]
    back: String,
}

/// The page to return to: one of the blacklist, never another place.
fn back_of(f: &BlacklistForm, default: &str) -> String {
    let b = f.back.trim();
    if b.starts_with("/admin/blacklist") && !b.contains("//") && !b.contains('\\') && !b.contains(['\r', '\n']) { b.to_owned() } else { default.to_owned() }
}

/// The answer to a change: back to the page, with the counts of what it did. A refusal marks the input that is wrong.
fn blacklist_done(s: &AdminSession, back: &str, outcome: std::result::Result<Option<BlacklistReport>, Failure>) -> Response {
    match outcome {
        Ok(report) => {
            let target = match report {
                Some(r) => format!("{back}{}added={}&removed={}&marked={}&msg=ok.saved", sep(back), r.added, r.removed, r.marked),
                None => format!("{back}{}msg=ok.saved", sep(back)),
            };
            if s.htmx { (StatusCode::OK, [("hx-redirect", target)]).into_response() } else { Redirect::to(&target).into_response() }
        }
        Err(f) => finish(s, back, Err(f)),
    }
}

/// The result of saving a word as a failure or the report.
fn saved(outcome: WordOutcome) -> std::result::Result<Option<BlacklistReport>, Failure> {
    match outcome {
        WordOutcome::Saved(report) => Ok(Some(report)),
        WordOutcome::Invalid => Err(Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.blacklist_pattern").at("pattern")),
        WordOutcome::Duplicate => Err(Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "err.blacklist_exists").at("pattern")),
        WordOutcome::Missing => Err(Failure::not_found()),
    }
}

pub async fn page(State(state): State<AppState>, s: AdminSession, Query(q): Query<BlacklistQuery>) -> Response {
    let report = q.added.zip(q.removed).map(|(added, removed)| BlacklistReport { added, removed, marked: q.marked.unwrap_or(0) });
    render(state.ui.blacklist(&state.store, view_for(&s, &q.page_query()), q.q.as_deref(), q.page, q.per, report).await)
}

pub async fn hits(State(state): State<AppState>, s: AdminSession, Query(q): Query<BlacklistQuery>) -> Response {
    render(state.ui.blacklist_hits(&state.store, view_for(&s, &q.page_query()), q.q.as_deref(), q.page, q.per).await)
}

/// The preview of a word (a fragment for the dialog): what it would hit.
pub async fn preview(State(state): State<AppState>, s: AdminSession, Query(q): Query<BlacklistQuery>) -> Response {
    render(state.ui.blacklist_preview(&state.store, view_for(&s, &PageQuery::default()), q.pattern.as_deref().unwrap_or_default()).await)
}

pub async fn add(State(state): State<AppState>, s: AdminSession, Form(f): Form<BlacklistForm>) -> Response {
    let back = back_of(&f, "/admin/blacklist");
    let outcome = async { saved(state.store.blacklist_add(&f.pattern, &f.note).await?) }.await;
    blacklist_done(&s, &back, outcome)
}

pub async fn update(State(state): State<AppState>, s: AdminSession, Form(f): Form<BlacklistForm>) -> Response {
    let back = back_of(&f, "/admin/blacklist");
    let outcome = async { saved(state.store.blacklist_update(f.id, &f.pattern, &f.note).await?) }.await;
    blacklist_done(&s, &back, outcome)
}

pub async fn delete(State(state): State<AppState>, s: AdminSession, Form(f): Form<BlacklistForm>) -> Response {
    let back = back_of(&f, "/admin/blacklist");
    let outcome = async { saved(state.store.blacklist_delete(f.id).await?) }.await;
    blacklist_done(&s, &back, outcome)
}

/// An administrator approves a marked medium (the mark goes and stays off while the word hits) or puts the mark back.
async fn decide(state: &AppState, s: &AdminSession, f: &BlacklistForm, approve: bool) -> Response {
    let back = back_of(f, "/admin/blacklist/hits");
    let outcome = async {
        let kind = crate::admin::pages::blacklist_pages::kind_from(&f.kind).ok_or_else(Failure::input)?;
        let changed = if approve { state.store.blacklist_approve(kind, f.id).await? } else { state.store.blacklist_unapprove(kind, f.id).await? };
        if changed { Ok(None) } else { Err(Failure::not_found()) }
    }
    .await;
    blacklist_done(s, &back, outcome)
}

pub async fn approve(State(state): State<AppState>, s: AdminSession, Form(f): Form<BlacklistForm>) -> Response {
    decide(&state, &s, &f, true).await
}

pub async fn unapprove(State(state): State<AppState>, s: AdminSession, Form(f): Form<BlacklistForm>) -> Response {
    decide(&state, &s, &f, false).await
}
