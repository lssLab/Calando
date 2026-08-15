use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::Write;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Delivery {
    pub status: String,
    pub identity: String,
    pub reason: String,
}

impl Delivery {
    fn unavailable(identity: String, reason: &str) -> Self {
        Self {
            status: "unavailable".to_owned(),
            identity,
            reason: reason.to_owned(),
        }
    }
}

#[cfg(unix)]
fn posix_target(endpoint: &str, expected_identity: &str) -> (Option<PathBuf>, String, String) {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    if !endpoint.starts_with("/dev/pts/") && !endpoint.starts_with("/dev/tty") {
        return (None, String::new(), "no-controlling-terminal".to_owned());
    }
    let Ok(target) = fs::canonicalize(endpoint) else {
        return (None, String::new(), "terminal-unreachable".to_owned());
    };
    let Ok(metadata) = fs::metadata(&target) else {
        return (None, String::new(), "terminal-unreachable".to_owned());
    };
    if !metadata.file_type().is_char_device() {
        return (None, String::new(), "not-character-device".to_owned());
    }
    // SAFETY: geteuid has no arguments or memory effects visible to Rust.
    if metadata.uid() != unsafe { libc::geteuid() } {
        return (None, String::new(), "terminal-owner-mismatch".to_owned());
    }
    let identity = format!("{}:{}:{}", metadata.dev(), metadata.ino(), metadata.rdev());
    if !expected_identity.is_empty() && identity != expected_identity {
        return (None, identity, "terminal-identity-changed".to_owned());
    }
    (Some(target), identity, "verified".to_owned())
}

#[cfg(not(unix))]
fn posix_target(_endpoint: &str, _expected_identity: &str) -> (Option<PathBuf>, String, String) {
    (None, String::new(), "posix-terminal-unavailable".to_owned())
}

pub fn probe_posix(endpoint: &str, expected_identity: &str) -> Delivery {
    let (target, identity, reason) = posix_target(endpoint, expected_identity);
    Delivery {
        status: if target.is_some() {
            "delivered"
        } else {
            "unavailable"
        }
        .to_owned(),
        identity,
        reason,
    }
}

pub fn write_posix(endpoint: &str, message: &str, expected_identity: &str) -> Delivery {
    let (target, identity, reason) = posix_target(endpoint, expected_identity);
    let Some(target) = target else {
        return Delivery::unavailable(identity, &reason);
    };
    #[cfg(not(unix))]
    let _ = (&target, message);
    #[cfg(unix)]
    let result = {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(target)
            .and_then(|mut file| {
                let payload = format!("\r\n{message}\r\n");
                file.write_all(payload.as_bytes())
            })
    };
    #[cfg(not(unix))]
    let result: std::io::Result<()> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "POSIX terminal unavailable",
    ));
    match result {
        Ok(()) => Delivery {
            status: "delivered".to_owned(),
            identity,
            reason: "written".to_owned(),
        },
        Err(_) => Delivery {
            status: "failed".to_owned(),
            identity,
            reason: "write-failed".to_owned(),
        },
    }
}

#[cfg(windows)]
mod windows_console {
    use std::ffi::{OsStr, c_void};
    use std::os::windows::ffi::OsStrExt;

    pub type Handle = *mut c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn FreeConsole() -> i32;
        fn GetConsoleProcessList(processes: *mut u32, process_count: u32) -> u32;
        fn GetConsoleWindow() -> Handle;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn WriteConsoleW(
            output: Handle,
            buffer: *const u16,
            characters: u32,
            written: *mut u32,
            reserved: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    fn is_private(process_count: u32, process_id: u32) -> bool {
        process_count == 1 && process_id == std::process::id()
    }

    pub fn detach_if_private() {
        let mut process_id = 0;
        // SAFETY: the one-element buffer is valid for the duration of the call.
        let process_count = unsafe { GetConsoleProcessList(&mut process_id, 1) };
        if is_private(process_count, process_id) {
            // SAFETY: only this process is attached, so detaching closes its private window.
            unsafe { FreeConsole() };
        }
    }

    pub fn attach(pid: u32, expected: &str) -> Result<(Handle, String), (String, &'static str)> {
        // SAFETY: the calls use only scalar values and process-owned console state.
        unsafe { FreeConsole() };
        if unsafe { AttachConsole(pid) } == 0 {
            return Err((String::new(), "target-has-no-attachable-console"));
        }
        let window = unsafe { GetConsoleWindow() } as usize;
        let identity = format!("console:{window:x}:target:{pid}");
        if !expected.is_empty() && identity != expected {
            unsafe { FreeConsole() };
            return Err((identity, "console-identity-changed"));
        }
        Ok((window as Handle, identity))
    }

    pub fn detach() {
        // SAFETY: releases the console attached by this module.
        unsafe { FreeConsole() };
    }

    pub fn write(message: &str) -> bool {
        let name: Vec<u16> = OsStr::new("CONOUT$").encode_wide().chain(Some(0)).collect();
        // SAFETY: name is NUL-terminated and all optional pointers are null.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                0x40000000,
                0x00000003,
                std::ptr::null_mut(),
                3,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle.is_null() || handle as isize == -1 {
            return false;
        }
        let text: Vec<u16> = format!("\r\n{message}\r\n").encode_utf16().collect();
        let mut written = 0;
        // SAFETY: text remains allocated for the duration of WriteConsoleW.
        let ok = unsafe {
            WriteConsoleW(
                handle,
                text.as_ptr(),
                text.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        } != 0
            && written == text.len() as u32;
        unsafe { CloseHandle(handle) };
        ok
    }

    #[test]
    fn only_the_current_process_owns_a_private_console() {
        let process_id = std::process::id();
        assert!(is_private(1, process_id));
        assert!(!is_private(0, process_id));
        assert!(!is_private(2, process_id));
        assert!(!is_private(1, process_id.wrapping_add(1)));
    }
}

#[cfg(windows)]
pub(crate) fn detach_private_console() {
    windows_console::detach_if_private();
}

pub fn probe_windows(pid: u32, expected_identity: &str) -> Delivery {
    #[cfg(windows)]
    {
        match windows_console::attach(pid, expected_identity) {
            Ok((_, identity)) => {
                windows_console::detach();
                Delivery {
                    status: "delivered".to_owned(),
                    identity,
                    reason: "verified".to_owned(),
                }
            }
            Err((identity, reason)) => Delivery::unavailable(identity, reason),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, expected_identity);
        Delivery::unavailable(String::new(), "windows-console-api-unavailable")
    }
}

pub fn write_windows(pid: u32, message: &str, expected_identity: &str) -> Delivery {
    #[cfg(windows)]
    {
        match windows_console::attach(pid, expected_identity) {
            Ok((_, identity)) => {
                let written = windows_console::write(message);
                windows_console::detach();
                Delivery {
                    status: if written { "delivered" } else { "failed" }.to_owned(),
                    identity,
                    reason: if written {
                        "written"
                    } else {
                        "console-write-failed"
                    }
                    .to_owned(),
                }
            }
            Err((identity, reason)) => Delivery::unavailable(identity, reason),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, message, expected_identity);
        Delivery::unavailable(String::new(), "windows-console-api-unavailable")
    }
}

pub fn probe(platform: &str, pid: u32, endpoint: &str, expected_identity: &str) -> Delivery {
    if platform == "windows" {
        probe_windows(pid, expected_identity)
    } else {
        probe_posix(endpoint, expected_identity)
    }
}

pub fn write(
    platform: &str,
    pid: u32,
    endpoint: &str,
    message: &str,
    expected_identity: &str,
) -> Delivery {
    if platform == "windows" {
        write_windows(pid, message, expected_identity)
    } else {
        write_posix(endpoint, message, expected_identity)
    }
}

pub fn identity_for_path(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path)
            .map(|metadata| format!("{}:{}:{}", metadata.dev(), metadata.ino(), metadata.rdev()))
            .unwrap_or_default()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(unix)]
    fn test_pty() -> (fs::File, fs::File, String) {
        use std::ffi::CStr;
        use std::os::fd::FromRawFd;

        let (mut master, mut slave) = (-1, -1);
        let mut name = [0 as libc::c_char; 256];
        // SAFETY: all output pointers reference initialized storage and the returned descriptors
        // are immediately owned by File values below.
        let result = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                name.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(
            result,
            0,
            "openpty failed: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: openpty wrote a NUL-terminated terminal path into name.
        let path = unsafe { CStr::from_ptr(name.as_ptr()) }
            .to_str()
            .unwrap()
            .to_owned();
        // SAFETY: master is a valid descriptor returned by openpty.
        let flags = unsafe { libc::fcntl(master, libc::F_GETFL) };
        assert!(flags >= 0);
        // SAFETY: F_SETFL only updates descriptor flags.
        assert_eq!(
            unsafe { libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        // SAFETY: ownership of both valid descriptors is transferred exactly once.
        let master = unsafe { fs::File::from_raw_fd(master) };
        let slave = unsafe { fs::File::from_raw_fd(slave) };
        (master, slave, path)
    }

    #[cfg(unix)]
    fn read_pty(master: &mut fs::File) -> String {
        use std::io::{ErrorKind, Read};

        let mut output = Vec::new();
        let mut buffer = [0_u8; 512];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => output.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => panic!("PTY read failed: {error}"),
            }
        }
        String::from_utf8_lossy(&output).into_owned()
    }

    #[test]
    fn regular_file_is_never_a_terminal() {
        let path =
            std::env::temp_dir().join(format!("memory-supervisor-terminal-{}", std::process::id()));
        fs::write(&path, "not a tty").unwrap();
        let result = probe_posix(path.to_str().unwrap(), "");
        assert_eq!(result.status, "unavailable");
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn exact_pty_receives_the_notice_and_identity_mismatch_writes_nothing() {
        let (mut selected, _selected_slave, selected_path) = test_pty();
        let (mut other, _other_slave, _other_path) = test_pty();
        let identity = identity_for_path(Path::new(&selected_path));
        assert!(!identity.is_empty());

        let mismatch = write_posix(&selected_path, "must-not-appear", "wrong-identity");
        assert_eq!(mismatch.status, "unavailable");
        assert_eq!(mismatch.reason, "terminal-identity-changed");

        let delivered = write_posix(&selected_path, "exact-pty-marker", &identity);
        assert_eq!(delivered.status, "delivered");
        let selected_output = read_pty(&mut selected);
        assert!(selected_output.contains("exact-pty-marker"));
        assert!(!selected_output.contains("must-not-appear"));
        assert!(read_pty(&mut other).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn exact_windows_console_is_revalidated_before_write() {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        use std::time::Duration;

        fn write_after_transient_detach(pid: u32, message: &str, expected: &str) -> Delivery {
            let mut result = write_windows(pid, message, expected);
            for _ in 0..49 {
                if result.reason != "target-has-no-attachable-console" {
                    return result;
                }
                std::thread::sleep(Duration::from_millis(100));
                result = write_windows(pid, message, expected);
            }
            result
        }

        let mut child = Command::new("cmd.exe")
            .args(["/d", "/c", "ping -n 30 127.0.0.1 >NUL"])
            .creation_flags(0x0000_0010)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let result = std::panic::catch_unwind(|| {
            let probe = (0..50)
                .find_map(|_| {
                    let probe = probe_windows(pid, "");
                    if probe.status == "delivered" {
                        Some(probe)
                    } else {
                        std::thread::sleep(Duration::from_millis(100));
                        None
                    }
                })
                .expect("Windows target console was not attachable");
            assert!(!probe.identity.is_empty());

            let mismatch = write_after_transient_detach(pid, "must-not-write", "wrong-console");
            assert_eq!(mismatch.status, "unavailable");
            assert_eq!(mismatch.reason, "console-identity-changed");

            let delivered = write_after_transient_detach(
                pid,
                "MEMORY_SUPERVISOR_CONSOLE_CANARY",
                &probe.identity,
            );
            assert_eq!(delivered.status, "delivered");
            assert_eq!(delivered.identity, probe.identity);
        });
        let _ = child.kill();
        let _ = child.wait();
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }
}
