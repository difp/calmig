//! Experimental, offline parser for a Following response. This is not a network source.
use crate::{to_photo, Account, Error, PhotoPost};
use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub struct FollowingFeedPage {
    pub photos: Vec<PhotoPost>,
    pub next_cursor: Option<String>,
    /// Some potentially eligible posts could not be verified or parsed.
    pub incomplete: bool,
}

/// Parse only a response known by the caller to belong to Following mode.
/// A verified following relationship is required for each author; unverified authors
/// are omitted and mark the page incomplete. The caller must still verify the request
/// and cannot assume that Instagram returned every post.
pub fn parse_following_feed_page(
    response: &Value,
    cutoff: i64,
    now: i64,
    cursor: Option<&str>,
) -> Result<FollowingFeedPage, Error> {
    if cutoff > now
        || response
            .get("errors")
            .and_then(Value::as_array)
            .is_some_and(|v| !v.is_empty())
    {
        return Err(Error::InvalidResponse);
    }
    let connection = response
        .pointer("/data/xdt_api__v1__feed__timeline__connection")
        .ok_or(Error::InvalidResponse)?;
    let edges = connection
        .get("edges")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidResponse)?;
    let page = connection.get("page_info").ok_or(Error::InvalidResponse)?;
    let more = page
        .get("has_next_page")
        .and_then(Value::as_bool)
        .ok_or(Error::InvalidResponse)?;
    let next_cursor = if more {
        let next = page
            .get("end_cursor")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidCursor)?;
        if next.is_empty()
            || next.len() > 2048
            || next.chars().any(char::is_control)
            || Some(next) == cursor
        {
            return Err(Error::InvalidCursor);
        }
        Some(next.to_owned())
    } else {
        None
    };
    let mut photos = Vec::new();
    let mut incomplete = false;
    for edge in edges {
        let Some(node) = edge.get("node").and_then(Value::as_object) else {
            incomplete = true;
            continue;
        };
        let others = [
            "ad",
            "explore_story",
            "end_of_feed_demarcator",
            "stories_netego",
            "suggested_users",
            "bloks_netego",
            "abra_icebreakers_in_feed_unit",
            "ad4ad_in_webfeed",
        ];
        let Some(media) = node.get("media").filter(|v| !v.is_null()) else {
            if !others
                .iter()
                .any(|key| node.get(*key).is_some_and(|v| !v.is_null()))
            {
                incomplete = true;
            }
            continue;
        };
        if others
            .iter()
            .any(|key| node.get(*key).is_some_and(|v| !v.is_null()))
        {
            incomplete = true;
            continue;
        }
        // A new populated union member could represent an ad or recommendation.
        if node
            .iter()
            .any(|(key, value)| key != "media" && key != "__typename" && !value.is_null())
        {
            incomplete = true;
            continue;
        }
        if [
            "is_sponsored",
            "is_recommended",
            "is_paid_partnership",
            "has_audio",
        ]
        .iter()
        .any(|key| media.get(*key).and_then(Value::as_bool) == Some(true))
            || media
                .get("product_type")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "clips" | "igtv"))
            || media
                .get("music_metadata")
                .is_some_and(|value| !value.is_null())
        {
            continue;
        }
        let Some(user) = media.get("user") else {
            incomplete = true;
            continue;
        };
        match user
            .pointer("/friendship_status/following")
            .and_then(Value::as_bool)
        {
            Some(true) => (),
            Some(false) => continue,
            None => {
                incomplete = true;
                continue;
            }
        }
        let Some(username) = user.get("username").and_then(Value::as_str).filter(|s| {
            !s.is_empty()
                && s.len() <= 30
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        }) else {
            incomplete = true;
            continue;
        };
        let id = user.get("pk").or_else(|| user.get("id")).and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        });
        let Some(id) =
            id.filter(|s| !s.is_empty() && s.len() <= 30 && s.bytes().all(|b| b.is_ascii_digit()))
        else {
            incomplete = true;
            continue;
        };
        let Some(kind) = media.get("media_type").and_then(Value::as_u64) else {
            incomplete = true;
            continue;
        };
        let Some(timestamp) = media.get("taken_at").and_then(Value::as_i64) else {
            incomplete = true;
            continue;
        };
        if !(cutoff..=now).contains(&timestamp) || !matches!(kind, 1 | 8) {
            continue;
        }
        if kind == 8
            && media
                .get("carousel_media")
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item.get("media_type").and_then(Value::as_u64) == Some(2)
                            || item.get("video_versions").is_some_and(|v| !v.is_null())
                            || item.get("has_audio").and_then(Value::as_bool) == Some(true)
                    })
                })
        {
            continue;
        }
        match to_photo(
            media,
            &Account {
                id,
                username: username.to_owned(),
            },
            cutoff,
            now,
        ) {
            Some(photo) => photos.push(photo),
            None => incomplete = true,
        }
    }
    photos.sort_by(|a, b| {
        b.published_at
            .cmp(&a.published_at)
            .then_with(|| a.code.cmp(&b.code))
    });
    Ok(FollowingFeedPage {
        photos,
        next_cursor,
        incomplete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn photo(code: &str, time: i64) -> Value {
        json!({"code":code,"taken_at":time,"media_type":1,"user":{"pk":"7","username":"friend","friendship_status":{"following":true}},"image_versions2":{"candidates":[{"url":"https://scontent.cdninstagram.com/photo.jpg","width":320}]}})
    }
    fn response(edges: Vec<Value>, more: bool, cursor: Option<&str>) -> Value {
        json!({"data":{"xdt_api__v1__feed__timeline__connection":{"edges":edges,"page_info":{"has_next_page":more,"end_cursor":cursor}}}})
    }
    fn edge(media: Value) -> Value {
        json!({"node":{"media":media}})
    }
    #[test]
    fn filters_union_video_music_and_unverified_authors_and_keeps_cursor() {
        let mut video = photo("video", 105);
        video["media_type"] = json!(2);
        let mut unverified = photo("unknown", 106);
        unverified["user"]["friendship_status"] = Value::Null;
        let mut music = photo("music", 107);
        music["has_audio"] = json!(true);
        let mut mixed = photo("mixed", 108);
        mixed["media_type"] = json!(8);
        mixed["carousel_media"] = json!([{"media_type":1},{"media_type":2}]);
        let page = response(
            vec![
                edge(photo("older", 101)),
                json!({"node":{"ad":{"id":"synthetic"}}}),
                edge(video),
                edge(unverified),
                edge(music),
                edge(mixed),
                edge(photo("newer", 104)),
            ],
            true,
            Some("next"),
        );
        let parsed = parse_following_feed_page(&page, 100, 110, None).unwrap();
        assert_eq!(
            parsed
                .photos
                .iter()
                .map(|p| p.code.as_str())
                .collect::<Vec<_>>(),
            vec!["newer", "older"]
        );
        assert_eq!(parsed.next_cursor.as_deref(), Some("next"));
        assert!(parsed.incomplete);
    }
    #[test]
    fn rejects_errors_and_broken_or_repeated_cursor() {
        assert_eq!(
            parse_following_feed_page(&json!({"errors":[{}]}), 0, 10, None),
            Err(Error::InvalidResponse)
        );
        assert_eq!(
            parse_following_feed_page(&response(vec![], true, None), 0, 10, None),
            Err(Error::InvalidCursor)
        );
        assert_eq!(
            parse_following_feed_page(&response(vec![], true, Some("same")), 0, 10, Some("same")),
            Err(Error::InvalidCursor)
        );
    }
    #[test]
    fn unknown_union_and_ambiguous_media_are_incomplete() {
        let page = response(
            vec![
                json!({"node":{"new_feed_type":{}}}),
                json!({"node":{"media":photo("ambiguous",5),"ad":{}}}),
                json!({"node":{"media":photo("unknown",5),"new_feed_type":{}}}),
            ],
            false,
            None,
        );
        let parsed = parse_following_feed_page(&page, 0, 10, None).unwrap();
        assert!(parsed.photos.is_empty());
        assert!(parsed.incomplete);
    }
}
