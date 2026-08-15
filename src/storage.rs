use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn temporary_path(path: &Path) -> io::Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "target path has no file name")
    })?;
    Ok(path.with_file_name(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    )))
}

#[cfg(unix)]
fn replace(temporary: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temporary, target)
}

#[cfg(windows)]
fn replace(temporary: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both buffers are stable, NUL-terminated UTF-16 paths for the duration of the call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn write_atomic_bytes(
    path: &Path,
    bytes: &[u8],
    mode: u32,
    private_parent: bool,
) -> io::Result<()> {
    #[cfg(not(unix))]
    let _ = mode;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target path has no parent"))?;
    if private_parent {
        ensure_private_dir(parent)?;
    } else {
        fs::create_dir_all(parent)?;
    }
    let temporary = temporary_path(path)?;
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(mode);
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        drop(file);
        replace(&temporary, path)
    })();
    let _ = fs::remove_file(&temporary);
    result
}

pub fn write_atomic_json<T: Serialize>(
    path: &Path,
    value: &T,
    mode: u32,
    private_parent: bool,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    write_atomic_bytes(path, &bytes, mode, private_parent)
}

pub fn write_atomic_text(path: &Path, value: &str, mode: u32) -> io::Result<()> {
    write_atomic_bytes(path, value.as_bytes(), mode, true)
}

pub fn append_bounded(path: &Path, line: &str, max_bytes: usize) -> io::Result<()> {
    if max_bytes < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "max_bytes must be at least 2",
        ));
    }
    let mut encoded = line.as_bytes().to_vec();
    if encoded.len() > max_bytes {
        let mut end = max_bytes - 1;
        while end > 0 && !line.is_char_boundary(end) {
            end -= 1;
        }
        encoded = line.as_bytes()[..end].to_vec();
        encoded.push(b'\n');
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "log path has no parent"))?;
    ensure_private_dir(parent)?;
    if fs::metadata(path)
        .map(|metadata| metadata.len().saturating_add(encoded.len() as u64) > max_bytes as u64)
        .unwrap_or(false)
    {
        let previous = path.with_file_name(format!(
            "{}.1",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        let _ = fs::remove_file(&previous);
        let _ = fs::rename(path, previous);
    }
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(&encoded)?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "memory-supervisor-storage-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn atomic_private_write_and_bounded_rotation_match_the_python_contract() {
        let root = temporary_directory();
        let value = serde_json::json!({"keep": "값"});
        let config = root.join("private/config.json");
        write_atomic_json(&config, &value, 0o600, true).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&config).unwrap()).unwrap(),
            value
        );

        let log = root.join("events.log");
        append_bounded(&log, "123456", 10).unwrap();
        append_bounded(&log, "abcdef", 10).unwrap();
        assert_eq!(fs::read(root.join("events.log.1")).unwrap(), b"123456");
        append_bounded(&log, &"x".repeat(30), 10).unwrap();
        assert_eq!(fs::read(&log).unwrap(), b"xxxxxxxxx\n");

        #[cfg(unix)]
        {
            assert_eq!(
                fs::metadata(&config).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(config.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
