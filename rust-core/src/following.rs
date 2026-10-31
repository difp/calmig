use crate::Session;
use reqwest::{blocking::Client as HttpClient, redirect::Policy, Url};
use serde::Deserialize;
use std::{collections::HashSet, io::Read, sync::Mutex, time::Duration};
use thiserror::Error;

const ORIGIN: &str = "https://i.instagram.com/";
const MAX_RESPONSE: u64 = 4_000_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("Instagram request failed")]
    Network,
    #[error("Instagram session is unavailable or requires verification")]
    Authentication,
    #[error("Instagram has limited requests")]
    RateLimited,
    #[error("Instagram requested feedback for this request")]
    FeedbackRequired,
    #[error("Instagram returned an error")]
    Remote,
    #[error("Invalid Instagram response")]
    InvalidResponse,
    #[error("Invalid or repeated pagination cursor")]
    InvalidCursor,
    #[error("The comment publication was not confirmed")]
    Unconfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowingPage {
    pub accounts: Vec<Account>,
    pub next_cursor: Option<String>,
}

/// Sensitive session material for a trusted local handoff. Never log or expose to UI.
pub struct SessionUpdate {
    pub authorization: String,
    pub claim: String,
    pub session_id: String,
    pub csrf: String,
}

pub struct Client {
    http: HttpClient,
    pub(crate) session: Session,
    origin: Url,
    credentials: Mutex<Credentials>,
}

struct Credentials {
    authorization: String,
    claim: String,
    session_id: String,
    csrf: String,
    next_request: u64,
    authorization_updated: u64,
    claim_updated: u64,
    session_updated: u64,
    csrf_updated: u64,
}

#[derive(Deserialize)]
struct ApiPage {
    users: Vec<ApiUser>,
    #[serde(default)]
    next_max_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct ApiUser {
    pk: Option<serde_json::Value>,
    pk_id: Option<String>,
    username: String,
}

impl Client {
    pub fn new(session: Session) -> Result<Self, Error> {
        Self::with_origin(session, Url::parse(ORIGIN).map_err(|_| Error::Network)?)
    }

    /// Local fixture server support; this method is omitted from production builds.
    #[cfg(feature = "test-support")]
    pub fn for_local_test(session: Session, origin: &str) -> Result<Self, Error> {
        let origin = Url::parse(origin).map_err(|_| Error::Network)?;
        if origin.scheme() != "http"
            || origin.host_str() != Some("127.0.0.1")
            || origin.username() != ""
            || origin.password().is_some()
        {
            return Err(Error::Network);
        }
        Self::with_origin(session, origin)
    }

    pub(crate) fn with_origin(session: Session, origin: Url) -> Result<Self, Error> {
        let http = HttpClient::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| Error::Network)?;
        let credentials = Credentials {
            authorization: session.authorization.clone(),
            claim: session.www_claim.clone(),
            session_id: session.session_id.clone(),
            csrf: session.csrf.clone(),
            next_request: 0,
            authorization_updated: 0,
            claim_updated: 0,
            session_updated: 0,
            csrf_updated: 0,
        };
        Ok(Self {
            http,
            session,
            origin,
            credentials: Mutex::new(credentials),
        })
    }

    pub(crate) fn get_json(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<serde_json::Value, Error> {
        self.request_json(reqwest::Method::GET, path, query, None)
    }

    pub(crate) fn post_form(
        &self,
        path: &str,
        form: &[(&str, String)],
    ) -> Result<serde_json::Value, Error> {
        self.request_json(reqwest::Method::POST, path, &[], Some(form))
    }

    fn request_json(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        form: Option<&[(&str, String)]>,
    ) -> Result<serde_json::Value, Error> {
        let url = self.origin.join(path).map_err(|_| Error::Network)?;
        let user_agent = format!(
            "Instagram 449.0.0.52.84 Android ({}; {}; 1079242191)",
            self.session.device_string, self.session.language
        );
        let (request_id, authorization, claim, cookies) = {
            let mut credentials = self.credentials.lock().map_err(|_| Error::Network)?;
            credentials.next_request = credentials
                .next_request
                .checked_add(1)
                .ok_or(Error::Network)?;
            (
                credentials.next_request,
                credentials.authorization.clone(),
                credentials.claim.clone(),
                format!(
                    "sessionid={}; ds_user_id={}; csrftoken={}",
                    credentials.session_id, self.session.user_id, credentials.csrf
                ),
            )
        };
        let mut request = self
            .http
            .request(method, url)
            .query(query)
            .header("User-Agent", user_agent)
            .header("Cookie", cookies)
            .header("Authorization", authorization)
            .header("X-IG-App-Locale", &self.session.language)
            .header("X-IG-Device-Locale", &self.session.language)
            .header("X-IG-Connection-Type", "WIFI")
            .header("X-IG-Capabilities", "3brTv10=")
            .header("X-IG-App-ID", "567067343352427")
            .header("X-IG-Device-ID", &self.session.uuid)
            .header("X-IG-Android-ID", &self.session.device_id)
            .header("X-IG-WWW-Claim", claim)
            .header(
                "X-Bloks-Version-Id",
                "799db3e387d7a21a19c72918b8698b6cd28a1516ade363792c100ed45a0f9f0b",
            )
            .header("X-FB-HTTP-Engine", "Liger");
        if let Some(form) = form {
            request = request.form(form);
        }
        let response = request.send().map_err(|_| Error::Network)?;
        let status = response.status();
        if status.as_u16() == 429 {
            return Err(Error::RateLimited);
        }
        if matches!(status.as_u16(), 401 | 403) {
            return Err(Error::Authentication);
        }
        if status.as_u16() == 400 {
            let mut body = Vec::new();
            response
                .take(64_001)
                .read_to_end(&mut body)
                .map_err(|_| Error::Network)?;
            if body.len() <= 64_000 {
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) {
                    if ["message", "error_type"].iter().any(|key| {
                        value.get(key).and_then(serde_json::Value::as_str)
                            == Some("feedback_required")
                    }) {
                        return Err(Error::FeedbackRequired);
                    }
                }
            }
            return Err(Error::Remote);
        }
        if !status.is_success() {
            return Err(Error::Remote);
        }
        {
            let mut credentials = self.credentials.lock().map_err(|_| Error::Network)?;
            if let Some(value) = response
                .headers()
                .get("ig-set-authorization")
                .and_then(|v| v.to_str().ok())
            {
                if request_id > credentials.authorization_updated
                    && value.starts_with("Bearer ")
                    && crate::session::header_text(value, 8192)
                {
                    credentials.authorization = value.to_owned();
                    credentials.authorization_updated = request_id;
                }
            }
            if let Some(value) = response
                .headers()
                .get("x-ig-set-www-claim")
                .and_then(|v| v.to_str().ok())
            {
                if request_id > credentials.claim_updated
                    && crate::session::header_value(value, 4096)
                {
                    credentials.claim = value.to_owned();
                    credentials.claim_updated = request_id;
                }
            }
            for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
                let Ok(cookie) = value
                    .to_str()
                    .ok()
                    .and_then(|s| cookie::Cookie::parse(s).ok())
                    .ok_or(())
                else {
                    continue;
                };
                if cookie.domain().is_some_and(|domain| {
                    !domain.eq_ignore_ascii_case("instagram.com")
                        && !domain.eq_ignore_ascii_case(".instagram.com")
                        && !domain.eq_ignore_ascii_case("i.instagram.com")
                }) || cookie.path().is_some_and(|path| path != "/")
                    || cookie
                        .max_age()
                        .is_some_and(|age| age.is_negative() || age.is_zero())
                    || cookie.expires_datetime().is_some_and(|expiry| {
                        expiry.unix_timestamp()
                            <= std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64
                    })
                    || !crate::session::header_value(cookie.value(), 4096)
                {
                    continue;
                }
                match cookie.name() {
                    "sessionid"
                        if request_id > credentials.session_updated
                            && cookie
                                .value()
                                .bytes()
                                .take_while(u8::is_ascii_digit)
                                .count()
                                == self.session.user_id.len()
                            && cookie.value().starts_with(&self.session.user_id) =>
                    {
                        credentials.session_id = cookie.value().to_owned();
                        credentials.session_updated = request_id;
                    }
                    "csrftoken" if request_id > credentials.csrf_updated => {
                        credentials.csrf = cookie.value().to_owned();
                        credentials.csrf_updated = request_id;
                    }
                    _ => {}
                }
            }
        }
        let mut body = Vec::new();
        response
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut body)
            .map_err(|_| Error::Network)?;
        if body.len() as u64 > MAX_RESPONSE {
            return Err(Error::InvalidResponse);
        }
        serde_json::from_slice(&body).map_err(|_| Error::InvalidResponse)
    }

    pub(crate) fn csrf(&self) -> Result<String, Error> {
        self.credentials
            .lock()
            .map(|c| c.csrf.clone())
            .map_err(|_| Error::Network)
    }

    /// Copy the current credentials for migration to a trusted local session owner.
    pub fn session_update(&self) -> Result<SessionUpdate, Error> {
        let c = self.credentials.lock().map_err(|_| Error::Network)?;
        Ok(SessionUpdate {
            authorization: c.authorization.clone(),
            claim: c.claim.clone(),
            session_id: c.session_id.clone(),
            csrf: c.csrf.clone(),
        })
    }

    /// One page of followed accounts. No requests to the recommendation feed are made.
    pub fn following_page(&self, cursor: Option<&str>) -> Result<FollowingPage, Error> {
        if cursor.is_some_and(|c| c.is_empty() || c.len() > 2048 || c.chars().any(char::is_control))
        {
            return Err(Error::InvalidCursor);
        }
        let path = format!("api/v1/friendships/{}/following/", self.session.user_id);
        let mut query = vec![
            ("count", "200"),
            ("order", "default"),
            ("query", ""),
            ("enable_groups", "false"),
            ("includes_hashtags", "false"),
        ];
        if let Some(cursor) = cursor {
            query.push(("max_id", cursor));
        }
        let body = self.get_json(&path, &query)?;
        let data: ApiPage = serde_json::from_value(body).map_err(|_| Error::InvalidResponse)?;
        if data.status.as_deref().is_some_and(|s| s != "ok") {
            return Err(Error::Remote);
        }
        let mut accounts = Vec::with_capacity(data.users.len());
        for user in data.users {
            let id = user
                .pk_id
                .or_else(|| match user.pk {
                    Some(serde_json::Value::String(s)) => Some(s),
                    Some(serde_json::Value::Number(n)) => Some(n.to_string()),
                    _ => None,
                })
                .ok_or(Error::InvalidResponse)?;
            if id.is_empty()
                || id.len() > 30
                || !id.bytes().all(|b| b.is_ascii_digit())
                || user.username.is_empty()
                || user.username.len() > 30
                || !user
                    .username
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
            {
                return Err(Error::InvalidResponse);
            }
            accounts.push(Account {
                id,
                username: user.username,
            });
        }
        let next_cursor = data.next_max_id.filter(|s| !s.is_empty());
        if next_cursor
            .as_ref()
            .is_some_and(|c| c.len() > 2048 || c.chars().any(char::is_control))
        {
            return Err(Error::InvalidCursor);
        }
        Ok(FollowingPage {
            accounts,
            next_cursor,
        })
    }

    pub fn following(&self) -> FollowingPager<'_> {
        FollowingPager {
            client: self,
            cursor: None,
            seen: HashSet::new(),
            finished: false,
        }
    }
}

pub struct FollowingPager<'a> {
    client: &'a Client,
    cursor: Option<String>,
    seen: HashSet<String>,
    finished: bool,
}

impl FollowingPager<'_> {
    pub fn next_page(&mut self) -> Result<Option<FollowingPage>, Error> {
        if self.finished {
            return Ok(None);
        }
        let page = self.client.following_page(self.cursor.as_deref())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::session;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn pages_follow_cursor_preserve_large_ids_and_reject_repeated_cursor() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 2048];
                while !request.ends_with(b"\r\n\r\n") {
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&buffer[..n]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with("GET /api/v1/friendships/123456/following/?"));
                assert!(request.contains("count=200"));
                let headers = request.to_ascii_lowercase();
                assert!(headers.contains("authorization: bearer igt:2:"));
                assert!(headers.contains("cookie: sessionid=123456%3asynthetic;"));
                assert_eq!(request.contains("max_id=next"), index == 1);
                let body = r#"{"status":"ok","users":[{"pk":12345678901234567890,"username":"friend"}],"next_max_id":"next"}"#;
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let client = Client::with_origin(
            session(),
            Url::parse(&format!("http://{address}/")).unwrap(),
        )
        .unwrap();
        let mut pager = client.following();
        let page = pager.next_page().unwrap().unwrap();
        assert_eq!(page.accounts[0].id, "12345678901234567890");
        assert_eq!(page.accounts[0].username, "friend");
        assert_eq!(pager.next_page(), Err(Error::InvalidCursor));
        server.join().unwrap();
    }

    #[test]
    fn rejects_invalid_cursor_without_making_a_request() {
        let client = Client::new(session()).unwrap();
        assert_eq!(
            client.following_page(Some("bad\nvalue")),
            Err(Error::InvalidCursor)
        );
    }

    #[test]
    fn updates_session_headers_and_ignores_invalid_replacements() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for index in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 2048];
                while !request.ends_with(b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                let headers = String::from_utf8(request).unwrap().to_ascii_lowercase();
                if index > 0 {
                    assert!(headers.contains("authorization: bearer updated-token\r\n"));
                    assert!(headers.contains("x-ig-www-claim: updated-claim\r\n"));
                    assert!(headers.contains("cookie: sessionid=123456%3arenewed; ds_user_id=123456; csrftoken=renewed\r\n"));
                }
                let updates = match index {
                    0 => "ig-set-authorization: Bearer updated-token\r\nx-ig-set-www-claim: updated-claim\r\nset-cookie: sessionid=123456%3Arenewed; Domain=.instagram.com; Path=/; Secure\r\nset-cookie: csrftoken=renewed; Domain=.instagram.com; Path=/; Secure\r\n",
                    1 => "ig-set-authorization: invalid\r\nx-ig-set-www-claim: invalid;claim\r\nset-cookie: sessionid=999%3Abad; Path=/\r\nset-cookie: csrftoken=evil; Domain=evil.example; Path=/\r\nset-cookie: csrftoken=expired; Max-Age=0; Path=/\r\n",
                    _ => "",
                };
                let body = r#"{"status":"ok","users":[]}"#;
                write!(stream, "HTTP/1.1 200 OK\r\n{updates}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let client = Client::with_origin(
            session(),
            Url::parse(&format!("http://{address}/")).unwrap(),
        )
        .unwrap();
        for _ in 0..3 {
            client.following_page(None).unwrap();
        }
        server.join().unwrap();
    }

    #[test]
    fn rate_limit_is_reported_without_response_body_or_retries() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = "private server details";
            write!(stream, "HTTP/1.1 429 Too Many Requests\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let client = Client::with_origin(
            session(),
            Url::parse(&format!("http://{address}/")).unwrap(),
        )
        .unwrap();
        assert_eq!(client.following_page(None), Err(Error::RateLimited));
        server.join().unwrap();
    }

    #[test]
    fn feedback_required_is_classified_without_retrying_or_exposing_server_text() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body =
                r#"{"message":"feedback_required","feedback_message":"private server text"}"#;
            write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            listener.set_nonblocking(true).unwrap();
            assert!(listener.accept().is_err());
        });
        let client = Client::with_origin(
            session(),
            Url::parse(&format!("http://{address}/")).unwrap(),
        )
        .unwrap();
        assert_eq!(client.following_page(None), Err(Error::FeedbackRequired));
        assert!(!Error::FeedbackRequired.to_string().contains("private"));
        server.join().unwrap();
    }
}
