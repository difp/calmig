//! Bounded snapshots of the official Following page, without transferring cookies.
use std::{
    sync::{atomic::Ordering, mpsc, Arc},
    thread,
    time::Duration,
};
use tauri::{Url, WebviewWindow};

const SNAPSHOT: &str = include_str!("web/following-snapshot.js");
const NEXT_PAGE: &str = include_str!("web/following-next.js");
const MAX_SNAPSHOT: usize = 4_000_000;

fn allowed_page(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("www.instagram.com")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|port| port == 443)
        && url.path() == "/"
        && url
            .query_pairs()
            .filter(|(key, _)| key == "variant")
            .map(|(_, value)| value.into_owned())
            .collect::<Vec<_>>()
            == ["following"]
}

fn parse_snapshot(
    text: &str,
    cutoff: i64,
    now: i64,
    cursor: Option<&str>,
) -> Result<Option<calmig_core::FollowingFeedPage>, ()> {
    if text.len() > MAX_SNAPSHOT {
        return Err(());
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| ())?;
    match value.get("status").and_then(serde_json::Value::as_str) {
        Some("pending") => Ok(None),
        // During navigation WebKit can report the new URI before the document changes.
        // The script returns no data from that old document; wait for the intended page.
        Some("wrong_page") if cursor.is_none() => Ok(None),
        Some("ready") => calmig_core::parse_following_feed_page(&value, cutoff, now, cursor)
            .map(Some)
            .map_err(|_| ()),
        _ => Err(()),
    }
}

async fn execute(window: WebviewWindow, script: String, asynchronous: bool) -> Result<String, ()> {
    tauri::async_runtime::spawn_blocking(move || {
        use javascriptcore::ValueExt;
        use webkit2gtk::{gio, WebViewExt};
        let (sender, receiver) = mpsc::sync_channel(1);
        window
            .with_webview(move |view| {
                let webview = view.inner();
                let approved = webview
                    .uri()
                    .and_then(|uri| Url::parse(&uri).ok())
                    .is_some_and(|uri| allowed_page(&uri));
                if !approved {
                    let _ = sender.send(Err(()));
                    return;
                }
                let finish =
                    move |result: Result<javascriptcore::Value, webkit2gtk::glib::Error>| {
                        let value = result
                            .ok()
                            .filter(|value| value.is_string())
                            .map(|value| value.to_str().to_string())
                            .filter(|value| value.len() <= MAX_SNAPSHOT)
                            .ok_or(());
                        let _ = sender.send(value);
                    };
                if asynchronous {
                    webview.call_async_javascript_function(
                        &script,
                        None,
                        None,
                        None,
                        None::<&gio::Cancellable>,
                        finish,
                    );
                } else {
                    webview.evaluate_javascript(
                        &script,
                        None,
                        None,
                        None::<&gio::Cancellable>,
                        finish,
                    );
                }
            })
            .map_err(|_| ())?;
        receiver
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| ())?
    })
    .await
    .map_err(|_| ())?
}

/// Read the first page. None means the DOM has not finished loading yet.
pub async fn read_snapshot(
    window: WebviewWindow,
    cutoff: i64,
    now: i64,
    account_id: &str,
) -> Result<Option<calmig_core::FollowingFeedPage>, ()> {
    if account_id.is_empty()
        || account_id.len() > 30
        || !account_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(());
    }
    let argument = serde_json::to_string(account_id).map_err(|_| ())?;
    let text = execute(window, format!("({SNAPSHOT})({argument})"), false).await?;
    parse_snapshot(&text, cutoff, now, None)
}

/// Fetch at most one next page; errors are terminal and never retried.
pub async fn read_next_page(
    window: WebviewWindow,
    cutoff: i64,
    now: i64,
    cursor: &str,
    account_id: &str,
) -> Result<calmig_core::FollowingFeedPage, ()> {
    if cursor.is_empty()
        || cursor.len() > 2048
        || cursor.chars().any(char::is_control)
        || account_id.is_empty()
        || account_id.len() > 30
        || !account_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(());
    }
    let argument = serde_json::to_string(cursor).map_err(|_| ())?;
    let account = serde_json::to_string(account_id).map_err(|_| ())?;
    let script = format!("return await ({NEXT_PAGE})({argument}, {account});");
    let text = execute(window, script, true).await?;
    parse_snapshot(&text, cutoff, now, Some(cursor)).and_then(|page| page.ok_or(()))
}

pub struct BrowserFollowingSource {
    pub window: WebviewWindow,
    pub job: Arc<crate::CalendarJob>,
    pub account_id: String,
}

impl calmig_core::FollowingFeedSource for BrowserFollowingSource {
    fn page(
        &self,
        cursor: Option<&str>,
        cutoff: i64,
        now: i64,
    ) -> Result<calmig_core::FollowingFeedPage, calmig_core::Error> {
        if let Some(cursor) = cursor {
            // A failed page is not retried; the calendar keeps any earlier results.
            for _ in 0..4 {
                if self.job.cancelled.load(Ordering::Relaxed) {
                    return Err(calmig_core::Error::Network);
                }
                thread::sleep(Duration::from_millis(100));
            }
            return tauri::async_runtime::block_on(read_next_page(
                self.window.clone(),
                cutoff,
                now,
                cursor,
                &self.account_id,
            ))
            .map_err(|_| calmig_core::Error::Remote);
        }
        for _ in 0..100 {
            if self.job.cancelled.load(Ordering::Relaxed) {
                return Err(calmig_core::Error::Network);
            }
            if self.window.url().ok().is_some_and(|url| allowed_page(&url)) {
                match tauri::async_runtime::block_on(read_snapshot(
                    self.window.clone(),
                    cutoff,
                    now,
                    &self.account_id,
                )) {
                    Ok(Some(page)) => return Ok(page),
                    Ok(None) => (),
                    Err(()) => return Err(calmig_core::Error::InvalidResponse),
                }
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err(calmig_core::Error::Network)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_allows_the_exact_following_origin_and_mode() {
        assert!(allowed_page(
            &"https://www.instagram.com/?variant=following"
                .parse()
                .unwrap()
        ));
        for url in [
            "http://www.instagram.com/?variant=following",
            "https://instagram.com/?variant=following",
            "https://www.instagram.com/accounts/login/?variant=following",
            "https://www.instagram.com/?variant=home",
            "https://www.instagram.com/?variant=following&variant=home",
            "https://www.instagram.com.evil.test/?variant=following",
        ] {
            assert!(!allowed_page(&url.parse().unwrap()));
        }
    }
    #[test]
    fn rejects_invalid_snapshots_and_preserves_pending_state() {
        assert!(parse_snapshot(r#"{"status":"pending"}"#, 0, 10, None)
            .unwrap()
            .is_none());
        assert!(parse_snapshot(r#"{"status":"wrong_page"}"#, 0, 10, None)
            .unwrap()
            .is_none());
        assert!(parse_snapshot(r#"{"status":"wrong_page"}"#, 0, 10, Some("cursor")).is_err());
        assert!(parse_snapshot(r#"{"status":"ready","data":{}}"#, 0, 10, None).is_err());
        assert!(parse_snapshot(&"x".repeat(MAX_SNAPSHOT + 1), 0, 10, None).is_err());
    }
}
