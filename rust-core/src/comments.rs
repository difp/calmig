//! Photo captions and comments. Comment text is never logged.
use crate::{Client, Error};
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub username: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentPage {
    pub caption: String,
    pub comments: Vec<Comment>,
    pub next_max: Option<String>,
    pub next_min: Option<String>,
}

/// Decode an Instagram shortcode into a decimal ID without narrowing large values.
pub fn media_id_from_code(code: &str) -> Result<String, Error> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    if code.is_empty() || code.len() > 30 {
        return Err(Error::InvalidResponse);
    }
    let mut digits = vec![0u8];
    for byte in code.bytes() {
        let mut carry = ALPHABET
            .iter()
            .position(|item| *item == byte)
            .ok_or(Error::InvalidResponse)? as u32;
        for digit in &mut digits {
            let value = u32::from(*digit) * 64 + carry;
            *digit = (value % 10) as u8;
            carry = value / 10;
        }
        while carry > 0 {
            digits.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    Ok(digits
        .iter()
        .rev()
        .map(|digit| char::from(b'0' + digit))
        .collect())
}

fn valid_media_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit())
}
fn valid_cursor(value: &str) -> bool {
    !value.is_empty() && value.len() <= 2048 && !value.chars().any(char::is_control)
}
fn cursor(value: Option<&Value>) -> Result<Option<String>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.is_empty() => Ok(None),
        Some(Value::String(value)) if valid_cursor(value) => Ok(Some(value.clone())),
        _ => Err(Error::InvalidCursor),
    }
}
fn truncate_text(value: &str, max: usize) -> String {
    let mut count = 0;
    value
        .chars()
        .take_while(|c| {
            count += c.len_utf16();
            count <= max
        })
        .collect()
}
fn add_comment(item: &Value, comments: &mut Vec<Comment>) {
    let Some(username) = item
        .get("user")
        .and_then(|x| x.get("username"))
        .and_then(Value::as_str)
    else {
        return;
    };
    let Some(text) = item.get("text").and_then(Value::as_str) else {
        return;
    };
    if username.is_empty()
        || username.len() > 30
        || !username
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
    {
        return;
    }
    comments.push(Comment {
        username: username.into(),
        text: truncate_text(text, 4000),
    });
}
fn hmac_hex(key: &[u8], data: &[u8]) -> Result<String, Error> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| Error::Unconfirmed)?;
    mac.update(data);
    Ok(mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

impl Client {
    pub fn comments_page(
        &self,
        media_id: &str,
        max_id: Option<&str>,
        min_id: Option<&str>,
    ) -> Result<CommentPage, Error> {
        if !valid_media_id(media_id) {
            return Err(Error::InvalidResponse);
        }
        if [max_id, min_id]
            .into_iter()
            .flatten()
            .any(|value| !valid_cursor(value))
        {
            return Err(Error::InvalidCursor);
        }
        let mut query = vec![("can_support_threading", "true")];
        if let Some(value) = max_id {
            query.push(("max_id", value));
        }
        if let Some(value) = min_id {
            query.push(("min_id", value));
        }
        let response = self.get_json(&format!("api/v1/media/{media_id}/comments/"), &query)?;
        if response
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|s| s != "ok")
        {
            return Err(Error::Remote);
        }
        let caption = response
            .get("caption")
            .and_then(|x| x.get("text"))
            .and_then(Value::as_str)
            .map(|s| truncate_text(s, 4000))
            .unwrap_or_default();
        let mut comments = Vec::new();
        if let Some(items) = response.get("comments").and_then(Value::as_array) {
            for item in items {
                add_comment(item, &mut comments);
                if let Some(replies) = item.get("preview_child_comments").and_then(Value::as_array)
                {
                    for reply in replies {
                        add_comment(reply, &mut comments);
                    }
                }
            }
        } else if response.get("comments").is_some_and(|x| !x.is_null()) {
            return Err(Error::InvalidResponse);
        }
        Ok(CommentPage {
            caption,
            comments,
            next_max: cursor(response.get("next_max_id"))?,
            next_min: cursor(response.get("next_min_id"))?,
        })
    }

    /// Send once; any network or parsing failure is unconfirmed and must not be retried automatically.
    pub fn send_comment(&self, media_id: &str, text: &str) -> Result<(), Error> {
        if !valid_media_id(media_id)
            || text.trim().is_empty()
            || text != text.trim()
            || text.encode_utf16().count() > 1800
        {
            return Err(Error::InvalidResponse);
        }
        let size = text.encode_utf16().count();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Unconfirmed)?
            .as_millis();
        let crumb = format!("{size} {} {} {timestamp}", 19_000 + size, size.div_ceil(2));
        let signature = hmac_hex(b"iN4$aGr0m", crumb.as_bytes())?;
        let breadcrumb = format!(
            "{}\n{}\n",
            STANDARD.encode(signature),
            STANDARD.encode(crumb)
        );
        let payload = json!({
            "user_breadcrumb":breadcrumb, "idempotence_token":Uuid::new_v4().to_string(),
            "_csrftoken":self.csrf()?, "radio_type":"wifi-none", "_uid":self.session.user_id,
            "device_id":self.session.device_id, "_uuid":self.session.uuid,
            "comment_text":text, "containermodule":"self_comments_v2"
        })
        .to_string();
        let signature = hmac_hex(
            b"9193488027538fd3450b83b7d05286d4ca9599a0f7eeed90d8c85925698a05dc",
            payload.as_bytes(),
        )?;
        let response = self
            .post_form(
                &format!("api/v1/media/{media_id}/comment/"),
                &[
                    ("ig_sig_key_version", "4".into()),
                    ("signed_body", format!("{signature}.{payload}")),
                ],
            )
            .map_err(|_| Error::Unconfirmed)?;
        if response.get("status").and_then(Value::as_str) != Some("ok")
            || !response
                .get("comment")
                .and_then(|v| v.get("pk"))
                .is_some_and(|v| match v {
                    Value::String(value) => !value.is_empty(),
                    Value::Number(value) => {
                        let value = value.to_string();
                        value.bytes().all(|byte| byte.is_ascii_digit())
                            && value.bytes().any(|byte| byte != b'0')
                    }
                    _ => false,
                })
        {
            return Err(Error::Unconfirmed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_shortcode_without_precision_loss() {
        assert_eq!(media_id_from_code("BA").unwrap(), "64");
        assert_eq!(
            media_id_from_code("__________").unwrap(),
            "1152921504606846975"
        );
        assert_eq!(
            media_id_from_code(&"_".repeat(30)).unwrap(),
            "1532495540865888858358347027150309183618739122183602175"
        );
        assert!(media_id_from_code("../bad").is_err());
        assert!(media_id_from_code(&"_".repeat(31)).is_err());
    }
    use crate::session::tests::session;
    use reqwest::Url;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

    fn read_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|s| s.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        String::from_utf8(bytes).unwrap()
    }
    fn respond(stream: &mut TcpStream, status: &str, body: &str) {
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    }
    fn fake_client(address: std::net::SocketAddr) -> Client {
        Client::with_origin(
            session(),
            Url::parse(&format!("http://{address}/")).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn reads_caption_comments_replies_and_both_cursors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("GET /api/v1/media/123/comments/?"));
            assert!(request.contains("max_id=old") && request.contains("min_id=new"));
            respond(
                &mut stream,
                "200 OK",
                r#"{"status":"ok","caption":{"text":"описание"},"comments":[{"text":"hello","user":{"username":"author"},"preview_child_comments":[{"text":"reply","user":{"username":"other"}}]}],"next_max_id":"older","next_min_id":"newer"}"#,
            );
        });
        let page = fake_client(addr)
            .comments_page("123", Some("old"), Some("new"))
            .unwrap();
        assert_eq!(page.caption, "описание");
        assert_eq!(page.comments.len(), 2);
        assert_eq!(page.comments[1].text, "reply");
        assert_eq!(page.next_max.as_deref(), Some("older"));
        assert_eq!(page.next_min.as_deref(), Some("newer"));
        worker.join().unwrap();
    }

    #[test]
    fn posts_signed_comment_once_and_requires_confirmation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("POST /api/v1/media/123/comment/"));
            let body = request.split_once("\r\n\r\n").unwrap().1;
            let url = Url::parse(&format!("http://localhost/?{body}")).unwrap();
            let values: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(
                values.get("ig_sig_key_version").map(String::as_str),
                Some("4")
            );
            let (signature, payload) = values["signed_body"].split_once('.').unwrap();
            assert_eq!(
                signature,
                hmac_hex(
                    b"9193488027538fd3450b83b7d05286d4ca9599a0f7eeed90d8c85925698a05dc",
                    payload.as_bytes()
                )
                .unwrap()
            );
            let json: Value = serde_json::from_str(payload).unwrap();
            assert_eq!(json["comment_text"], "тест 😀");
            assert_eq!(json["_uid"], "123456");
            assert!(Uuid::parse_str(json["idempotence_token"].as_str().unwrap()).is_ok());
            respond(
                &mut stream,
                "200 OK",
                r#"{"status":"ok","comment":{"pk":"123456"}}"#,
            );
        });
        fake_client(addr).send_comment("123", "тест 😀").unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn failed_post_is_unconfirmed_and_not_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            respond(&mut stream, "500 Server Error", "{}");
            listener.set_nonblocking(true).unwrap();
            assert!(listener.accept().is_err());
        });
        assert_eq!(
            fake_client(addr).send_comment("123", "hello"),
            Err(Error::Unconfirmed)
        );
        worker.join().unwrap();
    }
}
