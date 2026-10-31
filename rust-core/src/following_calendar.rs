//! Bounded, chronological calendar assembly for a verified Following page source.
use crate::{CalendarState, CalendarStatus, Error, FollowingFeedPage, PhotoPost};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_PAGES: usize = 20;

/// A source must request Following mode explicitly and never return a general timeline.
pub trait FollowingFeedSource {
    fn page(&self, cursor: Option<&str>, cutoff: i64, now: i64)
        -> Result<FollowingFeedPage, Error>;
}

/// Assemble pages without assuming that a short page or an old post ends pagination.
/// The source is experimental, so even a successful traversal is marked incomplete
/// until its completeness is independently verified against Instagram.
pub fn sync_following_calendar(
    source: &impl FollowingFeedSource,
    cutoff: i64,
    now: i64,
    cancelled: &AtomicBool,
    update: impl Fn(CalendarState),
) {
    sync_following_calendar_with_limit(source, cutoff, now, cancelled, MAX_PAGES, update)
}

/// Identical to `sync_following_calendar`, with a smaller bound for controlled checks.
fn sync_following_calendar_with_limit(
    source: &impl FollowingFeedSource,
    cutoff: i64,
    now: i64,
    cancelled: &AtomicBool,
    page_limit: usize,
    update: impl Fn(CalendarState),
) {
    let mut state = CalendarState {
        status: CalendarStatus::Loading,
        accounts: 0,
        photos: Vec::new(),
        incomplete: true,
        feedback_required: false,
    };
    let mut posts = HashMap::<String, PhotoPost>::new();
    let mut authors = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor: Option<String> = None;
    if cutoff > now {
        state.status = CalendarStatus::Error;
        update(state);
        return;
    }
    for _ in 0..page_limit.clamp(1, MAX_PAGES) {
        if cancelled.load(Ordering::Relaxed) {
            state.status = CalendarStatus::Cancelled;
            update(state);
            return;
        }
        let page = match source.page(cursor.as_deref(), cutoff, now) {
            Ok(page) => page,
            Err(error) => {
                if cancelled.load(Ordering::Relaxed) {
                    state.status = CalendarStatus::Cancelled;
                } else {
                    state.status = CalendarStatus::Error;
                    state.feedback_required = error == Error::FeedbackRequired;
                }
                update(state);
                return;
            }
        };
        if cancelled.load(Ordering::Relaxed) {
            state.status = CalendarStatus::Cancelled;
            update(state);
            return;
        }
        for photo in page.photos {
            authors.insert(photo.username.to_ascii_lowercase());
            posts.entry(photo.code.clone()).or_insert(photo);
        }
        state.accounts = authors.len();
        state.photos = posts.values().cloned().collect();
        state.photos.sort_by(|a, b| {
            b.published_at
                .cmp(&a.published_at)
                .then_with(|| a.code.cmp(&b.code))
        });
        match page.next_cursor {
            Some(next) => {
                if next.is_empty()
                    || next.len() > 2048
                    || next.chars().any(char::is_control)
                    || !cursors.insert(next.clone())
                {
                    state.status = CalendarStatus::Error;
                    update(state);
                    return;
                }
                cursor = Some(next);
                update(state.clone());
            }
            None => {
                state.status = CalendarStatus::Complete;
                update(state);
                return;
            }
        }
    }
    state.status = CalendarStatus::Complete;
    update(state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, sync::atomic::AtomicUsize};
    struct Fixture {
        calls: AtomicUsize,
        pages: RefCell<Vec<Result<FollowingFeedPage, Error>>>,
    }
    impl FollowingFeedSource for Fixture {
        fn page(&self, _: Option<&str>, _: i64, _: i64) -> Result<FollowingFeedPage, Error> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.pages.borrow_mut().remove(0)
        }
    }
    fn photo(code: &str, username: &str, timestamp: i64) -> PhotoPost {
        PhotoPost {
            code: code.into(),
            username: username.into(),
            url: String::new(),
            images: vec![],
            thumbnail: String::new(),
            published_at: timestamp,
            caption: String::new(),
        }
    }
    fn page(photos: Vec<PhotoPost>, cursor: Option<&str>) -> Result<FollowingFeedPage, Error> {
        Ok(FollowingFeedPage {
            photos,
            next_cursor: cursor.map(str::to_owned),
            incomplete: false,
        })
    }
    #[test]
    fn continues_after_short_pages_deduplicates_and_sorts() {
        let source = Fixture {
            calls: AtomicUsize::new(0),
            pages: RefCell::new(vec![
                page(vec![photo("old", "a", 1)], Some("next")),
                page(vec![photo("new", "b", 2), photo("old", "a", 1)], None),
            ]),
        };
        let states = RefCell::new(Vec::new());
        sync_following_calendar(&source, 0, 3, &AtomicBool::new(false), |s| {
            states.borrow_mut().push(s)
        });
        let states = states.borrow();
        let state = states.last().unwrap();
        assert_eq!(state.status, CalendarStatus::Complete);
        assert!(state.incomplete);
        assert_eq!(state.accounts, 2);
        assert_eq!(
            state
                .photos
                .iter()
                .map(|p| p.code.as_str())
                .collect::<Vec<_>>(),
            vec!["new", "old"]
        );
        assert_eq!(source.calls.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn failure_preserves_partial_results_and_does_not_retry() {
        let source = Fixture {
            calls: AtomicUsize::new(0),
            pages: RefCell::new(vec![
                page(vec![photo("photo", "a", 1)], Some("next")),
                Err(Error::FeedbackRequired),
            ]),
        };
        let states = RefCell::new(Vec::new());
        sync_following_calendar(&source, 0, 3, &AtomicBool::new(false), |s| {
            states.borrow_mut().push(s)
        });
        let states = states.borrow();
        let state = states.last().unwrap();
        assert_eq!(state.status, CalendarStatus::Error);
        assert!(state.feedback_required);
        assert!(state.incomplete);
        assert_eq!(state.photos.len(), 1);
        assert_eq!(source.calls.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn repeated_cursor_and_cancellation_stop_requests() {
        let source = Fixture {
            calls: AtomicUsize::new(0),
            pages: RefCell::new(vec![
                page(vec![], Some("repeat")),
                page(vec![], Some("repeat")),
            ]),
        };
        let states = RefCell::new(Vec::new());
        sync_following_calendar(&source, 0, 3, &AtomicBool::new(false), |s| {
            states.borrow_mut().push(s)
        });
        assert_eq!(
            states.borrow().last().unwrap().status,
            CalendarStatus::Error
        );
        assert_eq!(source.calls.load(Ordering::Relaxed), 2);
        let cancelled = AtomicBool::new(true);
        let source = Fixture {
            calls: AtomicUsize::new(0),
            pages: RefCell::new(vec![]),
        };
        let states = RefCell::new(Vec::new());
        sync_following_calendar(&source, 0, 3, &cancelled, |s| states.borrow_mut().push(s));
        assert_eq!(
            states.borrow().last().unwrap().status,
            CalendarStatus::Cancelled
        );
        assert_eq!(source.calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn page_limit_stops_without_claiming_completeness() {
        let source = Fixture {
            calls: AtomicUsize::new(0),
            pages: RefCell::new(vec![page(vec![], Some("a")), page(vec![], Some("b"))]),
        };
        let states = RefCell::new(Vec::new());
        sync_following_calendar_with_limit(&source, 0, 3, &AtomicBool::new(false), 2, |s| {
            states.borrow_mut().push(s)
        });
        assert_eq!(source.calls.load(Ordering::Relaxed), 2);
        assert_eq!(
            states.borrow().last().unwrap().status,
            CalendarStatus::Complete
        );
        assert!(states.borrow().last().unwrap().incomplete);
    }

    #[test]
    fn cancellation_during_a_failed_request_is_reported_as_cancelled() {
        struct Cancelling<'a>(&'a AtomicBool);
        impl FollowingFeedSource for Cancelling<'_> {
            fn page(&self, _: Option<&str>, _: i64, _: i64) -> Result<FollowingFeedPage, Error> {
                self.0.store(true, Ordering::Relaxed);
                Err(Error::Network)
            }
        }
        let cancelled = AtomicBool::new(false);
        let states = RefCell::new(Vec::new());
        sync_following_calendar(&Cancelling(&cancelled), 0, 3, &cancelled, |state| {
            states.borrow_mut().push(state)
        });
        assert_eq!(
            states.borrow().last().unwrap().status,
            CalendarStatus::Cancelled
        );
    }
}
