use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

const MAX_SESSION_BYTES: usize = 2_000_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("Invalid saved session")]
    Invalid,
}

// Deliberately does not implement Debug, Display or Serialize: all fields are sensitive.
pub struct Session {
    pub(crate) user_id: String,
    pub(crate) session_id: String,
    pub(crate) csrf: String,
    pub(crate) authorization: String,
    pub(crate) device_id: String,
    pub(crate) uuid: String,
    pub(crate) device_string: String,
    pub(crate) language: String,
    pub(crate) www_claim: String,
}

/// Only the cookies required by the adapter are imported; other names are ignored.
/// Do not log or persist these values in plaintext.
#[derive(Deserialize)]
pub struct BrowserCookie {
    pub name: String,
    pub value: String,
}

/// A stable device identity created and stored by the application for this account.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProfile {
    pub device_id: String,
    pub uuid: String,
    pub device_string: String,
    pub language: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySession {
    cookies: String,
    authorization: Option<String>,
    device_id: String,
    uuid: String,
    device_string: String,
    language: String,
    #[serde(default, rename = "igWWWClaim")]
    ig_www_claim: Option<String>,
}

#[derive(Deserialize)]
struct CookieJar {
    cookies: Vec<Cookie>,
}

#[derive(Deserialize)]
struct Cookie {
    key: String,
    value: String,
    domain: String,
    path: String,
    #[serde(default)]
    secure: bool,
}

pub(crate) fn header_value(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| (33..=126).contains(&b) && b != b';' && b != b',')
}

pub(crate) fn header_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|b| (32..=126).contains(&b))
}

impl Session {
    /// Import cookies obtained from the official Instagram WebView on this device.
    /// The caller owns lifecycle and secure storage of the stable device profile.
    pub fn from_browser_cookies(
        cookies: &[BrowserCookie],
        device: DeviceProfile,
    ) -> Result<Self, SessionError> {
        let required: Vec<_> = cookies
            .iter()
            .filter(|cookie| {
                matches!(
                    cookie.name.as_str(),
                    "sessionid" | "ds_user_id" | "csrftoken"
                )
            })
            .map(|cookie| {
                serde_json::json!({
                    "key": cookie.name, "value": cookie.value,
                    "domain": "instagram.com", "path": "/", "secure": true
                })
            })
            .collect();
        let jar = serde_json::json!({"cookies": required}).to_string();
        let state = serde_json::json!({
            "cookies": jar, "deviceId": device.device_id, "uuid": device.uuid,
            "deviceString": device.device_string, "language": device.language
        });
        Self::from_legacy_json(state.to_string().as_bytes())
    }

    /// Restore the JSON state written by the existing Node adapter. Never print the input.
    pub fn from_legacy_json(data: &[u8]) -> Result<Self, SessionError> {
        if data.len() > MAX_SESSION_BYTES {
            return Err(SessionError::Invalid);
        }
        let saved: LegacySession =
            serde_json::from_slice(data).map_err(|_| SessionError::Invalid)?;
        let jar: CookieJar =
            serde_json::from_str(&saved.cookies).map_err(|_| SessionError::Invalid)?;
        let mut values = HashMap::new();
        for cookie in jar.cookies {
            if !matches!(
                cookie.key.as_str(),
                "sessionid" | "ds_user_id" | "csrftoken"
            ) {
                continue;
            }
            if cookie.domain.trim_start_matches('.') != "instagram.com"
                || cookie.path != "/"
                || !cookie.secure
                || !header_value(&cookie.value, 4096)
                || values.insert(cookie.key, cookie.value).is_some()
            {
                return Err(SessionError::Invalid);
            }
        }
        let user_id = values.remove("ds_user_id").ok_or(SessionError::Invalid)?;
        let session_id = values.remove("sessionid").ok_or(SessionError::Invalid)?;
        let csrf = values.remove("csrftoken").ok_or(SessionError::Invalid)?;
        if user_id.len() > 30
            || !user_id.bytes().all(|b| b.is_ascii_digit())
            || session_id.bytes().take_while(u8::is_ascii_digit).count() != user_id.len()
            || !session_id.starts_with(&user_id)
            || !header_text(&saved.device_string, 512)
            || !header_value(&saved.device_id, 128)
            || !header_value(&saved.uuid, 128)
            || !header_value(&saved.language, 32)
        {
            return Err(SessionError::Invalid);
        }
        let authorization = match saved.authorization {
            Some(value) if header_text(&value, 8192) && value.starts_with("Bearer ") => value,
            Some(_) => return Err(SessionError::Invalid),
            None => {
                let data = serde_json::json!({"ds_user_id":user_id,"sessionid":session_id,"should_use_header_over_cookies":true});
                format!("Bearer IGT:2:{}", STANDARD.encode(data.to_string()))
            }
        };
        let www_claim = saved.ig_www_claim.unwrap_or_else(|| "0".into());
        if !header_value(&www_claim, 4096) {
            return Err(SessionError::Invalid);
        }
        Ok(Self {
            user_id,
            session_id,
            csrf,
            authorization,
            device_id: saved.device_id,
            uuid: saved.uuid,
            device_string: saved.device_string,
            language: saved.language,
            www_claim,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{json, Value};

    pub(crate) fn fixture() -> Value {
        let cookies = json!({"cookies": [
            {"key":"sessionid", "value":"123456%3Asynthetic", "domain":"instagram.com", "path":"/", "secure":true},
            {"key":"ds_user_id", "value":"123456", "domain":"instagram.com", "path":"/", "secure":true},
            {"key":"csrftoken", "value":"synthetic", "domain":"instagram.com", "path":"/", "secure":true}
        ]});
        json!({"cookies":cookies.to_string(), "deviceId":"android-synthetic", "uuid":"fake-uuid", "deviceString":"34/14; test device", "language":"en_US"})
    }

    pub(crate) fn session() -> Session {
        Session::from_legacy_json(fixture().to_string().as_bytes()).unwrap()
    }

    #[test]
    fn restores_legacy_state_and_derives_browser_authorization() {
        let restored = session();
        assert_eq!(restored.user_id, "123456");
        let payload = restored
            .authorization
            .strip_prefix("Bearer IGT:2:")
            .unwrap();
        let decoded = STANDARD.decode(payload).unwrap();
        let value: Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(value["sessionid"], "123456%3Asynthetic");
        assert_eq!(value["ds_user_id"], "123456");
    }

    #[test]
    fn imports_only_required_browser_cookies() {
        let cookies = [
            BrowserCookie {
                name: "sessionid".into(),
                value: "123456%3Asynthetic".into(),
            },
            BrowserCookie {
                name: "ds_user_id".into(),
                value: "123456".into(),
            },
            BrowserCookie {
                name: "csrftoken".into(),
                value: "synthetic".into(),
            },
            BrowserCookie {
                name: "unrelated".into(),
                value: "ignored".into(),
            },
        ];
        let device = DeviceProfile {
            device_id: "android-synthetic".into(),
            uuid: "fake-uuid".into(),
            device_string: "34/14; test device".into(),
            language: "en_US".into(),
        };
        let session = Session::from_browser_cookies(&cookies, device).unwrap();
        assert_eq!(session.user_id, "123456");
        assert_eq!(session.csrf, "synthetic");
    }

    #[test]
    fn rejects_mismatched_accounts_and_injected_headers() {
        let mut state = fixture();
        let mut jar: Value = serde_json::from_str(state["cookies"].as_str().unwrap()).unwrap();
        jar["cookies"][1]["value"] = json!("999");
        state["cookies"] = json!(jar.to_string());
        assert!(Session::from_legacy_json(state.to_string().as_bytes()).is_err());
        let mut state = fixture();
        state["deviceId"] = json!("device\r\ninjected: value");
        assert!(Session::from_legacy_json(state.to_string().as_bytes()).is_err());
        let mut state = fixture();
        state["authorization"] = json!("Bearer token\r\nInjected: value");
        assert!(Session::from_legacy_json(state.to_string().as_bytes()).is_err());
    }

    #[test]
    fn rejects_wrong_cookie_origin_and_duplicates() {
        for mode in ["wrong_origin", "duplicate"] {
            let mut state = fixture();
            let mut jar: Value = serde_json::from_str(state["cookies"].as_str().unwrap()).unwrap();
            if mode == "wrong_origin" {
                jar["cookies"][0]["domain"] = json!("evil.example");
            } else {
                let duplicate = jar["cookies"][0].clone();
                jar["cookies"].as_array_mut().unwrap().push(duplicate);
            }
            state["cookies"] = json!(jar.to_string());
            assert!(Session::from_legacy_json(state.to_string().as_bytes()).is_err());
        }
    }
}
