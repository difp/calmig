//! Instagram Direct operations. Message bodies must never be logged.
use crate::{Client, Error};
use serde_json::{json, Value};
use std::sync::OnceLock;
use uuid::Uuid;

fn message_links(text: &str) -> Vec<String> {
    if !text.contains('.') && !text.contains("://") {
        return Vec::new();
    }
    static LINKS: OnceLock<regex::Regex> = OnceLock::new();
    let links = LINKS.get_or_init(|| {
        regex::RegexBuilder::new(r#"(?:[a-z]+://|www\.)[^\s<>"'()]+|[\p{L}\p{N}](?:[\p{L}\p{N}-]{0,62}\.)+[\p{L}]{2,63}(?:[/?#][^\s<>"'()]*)?"#)
            .case_insensitive(true)
            .size_limit(40_000_000)
            .build()
            .expect("valid link pattern")
    });
    links
        .find_iter(text)
        .filter_map(|matched| {
            let candidate = matched.as_str();
            let with_scheme = if candidate.split_once("://").is_some_and(|(scheme, _)| {
                !scheme.is_empty() && scheme.bytes().all(|b| b.is_ascii_alphabetic())
            }) {
                candidate.to_owned()
            } else {
                format!("https://{candidate}")
            };
            reqwest::Url::parse(&with_scheme)
                .ok()
                .filter(|url| url.host_str().is_some())
                .map(|_| candidate.to_owned())
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub unread: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxPage {
    pub threads: Vec<Thread>,
    pub cursor: Option<String>,
    pub seq_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub mine: bool,
    pub attachment: bool,
    pub text: String,
    pub timestamp_ms: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePage {
    pub messages: Vec<Message>,
    pub cursor: Option<String>,
}

fn id(value: &Value) -> Option<String> {
    let value = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };
    if value.is_empty() || value.len() > 128 || !value.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(value)
}
fn cursor(value: Option<&str>) -> Result<Option<&str>, Error> {
    if value.is_some_and(|s| s.is_empty() || s.len() > 2048 || s.chars().any(char::is_control)) {
        return Err(Error::InvalidCursor);
    }
    Ok(value)
}
fn next_cursor(object: &Value) -> Result<Option<String>, Error> {
    if !object
        .get("has_older")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let value = object
        .get("oldest_cursor")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidCursor)?;
    cursor(Some(value))?;
    Ok(Some(value.to_owned()))
}
fn check_status(value: &Value) -> Result<(), Error> {
    if value
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|s| s != "ok")
    {
        return Err(Error::Remote);
    }
    Ok(())
}

impl Client {
    pub fn inbox_page(
        &self,
        cursor_value: Option<&str>,
        seq_id: Option<&str>,
    ) -> Result<InboxPage, Error> {
        let cursor_value = cursor(cursor_value)?;
        let seq_id = cursor(seq_id)?;
        let mut query = vec![
            ("visual_message_return_type", "unseen"),
            ("thread_message_limit", "10"),
            ("persistentBadging", "true"),
            ("limit", "20"),
        ];
        if let Some(value) = cursor_value {
            query.push(("cursor", value));
            query.push(("direction", "older"));
        }
        if let Some(value) = seq_id {
            query.push(("seq_id", value));
        }
        let result = self.get_json("api/v1/direct_v2/inbox/", &query)?;
        check_status(&result)?;
        let inbox = result.get("inbox").ok_or(Error::InvalidResponse)?;
        let raw_threads = inbox
            .get("threads")
            .and_then(Value::as_array)
            .ok_or(Error::InvalidResponse)?;
        let mut threads = Vec::with_capacity(raw_threads.len());
        for raw in raw_threads {
            let thread_id = raw
                .get("thread_id")
                .and_then(id)
                .ok_or(Error::InvalidResponse)?;
            let title = raw
                .get("thread_title")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    let users = raw.get("users").and_then(Value::as_array);
                    let names: Vec<&str> = users
                        .into_iter()
                        .flatten()
                        .filter(|user| {
                            user.get("pk").and_then(id).as_deref() != Some(&self.session.user_id)
                        })
                        .filter_map(|user| {
                            user.get("username")
                                .and_then(Value::as_str)
                                .filter(|x| !x.is_empty())
                                .or_else(|| {
                                    user.get("full_name")
                                        .and_then(Value::as_str)
                                        .filter(|x| !x.is_empty())
                                })
                        })
                        .collect();
                    if names.is_empty() {
                        "You".into()
                    } else {
                        names.join(", ")
                    }
                });
            if title.len() > 4096 {
                return Err(Error::InvalidResponse);
            }
            threads.push(Thread {
                id: thread_id,
                title,
                unread: raw.get("read_state").and_then(Value::as_u64) == Some(1),
            });
        }
        let seq_id = result
            .get("seq_id")
            .filter(|value| !value.is_null())
            .and_then(id);
        Ok(InboxPage {
            threads,
            cursor: next_cursor(inbox)?,
            seq_id,
        })
    }

    pub fn messages_page(
        &self,
        thread_id: &str,
        cursor_value: Option<&str>,
    ) -> Result<MessagePage, Error> {
        if id(&json!(thread_id)).is_none() {
            return Err(Error::InvalidResponse);
        }
        let mut query = vec![
            ("visual_message_return_type", "unseen"),
            ("direction", "older"),
            ("limit", "10"),
        ];
        if let Some(value) = cursor(cursor_value)? {
            query.push(("cursor", value));
        }
        let result = self.get_json(&format!("api/v1/direct_v2/threads/{thread_id}/"), &query)?;
        check_status(&result)?;
        let thread = result.get("thread").ok_or(Error::InvalidResponse)?;
        let items = thread
            .get("items")
            .and_then(Value::as_array)
            .ok_or(Error::InvalidResponse)?;
        let mut messages = Vec::new();
        for item in items {
            let Some(item_id) = item
                .get("item_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() <= 256)
            else {
                continue;
            };
            let Some(user_id) = item.get("user_id").and_then(id) else {
                continue;
            };
            let Some(timestamp_ms) = item
                .get("timestamp")
                .and_then(id)
                .and_then(|s| s.parse::<i64>().ok())
                .map(|n| n / 1000)
            else {
                continue;
            };
            let kind = item.get("item_type").and_then(Value::as_str).unwrap_or("");
            let (text, attachment) = match kind {
                "text" => (
                    item.get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    false,
                ),
                "link" => (
                    item.get("link")
                        .and_then(|v| v.get("text"))
                        .and_then(Value::as_str)
                        .or_else(|| item.get("text").and_then(Value::as_str))
                        .unwrap_or("[Sent a link]")
                        .to_owned(),
                    false,
                ),
                "like" => ("[Sent a ❤️]".into(), false),
                "reel_share" | "raven_media" | "clip" => (
                    "[Instagram CLI successfully blocked a brainrot]".into(),
                    false,
                ),
                "action_log" if item.get("hide_in_thread").and_then(Value::as_i64) == Some(0) => (
                    item.get("action_log")
                        .and_then(|v| v.get("description"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    false,
                ),
                "action_log" => continue,
                "media" | "media_share" => ("Вложение".into(), true),
                _ => (format!("[Unsupported Type: {kind}]"), false),
            };
            if text.len() > 64_000 {
                return Err(Error::InvalidResponse);
            }
            messages.push(Message {
                id: item_id.into(),
                mine: user_id == self.session.user_id,
                attachment,
                text,
                timestamp_ms,
            });
        }
        messages.reverse();
        Ok(MessagePage {
            messages,
            cursor: next_cursor(thread)?,
        })
    }

    /// Send text once; caller must not retry on an uncertain outcome.
    pub fn send_message(&self, thread_id: &str, text: &str) -> Result<(), Error> {
        self.send_direct(thread_id, text, None)
    }

    /// Choose the Direct endpoint based on the message, without retrying writes.
    pub fn send_automatic_message(&self, thread_id: &str, text: &str) -> Result<(), Error> {
        if text.encode_utf16().count() > 1000 {
            return Err(Error::InvalidResponse);
        }
        let links = message_links(text);
        if links.is_empty() {
            self.send_message(thread_id, text)
        } else {
            self.send_link(thread_id, text, &links)
        }
    }

    /// Preserve Instagram's link-share format without retrying uncertain writes.
    pub fn send_link(&self, thread_id: &str, text: &str, links: &[String]) -> Result<(), Error> {
        if links.is_empty()
            || links.len() > 100
            || links.iter().any(|link| {
                link.is_empty() || link.len() > 4000 || link.chars().any(char::is_control)
            })
        {
            return Err(Error::InvalidResponse);
        }
        self.send_direct(thread_id, text, Some(links))
    }

    fn send_direct(
        &self,
        thread_id: &str,
        text: &str,
        links: Option<&[String]>,
    ) -> Result<(), Error> {
        if id(&json!(thread_id)).is_none()
            || text.trim().is_empty()
            || text != text.trim()
            || text.encode_utf16().count() > 1000
        {
            return Err(Error::InvalidResponse);
        }
        let token = Uuid::new_v4().to_string();
        let mut form = vec![
            ("action", "send_item".to_owned()),
            ("thread_ids", json!([thread_id]).to_string()),
            ("client_context", token.clone()),
            ("mutation_token", token),
            ("_csrftoken", self.csrf()?),
            ("device_id", self.session.device_id.clone()),
            ("_uuid", self.session.uuid.clone()),
        ];
        let item = if let Some(links) = links {
            form.push(("link_text", text.to_owned()));
            form.push(("link_urls", json!(links).to_string()));
            "link"
        } else {
            form.push(("text", text.to_owned()));
            "text"
        };
        let result = self
            .post_form(
                &format!("api/v1/direct_v2/threads/broadcast/{item}/"),
                &form,
            )
            .map_err(|_| Error::Unconfirmed)?;
        if result.get("status").and_then(Value::as_str) != Some("ok") {
            return Err(Error::Unconfirmed);
        }
        let confirmed = result
            .get("payload")
            .and_then(|v| v.get("item_id"))
            .or_else(|| {
                result
                    .get("message_metadata")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
                    .and_then(|v| v.get("item_id"))
            })
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
        if !confirmed {
            return Err(Error::Unconfirmed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn detects_links_in_multilingual_messages_without_changing_text() {
        assert_eq!(
            message_links("Смотри https://example.com/a?x=1 и www.example.org/путь"),
            vec!["https://example.com/a?x=1", "www.example.org/путь"]
        );
        assert_eq!(
            message_links("дом.рф и example.com"),
            vec!["дом.рф", "example.com"]
        );
        assert!(message_links("Обычный текст.").is_empty());
        assert!(message_links("https://[invalid").is_empty());
    }

    #[test]
    fn inbox_preserves_large_identifiers_and_pagination() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("GET /api/v1/direct_v2/inbox/?"));
            assert!(request.contains("cursor=older"));
            respond(
                &mut stream,
                "200 OK",
                r#"{"status":"ok","seq_id":12345678901234567890,"inbox":{"has_older":true,"oldest_cursor":"next","threads":[{"thread_id":"340282366920938463463374607431768211","thread_title":"","users":[{"pk":123456,"username":"me"},{"pk":12345678901234567890,"username":"friend"}],"read_state":1}]}}"#,
            );
        });
        let page = fake_client(address)
            .inbox_page(Some("older"), None)
            .unwrap();
        assert_eq!(page.threads[0].title, "friend");
        assert!(page.threads[0].unread);
        assert_eq!(page.seq_id.as_deref(), Some("12345678901234567890"));
        assert_eq!(page.cursor.as_deref(), Some("next"));
        worker.join().unwrap();
    }
    #[test]
    fn messages_are_chronological_and_attachments_do_not_expose_video() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("GET /api/v1/direct_v2/threads/777/?"));
            respond(
                &mut stream,
                "200 OK",
                r#"{"status":"ok","thread":{"has_older":true,"oldest_cursor":"next","items":[{"item_id":"2","user_id":123456,"timestamp":"1760000001000000","item_type":"text","text":"привет"},{"item_id":"1","user_id":789,"timestamp":"1760000000000000","item_type":"media","media":{"video_versions":[{"url":"https://example.invalid/private"}]}}]}}"#,
            );
        });
        let page = fake_client(address).messages_page("777", None).unwrap();
        assert_eq!(page.messages.len(), 2);
        assert_eq!(page.messages[0].text, "Вложение");
        assert!(page.messages[0].attachment);
        assert!(page.messages[1].mine);
        assert_eq!(page.messages[1].text, "привет");
        assert_eq!(page.cursor.as_deref(), Some("next"));
        worker.join().unwrap();
    }
    #[test]
    fn sends_text_once_with_same_idempotence_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("POST /api/v1/direct_v2/threads/broadcast/text/"));
            let body = request.split_once("\r\n\r\n").unwrap().1;
            let url = Url::parse(&format!("http://localhost/?{body}")).unwrap();
            let values: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(values["thread_ids"], "[\"777\"]");
            assert_eq!(values["text"], "тест 😀");
            assert_eq!(values["client_context"], values["mutation_token"]);
            assert!(Uuid::parse_str(&values["client_context"]).is_ok());
            respond(
                &mut stream,
                "200 OK",
                r#"{"status":"ok","payload":{"item_id":"42"}}"#,
            );
        });
        fake_client(address)
            .send_automatic_message("777", "тест 😀")
            .unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn failed_send_is_unconfirmed_and_not_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            respond(&mut stream, "500 Error", "{}");
            listener.set_nonblocking(true).unwrap();
            assert!(listener.accept().is_err());
        });
        assert_eq!(
            fake_client(address).send_message("777", "hello"),
            Err(Error::Unconfirmed)
        );
        worker.join().unwrap();
    }

    #[test]
    fn sends_links_in_instagrams_link_share_format() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("POST /api/v1/direct_v2/threads/broadcast/link/"));
            let body = request.split_once("\r\n\r\n").unwrap().1;
            let url = Url::parse(&format!("http://localhost/?{body}")).unwrap();
            let values: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(values["link_text"], "See https://example.com");
            assert_eq!(values["link_urls"], "[\"https://example.com\"]");
            respond(
                &mut stream,
                "200 OK",
                r#"{"status":"ok","payload":{"item_id":"42"}}"#,
            );
        });
        fake_client(address)
            .send_automatic_message("777", "See https://example.com")
            .unwrap();
        worker.join().unwrap();
    }
}
