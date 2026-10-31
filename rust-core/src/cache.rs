//! Account-scoped calendar cache, compatible with the original adapter.
use crate::media::trusted_image;
use chrono::DateTime;
use reqwest::Url;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

const MAX_BYTES: usize = 2_000_000;
const MAX_POSTS: usize = 250;

fn filename(directory: &Path, username: &str) -> io::Result<PathBuf> {
    if username.is_empty()
        || username.len() > 30
        || !username
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        || username == "."
        || username == ".."
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid account",
        ));
    }
    let digest = Sha256::digest(username.to_ascii_lowercase().as_bytes());
    Ok(directory.join(format!("tikho-{digest:x}.json")))
}

fn valid_post(post: &Value, cutoff_ms: i64, now_ms: i64) -> bool {
    let Some(obj) = post.as_object() else {
        return false;
    };
    let Some(id) = obj.get("id").and_then(Value::as_str) else {
        return false;
    };
    let Some(username) = obj.get("username").and_then(Value::as_str) else {
        return false;
    };
    if id.encode_utf16().count() >= 100
        || username.is_empty()
        || username.len() > 30
        || !username
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
    {
        return false;
    }
    if obj
        .get("caption")
        .is_some_and(|c| c.as_str().is_none_or(|s| s.encode_utf16().count() > 4000))
    {
        return false;
    }
    let Some(time) = obj
        .get("publishedAt")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
    else {
        return false;
    };
    if time < cutoff_ms || time > now_ms {
        return false;
    }
    let Some(url) = obj.get("url").and_then(Value::as_str) else {
        return false;
    };
    let Some(code) = url
        .strip_prefix("https://www.instagram.com/p/")
        .and_then(|s| s.strip_suffix('/'))
    else {
        return false;
    };
    if code.is_empty()
        || !code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return false;
    }
    let Some(thumbnail) = obj.get("thumbnail").and_then(Value::as_str) else {
        return false;
    };
    let Some(images) = obj.get("images").and_then(Value::as_array) else {
        return false;
    };
    trusted_image(thumbnail)
        && !images.is_empty()
        && images.len() <= 20
        && images
            .iter()
            .all(|image| image.as_str().is_some_and(trusted_image))
        && Url::parse(url).is_ok()
}

/// Load only bounded and validated metadata. Invalid or missing files act as an empty cache.
pub fn load_calendar_cache(
    directory: &Path,
    username: &str,
    cutoff_ms: i64,
    now_ms: i64,
) -> io::Result<Vec<Value>> {
    let file = filename(directory, username)?;
    let Ok(file) = fs::File::open(file) else {
        return Ok(Vec::new());
    };
    let mut data = Vec::new();
    file.take((MAX_BYTES + 1) as u64).read_to_end(&mut data)?;
    if data.len() > MAX_BYTES {
        return Ok(Vec::new());
    }
    let Ok(Value::Array(posts)) = serde_json::from_slice::<Value>(&data) else {
        return Ok(Vec::new());
    };
    Ok(posts
        .into_iter()
        .filter(|p| valid_post(p, cutoff_ms, now_ms))
        .take(MAX_POSTS)
        .collect())
}

/// Atomically replace the cache using an owner-only temporary file in the same directory.
pub fn save_calendar_cache(
    directory: &Path,
    username: &str,
    posts: &[Value],
    cutoff_ms: i64,
    now_ms: i64,
) -> io::Result<()> {
    let destination = filename(directory, username)?;
    let mut safe = Vec::new();
    let mut size = 2usize;
    for post in posts
        .iter()
        .filter(|p| valid_post(p, cutoff_ms, now_ms))
        .take(MAX_POSTS)
    {
        let encoded = serde_json::to_vec(post)?;
        let extra = encoded.len() + usize::from(!safe.is_empty());
        if size + extra <= MAX_BYTES {
            size += extra;
            safe.push(encoded);
        }
    }
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory)?;
    let temporary = directory.join(format!(".tikho-{}.tmp", Uuid::new_v4().simple()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(b"[")?;
        for (index, post) in safe.iter().enumerate() {
            if index > 0 {
                file.write_all(b",")?;
            }
            file.write_all(post)?;
        }
        file.write_all(b"]")?;
        drop(file);
        fs::rename(&temporary, &destination)
    })();
    let _ = fs::remove_file(temporary);
    result
}

pub fn clear_calendar_cache(directory: &Path, username: &str) -> io::Result<()> {
    match fs::remove_file(filename(directory, username)?) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn post() -> Value {
        json!({"id":"ABC", "username":"friend", "url":"https://www.instagram.com/p/ABC/", "images":["https://scontent.cdninstagram.com/a.jpg"], "thumbnail":"https://scontent.cdninstagram.com/t.jpg", "publishedAt":"2026-10-30T12:00:00.000Z", "caption":"Привет"})
    }

    #[test]
    fn compatible_account_scoped_cache_filters_and_removes() {
        let dir = std::env::temp_dir().join(format!("calmig-{}", Uuid::new_v4()));
        let now = DateTime::parse_from_rfc3339("2026-10-31T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        let mut old = post();
        old["publishedAt"] = json!("2026-10-01T00:00:00Z");
        let mut unsafe_post = post();
        unsafe_post["thumbnail"] = json!("https://cdninstagram.com.evil.example/a");
        save_calendar_cache(
            &dir,
            "First",
            &[post(), old, unsafe_post],
            now - 7 * 86_400_000,
            now,
        )
        .unwrap();
        assert_eq!(
            load_calendar_cache(&dir, "first", now - 7 * 86_400_000, now).unwrap(),
            vec![post()]
        );
        assert!(
            load_calendar_cache(&dir, "second", now - 7 * 86_400_000, now)
                .unwrap()
                .is_empty()
        );
        let file = filename(&dir, "first").unwrap();
        assert_eq!(
            file.file_name().unwrap(),
            "tikho-a7937b64b8caa58f03721bb6bacf5c78cb235febe0e70b1b84cd99541461a08e.json"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(file).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        clear_calendar_cache(&dir, "first").unwrap();
        assert!(load_calendar_cache(&dir, "first", 0, now)
            .unwrap()
            .is_empty());
        assert!(clear_calendar_cache(&dir, "../other").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn oversized_and_malformed_cache_are_ignored() {
        let dir = std::env::temp_dir().join(format!("calmig-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let file = filename(&dir, "first").unwrap();
        fs::write(&file, vec![b' '; MAX_BYTES + 1]).unwrap();
        assert!(load_calendar_cache(&dir, "first", 0, i64::MAX)
            .unwrap()
            .is_empty());
        fs::write(&file, b"not json").unwrap();
        assert!(load_calendar_cache(&dir, "first", 0, i64::MAX)
            .unwrap()
            .is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
