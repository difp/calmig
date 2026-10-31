//! Update existing legacy sessions atomically. Never log their contents.
use crate::{Session, SessionUpdate};
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};
use uuid::Uuid;

/// Preserve unrelated legacy state while persisting refreshed credentials.
pub fn update_legacy_session(path: &Path, update: &SessionUpdate) -> io::Result<()> {
    let mut data = Vec::new();
    fs::File::open(path)?
        .take(2_000_001)
        .read_to_end(&mut data)?;
    if data.len() > 2_000_000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid session",
        ));
    }
    Session::from_legacy_json(&data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid session"))?;
    let mut state: Value = serde_json::from_slice(&data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid session"))?;
    let mut jar: Value = serde_json::from_str(
        state
            .get("cookies")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid session"))?,
    )
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid session"))?;
    let cookies = jar
        .get_mut("cookies")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid session"))?;
    for cookie in cookies {
        match cookie.get("key").and_then(Value::as_str) {
            Some("sessionid") => cookie["value"] = json!(update.session_id),
            Some("csrftoken") => cookie["value"] = json!(update.csrf),
            _ => {}
        }
    }
    state["cookies"] = json!(serde_json::to_string(&jar)?);
    state["authorization"] = json!(update.authorization);
    state["igWWWClaim"] = json!(update.claim);
    let bytes = serde_json::to_vec(&state)?;
    Session::from_legacy_json(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid session"))?;
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid session path"))?;
    let temp = dir.join(format!(".session-{}.tmp", Uuid::new_v4().simple()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.flush()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    let _ = fs::remove_file(temp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::fixture;
    #[test]
    fn refresh_is_atomic_validated_and_preserves_device() {
        let dir = std::env::temp_dir().join(format!("calmig-legacy-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("session.ts.json");
        fs::write(&path, fixture().to_string()).unwrap();
        let update = SessionUpdate {
            authorization: "Bearer refreshed".into(),
            claim: "claim".into(),
            session_id: "123456%3Anew".into(),
            csrf: "newcsrf".into(),
        };
        update_legacy_session(&path, &update).unwrap();
        let session = Session::from_legacy_json(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(session.session_id, "123456%3Anew");
        assert_eq!(session.csrf, "newcsrf");
        assert_eq!(session.device_id, "android-synthetic");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let before = fs::read(&path).unwrap();
        let invalid = SessionUpdate {
            session_id: "999%3Awrong".into(),
            ..update
        };
        assert!(update_legacy_session(&path, &invalid).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }
}
