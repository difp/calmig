#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(target_os = "linux")]
mod following_webview;
#[cfg(test)]
use calmig_core::Session as RustSession;
use calmig_core::{Account as RustAccount, Client as RustClient};
use chrono::{LocalResult, TimeZone};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[derive(Default)]
struct RustAdapter {
    inner: Mutex<RustAdapterState>,
}

#[derive(Default)]
struct RustAdapterState {
    generation: u64,
    client: Option<Arc<RustClient>>,
    calendar: Option<Arc<CalendarJob>>,
    next_calendar_id: u64,
    inbox_generation: u64,
    allowed_threads: HashSet<String>,
    message_cursors: HashMap<String, String>,
    message_cursor_history: HashMap<String, HashSet<String>>,
    sending_threads: HashSet<String>,
    inbox_cursor: Option<String>,
    inbox_cursor_history: HashSet<String>,
    inbox_seq_id: Option<String>,
    inbox_ready: bool,
    inbox_loading: bool,
    comment_generation: u64,
    photo_scope: u64,
    comment: Option<CommentState>,
    sending_comments: HashSet<String>,
    profile: Option<NativeProfileState>,
    calendar_view: Option<UiCalendar>,
    calendar_epoch: u64,
    auth_busy: bool,
}

struct AuthGuard {
    adapter: Arc<RustAdapter>,
    generation: u64,
}
impl Drop for AuthGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.adapter.inner.lock() {
            if state.generation == self.generation {
                state.auth_busy = false;
            }
        }
    }
}

struct UiCalendar {
    username: String,
    directory: std::path::PathBuf,
    photos: Vec<Value>,
    previous: Vec<Value>,
    status: &'static str,
    accounts: usize,
    incomplete: bool,
    feedback_required: bool,
    following_mode: bool,
    cached: bool,
    revision: u64,
    job_id: Option<u64>,
    job_revision: u64,
    last_save: std::time::Instant,
    last_saved_stamp: u64,
    legacy_session_file: Option<std::path::PathBuf>,
    legacy_session_stamp: Option<u64>,
}

fn calendar_bounds() -> Result<(i64, i64), ()> {
    let now = chrono::Local::now();
    let date = now
        .date_naive()
        .checked_sub_days(chrono::Days::new(6))
        .ok_or(())?;
    let midnight = date.and_hms_opt(0, 0, 0).ok_or(())?;
    let mut cutoff = None;
    // In rare zones where midnight is skipped, use the first valid local time.
    for minute in 0..=180 {
        let time = midnight
            .checked_add_signed(chrono::Duration::minutes(minute))
            .ok_or(())?;
        match chrono::Local.from_local_datetime(&time) {
            LocalResult::Single(value) => {
                cutoff = Some(value);
                break;
            }
            LocalResult::Ambiguous(first, _) => {
                cutoff = Some(first);
                break;
            }
            LocalResult::None => {}
        }
    }
    Ok((cutoff.ok_or(())?.timestamp_millis(), now.timestamp_millis()))
}

struct NativeProfileState {
    id: String,
    username: String,
    now: i64,
    cursor: Option<String>,
    seen: HashSet<String>,
    posts: HashSet<String>,
    loading: bool,
}

struct ProfileLoadGuard<'a> {
    adapter: &'a RustAdapter,
    generation: u64,
    photo_scope: u64,
}
impl Drop for ProfileLoadGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.adapter.inner.lock() {
            if state.generation == self.generation && state.photo_scope == self.photo_scope {
                if let Some(profile) = state.profile.as_mut() {
                    profile.loading = false;
                }
            }
        }
    }
}

struct CommentState {
    code: String,
    max_id: Option<String>,
    min_id: Option<String>,
    seen: HashSet<(Option<String>, Option<String>)>,
    loading: bool,
}

struct CommentLoadGuard<'a> {
    adapter: &'a RustAdapter,
    generation: u64,
    comment_generation: u64,
}
impl Drop for CommentLoadGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.adapter.inner.lock() {
            if state.generation == self.generation
                && state.comment_generation == self.comment_generation
            {
                if let Some(comment) = state.comment.as_mut() {
                    comment.loading = false;
                }
            }
        }
    }
}

struct CommentSendGuard<'a> {
    adapter: &'a RustAdapter,
    generation: u64,
    code: String,
}
impl Drop for CommentSendGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.adapter.inner.lock() {
            if state.generation == self.generation {
                state.sending_comments.remove(&self.code);
            }
        }
    }
}

struct InboxLoadGuard<'a> {
    adapter: &'a RustAdapter,
    generation: u64,
}
impl Drop for InboxLoadGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.adapter.inner.lock() {
            if state.generation == self.generation {
                state.inbox_loading = false;
            }
        }
    }
}

struct ThreadSendGuard<'a> {
    adapter: &'a RustAdapter,
    generation: u64,
    thread: String,
}

impl Drop for ThreadSendGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.adapter.inner.lock() {
            if state.generation == self.generation {
                state.sending_threads.remove(&self.thread);
            }
        }
    }
}

struct CalendarJob {
    id: u64,
    cancelled: AtomicBool,
    state: Mutex<(u64, Option<calmig_core::CalendarState>)>,
}

impl RustAdapter {
    fn generation(&self) -> Result<u64, String> {
        self.inner
            .lock()
            .map(|state| state.generation)
            .map_err(|_| "Ошибка адаптера".into())
    }
    fn clear(&self) -> Result<(), String> {
        let mut state = self.inner.lock().map_err(|_| "Ошибка адаптера")?;
        state.generation = state.generation.wrapping_add(1);
        state.auth_busy = false;
        state.inbox_generation = state.inbox_generation.wrapping_add(1);
        state.allowed_threads.clear();
        state.message_cursors.clear();
        state.message_cursor_history.clear();
        state.sending_threads.clear();
        state.inbox_cursor = None;
        state.inbox_cursor_history.clear();
        state.inbox_seq_id = None;
        state.inbox_ready = false;
        state.inbox_loading = false;
        state.comment_generation = state.comment_generation.wrapping_add(1);
        state.photo_scope = state.photo_scope.wrapping_add(1);
        state.comment = None;
        state.profile = None;
        state.calendar_view = None;
        state.calendar_epoch = state.calendar_epoch.wrapping_add(1);
        state.sending_comments.clear();
        if let Some(job) = state.calendar.take() {
            job.cancelled.store(true, Ordering::Relaxed);
        }
        let previous = state.client.take();
        drop(state);
        // reqwest's blocking client owns a runtime, which must not be dropped
        // directly inside a Tauri async command.
        if let Some(previous) = previous {
            std::thread::spawn(move || drop(previous));
        }
        Ok(())
    }
    fn disconnect_native(&self) -> Result<bool, String> {
        let (directory, username, session_file) = {
            let state = self.inner.lock().map_err(|_| "Ошибка адаптера")?;
            if state.client.is_none() {
                return Ok(false);
            }
            let view = state.calendar_view.as_ref().ok_or("Ошибка адаптера")?;
            (
                view.directory.clone(),
                view.username.clone(),
                view.legacy_session_file.clone(),
            )
        };
        self.clear()?;
        let state = self.inner.lock().map_err(|_| "Ошибка адаптера")?;
        if state.client.is_some() {
            return Err("Аккаунт изменился во время выхода".into());
        }
        calmig_core::clear_calendar_cache(&directory, &username)
            .map_err(|_| "Не удалось очистить кеш")?;
        if let Some(file) = session_file {
            match std::fs::remove_file(file) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("Не удалось удалить сессию".into()),
            }
        }
        Ok(true)
    }
    #[cfg(test)]
    fn attach(&self, generation: u64, value: &Value) -> Option<Arc<RustClient>> {
        let bytes = serde_json::to_vec(value).ok()?;
        let session = RustSession::from_legacy_json(&bytes).ok()?;
        let client = Arc::new(RustClient::new(session).ok()?);
        let mut state = self.inner.lock().ok()?;
        if state.generation != generation {
            return None;
        }
        state.client = Some(Arc::clone(&client));
        Some(client)
    }

    fn load_calendar(username: &str, directory: &str) -> Result<UiCalendar, ()> {
        if username.is_empty()
            || username.len() > 30
            || !username
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
            || username == "."
            || username == ".."
            || directory.is_empty()
            || directory.len() > 4096
        {
            return Err(());
        }
        let (cutoff, now) = calendar_bounds()?;
        let directory = std::path::PathBuf::from(directory);
        let photos =
            calmig_core::load_calendar_cache(&directory, username, cutoff, now).unwrap_or_default();
        Ok(UiCalendar {
            username: username.to_owned(),
            directory,
            cached: !photos.is_empty(),
            photos,
            previous: Vec::new(),
            status: "complete",
            accounts: 0,
            incomplete: true,
            feedback_required: false,
            following_mode: false,
            revision: 1,
            job_id: None,
            job_revision: 0,
            last_save: std::time::Instant::now(),
            last_saved_stamp: 0,
            legacy_session_file: None,
            legacy_session_stamp: None,
        })
    }

    #[cfg(test)]
    fn configure_calendar(
        &self,
        generation: u64,
        username: &str,
        directory: &str,
    ) -> Result<(), ()> {
        let calendar = Self::load_calendar(username, directory)?;
        let mut state = self.inner.lock().map_err(|_| ())?;
        if state.generation != generation || state.client.is_none() {
            return Err(());
        }
        state.calendar_view = Some(calendar);
        Ok(())
    }

    fn attach_native_client(
        &self,
        generation: u64,
        client: RustClient,
        username: &str,
        directory: &str,
        legacy_session_file: Option<std::path::PathBuf>,
    ) -> Result<(), ()> {
        let mut calendar = Self::load_calendar(username, directory)?;
        calendar.legacy_session_file = legacy_session_file;
        let mut state = self.inner.lock().map_err(|_| ())?;
        if state.generation != generation || state.client.is_some() {
            return Err(());
        }
        state.client = Some(Arc::new(client));
        state.calendar_view = Some(calendar);
        Ok(())
    }

    fn begin_auth(self: &Arc<Self>) -> Result<AuthGuard, String> {
        let mut state = self.inner.lock().map_err(|_| "Ошибка адаптера")?;
        if state.client.is_some() || state.auth_busy {
            return Err("Вход уже выполняется".into());
        }
        state.auth_busy = true;
        Ok(AuthGuard {
            adapter: Arc::clone(self),
            generation: state.generation,
        })
    }

    /// Persist refreshed legacy credentials without exposing them to the UI or logs.
    fn persist_native_session(&self, generation: u64) {
        use std::hash::{Hash, Hasher};
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.generation != generation {
            return;
        }
        let Some(client) = state.client.as_ref() else {
            return;
        };
        let Ok(update) = client.session_update() else {
            return;
        };
        let Some(view) = state.calendar_view.as_mut() else {
            return;
        };
        let Some(path) = view.legacy_session_file.as_ref() else {
            return;
        };
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        update.session_id.hash(&mut hash);
        update.csrf.hash(&mut hash);
        update.authorization.hash(&mut hash);
        update.claim.hash(&mut hash);
        let stamp = hash.finish();
        if view.legacy_session_stamp == Some(stamp) {
            return;
        }
        if calmig_core::update_legacy_session(path, &update).is_ok() {
            view.legacy_session_stamp = Some(stamp);
        }
    }

    #[cfg(target_os = "linux")]
    fn prepare_web_calendar(&self) -> Result<(Arc<CalendarJob>, String, i64, i64), ()> {
        let (cutoff, now) = calendar_bounds()?;
        let (job, account_id) = {
            let mut state = self.inner.lock().map_err(|_| ())?;
            if state.auth_busy {
                return Err(());
            }
            let account_id = state.client.as_ref().ok_or(())?.account_id().to_owned();
            if state.calendar_view.as_ref().ok_or(())?.status == "loading" {
                return Err(());
            }
            state.calendar_epoch = state.calendar_epoch.wrapping_add(1);
            if let Some(previous) = state.calendar.take() {
                previous.cancelled.store(true, Ordering::Relaxed);
            }
            state.next_calendar_id = state.next_calendar_id.checked_add(1).ok_or(())?;
            let job = Arc::new(CalendarJob {
                id: state.next_calendar_id,
                cancelled: AtomicBool::new(false),
                state: Mutex::new((0, None)),
            });
            state.calendar = Some(Arc::clone(&job));
            let view = state.calendar_view.as_mut().ok_or(())?;
            view.previous = view.photos.clone();
            view.cached = !view.previous.is_empty();
            view.status = "loading";
            view.accounts = 0;
            view.incomplete = true;
            view.feedback_required = false;
            view.following_mode = true;
            view.revision = view.revision.saturating_add(1);
            view.job_revision = 0;
            view.job_id = Some(job.id);
            (job, account_id)
        };
        Ok((job, account_id, cutoff, now))
    }

    #[cfg(target_os = "linux")]
    fn start_web_calendar(&self, window: WebviewWindow) -> Result<Value, ()> {
        if window.is_visible().map_err(|_| ())? {
            return Err(());
        }
        let (job, account_id, cutoff, now) = self.prepare_web_calendar()?;
        std::thread::spawn(move || {
            let url = "https://www.instagram.com/?variant=following".parse();
            if url.ok().is_none_or(|url| window.navigate(url).is_err()) {
                if let Ok(mut state) = job.state.lock() {
                    state.0 = state.0.saturating_add(1);
                    state.1 = Some(calmig_core::CalendarState {
                        status: calmig_core::CalendarStatus::Error,
                        accounts: 0,
                        photos: Vec::new(),
                        incomplete: true,
                        feedback_required: false,
                    });
                }
                return;
            }
            let source = following_webview::BrowserFollowingSource {
                window,
                job: Arc::clone(&job),
                account_id,
            };
            calmig_core::sync_following_calendar(
                &source,
                cutoff.div_euclid(1000),
                now.div_euclid(1000),
                &job.cancelled,
                |value| {
                    if let Ok(mut state) = job.state.lock() {
                        state.0 = state.0.saturating_add(1);
                        state.1 = Some(value);
                    }
                },
            );
        });
        Ok(json!({"started":true}))
    }

    fn calendar_request(&self, request: &Value) -> Result<Value, ()> {
        let action = request.get("action").and_then(Value::as_str).ok_or(())?;
        match action {
            "calendar-start" => {
                let (cutoff, now) = calendar_bounds()?;
                let (generation, epoch) = {
                    let mut state = self.inner.lock().map_err(|_| ())?;
                    if state.client.is_none() {
                        return Err(());
                    }
                    let generation = state.generation;
                    if state.calendar_view.as_ref().ok_or(())?.status == "loading" {
                        return Err(());
                    }
                    state.calendar_epoch = state.calendar_epoch.wrapping_add(1);
                    let epoch = state.calendar_epoch;
                    let view = state.calendar_view.as_mut().ok_or(())?;
                    view.previous = view.photos.clone();
                    view.cached = !view.previous.is_empty();
                    view.status = "loading";
                    view.accounts = 0;
                    view.incomplete = false;
                    view.feedback_required = false;
                    view.following_mode = false;
                    view.revision = view.revision.saturating_add(1);
                    view.job_revision = 0;
                    view.job_id = None;
                    (generation, epoch)
                };
                let started = self.native_request(&json!({"action":"calendar-sync-start","generation":generation,"epoch":epoch,"cutoff":cutoff.div_euclid(1000),"now":now.div_euclid(1000)}));
                let job_id = match started
                    .as_ref()
                    .ok()
                    .and_then(|result| result.get("job"))
                    .and_then(Value::as_u64)
                {
                    Some(id) => id,
                    None => {
                        if let Ok(mut state) = self.inner.lock() {
                            if state.generation == generation && state.calendar_epoch == epoch {
                                if let Some(view) = state.calendar_view.as_mut() {
                                    view.status = "error";
                                    view.incomplete = true;
                                    view.revision = view.revision.saturating_add(1);
                                }
                            }
                        }
                        return Err(());
                    }
                };
                let mut state = self.inner.lock().map_err(|_| ())?;
                if state.generation != generation || state.calendar_epoch != epoch {
                    if let Some(job) = state.calendar.as_ref().filter(|job| job.id == job_id) {
                        job.cancelled.store(true, Ordering::Relaxed);
                    }
                    return Err(());
                }
                state.calendar_view.as_mut().ok_or(())?.job_id = Some(job_id);
                Ok(json!({"started":true}))
            }
            "calendar-stop" => {
                let state = self.inner.lock().map_err(|_| ())?;
                if state.client.is_none() || state.calendar_view.is_none() {
                    return Err(());
                }
                if let Some(job) = &state.calendar {
                    job.cancelled.store(true, Ordering::Relaxed);
                }
                Ok(json!({"stopping":true}))
            }
            "calendar-clear" => {
                let mut state = self.inner.lock().map_err(|_| ())?;
                if state.client.is_none() {
                    return Err(());
                }
                state.calendar_epoch = state.calendar_epoch.wrapping_add(1);
                if let Some(job) = state.calendar.take() {
                    job.cancelled.store(true, Ordering::Relaxed);
                }
                let view = state.calendar_view.as_mut().ok_or(())?;
                view.photos.clear();
                view.previous.clear();
                view.status = "complete";
                view.accounts = 0;
                view.incomplete = false;
                view.feedback_required = false;
                view.cached = false;
                view.job_id = None;
                view.revision = view.revision.saturating_add(1);
                calmig_core::clear_calendar_cache(&view.directory, &view.username)
                    .map_err(|_| ())?;
                Ok(json!({"cleared":true}))
            }
            "calendar-status" => {
                use std::hash::{Hash, Hasher};
                let mut state = self.inner.lock().map_err(|_| ())?;
                if state.client.is_none() {
                    return Err(());
                }
                let snapshot = state.calendar.as_ref().and_then(|job| {
                    job.state
                        .lock()
                        .ok()
                        .map(|value| (job.id, value.0, value.1.clone()))
                });
                let view = state.calendar_view.as_mut().ok_or(())?;
                if let Some((job_id, revision, Some(calendar))) = snapshot {
                    if view.job_id == Some(job_id) && revision > view.job_revision {
                        let status = match calendar.status {
                            calmig_core::CalendarStatus::Loading => "loading",
                            calmig_core::CalendarStatus::Complete => "complete",
                            calmig_core::CalendarStatus::Cancelled => "cancelled",
                            calmig_core::CalendarStatus::Error => "error",
                        };
                        let mut photos = HashMap::<String, Value>::new();
                        let keep_previous = status != "complete" || calendar.incomplete;
                        if keep_previous {
                            for post in &view.previous {
                                if let Some(id) = post.get("id").and_then(Value::as_str) {
                                    photos.insert(id.to_owned(), post.clone());
                                }
                            }
                        }
                        for post in &calendar.photos {
                            let time = chrono::DateTime::from_timestamp(post.published_at, 0)
                                .ok_or(())?
                                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                            photos.insert(post.code.clone(),json!({"id":post.code,"username":post.username,"url":post.url,"images":post.images,"thumbnail":post.thumbnail,"caption":post.caption,"publishedAt":time}));
                        }
                        view.photos = photos.into_values().collect();
                        view.photos.sort_by_cached_key(|post| {
                            std::cmp::Reverse(
                                post.get("publishedAt")
                                    .and_then(Value::as_str)
                                    .and_then(|value| {
                                        chrono::DateTime::parse_from_rfc3339(value).ok()
                                    })
                                    .map(|time| time.timestamp_millis()),
                            )
                        });
                        view.status = status;
                        view.accounts = calendar.accounts;
                        view.incomplete = calendar.incomplete;
                        view.feedback_required = calendar.feedback_required;
                        view.cached = keep_previous && !view.previous.is_empty();
                        view.job_revision = revision;
                        view.revision = view.revision.saturating_add(1);
                        let mut stamp = std::collections::hash_map::DefaultHasher::new();
                        for post in &view.photos {
                            for key in ["id", "thumbnail", "caption"] {
                                post.get(key).and_then(Value::as_str).hash(&mut stamp);
                            }
                        }
                        let stamp = stamp.finish();
                        if status != "loading"
                            || (!view.photos.is_empty()
                                && stamp != view.last_saved_stamp
                                && view.last_save.elapsed() >= Duration::from_millis(1500))
                        {
                            if let Ok((cutoff, now)) = calendar_bounds() {
                                let _ = calmig_core::save_calendar_cache(
                                    &view.directory,
                                    &view.username,
                                    &view.photos,
                                    cutoff,
                                    now,
                                );
                            }
                            view.last_save = std::time::Instant::now();
                            view.last_saved_stamp = stamp;
                        }
                    }
                }
                if request.get("revision").and_then(Value::as_u64) == Some(view.revision) {
                    return Ok(json!({"unchanged":true,"revision":view.revision}));
                }
                Ok(
                    json!({"status":view.status,"accounts":view.accounts,"photos":view.photos,"incomplete":view.incomplete,"cached":view.cached,"revision":view.revision,"source":if view.following_mode {"following"} else {"profiles"},"reason":if view.feedback_required {Some("feedback_required")} else {None::<&str>}}),
                )
            }
            _ => Err(()),
        }
    }

    /// Handle the UI's Direct protocol in Rust when a native session is attached.
    fn direct_request(&self, request: &Value) -> Result<Value, ()> {
        let action = request.get("action").and_then(Value::as_str).ok_or(())?;
        let generation = {
            let state = self.inner.lock().map_err(|_| ())?;
            if state.client.is_none() {
                return Err(());
            }
            state.generation
        };
        match action {
            "threads" => {
                let more = request
                    .get("more")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let (cursor, seq_id, _guard) = {
                    let mut state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation || state.inbox_loading {
                        return Err(());
                    }
                    if more && (!state.inbox_ready || state.inbox_cursor.is_none()) {
                        return Err(());
                    }
                    if !more {
                        state.inbox_cursor = None;
                        state.inbox_cursor_history.clear();
                        state.inbox_seq_id = None;
                        state.inbox_ready = false;
                    }
                    state.inbox_loading = true;
                    let guard = InboxLoadGuard {
                        adapter: self,
                        generation,
                    };
                    (
                        state.inbox_cursor.clone(),
                        state.inbox_seq_id.clone(),
                        guard,
                    )
                };
                let result = self.native_request(&json!({"action":"inbox-page", "generation":generation,"cursor":cursor,"seqId":seq_id}))?;
                let cursor = result
                    .get("cursor")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let seq_id = result
                    .get("seqId")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let mut state = self.inner.lock().map_err(|_| ())?;
                if state.generation != generation {
                    return Err(());
                }
                if let Some(next) = cursor.as_ref() {
                    if !state.inbox_cursor_history.insert(next.clone()) {
                        return Err(());
                    }
                }
                state.inbox_cursor = cursor;
                state.inbox_seq_id = seq_id;
                state.inbox_ready = true;
                Ok(
                    json!({"threads":result.get("threads").ok_or(())?, "hasMore":state.inbox_cursor.is_some()}),
                )
            }
            "messages" => {
                let thread = request.get("thread").and_then(Value::as_str).ok_or(())?;
                let cursor = match request.get("cursor") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(value)) => Some(value.as_str()),
                    _ => return Err(()),
                };
                let result = self.native_request(&json!({"action":"messages-page","generation":generation,"thread":thread,"cursor":cursor}))?;
                let messages = result.get("messages").and_then(Value::as_array).ok_or(())?.iter().map(|message| {
                    let timestamp = message.get("timestampMs").and_then(Value::as_i64).and_then(chrono::DateTime::from_timestamp_millis).ok_or(())?;
                    Ok(json!({"id":message.get("id").ok_or(())?,"mine":message.get("mine").ok_or(())?,"attachment":message.get("attachment").ok_or(())?,"text":message.get("text").ok_or(())?,"timestamp":timestamp.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)}))
                }).collect::<Result<Vec<_>, ()>>()?;
                {
                    let state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation || !state.allowed_threads.contains(thread) {
                        return Err(());
                    }
                }
                Ok(json!({"messages":messages,"cursor":result.get("cursor").ok_or(())?}))
            }
            "send" => {
                let thread = request.get("thread").and_then(Value::as_str).ok_or(())?;
                let raw = request.get("text").and_then(Value::as_str).ok_or(())?;
                if raw.encode_utf16().count() > 1000 || raw.trim().is_empty() {
                    return Err(());
                }
                self.native_request(&json!({"action":"message-send","generation":generation,"thread":thread,"text":raw.trim()}))
            }
            _ => Err(()),
        }
    }

    fn invalidate_photos(&self) -> Result<(u64, u64, bool), String> {
        let mut state = self.inner.lock().map_err(|_| "Ошибка адаптера")?;
        state.photo_scope = state.photo_scope.wrapping_add(1);
        state.comment_generation = state.comment_generation.wrapping_add(1);
        state.comment = None;
        state.profile = None;
        Ok((state.generation, state.photo_scope, state.client.is_some()))
    }

    fn profile_request(
        &self,
        request: &Value,
        generation: u64,
        photo_scope: u64,
    ) -> Result<Value, ()> {
        let action = request.get("action").and_then(Value::as_str).ok_or(())?;
        let convert = |page: &Value| -> Result<(Vec<Value>, Option<String>), ()> {
            let photos = page.get("photos").and_then(Value::as_array).ok_or(())?.iter().map(|photo| {
                let time = photo.get("publishedAtSeconds").and_then(Value::as_i64).and_then(|s| chrono::DateTime::from_timestamp(s, 0)).ok_or(())?;
                Ok(json!({"id":photo.get("id").ok_or(())?,"username":photo.get("username").ok_or(())?,"url":photo.get("url").ok_or(())?,"images":photo.get("images").ok_or(())?,"thumbnail":photo.get("thumbnail").ok_or(())?,"caption":photo.get("caption").ok_or(())?,"publishedAt":time.to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}))
            }).collect::<Result<Vec<_>, ()>>()?;
            let cursor = match page.get("nextCursor") {
                Some(Value::Null) => None,
                Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
                _ => return Err(()),
            };
            Ok((photos, cursor))
        };
        if action == "profile-open" {
            let username = request.get("username").and_then(Value::as_str).ok_or(())?;
            let info = self.native_request(
                &json!({"action":"profile-info","generation":generation,"username":username}),
            )?;
            let id = info.get("id").and_then(Value::as_str).ok_or(())?;
            let username = info.get("username").and_then(Value::as_str).ok_or(())?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| ())?
                .as_secs() as i64;
            let page = self.native_request(&json!({"action":"profile-page","generation":generation,"id":id,"username":username,"cutoff":0,"now":now}))?;
            let (photos, cursor) = convert(&page)?;
            let posts = photos
                .iter()
                .filter_map(|post| post.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect();
            let mut seen = HashSet::new();
            if let Some(next) = &cursor {
                seen.insert(next.clone());
            }
            let mut state = self.inner.lock().map_err(|_| ())?;
            if state.generation != generation || state.photo_scope != photo_scope {
                return Err(());
            }
            state.profile = Some(NativeProfileState {
                id: id.into(),
                username: username.into(),
                now,
                cursor: cursor.clone(),
                seen,
                posts,
                loading: false,
            });
            Ok(
                json!({"username":username,"name":info.get("name").ok_or(())?,"bio":info.get("bio").ok_or(())?,"photos":photos,"more":cursor.is_some()}),
            )
        } else if action == "profile-more" {
            let (id, username, now, cursor, _guard) = {
                let mut state = self.inner.lock().map_err(|_| ())?;
                if state.generation != generation || state.photo_scope != photo_scope {
                    return Err(());
                }
                let profile = state.profile.as_mut().ok_or(())?;
                if profile.loading || profile.cursor.is_none() {
                    return Err(());
                }
                profile.loading = true;
                (
                    profile.id.clone(),
                    profile.username.clone(),
                    profile.now,
                    profile.cursor.clone(),
                    ProfileLoadGuard {
                        adapter: self,
                        generation,
                        photo_scope,
                    },
                )
            };
            let page = self.native_request(&json!({"action":"profile-page","generation":generation,"id":id,"username":username,"cutoff":0,"now":now,"cursor":cursor}))?;
            let (photos, cursor) = convert(&page)?;
            let mut state = self.inner.lock().map_err(|_| ())?;
            if state.generation != generation || state.photo_scope != photo_scope {
                return Err(());
            }
            let profile = state.profile.as_mut().ok_or(())?;
            if let Some(next) = &cursor {
                if !profile.seen.insert(next.clone()) {
                    return Err(());
                }
            }
            profile.posts.extend(
                photos
                    .iter()
                    .filter_map(|post| post.get("id").and_then(Value::as_str).map(str::to_owned)),
            );
            profile.cursor = cursor.clone();
            Ok(json!({"photos":photos,"more":cursor.is_some()}))
        } else {
            Err(())
        }
    }

    fn photo_request(
        &self,
        request: &Value,
        generation: u64,
        photo_scope: u64,
        username: &str,
    ) -> Result<Value, ()> {
        let action = request.get("action").and_then(Value::as_str).ok_or(())?;
        let code = request.get("code").and_then(Value::as_str).ok_or(())?;
        calmig_core::media_id_from_code(code).map_err(|_| ())?;
        match action {
            "photo-details" | "photo-comments-more" => {
                let (max_id, min_id, comment_generation, _guard) = {
                    let mut state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation
                        || state.photo_scope != photo_scope
                        || state.client.is_none()
                    {
                        return Err(());
                    }
                    if action == "photo-details" {
                        state.comment_generation = state.comment_generation.wrapping_add(1);
                        state.comment = Some(CommentState {
                            code: code.to_owned(),
                            max_id: None,
                            min_id: None,
                            seen: HashSet::new(),
                            loading: false,
                        });
                    }
                    let current_generation = state.comment_generation;
                    let comment = state.comment.as_mut().ok_or(())?;
                    if comment.code != code
                        || comment.loading
                        || (action == "photo-comments-more"
                            && comment.max_id.is_none()
                            && comment.min_id.is_none())
                    {
                        return Err(());
                    }
                    comment.loading = true;
                    (
                        comment.max_id.clone(),
                        comment.min_id.clone(),
                        current_generation,
                        CommentLoadGuard {
                            adapter: self,
                            generation,
                            comment_generation: current_generation,
                        },
                    )
                };
                let result = self.native_request(&json!({"action":"comments-page","generation":generation,"code":code,"maxId":max_id,"minId":min_id}))?;
                let next_max = result
                    .get("nextMax")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let next_min = result
                    .get("nextMin")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let mut state = self.inner.lock().map_err(|_| ())?;
                if state.generation != generation
                    || state.photo_scope != photo_scope
                    || state.comment_generation != comment_generation
                {
                    return Err(());
                }
                let comment = state.comment.as_mut().ok_or(())?;
                if (next_max.is_some() || next_min.is_some())
                    && !comment.seen.insert((next_max.clone(), next_min.clone()))
                {
                    return Err(());
                }
                comment.max_id = next_max;
                comment.min_id = next_min;
                Ok(
                    json!({"caption":result.get("caption").ok_or(())?,"comments":result.get("comments").ok_or(())?,"more":comment.max_id.is_some() || comment.min_id.is_some()}),
                )
            }
            "photo-comment-send" => {
                let raw = request.get("text").and_then(Value::as_str).ok_or(())?;
                if raw.trim().is_empty() || raw.trim().encode_utf16().count() > 1800 {
                    return Err(());
                }
                let _guard = {
                    let mut state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation
                        || state.photo_scope != photo_scope
                        || state.client.is_none()
                        || !state.sending_comments.insert(code.to_owned())
                    {
                        return Err(());
                    }
                    CommentSendGuard {
                        adapter: self,
                        generation,
                        code: code.to_owned(),
                    }
                };
                self.native_request(&json!({"action":"comment-send","generation":generation,"code":code,"text":raw.trim()}))?;
                if self.inner.lock().map_err(|_| ())?.photo_scope != photo_scope {
                    return Err(());
                }
                Ok(json!({"sent":true,"username":username}))
            }
            _ => Err(()),
        }
    }

    fn native_request(&self, request: &Value) -> Result<Value, ()> {
        match request.get("action").and_then(Value::as_str) {
            Some("device-profile") => {
                let generation = request
                    .get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                if self.inner.lock().map_err(|_| ())?.generation != generation {
                    return Err(());
                }
                let directory =
                    Path::new(request.get("directory").and_then(Value::as_str).ok_or(())?);
                let user_id = request.get("userId").and_then(Value::as_str).ok_or(())?;
                let candidate = calmig_core::legacy_device(user_id).map_err(|_| ())?;
                let device = calmig_core::load_or_import_device(directory, user_id, &candidate)
                    .map_err(|_| ())?;
                if self.inner.lock().map_err(|_| ())?.generation != generation {
                    return Err(());
                }
                serde_json::to_value(device).map_err(|_| ())
            }
            Some("calendar-sync-start") => {
                let generation = request
                    .get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                let cutoff = request.get("cutoff").and_then(Value::as_i64).ok_or(())?;
                let now = request.get("now").and_then(Value::as_i64).ok_or(())?;
                if cutoff > now || cutoff < 0 || now - cutoff > 8 * 86_400 {
                    return Err(());
                }
                let (job, client) = {
                    let mut state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation {
                        return Err(());
                    }
                    if request
                        .get("epoch")
                        .is_some_and(|value| value.as_u64() != Some(state.calendar_epoch))
                    {
                        return Err(());
                    }
                    let client = Arc::clone(state.client.as_ref().ok_or(())?);
                    if let Some(old) = state.calendar.take() {
                        old.cancelled.store(true, Ordering::Relaxed);
                    }
                    state.next_calendar_id = state.next_calendar_id.checked_add(1).ok_or(())?;
                    let job = Arc::new(CalendarJob {
                        id: state.next_calendar_id,
                        cancelled: AtomicBool::new(false),
                        state: Mutex::new((0, None)),
                    });
                    state.calendar = Some(Arc::clone(&job));
                    (job, client)
                };
                let id = job.id;
                std::thread::spawn(move || {
                    calmig_core::sync_calendar(
                        client.as_ref(),
                        cutoff,
                        now,
                        &job.cancelled,
                        |value| {
                            if let Ok(mut state) = job.state.lock() {
                                state.0 = state.0.saturating_add(1);
                                state.1 = Some(value);
                            }
                        },
                        Duration::from_millis(350),
                    );
                });
                Ok(json!({"job":id}))
            }
            Some("calendar-sync-status") | Some("calendar-sync-cancel") => {
                let generation = request
                    .get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                let id = request.get("job").and_then(Value::as_u64).ok_or(())?;
                let job = {
                    let state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation {
                        return Err(());
                    }
                    Arc::clone(
                        state
                            .calendar
                            .as_ref()
                            .filter(|job| job.id == id)
                            .ok_or(())?,
                    )
                };
                if request.get("action").and_then(Value::as_str) == Some("calendar-sync-cancel") {
                    job.cancelled.store(true, Ordering::Relaxed);
                    return Ok(Value::Null);
                }
                let state = job.state.lock().map_err(|_| ())?;
                if request.get("revision").and_then(Value::as_u64) == Some(state.0) {
                    return Ok(json!({"revision":state.0,"unchanged":true}));
                }
                let Some(calendar) = state.1.as_ref() else {
                    return Ok(
                        json!({"revision":state.0,"status":"loading","accounts":0,"photos":[],"incomplete":false}),
                    );
                };
                let status = match calendar.status {
                    calmig_core::CalendarStatus::Loading => "loading",
                    calmig_core::CalendarStatus::Complete => "complete",
                    calmig_core::CalendarStatus::Cancelled => "cancelled",
                    calmig_core::CalendarStatus::Error => "error",
                };
                let photos: Vec<Value> = calendar.photos.iter().map(|photo| json!({"id":photo.code,"username":photo.username,"url":photo.url,"images":photo.images,"thumbnail":photo.thumbnail,"caption":photo.caption,"publishedAtSeconds":photo.published_at})).collect();
                Ok(
                    json!({"revision":state.0,"status":status,"accounts":calendar.accounts,"photos":photos,"incomplete":calendar.incomplete,"reason":if calendar.feedback_required {Some("feedback_required")} else {None::<&str>}}),
                )
            }
            Some("browser-verify") => {
                let generation = request
                    .get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                if self.inner.lock().map_err(|_| ())?.generation != generation {
                    return Err(());
                }
                let cookies: Vec<calmig_core::BrowserCookie> =
                    serde_json::from_value(request.get("cookies").cloned().ok_or(())?)
                        .map_err(|_| ())?;
                if cookies.len() > 16 {
                    return Err(());
                }
                let directory =
                    Path::new(request.get("directory").and_then(Value::as_str).ok_or(())?);
                let client =
                    calmig_core::client_from_browser(&cookies, directory).map_err(|_| ())?;
                let account = client.current_account().map_err(|_| ())?;
                let update = client.session_update().map_err(|_| ())?;
                if self.inner.lock().map_err(|_| ())?.generation != generation {
                    return Err(());
                }
                Ok(
                    json!({"username":account.username, "authorization":update.authorization,
                    "claim":update.claim, "sessionId":update.session_id, "csrf":update.csrf}),
                )
            }
            Some("cache-load") | Some("cache-save") | Some("cache-clear") => {
                let directory =
                    Path::new(request.get("directory").and_then(Value::as_str).ok_or(())?);
                let username = request.get("username").and_then(Value::as_str).ok_or(())?;
                match request.get("action").and_then(Value::as_str) {
                    Some("cache-clear") => {
                        calmig_core::clear_calendar_cache(directory, username).map_err(|_| ())?
                    }
                    action => {
                        let cutoff = request.get("cutoff").and_then(Value::as_i64).ok_or(())?;
                        let now = request.get("now").and_then(Value::as_i64).ok_or(())?;
                        if cutoff > now {
                            return Err(());
                        }
                        if action == Some("cache-load") {
                            return Ok(json!(calmig_core::load_calendar_cache(
                                directory, username, cutoff, now
                            )
                            .map_err(|_| ())?));
                        }
                        let photos = request.get("photos").and_then(Value::as_array).ok_or(())?;
                        calmig_core::save_calendar_cache(directory, username, photos, cutoff, now)
                            .map_err(|_| ())?;
                    }
                }
                Ok(Value::Null)
            }
            Some("following-status") => {
                let state = self.inner.lock().map_err(|_| ())?;
                if std::env::var_os("CALMIG_NATIVE_DIAGNOSTICS").is_some() {
                    eprintln!("Native following available: {}", state.client.is_some());
                }
                Ok(if state.client.is_some() {
                    json!({"generation": state.generation})
                } else {
                    Value::Null
                })
            }
            Some("following-page") => {
                let generation = request
                    .get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                let cursor = match request.get("cursor") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(cursor)) => Some(cursor.as_str()),
                    _ => return Err(()),
                };
                let client = {
                    let state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation {
                        return Err(());
                    }
                    Arc::clone(state.client.as_ref().ok_or(())?)
                };
                let page = match client.following_page(cursor) {
                    Ok(page) => page,
                    Err(error) => {
                        if std::env::var_os("CALMIG_NATIVE_DIAGNOSTICS").is_some() {
                            eprintln!("Native following: {error}");
                        }
                        return Err(());
                    }
                };
                let state = self.inner.lock().map_err(|_| ())?;
                if state.generation != generation
                    || !state
                        .client
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(active, &client))
                {
                    return Err(());
                }
                if std::env::var_os("CALMIG_NATIVE_DIAGNOSTICS").is_some() {
                    eprintln!(
                        "Native following: {} accounts; more: {}",
                        page.accounts.len(),
                        page.next_cursor.is_some()
                    );
                }
                let users: Vec<Value> = page
                    .accounts
                    .into_iter()
                    .map(|account| json!({"pk": account.id, "username": account.username}))
                    .collect();
                Ok(json!({"users": users, "next_max_id": page.next_cursor}))
            }
            Some("profile-page") => {
                let generation = request
                    .get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                let id = request.get("id").and_then(Value::as_str).ok_or(())?;
                let username = request.get("username").and_then(Value::as_str).ok_or(())?;
                let cutoff = request.get("cutoff").and_then(Value::as_i64).ok_or(())?;
                let now = request.get("now").and_then(Value::as_i64).ok_or(())?;
                let cursor = match request.get("cursor") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(cursor)) => Some(cursor.as_str()),
                    _ => return Err(()),
                };
                let client = {
                    let state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation {
                        return Err(());
                    }
                    Arc::clone(state.client.as_ref().ok_or(())?)
                };
                let author = RustAccount {
                    id: id.into(),
                    username: username.into(),
                };
                let page = match client.profile_photos_page(&author, cursor, cutoff, now) {
                    Ok(page) => page,
                    Err(error) => {
                        if std::env::var_os("CALMIG_NATIVE_DIAGNOSTICS").is_some() {
                            eprintln!("Native profile: {error}");
                        }
                        return Err(());
                    }
                };
                let state = self.inner.lock().map_err(|_| ())?;
                if state.generation != generation
                    || !state
                        .client
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(active, &client))
                {
                    return Err(());
                }
                if std::env::var_os("CALMIG_NATIVE_DIAGNOSTICS").is_some() {
                    eprintln!(
                        "Native profile: {} photos; more: {}",
                        page.photos.len(),
                        page.next_cursor.is_some()
                    );
                }
                let photos: Vec<Value> = page
                    .photos
                    .into_iter()
                    .map(|photo| {
                        json!({
                            "id": photo.code, "username": photo.username, "url": photo.url,
                            "images": photo.images, "thumbnail": photo.thumbnail,
                            "publishedAtSeconds": photo.published_at, "caption": photo.caption,
                        })
                    })
                    .collect();
                Ok(json!({"photos":photos, "itemCount":page.item_count,
                    "allOlderThanCutoff":page.all_older_than_cutoff, "nextCursor":page.next_cursor}))
            }
            Some("profile-info") => {
                let generation = request
                    .get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                let username = request.get("username").and_then(Value::as_str).ok_or(())?;
                let client = {
                    let state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation {
                        return Err(());
                    }
                    Arc::clone(state.client.as_ref().ok_or(())?)
                };
                let info = client.profile_info(username).map_err(|_| ())?;
                let state = self.inner.lock().map_err(|_| ())?;
                if state.generation != generation
                    || !state
                        .client
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(active, &client))
                {
                    return Err(());
                }
                Ok(
                    json!({"id":info.account.id,"username":info.account.username,"name":info.name,"bio":info.bio}),
                )
            }
            Some("comments-page") | Some("comment-send") => {
                let generation = request
                    .get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                let code = request.get("code").and_then(Value::as_str).ok_or(())?;
                let media_id = calmig_core::media_id_from_code(code).map_err(|_| ())?;
                let client = {
                    let state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation {
                        return Err(());
                    }
                    Arc::clone(state.client.as_ref().ok_or(())?)
                };
                let result = if request.get("action").and_then(Value::as_str)
                    == Some("comments-page")
                {
                    let parse_cursor = |name: &str| -> Result<Option<&str>, ()> {
                        match request.get(name) {
                            None | Some(Value::Null) => Ok(None),
                            Some(Value::String(cursor)) => Ok(Some(cursor.as_str())),
                            _ => Err(()),
                        }
                    };
                    let page = client
                        .comments_page(&media_id, parse_cursor("maxId")?, parse_cursor("minId")?)
                        .map_err(|_| {
                            if std::env::var_os("CALMIG_NATIVE_DIAGNOSTICS").is_some() {
                                eprintln!("Native comments: request failed");
                            }
                        })?;
                    if std::env::var_os("CALMIG_NATIVE_DIAGNOSTICS").is_some() {
                        eprintln!(
                            "Native comments: {} comments; more: {}",
                            page.comments.len(),
                            page.next_max.is_some() || page.next_min.is_some()
                        );
                    }
                    let comments: Vec<Value> = page
                        .comments
                        .into_iter()
                        .map(|comment| json!({"username":comment.username, "text":comment.text}))
                        .collect();
                    json!({"caption":page.caption, "comments":comments, "nextMax":page.next_max, "nextMin":page.next_min})
                } else {
                    let text = request.get("text").and_then(Value::as_str).ok_or(())?;
                    client.send_comment(&media_id, text).map_err(|_| ())?;
                    json!({"sent":true})
                };
                let state = self.inner.lock().map_err(|_| ())?;
                if state.generation != generation
                    || !state
                        .client
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(active, &client))
                {
                    return Err(());
                }
                Ok(result)
            }
            Some("inbox-page") | Some("messages-page") | Some("message-send") => {
                let generation = request
                    .get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(())?;
                let action = request.get("action").and_then(Value::as_str).ok_or(())?;
                let optional = |name: &str| -> Result<Option<&str>, ()> {
                    match request.get(name) {
                        None | Some(Value::Null) => Ok(None),
                        Some(Value::String(value)) => Ok(Some(value.as_str())),
                        _ => Err(()),
                    }
                };
                let cursor = optional("cursor")?;
                let thread = if action == "inbox-page" {
                    None
                } else {
                    Some(request.get("thread").and_then(Value::as_str).ok_or(())?)
                };
                let (client, inbox_generation, _send_guard) = {
                    let mut state = self.inner.lock().map_err(|_| ())?;
                    if state.generation != generation {
                        return Err(());
                    }
                    let client = Arc::clone(state.client.as_ref().ok_or(())?);
                    if action == "inbox-page" && cursor.is_none() {
                        state.inbox_generation = state.inbox_generation.wrapping_add(1);
                        state.allowed_threads.clear();
                        state.message_cursors.clear();
                        state.message_cursor_history.clear();
                    }
                    if thread.is_some_and(|id| !state.allowed_threads.contains(id)) {
                        return Err(());
                    }
                    if action == "messages-page"
                        && cursor.is_some_and(|cursor| {
                            thread
                                .and_then(|id| state.message_cursors.get(id))
                                .map(String::as_str)
                                != Some(cursor)
                        })
                    {
                        return Err(());
                    }
                    let guard = if action == "message-send" {
                        let thread = thread.ok_or(())?;
                        if !state.sending_threads.insert(thread.to_owned()) {
                            return Err(());
                        }
                        Some(ThreadSendGuard {
                            adapter: self,
                            generation,
                            thread: thread.to_owned(),
                        })
                    } else {
                        None
                    };
                    (client, state.inbox_generation, guard)
                };
                let mut new_threads = Vec::new();
                let mut new_cursor = None;
                let result = match action {
                    "inbox-page" => {
                        let page = client
                            .inbox_page(cursor, optional("seqId")?)
                            .map_err(|_| ())?;
                        new_threads = page
                            .threads
                            .iter()
                            .map(|thread| thread.id.clone())
                            .collect();
                        let threads: Vec<Value> = page.threads.into_iter().map(|thread| json!({"id":thread.id,"title":thread.title,"unread":thread.unread})).collect();
                        json!({"threads":threads,"cursor":page.cursor,"seqId":page.seq_id})
                    }
                    "messages-page" => {
                        let page = client
                            .messages_page(thread.ok_or(())?, cursor)
                            .map_err(|_| ())?;
                        new_cursor = Some(page.cursor.clone());
                        let messages: Vec<Value> = page.messages.into_iter().map(|message| json!({"id":message.id,"mine":message.mine,"attachment":message.attachment,"text":message.text,"timestampMs":message.timestamp_ms})).collect();
                        json!({"messages":messages,"cursor":page.cursor})
                    }
                    "message-send" => {
                        let text = request.get("text").and_then(Value::as_str).ok_or(())?;
                        client
                            .send_automatic_message(thread.ok_or(())?, text)
                            .map_err(|_| ())?;
                        json!({"sent":true})
                    }
                    _ => return Err(()),
                };
                let mut state = self.inner.lock().map_err(|_| ())?;
                if state.generation != generation
                    || state.inbox_generation != inbox_generation
                    || !state
                        .client
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(active, &client))
                    || thread.is_some_and(|id| !state.allowed_threads.contains(id))
                {
                    return Err(());
                }
                state.allowed_threads.extend(new_threads);
                if let Some(next) = new_cursor {
                    let thread = thread.ok_or(())?;
                    if cursor.is_none() {
                        state.message_cursor_history.remove(thread);
                    }
                    if let Some(next) = next {
                        let seen = state
                            .message_cursor_history
                            .entry(thread.to_owned())
                            .or_default();
                        if !seen.insert(next.clone()) {
                            return Err(());
                        }
                        state.message_cursors.insert(thread.to_owned(), next);
                    } else {
                        state.message_cursors.remove(thread);
                        state.message_cursor_history.remove(thread);
                    }
                }
                Ok(result)
            }
            _ => Err(()),
        }
    }
}

#[tauri::command]
async fn bridge(
    request: Value,
    app: AppHandle,
    window: WebviewWindow,
    rust: State<'_, Arc<RustAdapter>>,
) -> Result<Value, String> {
    if window.label() != "main" {
        return Err("Недоступная команда".into());
    }
    // Browser credentials and the imported native session are never exposed to JavaScript.
    if matches!(
        request.get("action").and_then(Value::as_str),
        Some("browser-login" | "photo-authorize" | "profile-authorize")
    ) || request.get("nativeReply").is_some()
        || request.get("nativeRequest").is_some()
    {
        return Err("Недоступная команда".into());
    }
    if request.get("action").and_then(Value::as_str) == Some("restore") {
        let username = request
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if username.is_empty()
            || username.len() > 30
            || username == "."
            || username == ".."
            || !username
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        {
            return Err("Некорректный аккаунт".into());
        }
        let username = username.to_owned();
        let home = app
            .path()
            .home_dir()
            .map_err(|_| "Не удалось открыть данные приложения")?;
        let (_, cache, users) =
            legacy_data_paths(&home).map_err(|_| "Не удалось открыть настройки приложения")?;
        let cache = cache.to_str().ok_or("Некорректный путь кеша")?.to_owned();
        let file = users.join(&username).join("session.ts.json");
        let native = Arc::clone(&rust);
        let auth = native.begin_auth()?;
        let generation = auth.generation;
        return tauri::async_runtime::spawn_blocking(move || {
            let _auth = auth;
            use std::io::Read;
            let mut data = Vec::new();
            let restored = std::fs::File::open(&file)
                .and_then(|file| file.take(2_000_001).read_to_end(&mut data))
                .is_ok()
                && data.len() <= 2_000_000;
            let session = restored
                .then(|| calmig_core::Session::from_legacy_json(&data).ok())
                .flatten();
            let Some(session) = session else {
                return Err(
                    "Не удалось открыть сохранённую сессию. Войдите через Instagram.".into(),
                );
            };
            let client = RustClient::new(session).map_err(|_| "Не удалось открыть сессию")?;
            let account = client
                .current_account()
                .map_err(|_| "Не удалось подтвердить аккаунт")?;
            if !account.username.eq_ignore_ascii_case(&username) {
                return Err("Не удалось подтвердить аккаунт".into());
            }
            native
                .attach_native_client(generation, client, &account.username, &cache, Some(file))
                .map_err(|_| "Не удалось подключить аккаунт")?;
            native.persist_native_session(generation);
            Ok(json!({"connected":true,"username":account.username}))
        })
        .await
        .map_err(|_| "Ошибка адаптера")?;
    }
    if request.get("action").and_then(Value::as_str) == Some("status") {
        let current = rust.inner.lock().map_err(|_| "Ошибка адаптера")?;
        if let (Some(_), Some(view)) = (&current.client, &current.calendar_view) {
            return Ok(json!({"connected":true,"username":view.username}));
        }
    }
    if request.get("action").and_then(Value::as_str) == Some("disconnect") {
        if rust.disconnect_native()? {
            return Ok(json!({"connected":false}));
        }
        rust.clear()?;
    }
    let action = request.get("action").and_then(Value::as_str);
    let invalidated_profile = if matches!(action, Some("calendar-clear" | "profile-open")) {
        Some(rust.invalidate_photos()?)
    } else {
        None
    };
    #[cfg(target_os = "linux")]
    if action == Some("calendar-start") {
        if let Some(browser) = app.get_webview_window("instagram-login") {
            let native = Arc::clone(&rust);
            return tauri::async_runtime::spawn_blocking(move || {
                native.start_web_calendar(browser)
            })
            .await
            .map_err(|_| "Не удалось начать загрузку".to_owned())?
            .map_err(|_| "Не удалось начать загрузку".into());
        }
    }
    if matches!(
        action,
        Some("calendar-start" | "calendar-status" | "calendar-stop" | "calendar-clear")
    ) && rust
        .inner
        .lock()
        .map_err(|_| "Ошибка адаптера")?
        .client
        .is_some()
    {
        let native = Arc::clone(&rust);
        return tauri::async_runtime::spawn_blocking(move || {
            let generation = native.generation().ok();
            let result = native.calendar_request(&request);
            if let Some(generation) = generation {
                native.persist_native_session(generation);
            }
            result
        })
        .await
        .map_err(|_| "Ошибка адаптера")?
        .map_err(|_| "Не удалось выполнить запрос. Проверьте уведомления Instagram.".into());
    }
    if matches!(
        request.get("action").and_then(Value::as_str),
        Some("profile-open" | "profile-more")
    ) && rust
        .inner
        .lock()
        .map_err(|_| "Ошибка адаптера")?
        .client
        .is_some()
    {
        let native = Arc::clone(&rust);
        let (generation, scope) = if let Some((generation, scope, _)) = invalidated_profile {
            (generation, scope)
        } else {
            let state = native.inner.lock().map_err(|_| "Ошибка адаптера")?;
            (state.generation, state.photo_scope)
        };
        return tauri::async_runtime::spawn_blocking(move || {
            if request.get("action").and_then(Value::as_str) == Some("profile-open") {
                let username = request
                    .get("username")
                    .and_then(Value::as_str)
                    .ok_or("Некорректный профиль")?;
                let current = native.inner.lock().map_err(|_| "Ошибка адаптера")?;
                if current.generation != generation
                    || current.photo_scope != scope
                    || !current.calendar_view.as_ref().is_some_and(|view| {
                        view.photos.iter().any(|photo| {
                            photo
                                .get("username")
                                .and_then(Value::as_str)
                                .is_some_and(|author| author.eq_ignore_ascii_case(username))
                        })
                    })
                {
                    return Err("Некорректный профиль".into());
                }
            }
            let result = native.profile_request(&request, generation, scope);
            native.persist_native_session(generation);
            result.map_err(|_| {
                "Не удалось выполнить запрос. Проверьте уведомления Instagram.".to_owned()
            })
        })
        .await
        .map_err(|_| "Ошибка адаптера")?;
    }
    if matches!(
        request.get("action").and_then(Value::as_str),
        Some("photo-details" | "photo-comments-more" | "photo-comment-send")
    ) && rust
        .inner
        .lock()
        .map_err(|_| "Ошибка адаптера")?
        .client
        .is_some()
    {
        let native = Arc::clone(&rust);
        let (generation, photo_scope) = {
            let current = native.inner.lock().map_err(|_| "Ошибка адаптера")?;
            (current.generation, current.photo_scope)
        };
        return tauri::async_runtime::spawn_blocking(move || {
            let code = request
                .get("code")
                .and_then(Value::as_str)
                .ok_or("Некорректная публикация")?;
            let username = {
                let current = native.inner.lock().map_err(|_| "Ошибка адаптера")?;
                if current.generation != generation || current.photo_scope != photo_scope {
                    return Err("Некорректная публикация".into());
                }
                let view = current.calendar_view.as_ref().ok_or("Ошибка адаптера")?;
                let allowed = view
                    .photos
                    .iter()
                    .any(|photo| photo.get("id").and_then(Value::as_str) == Some(code))
                    || current
                        .profile
                        .as_ref()
                        .is_some_and(|profile| profile.posts.contains(code));
                if !allowed {
                    return Err("Некорректная публикация".into());
                }
                view.username.clone()
            };
            let result = native.photo_request(&request, generation, photo_scope, &username);
            native.persist_native_session(generation);
            result.map_err(|_| {
                "Не удалось выполнить запрос. Проверьте уведомления Instagram.".to_owned()
            })
        })
        .await
        .map_err(|_| "Ошибка адаптера")?;
    }
    if matches!(
        request.get("action").and_then(Value::as_str),
        Some("threads" | "messages" | "send")
    ) && rust
        .inner
        .lock()
        .map_err(|_| "Ошибка адаптера")?
        .client
        .is_some()
    {
        let native = Arc::clone(&rust);
        return tauri::async_runtime::spawn_blocking(move || {
            let generation = native.generation().ok();
            let result = native.direct_request(&request);
            if let Some(generation) = generation {
                native.persist_native_session(generation);
            }
            result
        })
        .await
        .map_err(|_| "Ошибка адаптера")?
        .map_err(|_| "Не удалось выполнить запрос. Проверьте уведомления Instagram.".into());
    }
    Err("Недоступная команда или аккаунт не подключён".into())
}

fn allowed_login_url(url: &tauri::Url) -> bool {
    if url.as_str() == "about:blank" {
        return true;
    }
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && (matches!(
            url.host_str(),
            Some(
                "www.instagram.com"
                    | "instagram.com"
                    | "accountscenter.instagram.com"
                    | "accountscenter.meta.com"
                    | "accountscenter.facebook.com"
                    | "www.facebook.com"
                    | "m.facebook.com"
            )
        ) || (url.host_str() == Some("www.fbsbx.com") && url.path().starts_with("/captcha/"))
            || (url.host_str() == Some("www.google.com") && url.path().starts_with("/recaptcha/")))
}

/// Read only the legacy paths needed to preserve existing local data.
fn legacy_data_paths(
    home: &Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf), ()> {
    use std::io::Read;
    let default_base = home.join(".instagram-cli");
    let mut data = default_base.clone();
    let mut cache = default_base.join("cache");
    let mut users = default_base.join("users");
    if let Ok(file) = std::fs::File::open(default_base.join("config.ts.yaml")) {
        let mut bytes = Vec::new();
        file.take(1_000_001)
            .read_to_end(&mut bytes)
            .map_err(|_| ())?;
        if bytes.len() > 1_000_000 {
            return Err(());
        }
        if let Ok(config) = serde_yaml::from_slice::<serde_yaml::Value>(&bytes) {
            if let Some(advanced) = config.get("advanced") {
                if let Some(value) = advanced.get("dataDir") {
                    data = std::path::PathBuf::from(value.as_str().ok_or(())?);
                }
                if let Some(value) = advanced.get("cacheDir") {
                    cache = std::path::PathBuf::from(value.as_str().ok_or(())?);
                }
                if let Some(value) = advanced.get("usersDir") {
                    users = std::path::PathBuf::from(value.as_str().ok_or(())?);
                }
            }
        }
    }
    if data.as_os_str().is_empty() || cache.as_os_str().is_empty() || users.as_os_str().is_empty() {
        return Err(());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if data.is_relative() {
        data = root.join(data);
    }
    if cache.is_relative() {
        cache = root.join(cache);
    }
    if users.is_relative() {
        users = root.join(users);
    }
    Ok((data.join("calmig-devices"), cache, users))
}

fn browser_data_directory(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|p| p.join("web-login"))
        .map_err(|_| "Не удалось открыть данные входа".into())
}

fn show_login_window(app: &AppHandle, visible: bool, language: Option<&str>) -> Result<(), String> {
    let title = if language == Some("en") {
        "Sign in to Instagram"
    } else {
        "Вход в Instagram"
    };
    if let Some(window) = app.get_webview_window("instagram-login") {
        if visible {
            window
                .set_title(title)
                .map_err(|_| "Не удалось открыть окно входа")?;
            window.show().map_err(|_| "Не удалось открыть окно входа")?;
            window
                .set_focus()
                .map_err(|_| "Не удалось открыть окно входа")?;
        }
        return Ok(());
    }
    let directory = browser_data_directory(app)?;
    std::fs::create_dir_all(&directory).map_err(|_| "Не удалось открыть данные входа")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Не удалось защитить данные входа")?;
    }
    let url = "https://www.instagram.com/accounts/login/"
        .parse()
        .map_err(|_| "Некорректный адрес")?;
    WebviewWindowBuilder::new(app, "instagram-login", WebviewUrl::External(url))
        .title(title)
        .visible(visible)
        .inner_size(620.0, 760.0)
        .data_directory(directory)
        .on_navigation(allowed_login_url)
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()
        .map_err(|_| "Не удалось открыть окно входа")?;
    Ok(())
}

#[tauri::command]
async fn open_instagram_login(
    app: AppHandle,
    caller: WebviewWindow,
    language: Option<String>,
) -> Result<(), String> {
    if caller.label() != "main" {
        return Err("Недоступная команда".into());
    }
    show_login_window(&app, true, language.as_deref())
}

#[tauri::command]
async fn complete_instagram_login(
    app: AppHandle,
    caller: WebviewWindow,
    rust: State<'_, Arc<RustAdapter>>,
) -> Result<Value, String> {
    if caller.label() != "main" {
        return Err("Недоступная команда".into());
    }
    let window = app
        .get_webview_window("instagram-login")
        .ok_or("Сначала откройте страницу входа")?;
    let home = app
        .path()
        .home_dir()
        .map_err(|_| "Не удалось открыть данные приложения")?;
    let (device_dir, cache_dir, _) =
        legacy_data_paths(&home).map_err(|_| "Не удалось открыть настройки приложения")?;
    let cache_dir = cache_dir
        .to_str()
        .ok_or("Некорректный путь кеша")?
        .to_owned();
    let native = Arc::clone(&rust);
    let auth = native.begin_auth()?;
    let generation = auth.generation;
    tauri::async_runtime::spawn_blocking(move || {
        let _auth = auth;
        let url = "https://www.instagram.com/"
            .parse()
            .map_err(|_| "Некорректный адрес")?;
        let cookies = window
            .cookies_for_url(url)
            .map_err(|_| "Не удалось получить сессию")?;
        let mut required = Vec::new();
        for name in ["sessionid", "ds_user_id", "csrftoken"] {
            let matching: Vec<_> = cookies
                .iter()
                .filter(|cookie| cookie.name() == name)
                .collect();
            if matching.len() != 1 {
                return Err("Войдите на странице Instagram и повторите".into());
            }
            required.push(calmig_core::BrowserCookie {
                name: name.into(),
                value: matching[0].value().to_owned(),
            });
        }
        let client = calmig_core::client_from_browser(&required, &device_dir)
            .map_err(|_| "Не удалось подготовить сессию")?;
        let account = client
            .current_account()
            .map_err(|_| "Не удалось подтвердить аккаунт")?;
        native
            .attach_native_client(generation, client, &account.username, &cache_dir, None)
            .map_err(|_| "Не удалось подключить аккаунт")?;
        let _ = window.hide();
        Ok(json!({"connected":true,"username":account.username}))
    })
    .await
    .map_err(|_| "Не удалось подключить аккаунт")?
}

#[tauri::command]
async fn restore_instagram_login(
    app: AppHandle,
    caller: WebviewWindow,
    rust: State<'_, Arc<RustAdapter>>,
    language: Option<String>,
) -> Result<Option<Value>, String> {
    if caller.label() != "main" {
        return Err("Недоступная команда".into());
    }
    if !browser_data_directory(&app)?.exists() {
        return Ok(None);
    }
    show_login_window(&app, false, language.as_deref())?;
    Ok(complete_instagram_login(app, caller, rust).await.ok())
}

#[tauri::command]
async fn clear_instagram_login(
    app: AppHandle,
    caller: WebviewWindow,
    rust: State<'_, Arc<RustAdapter>>,
) -> Result<Value, String> {
    if caller.label() != "main" {
        return Err("Недоступная команда".into());
    }
    let native_connected = rust.disconnect_native()?;
    if !native_connected {
        rust.clear()?;
    }
    if let Some(window) = app.get_webview_window("instagram-login") {
        window
            .clear_all_browsing_data()
            .map_err(|_| "Не удалось удалить данные входа")?;
        let _ = window.hide();
    } else {
        let directory = browser_data_directory(&app)?;
        if directory.exists() {
            std::fs::remove_dir_all(directory).map_err(|_| "Не удалось удалить данные входа")?;
        }
    }
    Ok(json!({"nativeDisconnected":native_connected}))
}
fn main() {
    let native = Arc::new(RustAdapter::default());
    tauri::Builder::default()
        .manage(native)
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "instagram-login" {
                    api.prevent_close();
                    let _ = window.hide();
                } else if window.label() == "main" {
                    if let Some(auth) = window.app_handle().get_webview_window("instagram-login") {
                        let _ = auth.destroy();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            bridge,
            open_instagram_login,
            complete_instagram_login,
            restore_instagram_login,
            clear_instagram_login
        ])
        .run(tauri::generate_context!())
        .expect("Не удалось запустить приложение");
}

#[cfg(test)]
mod tests {
    use super::{allowed_login_url, RustAdapter};
    use serde_json::json;

    #[test]
    fn native_cache_round_trip() {
        let adapter = RustAdapter::default();
        let directory = std::env::temp_dir().join(format!(
            "calmig-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let directory_string = directory.to_str().unwrap();
        let post = json!({"id":"ABC", "username":"friend", "url":"https://www.instagram.com/p/ABC/", "images":["https://scontent.cdninstagram.com/a.jpg"], "thumbnail":"https://scontent.cdninstagram.com/t.jpg", "publishedAt":"2026-10-30T12:00:00.000Z"});
        let args = json!({"action":"cache-save", "directory":directory_string, "username":"first", "now":1793491200000i64, "cutoff":1792800000000i64, "photos":[post]});
        adapter.native_request(&args).unwrap();
        let result = adapter.native_request(&json!({"action":"cache-load", "directory":directory_string, "username":"first", "now":1793491200000i64, "cutoff":1792800000000i64})).unwrap();
        assert_eq!(result, json!([post]));
        adapter
            .native_request(
                &json!({"action":"cache-clear", "directory":directory_string, "username":"first"}),
            )
            .unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn legacy_settings_preserve_default_and_configured_data_paths() {
        let home = std::env::temp_dir().join(format!(
            "calmig-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(home.join(".instagram-cli")).unwrap();
        let (devices, cache, users) = super::legacy_data_paths(&home).unwrap();
        assert_eq!(devices, home.join(".instagram-cli/calmig-devices"));
        assert_eq!(cache, home.join(".instagram-cli/cache"));
        assert_eq!(users, home.join(".instagram-cli/users"));
        std::fs::write(
            home.join(".instagram-cli/config.ts.yaml"),
            "advanced:\n  dataDir: '/tmp/example data'\n  cacheDir: '/tmp/example cache'\n",
        )
        .unwrap();
        let (devices, cache, _) = super::legacy_data_paths(&home).unwrap();
        assert_eq!(
            devices,
            std::path::PathBuf::from("/tmp/example data/calmig-devices")
        );
        assert_eq!(cache, std::path::PathBuf::from("/tmp/example cache"));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn restored_legacy_session_is_updated_and_removed_on_logout() {
        let dir = std::env::temp_dir().join(format!(
            "calmig-restore-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("session.ts.json");
        let cookies = json!({"cookies":[{"key":"sessionid","value":"123456%3Asynthetic","domain":"instagram.com","path":"/","secure":true},{"key":"ds_user_id","value":"123456","domain":"instagram.com","path":"/","secure":true},{"key":"csrftoken","value":"synthetic","domain":"instagram.com","path":"/","secure":true}]});
        let state = json!({"cookies":cookies.to_string(),"deviceId":"android-test","uuid":"uuid","deviceString":"34/14; test","language":"en_US"});
        std::fs::write(&file, state.to_string()).unwrap();
        let session = calmig_core::Session::from_legacy_json(state.to_string().as_bytes()).unwrap();
        let client = calmig_core::Client::for_local_test(session, "http://127.0.0.1:1/").unwrap();
        let adapter = RustAdapter::default();
        adapter
            .attach_native_client(
                0,
                client,
                "synthetic",
                dir.to_str().unwrap(),
                Some(file.clone()),
            )
            .unwrap();
        adapter.persist_native_session(0);
        assert!(calmig_core::Session::from_legacy_json(&std::fs::read(&file).unwrap()).is_ok());
        assert!(adapter.disconnect_native().unwrap());
        assert!(!file.exists());
        assert!(adapter.inner.lock().unwrap().client.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_device_profile_remains_stable() {
        let adapter = RustAdapter::default();
        let directory = std::env::temp_dir().join(format!(
            "calmig-device-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let request = json!({"action":"device-profile","generation":0,"directory":directory.to_str().unwrap(),"userId":"123"});
        let device = adapter.native_request(&request).unwrap();
        assert_eq!(adapter.native_request(&request).unwrap(), device);
        adapter.clear().unwrap();
        assert!(adapter.native_request(&request).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rust_direct_ui_loads_inbox_and_messages_from_fixture_server() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::Arc,
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 8192];
                let length = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..length]);
                let body = if index == 0 {
                    assert!(request.starts_with("GET /api/v1/direct_v2/inbox/"));
                    r#"{"status":"ok","seq_id":"9","inbox":{"has_older":false,"threads":[{"thread_id":"777","thread_title":"Friend","read_state":0}]}}"#
                } else {
                    assert!(request.starts_with("GET /api/v1/direct_v2/threads/777/"));
                    r#"{"status":"ok","thread":{"has_older":false,"items":[{"item_id":"2","user_id":"123456","timestamp":"1760000001000000","item_type":"text","text":"hello"}]}}"#
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let cookies = json!({"cookies":[
            {"key":"sessionid","value":"123456%3Asynthetic","domain":"instagram.com","path":"/","secure":true},
            {"key":"ds_user_id","value":"123456","domain":"instagram.com","path":"/","secure":true},
            {"key":"csrftoken","value":"synthetic","domain":"instagram.com","path":"/","secure":true}
        ]});
        let session = json!({"cookies":cookies.to_string(),"deviceId":"android-test","uuid":"uuid","deviceString":"34/14; test","language":"en_US"});
        let session =
            calmig_core::Session::from_legacy_json(session.to_string().as_bytes()).unwrap();
        let client =
            calmig_core::Client::for_local_test(session, &format!("http://{address}/")).unwrap();
        let adapter = RustAdapter::default();
        adapter.inner.lock().unwrap().client = Some(Arc::new(client));
        let inbox = adapter
            .direct_request(&json!({"action":"threads"}))
            .unwrap();
        assert_eq!(inbox["threads"][0]["id"], "777");
        assert_eq!(inbox["hasMore"], false);
        assert!(adapter
            .direct_request(&json!({"action":"messages","thread":"888"}))
            .is_err());
        let messages = adapter
            .direct_request(&json!({"action":"messages","thread":"777"}))
            .unwrap();
        assert_eq!(messages["messages"][0]["text"], "hello");
        assert_eq!(
            messages["messages"][0]["timestamp"],
            "2025-10-09T08:53:21.000Z"
        );
        assert_eq!(messages["cursor"], serde_json::Value::Null);
        server.join().unwrap();
        adapter.clear().unwrap();
    }

    #[test]
    fn rust_photo_ui_preserves_comment_pagination() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::Arc,
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 8192];
                let length = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..length]);
                assert!(request.starts_with("GET /api/v1/media/64/comments/?"));
                assert_eq!(request.contains("max_id=older"), index == 1);
                let body = if index == 0 {
                    r#"{"status":"ok","caption":{"text":"caption"},"comments":[{"text":"first","user":{"username":"friend"}}],"next_max_id":"older"}"#
                } else {
                    r#"{"status":"ok","comments":[{"text":"second","user":{"username":"friend"}}]}"#
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let cookies = json!({"cookies":[
            {"key":"sessionid","value":"123456%3Asynthetic","domain":"instagram.com","path":"/","secure":true},
            {"key":"ds_user_id","value":"123456","domain":"instagram.com","path":"/","secure":true},
            {"key":"csrftoken","value":"synthetic","domain":"instagram.com","path":"/","secure":true}
        ]});
        let session = json!({"cookies":cookies.to_string(),"deviceId":"android-test","uuid":"uuid","deviceString":"34/14; test","language":"en_US"});
        let session =
            calmig_core::Session::from_legacy_json(session.to_string().as_bytes()).unwrap();
        let client =
            calmig_core::Client::for_local_test(session, &format!("http://{address}/")).unwrap();
        let adapter = RustAdapter::default();
        adapter.inner.lock().unwrap().client = Some(Arc::new(client));
        let first = adapter
            .photo_request(&json!({"action":"photo-details","code":"BA"}), 0, 0, "me")
            .unwrap();
        assert_eq!(first["caption"], "caption");
        assert_eq!(first["more"], true);
        let next = adapter
            .photo_request(
                &json!({"action":"photo-comments-more","code":"BA"}),
                0,
                0,
                "me",
            )
            .unwrap();
        assert_eq!(next["comments"][0]["text"], "second");
        assert_eq!(next["more"], false);
        assert!(adapter
            .photo_request(
                &json!({"action":"photo-comments-more","code":"BA"}),
                0,
                0,
                "me"
            )
            .is_err());
        adapter
            .inner
            .lock()
            .unwrap()
            .sending_comments
            .insert("BA".into());
        assert!(adapter
            .photo_request(
                &json!({"action":"photo-comment-send","code":"BA","text":"synthetic"}),
                0,
                0,
                "me"
            )
            .is_err());
        server.join().unwrap();
        adapter.clear().unwrap();
    }

    #[test]
    fn rust_profile_ui_loads_photo_pages_and_invalidates_on_reset() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::Arc,
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for index in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0u8; 8192];
                let n = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..n]);
                let body = match index {
                    0 => {
                        assert!(request.starts_with("GET /api/v1/users/friend/usernameinfo/"));
                        r#"{"status":"ok","user":{"pk":"7","username":"friend","full_name":"Name","biography":"Bio"}}"#
                    }
                    1 => {
                        assert!(request.starts_with("GET /api/v1/feed/user/7/"));
                        r#"{"status":"ok","items":[{"code":"ABC","taken_at":1000,"media_type":1,"image_versions2":{"candidates":[{"url":"https://scontent.cdninstagram.com/a","width":320}]}}],"more_available":true,"next_max_id":"next"}"#
                    }
                    _ => {
                        assert!(request.contains("max_id=next"));
                        r#"{"status":"ok","items":[{"code":"DEF","taken_at":999,"media_type":1,"image_versions2":{"candidates":[{"url":"https://scontent.cdninstagram.com/b","width":320}]}}],"more_available":false}"#
                    }
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let cookies = json!({"cookies":[{"key":"sessionid","value":"123456%3Asynthetic","domain":"instagram.com","path":"/","secure":true},{"key":"ds_user_id","value":"123456","domain":"instagram.com","path":"/","secure":true},{"key":"csrftoken","value":"synthetic","domain":"instagram.com","path":"/","secure":true}]});
        let session = json!({"cookies":cookies.to_string(),"deviceId":"android-test","uuid":"uuid","deviceString":"34/14; test","language":"en_US"});
        let session =
            calmig_core::Session::from_legacy_json(session.to_string().as_bytes()).unwrap();
        let client =
            calmig_core::Client::for_local_test(session, &format!("http://{address}/")).unwrap();
        let adapter = RustAdapter::default();
        adapter.inner.lock().unwrap().client = Some(Arc::new(client));
        let first = adapter
            .profile_request(&json!({"action":"profile-open","username":"friend"}), 0, 0)
            .unwrap();
        assert_eq!(first["username"], "friend");
        assert_eq!(first["photos"][0]["id"], "ABC");
        assert_eq!(first["more"], true);
        let second = adapter
            .profile_request(&json!({"action":"profile-more"}), 0, 0)
            .unwrap();
        assert_eq!(second["photos"][0]["id"], "DEF");
        assert_eq!(second["more"], false);
        assert!(adapter
            .inner
            .lock()
            .unwrap()
            .profile
            .as_ref()
            .unwrap()
            .posts
            .contains("DEF"));
        adapter.invalidate_photos().unwrap();
        assert!(adapter
            .profile_request(&json!({"action":"profile-more"}), 0, 0)
            .is_err());
        server.join().unwrap();
        adapter.clear().unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn browser_calendar_preserves_cache_and_marks_results_incomplete() {
        use std::sync::Arc;
        let cookies = json!({"cookies":[{"key":"sessionid","value":"123456%3Asynthetic","domain":"instagram.com","path":"/","secure":true},{"key":"ds_user_id","value":"123456","domain":"instagram.com","path":"/","secure":true},{"key":"csrftoken","value":"synthetic","domain":"instagram.com","path":"/","secure":true}]});
        let session = json!({"cookies":cookies.to_string(),"deviceId":"android-test","uuid":"uuid","deviceString":"34/14; test","language":"en_US"});
        let session =
            calmig_core::Session::from_legacy_json(session.to_string().as_bytes()).unwrap();
        let adapter = RustAdapter::default();
        adapter.inner.lock().unwrap().client =
            Some(Arc::new(calmig_core::Client::new(session).unwrap()));
        let directory =
            std::env::temp_dir().join(format!("calmig-following-ui-{}", std::process::id()));
        adapter
            .configure_calendar(0, "synthetic", directory.to_str().unwrap())
            .unwrap();
        let (_, now) = super::calendar_bounds().unwrap();
        let cached = json!({"id":"CACHED","username":"friend","url":"https://www.instagram.com/p/CACHED/","images":["https://scontent.cdninstagram.com/a"],"thumbnail":"https://scontent.cdninstagram.com/a","publishedAt":chrono::DateTime::from_timestamp_millis(now).unwrap().to_rfc3339()});
        adapter
            .inner
            .lock()
            .unwrap()
            .calendar_view
            .as_mut()
            .unwrap()
            .photos
            .push(cached);
        let (job, account, _, _) = adapter.prepare_web_calendar().unwrap();
        assert_eq!(account, "123456");
        assert!(adapter.prepare_web_calendar().is_err());
        let photo = calmig_core::PhotoPost {
            code: "NEW".into(),
            username: "friend".into(),
            url: "https://www.instagram.com/p/NEW/".into(),
            images: vec!["https://scontent.cdninstagram.com/b".into()],
            thumbnail: "https://scontent.cdninstagram.com/b".into(),
            published_at: now.div_euclid(1000),
            caption: String::new(),
        };
        *job.state.lock().unwrap() = (
            1,
            Some(calmig_core::CalendarState {
                status: calmig_core::CalendarStatus::Complete,
                accounts: 1,
                photos: vec![photo],
                incomplete: true,
                feedback_required: false,
            }),
        );
        let result = adapter
            .calendar_request(&json!({"action":"calendar-status"}))
            .unwrap();
        assert_eq!(result["source"], "following");
        assert_eq!(result["incomplete"], true);
        assert_eq!(result["cached"], true);
        assert_eq!(result["photos"].as_array().unwrap().len(), 2);
        adapter.clear().unwrap();
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn rust_calendar_ui_syncs_filters_and_persists_fixture_photos() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::Arc,
            thread,
            time::{Duration, Instant, SystemTime, UNIX_EPOCH},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let server = thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0u8; 8192];
                let n = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..n]);
                let body = if index == 0 {
                    assert!(request.starts_with("GET /api/v1/friendships/123456/following/"));
                    json!({"status":"ok","users":[{"pk":"7","username":"friend"}]}).to_string()
                } else {
                    assert!(request.starts_with("GET /api/v1/feed/user/7/"));
                    json!({"status":"ok","items":[{"code":"ABC","taken_at":now,"media_type":1,"image_versions2":{"candidates":[{"url":"https://scontent.cdninstagram.com/a","width":320}]}},{"code":"VIDEO","taken_at":now,"media_type":2}],"more_available":false}).to_string()
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let cookies = json!({"cookies":[{"key":"sessionid","value":"123456%3Asynthetic","domain":"instagram.com","path":"/","secure":true},{"key":"ds_user_id","value":"123456","domain":"instagram.com","path":"/","secure":true},{"key":"csrftoken","value":"synthetic","domain":"instagram.com","path":"/","secure":true}]});
        let session = json!({"cookies":cookies.to_string(),"deviceId":"android-test","uuid":"uuid","deviceString":"34/14; test","language":"en_US"});
        let session =
            calmig_core::Session::from_legacy_json(session.to_string().as_bytes()).unwrap();
        let client =
            calmig_core::Client::for_local_test(session, &format!("http://{address}/")).unwrap();
        let adapter = RustAdapter::default();
        adapter.inner.lock().unwrap().client = Some(Arc::new(client));
        let dir =
            std::env::temp_dir().join(format!("calmig-calendar-{}-{}", std::process::id(), now));
        adapter
            .configure_calendar(0, "synthetic", dir.to_str().unwrap())
            .unwrap();
        assert!(adapter
            .calendar_request(&json!({"action":"calendar-start"}))
            .is_ok());
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            let state = adapter
                .calendar_request(&json!({"action":"calendar-status"}))
                .unwrap();
            if state["status"] != "loading" {
                break state;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(result["status"], "complete");
        assert_eq!(result["photos"].as_array().unwrap().len(), 1);
        assert_eq!(result["photos"][0]["id"], "ABC");
        let (cutoff, now) = super::calendar_bounds().unwrap();
        assert_eq!(
            calmig_core::load_calendar_cache(&dir, "synthetic", cutoff, now)
                .unwrap()
                .len(),
            1
        );
        adapter.invalidate_photos().unwrap();
        adapter
            .calendar_request(&json!({"action":"calendar-clear"}))
            .unwrap();
        assert!(
            calmig_core::load_calendar_cache(&dir, "synthetic", cutoff, now)
                .unwrap()
                .is_empty()
        );
        server.join().unwrap();
        adapter.clear().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rust_calendar_ui_retains_cached_photos_after_feedback_restriction() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::Arc,
            thread,
            time::{Duration, Instant, SystemTime, UNIX_EPOCH},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 8192];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"message":"feedback_required"}"#;
            write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let cookies = json!({"cookies":[{"key":"sessionid","value":"123456%3Asynthetic","domain":"instagram.com","path":"/","secure":true},{"key":"ds_user_id","value":"123456","domain":"instagram.com","path":"/","secure":true},{"key":"csrftoken","value":"synthetic","domain":"instagram.com","path":"/","secure":true}]});
        let session = json!({"cookies":cookies.to_string(),"deviceId":"android-test","uuid":"uuid","deviceString":"34/14; test","language":"en_US"});
        let session =
            calmig_core::Session::from_legacy_json(session.to_string().as_bytes()).unwrap();
        let client =
            calmig_core::Client::for_local_test(session, &format!("http://{address}/")).unwrap();
        let adapter = RustAdapter::default();
        adapter.inner.lock().unwrap().client = Some(Arc::new(client));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "calmig-calendar-error-{}-{nonce}",
            std::process::id()
        ));
        let (cutoff, now) = super::calendar_bounds().unwrap();
        let timestamp = chrono::DateTime::from_timestamp_millis(now)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let cached = json!({"id":"ABC","username":"friend","url":"https://www.instagram.com/p/ABC/","images":["https://scontent.cdninstagram.com/a"],"thumbnail":"https://scontent.cdninstagram.com/a","publishedAt":timestamp});
        calmig_core::save_calendar_cache(&dir, "synthetic", &[cached], cutoff, now).unwrap();
        adapter
            .configure_calendar(0, "synthetic", dir.to_str().unwrap())
            .unwrap();
        assert!(adapter
            .calendar_request(&json!({"action":"calendar-start"}))
            .is_ok());
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            let state = adapter
                .calendar_request(&json!({"action":"calendar-status"}))
                .unwrap();
            if state["status"] != "loading" {
                break state;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(result["status"], "error");
        assert_eq!(result["reason"], "feedback_required");
        assert_eq!(result["incomplete"], true);
        assert_eq!(result["cached"], true);
        assert_eq!(result["photos"][0]["id"], "ABC");
        server.join().unwrap();
        adapter.clear().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clearing_calendar_during_request_does_not_restore_stale_photos() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::{mpsc, Arc},
            thread,
            time::{Duration, SystemTime, UNIX_EPOCH},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 8192];
            let _ = stream.read(&mut request).unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let body = r#"{"status":"ok","users":[]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let cookies = json!({"cookies":[{"key":"sessionid","value":"123456%3Asynthetic","domain":"instagram.com","path":"/","secure":true},{"key":"ds_user_id","value":"123456","domain":"instagram.com","path":"/","secure":true},{"key":"csrftoken","value":"synthetic","domain":"instagram.com","path":"/","secure":true}]});
        let saved = json!({"cookies":cookies.to_string(),"deviceId":"android-test","uuid":"uuid","deviceString":"34/14; test","language":"en_US"});
        let session = calmig_core::Session::from_legacy_json(saved.to_string().as_bytes()).unwrap();
        let client =
            calmig_core::Client::for_local_test(session, &format!("http://{address}/")).unwrap();
        let adapter = RustAdapter::default();
        adapter.inner.lock().unwrap().client = Some(Arc::new(client));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "calmig-calendar-clear-{}-{nonce}",
            std::process::id()
        ));
        adapter
            .configure_calendar(0, "synthetic", dir.to_str().unwrap())
            .unwrap();
        adapter
            .calendar_request(&json!({"action":"calendar-start"}))
            .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        adapter.invalidate_photos().unwrap();
        adapter
            .calendar_request(&json!({"action":"calendar-clear"}))
            .unwrap();
        release_tx.send(()).unwrap();
        server.join().unwrap();
        let state = adapter
            .calendar_request(&json!({"action":"calendar-status"}))
            .unwrap();
        assert_eq!(state["status"], "complete");
        assert!(state["photos"].as_array().unwrap().is_empty());
        adapter.clear().unwrap();
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn stale_auth_guard_cannot_unlock_a_new_login() {
        let adapter = std::sync::Arc::new(RustAdapter::default());
        let first = adapter.begin_auth().unwrap();
        assert!(adapter.begin_auth().is_err());
        adapter.clear().unwrap();
        let second = adapter.begin_auth().unwrap();
        drop(first);
        assert!(adapter.begin_auth().is_err());
        drop(second);
        assert!(adapter.begin_auth().is_ok());
    }

    #[test]
    fn logout_invalidates_pending_rust_session_import() {
        let adapter = RustAdapter::default();
        let generation = adapter.generation().unwrap();
        let cookies = json!({"cookies": [
            {"key":"sessionid", "value":"123456%3Asynthetic", "domain":"instagram.com", "path":"/", "secure":true},
            {"key":"ds_user_id", "value":"123456", "domain":"instagram.com", "path":"/", "secure":true},
            {"key":"csrftoken", "value":"synthetic", "domain":"instagram.com", "path":"/", "secure":true}
        ]});
        let state = json!({"cookies":cookies.to_string(), "deviceId":"android-test", "uuid":"test-uuid", "deviceString":"34/14; test device", "language":"en_US"});
        adapter.clear().unwrap();
        assert!(adapter.attach(generation, &state).is_none());
        assert!(adapter.inner.lock().unwrap().client.is_none());
        let current = adapter.generation().unwrap();
        assert!(adapter.attach(current, &state).is_some());
        assert!(adapter
            .direct_request(&json!({"action":"threads","more":true}))
            .is_err());
        assert!(adapter
            .direct_request(&json!({"action":"messages","thread":"777"}))
            .is_err());
        assert!(adapter
            .direct_request(&json!({"action":"send","thread":"777","text":"synthetic"}))
            .is_err());
        assert!(adapter
            .native_request(&json!({"action":"messages-page","generation":current,"thread":"777"}))
            .is_err());
        assert!(adapter.native_request(&json!({"action":"message-send","generation":current,"thread":"777","text":"synthetic"})).is_err());
        adapter
            .inner
            .lock()
            .unwrap()
            .allowed_threads
            .insert("777".into());
        assert!(adapter.native_request(&json!({"action":"messages-page","generation":current,"thread":"777","cursor":"unissued"})).is_err());
        adapter
            .inner
            .lock()
            .unwrap()
            .sending_threads
            .insert("777".into());
        assert!(adapter.native_request(&json!({"action":"message-send","generation":current,"thread":"777","text":"synthetic"})).is_err());
        assert_eq!(
            adapter.native_request(&json!({"action":"following-status"})),
            Ok(json!({"generation":current}))
        );
        tauri::async_runtime::block_on(async { adapter.clear() }).unwrap();
        assert!(adapter.inner.lock().unwrap().client.is_none());
        assert!(adapter.inner.lock().unwrap().allowed_threads.is_empty());
        assert_eq!(
            adapter.native_request(&json!({"action":"following-status"})),
            Ok(json!(null))
        );
        assert!(adapter
            .native_request(&json!({"action":"following-page", "generation":current}))
            .is_err());
        assert!(adapter
            .native_request(
                &json!({"action":"browser-verify", "generation":generation, "session":state})
            )
            .is_err());
        assert!(adapter.native_request(&json!({
            "action":"profile-page", "generation":current, "id":"7", "username":"friend", "cutoff":1, "now":2
        })).is_err());
    }

    #[test]
    fn login_window_allows_only_official_https_hosts() {
        assert!(allowed_login_url(
            &"https://www.instagram.com/accounts/login/".parse().unwrap()
        ));
        assert!(allowed_login_url(
            &"https://accountscenter.meta.com/".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"https://instagram.com.evil.example/".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"http://www.instagram.com/".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"https://user:pass@www.instagram.com/".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"https://www.instagram.com:444/".parse().unwrap()
        ));
        assert!(allowed_login_url(
            &"https://www.fbsbx.com/captcha/recaptcha/iframe/"
                .parse()
                .unwrap()
        ));
        assert!(allowed_login_url(
            &"https://www.google.com/recaptcha/enterprise/anchor"
                .parse()
                .unwrap()
        ));
        assert!(!allowed_login_url(
            &"https://www.fbsbx.com/other/".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"https://www.google.com/search".parse().unwrap()
        ));
        assert!(!allowed_login_url(
            &"https://www.google.com.evil.test/recaptcha/"
                .parse()
                .unwrap()
        ));
    }
}
