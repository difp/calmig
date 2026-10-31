//! Profile metadata from the same authenticated API used for calendar photos.
use crate::{Account, Client, Error};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileInfo {
    pub account: Account,
    pub name: String,
    pub bio: String,
}

fn string_id(value: &Value) -> Option<String> {
    let id = match value {
        Value::String(id) => id.clone(),
        Value::Number(id) => id.to_string(),
        _ => return None,
    };
    if id.is_empty() || id.len() > 30 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(id)
}
fn truncate(value: &str, max: usize) -> String {
    let mut units = 0;
    value
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= max
        })
        .collect()
}
impl Client {
    /// Confirm that the session belongs to the account returned by Instagram.
    pub fn current_account(&self) -> Result<Account, Error> {
        let body = self.get_json(&format!("api/v1/users/{}/info/", self.session.user_id), &[])?;
        if body
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|s| s != "ok")
        {
            return Err(Error::Remote);
        }
        let user = body.get("user").ok_or(Error::InvalidResponse)?;
        let id = ["pk_id", "id", "pk"]
            .into_iter()
            .find_map(|key| user.get(key).and_then(string_id))
            .ok_or(Error::InvalidResponse)?;
        let username = user
            .get("username")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidResponse)?;
        if id != self.session.user_id
            || username.is_empty()
            || username.len() > 30
            || username == "."
            || username == ".."
            || !username
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        {
            return Err(Error::InvalidResponse);
        }
        Ok(Account {
            id,
            username: username.to_owned(),
        })
    }

    pub fn profile_info(&self, username: &str) -> Result<ProfileInfo, Error> {
        if username.is_empty()
            || username.len() > 30
            || username == "."
            || username == ".."
            || !username
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        {
            return Err(Error::InvalidResponse);
        }
        let body = self.get_json(&format!("api/v1/users/{username}/usernameinfo/"), &[])?;
        if body
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|s| s != "ok")
        {
            return Err(Error::Remote);
        }
        let user = body.get("user").ok_or(Error::InvalidResponse)?;
        let result_name = user
            .get("username")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidResponse)?;
        if !result_name.eq_ignore_ascii_case(username) {
            return Err(Error::InvalidResponse);
        }
        let id = user
            .get("pk_id")
            .or_else(|| user.get("id"))
            .or_else(|| user.get("pk"))
            .and_then(string_id)
            .ok_or(Error::InvalidResponse)?;
        let name = truncate(
            user.get("full_name").and_then(Value::as_str).unwrap_or(""),
            150,
        );
        let bio = truncate(
            user.get("biography").and_then(Value::as_str).unwrap_or(""),
            2000,
        );
        Ok(ProfileInfo {
            account: Account {
                id,
                username: result_name.into(),
            },
            name,
            bio,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::session;
    use reqwest::Url;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    #[test]
    fn account_confirmation_rejects_wrong_id_and_invalid_username() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            for body in [
                r#"{"status":"ok","user":{"pk":123456,"username":"friend"}}"#,
                r#"{"status":"ok","user":{"pk":"654321","username":"friend"}}"#,
                r#"{"status":"ok","user":{"pk":"123456","username":"../bad"}}"#,
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut req = [0; 4096];
                let n = stream.read(&mut req).unwrap();
                assert!(String::from_utf8_lossy(&req[..n])
                    .starts_with("GET /api/v1/users/123456/info/"));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
        });
        let client =
            Client::with_origin(session(), Url::parse(&format!("http://{addr}/")).unwrap())
                .unwrap();
        assert_eq!(client.current_account().unwrap().username, "friend");
        assert_eq!(client.current_account(), Err(Error::InvalidResponse));
        assert_eq!(client.current_account(), Err(Error::InvalidResponse));
        worker.join().unwrap();
    }
    #[test]
    fn profile_lookup_keeps_large_id_and_rejects_wrong_user() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut req = [0; 4096];
                let n = stream.read(&mut req).unwrap();
                assert!(String::from_utf8_lossy(&req[..n])
                    .starts_with("GET /api/v1/users/friend/usernameinfo/"));
                let body = if index == 0 {
                    r#"{"status":"ok","user":{"pk":12345678901234567890,"username":"friend","full_name":"Имя","biography":"Текст"}}"#
                } else {
                    r#"{"status":"ok","user":{"pk":"7","username":"other"}}"#
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
        let client =
            Client::with_origin(session(), Url::parse(&format!("http://{addr}/")).unwrap())
                .unwrap();
        let profile = client.profile_info("friend").unwrap();
        assert_eq!(profile.account.id, "12345678901234567890");
        assert_eq!(profile.name, "Имя");
        assert_eq!(profile.bio, "Текст");
        assert_eq!(client.profile_info("friend"), Err(Error::InvalidResponse));
        worker.join().unwrap();
        assert_eq!(client.profile_info("../bad"), Err(Error::InvalidResponse));
    }
}
