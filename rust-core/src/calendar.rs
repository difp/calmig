//! Incremental, chronological calendar sync over followed accounts only.
use crate::{Account, Client, Error, FollowingPage, PhotoPost, ProfilePage};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub trait CalendarSource: Sync {
    fn following_page(&self, cursor: Option<&str>) -> Result<FollowingPage, Error>;
    fn profile_page(
        &self,
        author: &Account,
        cursor: Option<&str>,
        cutoff: i64,
        now: i64,
    ) -> Result<ProfilePage, Error>;
}
impl CalendarSource for Client {
    fn following_page(&self, cursor: Option<&str>) -> Result<FollowingPage, Error> {
        self.following_page(cursor)
    }
    fn profile_page(
        &self,
        author: &Account,
        cursor: Option<&str>,
        cutoff: i64,
        now: i64,
    ) -> Result<ProfilePage, Error> {
        self.profile_photos_page(author, cursor, cutoff, now)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarStatus {
    Loading,
    Complete,
    Cancelled,
    Error,
}
#[derive(Debug, Clone)]
pub struct CalendarState {
    pub status: CalendarStatus,
    pub accounts: usize,
    pub photos: Vec<PhotoPost>,
    pub incomplete: bool,
    pub feedback_required: bool,
}
struct Progress {
    state: CalendarState,
    posts: HashMap<String, PhotoPost>,
}
impl Progress {
    fn publish(&self, update: &(impl Fn(CalendarState) + Sync)) {
        let mut state = self.state.clone();
        state.photos = self.posts.values().cloned().collect();
        state.photos.sort_by(|a, b| {
            b.published_at
                .cmp(&a.published_at)
                .then_with(|| a.code.cmp(&b.code))
        });
        update(state);
    }
}

/// Run a cancellable sync. No timeline, recommendations or media other than still photographs are requested.
/// The caller supplies the seven-day local cutoff, in Unix seconds.
pub fn sync_calendar(
    source: &impl CalendarSource,
    cutoff: i64,
    now: i64,
    cancelled: &AtomicBool,
    update: impl Fn(CalendarState) + Sync,
    delay: Duration,
) {
    let progress = Mutex::new(Progress {
        state: CalendarState {
            status: CalendarStatus::Loading,
            accounts: 0,
            photos: Vec::new(),
            incomplete: false,
            feedback_required: false,
        },
        posts: HashMap::new(),
    });
    let failed = AtomicBool::new(false);
    let feedback_required = AtomicBool::new(false);
    let next_slot = Mutex::new(Instant::now());
    let mut users = HashSet::new();
    let mut following_cursors = HashSet::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    if cutoff > now {
        failed.store(true, Ordering::Relaxed);
    }
    while !failed.load(Ordering::Relaxed) && !cancelled.load(Ordering::Relaxed) {
        if pages >= 100 {
            progress.lock().unwrap().state.incomplete = true;
            break;
        }
        let page = match source.following_page(cursor.as_deref()) {
            Ok(page) => page,
            Err(error) => {
                if error == Error::FeedbackRequired {
                    feedback_required.store(true, Ordering::Relaxed);
                }
                failed.store(true, Ordering::Relaxed);
                break;
            }
        };
        pages += 1;
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        if let Some(next) = &page.next_cursor {
            if next.is_empty()
                || next.len() > 2048
                || next.chars().any(char::is_control)
                || !following_cursors.insert(next.clone())
            {
                failed.store(true, Ordering::Relaxed);
                break;
            }
        }
        if page.accounts.is_empty() && page.next_cursor.is_some() {
            progress.lock().unwrap().state.incomplete = true;
            break;
        }
        let queue: Vec<Account> = page
            .accounts
            .into_iter()
            .filter(|account| users.insert(account.id.clone()))
            .collect();
        let index = Mutex::new(0usize);
        thread::scope(|scope| {
            for _ in 0..queue.len().min(3) {
                let queue = &queue;
                let index = &index;
                let progress = &progress;
                let failed = &failed;
                let feedback_required = &feedback_required;
                let next_slot = &next_slot;
                let update = &update;
                scope.spawn(move || loop {
                    if cancelled.load(Ordering::Relaxed) || failed.load(Ordering::Relaxed) {
                        return;
                    }
                    let account = {
                        let mut index = index.lock().unwrap();
                        let value = queue.get(*index);
                        *index += 1;
                        value
                    };
                    let Some(account) = account else { return };
                    let mut profile_cursor: Option<String> = None;
                    let mut seen = HashSet::new();
                    let mut count = 0;
                    loop {
                        let slot = {
                            let mut slot = next_slot.lock().unwrap();
                            let time = Instant::now().max(*slot);
                            *slot = time + delay;
                            time
                        };
                        while Instant::now() < slot {
                            if cancelled.load(Ordering::Relaxed) || failed.load(Ordering::Relaxed) {
                                return;
                            }
                            thread::sleep(
                                slot.saturating_duration_since(Instant::now())
                                    .min(Duration::from_millis(25)),
                            );
                        }
                        if cancelled.load(Ordering::Relaxed) || failed.load(Ordering::Relaxed) {
                            return;
                        }
                        let page = match source.profile_page(
                            account,
                            profile_cursor.as_deref(),
                            cutoff,
                            now,
                        ) {
                            Ok(page) => page,
                            Err(error) => {
                                if error == Error::FeedbackRequired {
                                    feedback_required.store(true, Ordering::Relaxed);
                                }
                                failed.store(true, Ordering::Relaxed);
                                return;
                            }
                        };
                        if cancelled.load(Ordering::Relaxed) || failed.load(Ordering::Relaxed) {
                            return;
                        }
                        count += 1;
                        if let Some(next) = &page.next_cursor {
                            if next.is_empty()
                                || next.len() > 2048
                                || next.chars().any(char::is_control)
                                || !seen.insert(next.clone())
                            {
                                failed.store(true, Ordering::Relaxed);
                                return;
                            }
                        }
                        let mut p = progress.lock().unwrap();
                        let found = !page.photos.is_empty();
                        for photo in page.photos {
                            p.posts.insert(photo.code.clone(), photo);
                        }
                        if page.all_older_than_cutoff || page.next_cursor.is_none() {
                            p.state.accounts += 1;
                            p.publish(update);
                            break;
                        }
                        if page.item_count == 0 || count >= 10 {
                            p.state.incomplete = true;
                            p.state.accounts += 1;
                            p.publish(update);
                            break;
                        }
                        if found {
                            p.publish(update);
                        }
                        drop(p);
                        profile_cursor = page.next_cursor;
                    }
                });
            }
        });
        if failed.load(Ordering::Relaxed) || cancelled.load(Ordering::Relaxed) {
            break;
        }
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    let mut p = progress.lock().unwrap();
    p.state.status = if cancelled.load(Ordering::Relaxed) {
        CalendarStatus::Cancelled
    } else if failed.load(Ordering::Relaxed) {
        p.state.incomplete = true;
        p.state.feedback_required = feedback_required.load(Ordering::Relaxed);
        CalendarStatus::Error
    } else {
        CalendarStatus::Complete
    };
    p.publish(&update);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::session;
    use reqwest::Url;
    use std::sync::atomic::AtomicUsize;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    struct Fake {
        fail: bool,
        calls: AtomicUsize,
    }
    impl CalendarSource for Fake {
        fn following_page(&self, cursor: Option<&str>) -> Result<FollowingPage, Error> {
            Ok(if cursor.is_none() {
                FollowingPage {
                    accounts: vec![
                        Account {
                            id: "1".into(),
                            username: "one".into(),
                        },
                        Account {
                            id: "2".into(),
                            username: "two".into(),
                        },
                    ],
                    next_cursor: Some("next".into()),
                }
            } else {
                FollowingPage {
                    accounts: vec![Account {
                        id: "1".into(),
                        username: "one".into(),
                    }],
                    next_cursor: None,
                }
            })
        }
        fn profile_page(
            &self,
            author: &Account,
            _cursor: Option<&str>,
            _cutoff: i64,
            _now: i64,
        ) -> Result<ProfilePage, Error> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.fail && author.id == "2" {
                return Err(Error::Network);
            }
            Ok(ProfilePage {
                photos: vec![PhotoPost {
                    code: author.id.clone(),
                    username: author.username.clone(),
                    url: String::new(),
                    images: vec![],
                    thumbnail: String::new(),
                    published_at: if author.id == "1" { 1 } else { 2 },
                    caption: String::new(),
                }],
                next_cursor: None,
                item_count: 1,
                all_older_than_cutoff: false,
            })
        }
    }
    #[test]
    fn follows_pages_once_and_publishes_photos_in_chronological_order() {
        let source = Fake {
            fail: false,
            calls: AtomicUsize::new(0),
        };
        let states = Mutex::new(Vec::new());
        sync_calendar(
            &source,
            0,
            3,
            &AtomicBool::new(false),
            |s| states.lock().unwrap().push(s),
            Duration::ZERO,
        );
        let states = states.lock().unwrap();
        let last = states.last().unwrap();
        assert_eq!(last.status, CalendarStatus::Complete);
        assert_eq!(last.accounts, 2);
        assert_eq!(
            last.photos
                .iter()
                .map(|p| p.code.as_str())
                .collect::<Vec<_>>(),
            vec!["2", "1"]
        );
        assert_eq!(source.calls.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn errors_are_incomplete_and_cancellation_stops_requests() {
        let source = Fake {
            fail: true,
            calls: AtomicUsize::new(0),
        };
        let states = Mutex::new(Vec::new());
        sync_calendar(
            &source,
            0,
            3,
            &AtomicBool::new(false),
            |s| states.lock().unwrap().push(s),
            Duration::ZERO,
        );
        let states = states.lock().unwrap();
        assert_eq!(states.last().unwrap().status, CalendarStatus::Error);
        assert!(states.last().unwrap().incomplete);
        assert!(!states.last().unwrap().feedback_required);
        let cancelled = AtomicBool::new(true);
        let states = Mutex::new(Vec::new());
        sync_calendar(
            &source,
            0,
            3,
            &cancelled,
            |s| states.lock().unwrap().push(s),
            Duration::ZERO,
        );
        assert_eq!(
            states.lock().unwrap().last().unwrap().status,
            CalendarStatus::Cancelled
        );
    }

    #[test]
    fn feedback_restriction_is_carried_to_the_calendar_state() {
        struct Feedback;
        impl CalendarSource for Feedback {
            fn following_page(&self, _: Option<&str>) -> Result<FollowingPage, Error> {
                Err(Error::FeedbackRequired)
            }
            fn profile_page(
                &self,
                _: &Account,
                _: Option<&str>,
                _: i64,
                _: i64,
            ) -> Result<ProfilePage, Error> {
                unreachable!()
            }
        }
        let states = Mutex::new(Vec::new());
        sync_calendar(
            &Feedback,
            0,
            1,
            &AtomicBool::new(false),
            |state| states.lock().unwrap().push(state),
            Duration::ZERO,
        );
        let states = states.lock().unwrap();
        let result = states.last().unwrap();
        assert_eq!(result.status, CalendarStatus::Error);
        assert!(result.incomplete && result.feedback_required);
    }

    #[test]
    fn full_sync_fetches_following_and_filters_profile_media() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 8192];
                let n = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..n]);
                let body = if index == 0 {
                    assert!(request.starts_with("GET /api/v1/friendships/123456/following/"));
                    r#"{"status":"ok","users":[{"pk":"7","username":"friend"}]}"#
                } else {
                    assert!(request.starts_with("GET /api/v1/feed/user/7/"));
                    r#"{"status":"ok","items":[{"code":"PHOTO","taken_at":1000,"media_type":1,"image_versions2":{"candidates":[{"url":"https://scontent.cdninstagram.com/p.jpg","width":320}]}},{"code":"VIDEO","taken_at":1001,"media_type":2}],"more_available":false}"#
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
        let client = Client::with_origin(
            session(),
            Url::parse(&format!("http://{address}/")).unwrap(),
        )
        .unwrap();
        let states = Mutex::new(Vec::new());
        sync_calendar(
            &client,
            900,
            1100,
            &AtomicBool::new(false),
            |s| states.lock().unwrap().push(s),
            Duration::ZERO,
        );
        server.join().unwrap();
        let states = states.lock().unwrap();
        let last = states.last().unwrap();
        assert_eq!(last.status, CalendarStatus::Complete);
        assert_eq!(last.photos.len(), 1);
        assert_eq!(last.photos[0].code, "PHOTO");
    }
}
