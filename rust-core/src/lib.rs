//! Rust implementation of CalmIG's Instagram adapter, under development.
//!
//! A restored session contains credentials. Never log or serialize it for diagnostics.
mod browser_login;
mod cache;
mod calendar;
mod comments;
mod device;
mod direct;
mod following;
mod following_calendar;
mod following_feed;
mod legacy_device;
mod legacy_storage;
mod media;
mod profile;
mod session;

pub use browser_login::{client_from_browser, BrowserLoginError};
pub use cache::{clear_calendar_cache, load_calendar_cache, save_calendar_cache};
pub use calendar::{sync_calendar, CalendarSource, CalendarState, CalendarStatus};
pub use comments::{media_id_from_code, Comment, CommentPage};
pub use device::load_or_import_device;
pub use direct::{InboxPage, Message, MessagePage, Thread};
pub use following::{Account, Client, Error, FollowingPage, FollowingPager, SessionUpdate};
pub use following_calendar::{sync_following_calendar, FollowingFeedSource};
pub use following_feed::{parse_following_feed_page, FollowingFeedPage};
pub use legacy_device::legacy_device;
pub use legacy_storage::update_legacy_session;
pub use media::{to_photo, PhotoPost, ProfilePage, ProfilePager};
pub use profile::ProfileInfo;
pub use session::{BrowserCookie, DeviceProfile, Session, SessionError};
