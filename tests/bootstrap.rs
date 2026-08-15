#![cfg(unix)]

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_directory() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "memory-supervisor-bootstrap-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn release_fixture(root: &Path, label: &str, exit_code: i32) -> (PathBuf, PathBuf) {
    let fixture = root.join(format!("fixture-{label}"));
    let source = fixture.join("memory-supervisor");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("version"), format!("{label}\n")).unwrap();
    fs::write(
        source.join("install.sh"),
        format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' '{label}' > \"$MEMORY_SUPERVISOR_INSTALL_ROOT/install-ran\"\nexit {exit_code}\n"
        ),
    )
    .unwrap();

    let archive = root.join(format!("{label}.tar.gz"));
    let status = Command::new("tar")
        .args(["-czf"])
        .arg(&archive)
        .arg("-C")
        .arg(&fixture)
        .arg("memory-supervisor")
        .status()
        .unwrap();
    assert!(status.success());
    let digest = Sha256::digest(fs::read(&archive).unwrap());
    let checksum = root.join(format!("{label}.tar.gz.sha256"));
    fs::write(
        &checksum,
        format!("{digest:x}  memory-supervisor-source.tar.gz\n"),
    )
    .unwrap();
    (archive, checksum)
}

fn run_bootstrap(root: &Path, archive: &Path, checksum: &Path) -> bool {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Command::new("/bin/sh")
        .arg(repository.join("bootstrap.sh"))
        .env("HOME", root.join("home"))
        .env("MEMORY_SUPERVISOR_INSTALL_ROOT", root.join("installed"))
        .env("MEMORY_SUPERVISOR_SOURCE_ARCHIVE_FILE", archive)
        .env("MEMORY_SUPERVISOR_SOURCE_ARCHIVE_SHA256_FILE", checksum)
        .status()
        .unwrap()
        .success()
}

#[test]
fn release_bootstrap_installs_updates_and_rolls_back_without_git() {
    let root = temp_directory();
    fs::create_dir_all(root.join("home")).unwrap();

    let (first, first_hash) = release_fixture(&root, "first", 0);
    assert!(run_bootstrap(&root, &first, &first_hash));
    let installed = root.join("installed");
    assert_eq!(
        fs::read_to_string(installed.join("version")).unwrap(),
        "first\n"
    );
    assert_eq!(
        fs::read_to_string(installed.join("install-ran")).unwrap(),
        "first\n"
    );
    assert!(
        installed
            .join(".memory-supervisor-release-source")
            .is_file()
    );
    assert!(!installed.join(".git").exists());

    let (second, second_hash) = release_fixture(&root, "second", 0);
    assert!(run_bootstrap(&root, &second, &second_hash));
    assert_eq!(
        fs::read_to_string(installed.join("version")).unwrap(),
        "second\n"
    );

    let (broken, broken_hash) = release_fixture(&root, "broken", 42);
    assert!(!run_bootstrap(&root, &broken, &broken_hash));
    assert_eq!(
        fs::read_to_string(installed.join("version")).unwrap(),
        "second\n"
    );

    fs::remove_dir_all(root).unwrap();
}
