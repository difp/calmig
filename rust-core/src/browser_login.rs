//! Prepare browser login without Node while preserving the legacy device identity.
use crate::{legacy_device, load_or_import_device, BrowserCookie, Client, Error, Session};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrowserLoginError {
    #[error("Invalid browser session")]
    InvalidSession,
    #[error("Unable to access the device profile")]
    DeviceStorage,
    #[error("Unable to initialize the Instagram client")]
    Client,
}

/// Prepare a client from official WebView cookies and the account's stable device.
/// The caller must confirm the account with `current_account()` before exposing it.
pub fn client_from_browser(
    cookies: &[BrowserCookie],
    directory: &Path,
) -> Result<Client, BrowserLoginError> {
    let mut ids = cookies.iter().filter(|cookie| cookie.name == "ds_user_id");
    let id = ids
        .next()
        .ok_or(BrowserLoginError::InvalidSession)?
        .value
        .as_str();
    if ids.next().is_some() {
        return Err(BrowserLoginError::InvalidSession);
    }
    let candidate = legacy_device(id).map_err(|_| BrowserLoginError::InvalidSession)?;
    // Validate all required cookies before touching the filesystem.
    Session::from_browser_cookies(cookies, candidate.clone())
        .map_err(|_| BrowserLoginError::InvalidSession)?;
    let device = load_or_import_device(directory, id, &candidate)
        .map_err(|_| BrowserLoginError::DeviceStorage)?;
    let session = Session::from_browser_cookies(cookies, device)
        .map_err(|_| BrowserLoginError::InvalidSession)?;
    Client::new(session).map_err(|_: Error| BrowserLoginError::Client)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    fn cookies() -> Vec<BrowserCookie> {
        vec![
            BrowserCookie {
                name: "ds_user_id".into(),
                value: "123456".into(),
            },
            BrowserCookie {
                name: "sessionid".into(),
                value: "123456%3Asynthetic".into(),
            },
            BrowserCookie {
                name: "csrftoken".into(),
                value: "synthetic".into(),
            },
        ]
    }
    #[test]
    fn imports_existing_identity_before_creating_client() {
        let dir = std::env::temp_dir().join(format!("calmig-login-{}", Uuid::new_v4()));
        let client = client_from_browser(&cookies(), &dir).unwrap();
        assert_eq!(client.session.device_id, "android-fd3177648aed3dd4");
        assert_eq!(client.session.uuid, "d95b0fd8-223d-5b96-a300-c54dca01e620");
        drop(client);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn invalid_session_does_not_create_device_files() {
        let dir = std::env::temp_dir().join(format!("calmig-login-{}", Uuid::new_v4()));
        let mut input = cookies();
        input[1].value = "987%3Awrong".into();
        assert!(client_from_browser(&input, &dir).is_err());
        assert!(!dir.exists());
    }
}
