//! Stable, account-scoped device identities. No session cookies are stored here.
use crate::{
    session::{header_text, header_value},
    DeviceProfile,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

fn file_for(directory: &Path, account_id: &str) -> io::Result<PathBuf> {
    if account_id.is_empty()
        || account_id.len() > 30
        || !account_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid account",
        ));
    }
    let digest = Sha256::digest(account_id.as_bytes());
    Ok(directory.join(format!("device-{digest:x}.json")))
}
fn valid(device: &DeviceProfile) -> bool {
    header_value(&device.device_id, 128)
        && header_value(&device.uuid, 128)
        && header_text(&device.device_string, 512)
        && header_value(&device.language, 32)
}
fn read_device(path: &Path) -> io::Result<DeviceProfile> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(8193).read_to_end(&mut bytes)?;
    if bytes.len() > 8192 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid device profile",
        ));
    }
    let device: DeviceProfile = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid device profile"))?;
    if !valid(&device) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid device profile",
        ));
    }
    Ok(device)
}

/// Restore a device or atomically import the existing adapter's identity on first use.
/// A damaged existing file is an error: silently replacing it would change device identity.
pub fn load_or_import_device(
    directory: &Path,
    account_id: &str,
    candidate: &DeviceProfile,
) -> io::Result<DeviceProfile> {
    let target = file_for(directory, account_id)?;
    match read_device(&target) {
        Ok(device) => return Ok(device),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if !valid(candidate) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid device profile",
        ));
    }
    let mut directory_builder = fs::DirBuilder::new();
    directory_builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory_builder.mode(0o700);
    }
    directory_builder.create(directory)?;
    let temp = directory.join(format!(".device-{}.tmp", Uuid::new_v4().simple()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        serde_json::to_writer(&mut file, candidate)?;
        file.flush()?;
        drop(file);
        match fs::hard_link(&temp, &target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    })();
    let _ = fs::remove_file(temp);
    result?;
    read_device(&target)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn device(id: &str) -> DeviceProfile {
        DeviceProfile {
            device_id: id.into(),
            uuid: "uuid".into(),
            device_string: "34/14; test".into(),
            language: "en_US".into(),
        }
    }
    #[test]
    fn import_is_account_scoped_stable_and_owner_only() {
        let dir = std::env::temp_dir().join(format!("calmig-device-{}", Uuid::new_v4()));
        let original = device("android-original");
        let first = load_or_import_device(&dir, "123", &original).unwrap();
        let second = load_or_import_device(&dir, "123", &device("android-changed")).unwrap();
        assert!(first == original && second == original);
        assert!(
            load_or_import_device(&dir, "456", &device("android-other"))
                .unwrap()
                .device_id
                == "android-other"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(file_for(&dir, "123").unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn corrupted_profile_is_not_silently_replaced() {
        let dir = std::env::temp_dir().join(format!("calmig-device-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let target = file_for(&dir, "123").unwrap();
        fs::write(&target, b"damaged").unwrap();
        assert!(load_or_import_device(&dir, "123", &device("android-new")).is_err());
        assert_eq!(fs::read(target).unwrap(), b"damaged");
        assert!(load_or_import_device(&dir, "../bad", &device("x")).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_imports_choose_one_complete_identity() {
        let dir = std::env::temp_dir().join(format!("calmig-device-{}", Uuid::new_v4()));
        let start = std::sync::Barrier::new(2);
        let (first, second) = std::thread::scope(|scope| {
            let one = scope.spawn(|| {
                start.wait();
                load_or_import_device(&dir, "123", &device("android-one")).unwrap()
            });
            let two = scope.spawn(|| {
                start.wait();
                load_or_import_device(&dir, "123", &device("android-two")).unwrap()
            });
            (one.join().unwrap(), two.join().unwrap())
        });
        assert!(first == second);
        assert!(first.device_id == "android-one" || first.device_id == "android-two");
        fs::remove_dir_all(dir).unwrap();
    }
}
