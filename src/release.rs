//! Signed release downloads: `calando update` fetches `SHA256SUMS` and its Ed25519 signature,
//! checks them with the public key built into this binary, then takes only files whose SHA-256
//! the signed list names.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The release signing key; the private half is the `CALANDO_RELEASE_SIGNING_KEY` CI secret.
const PUBLIC_KEY: [u8; 32] = [
    0x44, 0xb0, 0x24, 0x71, 0x57, 0x1b, 0xec, 0x36, 0xd0, 0x8c, 0x28, 0x62, 0xda, 0xe2, 0xe2, 0x32,
    0x93, 0x52, 0x93, 0x84, 0x1d, 0xce, 0x56, 0x5b, 0x72, 0x64, 0x59, 0x64, 0x76, 0x53, 0x6e, 0x3e,
];

pub struct Verified {
    pub version: String,
    pub source: PathBuf,
    pub source_checksum: PathBuf,
    pub binary: Option<PathBuf>,
}

/// `SHA256SUMS` is `calando <version>` followed by `sha256sum` lines.
fn parse(list: &str) -> Option<(String, Vec<(String, String)>)> {
    let mut lines = list.lines();
    let version = lines.next()?.strip_prefix("calando ")?.trim().to_owned();
    let entries = lines
        .filter_map(|line| {
            let (hash, name) = line.split_once(char::is_whitespace)?;
            Some((
                name.trim().trim_start_matches('*').to_owned(),
                hash.to_owned(),
            ))
        })
        .collect();
    Some((version, entries))
}

pub fn verify_list(
    list: &[u8],
    signature: &[u8],
) -> Result<(String, Vec<(String, String)>), String> {
    let key = ed25519_compact::PublicKey::new(PUBLIC_KEY);
    let signature = ed25519_compact::Signature::from_slice(signature)
        .map_err(|_| "release signature is malformed".to_owned())?;
    key.verify(list, &signature)
        .map_err(|_| "release signature does not match the Calando release key".to_owned())?;
    parse(&String::from_utf8_lossy(list))
        .ok_or_else(|| "signed release list is malformed".to_owned())
}

fn binary_asset() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("calando-x86_64-pc-windows-msvc.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("calando-x86_64-unknown-linux-gnu")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("calando-aarch64-apple-darwin")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("calando-x86_64-apple-darwin")
    } else {
        None
    }
}

fn download(url: &str, path: &Path) -> Result<(), String> {
    let ok = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" })
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "-fsSL",
            "--max-time",
            "300",
            "-o",
        ])
        .arg(path)
        .arg(url)
        .status()
        .is_ok_and(|status| status.success());
    ok.then_some(())
        .ok_or_else(|| format!("download failed: {url}"))
}

fn checked(
    base: &str,
    name: &str,
    entries: &[(String, String)],
    dir: &Path,
) -> Result<PathBuf, String> {
    let expected = entries
        .iter()
        .find(|(entry, _)| entry == name)
        .map(|(_, hash)| hash.to_ascii_lowercase())
        .ok_or_else(|| format!("signed release list has no {name}"))?;
    let path = dir.join(name);
    download(&format!("{base}/{name}"), &path)?;
    let bytes = fs::read(&path).map_err(|error| format!("{name}: {error}"))?;
    if format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err(format!("{name} does not match the signed release list"));
    }
    Ok(path)
}

/// Downloads and verifies the latest release into `dir`. Refuses an older version than this one.
pub fn fetch(base: &str, dir: &Path) -> Result<Verified, String> {
    let list_path = dir.join("SHA256SUMS");
    let signature_path = dir.join("SHA256SUMS.sig");
    download(&format!("{base}/SHA256SUMS"), &list_path)?;
    download(&format!("{base}/SHA256SUMS.sig"), &signature_path)?;
    let list = fs::read(&list_path).map_err(|error| error.to_string())?;
    let signature = fs::read(&signature_path).map_err(|error| error.to_string())?;
    let (version, entries) = verify_list(&list, &signature)?;
    if crate::update::is_newer(env!("CARGO_PKG_VERSION"), &version) {
        return Err(format!(
            "signed release {version} is older than this Calando"
        ));
    }
    let source_name = if cfg!(windows) {
        "calando-source.zip"
    } else {
        "calando-source.tar.gz"
    };
    let source = checked(base, source_name, &entries, dir)?;
    // bootstrap re-checks the archive against a `sha256sum`-style file.
    let source_checksum = dir.join(format!("{source_name}.sha256"));
    let hash = format!(
        "{:x}",
        Sha256::digest(fs::read(&source).map_err(|e| e.to_string())?)
    );
    fs::write(&source_checksum, format!("{hash}  {source_name}\n")).map_err(|e| e.to_string())?;
    let binary = match binary_asset() {
        Some(name) => {
            let path = checked(base, name, &entries, dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
            }
            Some(path)
        }
        None => None,
    };
    Ok(Verified {
        version,
        source,
        source_checksum,
        binary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signed by the release key; fails if `PUBLIC_KEY` stops matching the CI secret.
    #[test]
    fn a_list_signed_by_the_release_key_verifies() {
        let list = b"calando 0.3.0\nabc  calando-source.tar.gz\n";
        let signature: Vec<u8> = (0..64)
            .map(|i| u8::from_str_radix(&SIGNED[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        let (version, entries) = verify_list(list, &signature).unwrap();
        assert_eq!(version, "0.3.0");
        assert_eq!(
            entries,
            vec![("calando-source.tar.gz".to_owned(), "abc".to_owned())]
        );
        let mut tampered = list.to_vec();
        tampered[9] = b'9';
        assert!(verify_list(&tampered, &signature).is_err());
    }
    const SIGNED: &str = "fb47361b85550fbbabcbaf95dd48d919432679e74ffa15a474026a1f7fdf57cd7329c5b29a022af4fe56ec3549da6e4dfb6cf744105eed3f41fab1d42f934708";

    #[test]
    fn a_list_without_a_valid_signature_is_refused() {
        let list = b"calando 9.9.9\nabc  calando-source.tar.gz\n";
        assert!(verify_list(list, &[0u8; 64]).is_err());
        assert!(verify_list(list, b"short").is_err());
    }

    #[test]
    fn the_signed_list_names_a_version_and_files() {
        let (version, entries) =
            parse("calando 0.3.0\nAB12  calando-source.tar.gz\nCD34 *calando-x86_64-pc-windows-msvc.exe\n")
                .unwrap();
        assert_eq!(version, "0.3.0");
        assert_eq!(entries[1].0, "calando-x86_64-pc-windows-msvc.exe");
        assert!(parse("SHA256SUMS\n").is_none());
    }
}
