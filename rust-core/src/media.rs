//! Filter a profile's media into the photo-only calendar format.
use crate::{Account, Client, Error};
use reqwest::Url;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoPost {
    pub code: String,
    pub username: String,
    pub url: String,
    pub images: Vec<String>,
    pub thumbnail: String,
    /// Unix timestamp in seconds; the presentation layer chooses local dates.
    pub published_at: i64,
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilePage {
    pub photos: Vec<PhotoPost>,
    pub next_cursor: Option<String>,
    pub item_count: usize,
    pub all_older_than_cutoff: bool,
}

pub struct ProfilePager<'a> {
    client: &'a Client,
    author: Account,
    cutoff: i64,
    now: i64,
    cursor: Option<String>,
    seen: HashSet<String>,
    finished: bool,
}

impl ProfilePager<'_> {
    pub fn next_page(&mut self) -> Result<Option<ProfilePage>, Error> {
        if self.finished {
            return Ok(None);
        }
        let page = self.client.profile_photos_page(
            &self.author,
            self.cursor.as_deref(),
            self.cutoff,
            self.now,
        )?;
        if let Some(next) = &page.next_cursor {
            if !self.seen.insert(next.clone()) {
                return Err(Error::InvalidCursor);
            }
        } else {
            self.finished = true;
        }
        self.cursor = page.next_cursor.clone();
        Ok(Some(page))
    }
}

impl Client {
    pub fn profile_photos(&self, author: Account, cutoff: i64, now: i64) -> ProfilePager<'_> {
        ProfilePager {
            client: self,
            author,
            cutoff,
            now,
            cursor: None,
            seen: HashSet::new(),
            finished: false,
        }
    }
    /// Fetch one profile page, retaining only eligible still photographs.
    pub fn profile_photos_page(
        &self,
        author: &Account,
        cursor: Option<&str>,
        cutoff: i64,
        now: i64,
    ) -> Result<ProfilePage, Error> {
        if author.id.is_empty()
            || author.id.len() > 30
            || !author.id.bytes().all(|byte| byte.is_ascii_digit())
            || author.username.is_empty()
            || author.username.len() > 30
            || !author
                .username
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.')
            || cutoff > now
        {
            return Err(Error::InvalidResponse);
        }
        if cursor.is_some_and(|c| c.is_empty() || c.len() > 2048 || c.chars().any(char::is_control))
        {
            return Err(Error::InvalidCursor);
        }
        let path = format!("api/v1/feed/user/{}/", author.id);
        let mut query = Vec::new();
        if let Some(cursor) = cursor {
            query.push(("max_id", cursor));
        }
        let data = self.get_json(&path, &query)?;
        if data
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| status != "ok")
        {
            return Err(Error::Remote);
        }
        let items = data
            .get("items")
            .and_then(Value::as_array)
            .ok_or(Error::InvalidResponse)?;
        let more = data
            .get("more_available")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let next_cursor = if more {
            let value = data
                .get("next_max_id")
                .and_then(Value::as_str)
                .ok_or(Error::InvalidCursor)?;
            if value.is_empty()
                || value.len() > 2048
                || value.chars().any(char::is_control)
                || Some(value) == cursor
            {
                return Err(Error::InvalidCursor);
            }
            Some(value.to_owned())
        } else {
            None
        };
        let all_older_than_cutoff = !items.is_empty()
            && items.iter().all(|item| {
                item.get("taken_at")
                    .and_then(Value::as_i64)
                    .is_some_and(|time| time < cutoff)
            });
        Ok(ProfilePage {
            photos: items
                .iter()
                .filter_map(|item| to_photo(item, author, cutoff, now))
                .collect(),
            next_cursor,
            item_count: items.len(),
            all_older_than_cutoff,
        })
    }
}

pub(crate) fn trusted_image(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    ["cdninstagram.com", "fbcdn.net", "instagram.com"]
        .iter()
        .any(|domain| {
            host == *domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
}

fn image_url(resource: &Value, thumbnail: bool) -> Option<String> {
    let candidates = resource
        .get("image_versions2")?
        .get("candidates")?
        .as_array()?;
    let mut valid = Vec::new();
    for candidate in candidates {
        let Some(url) = candidate
            .get("url")
            .and_then(Value::as_str)
            .filter(|s| trusted_image(s))
        else {
            continue;
        };
        let width = candidate
            .get("width")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite());
        valid.push((url, width));
    }
    let mut sized: Vec<_> = valid
        .iter()
        .filter_map(|(url, width)| width.map(|width| (*url, width)))
        .collect();
    if sized.is_empty() {
        return valid.first().map(|(url, _)| (*url).to_owned());
    }
    sized.sort_by(|a, b| a.1.total_cmp(&b.1));
    let selected = if thumbnail {
        sized
            .iter()
            .find(|(_, width)| *width >= 320.0)
            .or_else(|| sized.last())
    } else {
        sized.last()
    }?;
    Some(selected.0.to_owned())
}

/// Return a post only if every item is a still image in the requested time window.
pub fn to_photo(media: &Value, author: &Account, cutoff: i64, now: i64) -> Option<PhotoPost> {
    let time = media.get("taken_at")?.as_i64()?;
    if time < cutoff
        || time > now
        || media.get("is_paid_partnership").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let code = media.get("code")?.as_str()?;
    if code.is_empty()
        || !code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return None;
    }
    let resources: Vec<&Value> = match media.get("media_type")?.as_u64()? {
        1 => vec![media],
        8 => media.get("carousel_media")?.as_array()?.iter().collect(),
        _ => return None,
    };
    if resources.is_empty()
        || resources.len() > 20
        || resources.iter().any(|item| {
            item.get("media_type").and_then(Value::as_u64) != Some(1)
                || item.get("video_versions").is_some_and(|v| !v.is_null())
        })
    {
        return None;
    }
    let images = resources
        .iter()
        .map(|item| image_url(item, false))
        .collect::<Option<Vec<_>>>()?;
    let thumbnail = image_url(resources[0], true)?;
    let caption_text = media
        .get("caption")
        .and_then(|v| v.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut utf16_length = 0;
    let caption = caption_text
        .chars()
        .take_while(|character| {
            utf16_length += character.len_utf16();
            utf16_length <= 4000
        })
        .collect();
    Some(PhotoPost {
        code: code.into(),
        username: author.username.clone(),
        url: format!("https://www.instagram.com/p/{code}/"),
        images,
        thumbnail,
        published_at: time,
        caption,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::session;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn author() -> Account {
        Account {
            id: "7".into(),
            username: "friend".into(),
        }
    }
    fn image() -> Value {
        json!({"media_type":1, "image_versions2":{"candidates":[
            {"url":"https://scontent.cdninstagram.com/small.jpg", "width":320},
            {"url":"https://scontent.cdninstagram.com/large.jpg", "width":1200}
        ]}})
    }
    fn post() -> Value {
        let mut post = image();
        post["code"] = json!("ABC");
        post["taken_at"] = json!(1000);
        post
    }

    #[test]
    fn keeps_photo_carousels_and_plain_text_caption() {
        let mut media = post();
        media["media_type"] = json!(8);
        media["carousel_media"] = json!([image(), image()]);
        media["caption"] = json!({"text":"<b>Привет</b>"});
        let photo = to_photo(&media, &author(), 900, 1100).unwrap();
        assert_eq!(photo.images.len(), 2);
        assert!(photo.images[0].ends_with("large.jpg"));
        assert!(photo.thumbnail.ends_with("small.jpg"));
        assert_eq!(photo.caption, "<b>Привет</b>");
    }

    #[test]
    fn caption_limit_respects_utf16_without_splitting_unicode() {
        let mut media = post();
        media["caption"] = json!({"text": "😀".repeat(2001)});
        let photo = to_photo(&media, &author(), 900, 1100).unwrap();
        assert_eq!(photo.caption.encode_utf16().count(), 4000);
        assert_eq!(photo.caption.chars().count(), 2000);
    }

    #[test]
    fn excludes_video_sponsored_old_and_untrusted_media() {
        let mut video = post();
        video["media_type"] = json!(2);
        let mut mixed = post();
        mixed["media_type"] = json!(8);
        mixed["carousel_media"] = json!([image(), {"media_type":2}]);
        let mut disguised = post();
        disguised["video_versions"] = json!([]);
        let mut sponsor = post();
        sponsor["is_paid_partnership"] = json!(true);
        let mut old = post();
        old["taken_at"] = json!(899);
        let mut hostile = post();
        hostile["image_versions2"]["candidates"][0]["url"] =
            json!("https://cdninstagram.com.evil.example/a.jpg");
        hostile["image_versions2"]["candidates"][1]["url"] = json!("http://cdninstagram.com/a.jpg");
        for media in [video, mixed, disguised, sponsor, old, hostile] {
            assert!(to_photo(&media, &author(), 900, 1100).is_none());
        }
    }

    #[test]
    fn profile_request_filters_video_and_returns_next_cursor() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("GET /api/v1/feed/user/7/?max_id=previous "));
            let mut video = post();
            video["media_type"] = json!(2);
            let body = json!({"status":"ok", "items":[post(), video], "more_available":true, "next_max_id":"next"}).to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let client = Client::with_origin(
            session(),
            Url::parse(&format!("http://{address}/")).unwrap(),
        )
        .unwrap();
        let page = client
            .profile_photos_page(&author(), Some("previous"), 900, 1100)
            .unwrap();
        assert_eq!(page.photos.len(), 1);
        assert_eq!(page.item_count, 2);
        assert_eq!(page.next_cursor.as_deref(), Some("next"));
        assert!(!page.all_older_than_cutoff);
        server.join().unwrap();
    }

    #[test]
    fn profile_pager_rejects_a_cursor_cycle() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for cursor in ["first", "second", "first"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                let body =
                    json!({"status":"ok", "items":[], "more_available":true, "next_max_id":cursor})
                        .to_string();
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
        let mut pager = client.profile_photos(author(), 900, 1100);
        assert!(pager.next_page().unwrap().is_some());
        assert!(pager.next_page().unwrap().is_some());
        assert_eq!(pager.next_page(), Err(Error::InvalidCursor));
        server.join().unwrap();
    }
}
