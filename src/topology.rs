//! Co-tenancy topology: the third adapter axis. The OS-platform adapter measures one kernel and the
//! AI-CLI adapter integrates one CLI; this layer classifies how several kernels on one physical
//! machine relate — whether they compete for dynamically shared physical RAM, and whether their
//! rendezvous directory actually proves they share it. See `docs/guides/federation-topology.md`.

use std::env;
use std::fs;
use std::path::Path;

/// How this kernel sits on the physical machine.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Topology {
    /// Bare metal, or a guest whose host is not ours to coordinate with.
    Native,
    /// A WSL2 Linux guest that balloons dynamic RAM against a Windows host.
    Wsl2,
    /// A container sharing the host kernel and its RAM through a cgroup.
    Container,
    /// A hypervisor VM guest with a fixed memory slice — isolated, self-only.
    VmFixed,
    /// A hypervisor VM guest with dynamic (ballooned) memory — a co-tenant of its host.
    VmDynamic,
    /// A cloud or type-1 VM instance whose host is not manageable — treated like native.
    Cloud,
}

impl Topology {
    pub fn name(self) -> &'static str {
        match self {
            Topology::Native => "native",
            Topology::Wsl2 => "wsl2",
            Topology::Container => "container",
            Topology::VmFixed => "vm-fixed",
            Topology::VmDynamic => "vm-dynamic",
            Topology::Cloud => "cloud",
        }
    }

    fn from_name(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "native" => Some(Topology::Native),
            "wsl2" | "wsl" => Some(Topology::Wsl2),
            "container" => Some(Topology::Container),
            "vm" | "vmfixed" | "vm-fixed" => Some(Topology::VmFixed),
            "vmdynamic" | "vm-dynamic" => Some(Topology::VmDynamic),
            "cloud" => Some(Topology::Cloud),
            _ => None,
        }
    }

    /// Does this kernel compete for dynamically shared physical RAM with reachable peers, so its
    /// admission decision must federate? A fixed VM and a cloud instance are isolated and answer no;
    /// failing to know resolves to no (self-only), never a wrong cross-kernel action.
    pub fn is_co_tenant(self) -> bool {
        matches!(
            self,
            Topology::Wsl2 | Topology::Container | Topology::VmDynamic
        )
    }

    pub fn describe(self) -> String {
        match self {
            Topology::Native => "native host; no co-resident kernels to coordinate".to_owned(),
            Topology::Wsl2 => {
                "WSL2 Linux guest, co-tenant with the Windows host over shared physical RAM"
                    .to_owned()
            }
            Topology::Container => {
                "container sharing the host kernel and its RAM through a cgroup".to_owned()
            }
            Topology::VmFixed => {
                "hypervisor VM guest with a fixed memory slice; isolated, self-only".to_owned()
            }
            Topology::VmDynamic => {
                "hypervisor VM guest with dynamic (ballooned) memory, co-tenant with the host"
                    .to_owned()
            }
            Topology::Cloud => {
                "cloud/type-1 instance; host is not coordinable, treated as native".to_owned()
            }
        }
    }
}

/// Detect the co-tenancy topology, best-effort, failing safe to `Native`.
pub fn detect() -> Topology {
    if let Ok(forced) = env::var("MEMORY_SUPERVISOR_FORCE_TOPOLOGY")
        && let Some(topology) = Topology::from_name(&forced)
    {
        return topology;
    }
    if cfg!(target_os = "macos") || cfg!(windows) {
        // A Windows/macOS kernel is either bare metal or a VM guest. WSL is never here (it is
        // Linux). Cloud/VM detection is best-effort; ambiguity resolves to native.
        return dmi_topology().unwrap_or(Topology::Native);
    }
    if fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .to_lowercase()
        .contains("microsoft")
    {
        return Topology::Wsl2;
    }
    if is_container() {
        return Topology::Container;
    }
    dmi_topology().unwrap_or(Topology::Native)
}

fn is_container() -> bool {
    if Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists() {
        return true;
    }
    fs::read_to_string("/proc/1/cgroup").is_ok_and(|cgroup| {
        cgroup.contains("docker") || cgroup.contains("containerd") || cgroup.contains("kubepods")
    })
}

/// Classify VM vs cloud from firmware identity (Linux DMI; elsewhere `None`).
fn dmi_topology() -> Option<Topology> {
    let read = |name: &str| {
        fs::read_to_string(format!("/sys/class/dmi/id/{name}"))
            .ok()
            .map(|value| value.trim().to_lowercase())
    };
    let haystack = format!(
        "{} {}",
        read("sys_vendor").unwrap_or_default(),
        read("product_name").unwrap_or_default()
    );
    let cloud = [
        "amazon",
        "ec2",
        "google",
        "digitalocean",
        "alibaba",
        "oracle",
    ];
    if cloud.iter().any(|marker| haystack.contains(marker)) {
        return Some(Topology::Cloud);
    }
    let vm = [
        "virtualbox",
        "vmware",
        "kvm",
        "qemu",
        "parallels",
        "hyper-v",
        "microsoft corporation",
        "bochs",
        "xen",
    ];
    if vm.iter().any(|marker| haystack.contains(marker)) {
        return Some(if vm_has_balloon() {
            Topology::VmDynamic
        } else {
            Topology::VmFixed
        });
    }
    None
}

/// A memory balloon device means the hypervisor can reclaim or add this guest's RAM at runtime, so
/// the guest competes with the host for physical RAM — a dynamic co-tenant rather than a fixed,
/// isolated slice. The balloon driver is loaded exactly when the hypervisor exposes the device;
/// its absence is a fixed slice. (Detected on a Linux guest; a Windows/macOS guest resolves to the
/// fixed, self-only default, which is safe.)
fn vm_has_balloon() -> bool {
    fs::read_to_string("/proc/modules").is_ok_and(|modules| {
        [
            "virtio_balloon",
            "hv_balloon",
            "vmw_balloon",
            "xen_balloon",
            "prl_balloon",
        ]
        .iter()
        .any(|driver| modules.contains(driver))
    }) || fs::read_dir("/sys/bus/virtio/devices").is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            fs::read_to_string(entry.path().join("device")).is_ok_and(|id| id.trim() == "0x0005")
        })
    })
}

/// Is the rendezvous directory on a host-local filesystem, so that sharing it proves co-residency?
/// A network filesystem (NFS, SMB/CIFS) is not co-residency proof: unrelated physical machines can
/// mount it, and federating across it would couple kernels that share no RAM. WSL2's `/mnt/c` is a
/// 9p mount of the host filesystem and is therefore host-local. Unknown resolves to host-local so a
/// missing stat never silently drops a legitimate WSL2 or container peer.
pub fn channel_is_host_local(directory: &Path) -> bool {
    // Diagnostic/test override: force the co-residency verdict without a real network mount, so the
    // network->self path is exercisable and an operator can scope a channel they know is remote.
    if let Ok(forced) = env::var("MEMORY_SUPERVISOR_FORCE_CHANNEL_LOCALITY") {
        match forced.trim().to_lowercase().as_str() {
            "network" | "remote" | "false" => return false,
            "local" | "host" | "true" => return true,
            _ => {}
        }
    }
    #[cfg(unix)]
    {
        let mut probe = directory;
        loop {
            if let Some(host_local) = statfs_host_local(probe) {
                return host_local;
            }
            match probe.parent() {
                Some(parent) if parent != probe => probe = parent,
                _ => return true,
            }
        }
    }
    #[cfg(windows)]
    {
        windows_path_is_host_local(directory)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = directory;
        true
    }
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn linux_filesystem_is_host_local(fs_type: u64) -> bool {
    // Network filesystem magics; everything else (ext4, tmpfs, overlay, 9p) is host-local.
    !matches!(
        fs_type,
        0x6969 | 0x517B | 0xFF53_4D42 | 0xFE53_4D42 | 0x564C
    )
}

#[cfg(any(target_os = "macos", test))]
fn macos_filesystem_is_host_local(flags: u64) -> bool {
    flags & 0x0000_1000 != 0 // Darwin MNT_LOCAL
}

#[cfg(all(unix, not(target_os = "macos")))]
fn statfs_host_local(directory: &Path) -> Option<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(directory.as_os_str().as_bytes()).ok()?;
    // SAFETY: `stat` is zeroed then filled by `statfs`; `path` is a valid NUL-terminated C string.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some(linux_filesystem_is_host_local(stat.f_type as u64))
}

#[cfg(target_os = "macos")]
fn statfs_host_local(directory: &Path) -> Option<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(directory.as_os_str().as_bytes()).ok()?;
    // SAFETY: `stat` is zeroed then filled by `statfs`; `path` is a valid NUL-terminated C string.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some(macos_filesystem_is_host_local(stat.f_flags as u64))
}

#[cfg(any(windows, test))]
fn windows_drive_is_host_local(kind: u32) -> bool {
    kind != 4 // Win32 DRIVE_REMOTE
}

#[cfg(windows)]
fn windows_path_is_host_local(directory: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    let raw = directory.to_string_lossy();
    let path = raw.strip_prefix(r"\\?\").unwrap_or(raw.as_ref());
    if path.starts_with(r"UNC\") || path.starts_with(r"\\") {
        return false;
    }
    let bytes = path.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return true;
    }
    let root = format!("{}:\\", bytes[0] as char);
    let mut wide: Vec<_> = std::ffi::OsStr::new(&root).encode_wide().collect();
    wide.push(0);
    // SAFETY: `wide` is a valid NUL-terminated UTF-16 drive-root string.
    windows_drive_is_host_local(unsafe { GetDriveTypeW(wide.as_ptr()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_forced(value: &str, body: impl FnOnce()) {
        let previous = env::var("MEMORY_SUPERVISOR_FORCE_TOPOLOGY").ok();
        unsafe { env::set_var("MEMORY_SUPERVISOR_FORCE_TOPOLOGY", value) };
        body();
        match previous {
            Some(value) => unsafe { env::set_var("MEMORY_SUPERVISOR_FORCE_TOPOLOGY", value) },
            None => unsafe { env::remove_var("MEMORY_SUPERVISOR_FORCE_TOPOLOGY") },
        }
    }

    #[test]
    fn co_tenant_only_for_shared_dynamic_ram() {
        assert!(Topology::Wsl2.is_co_tenant());
        assert!(Topology::Container.is_co_tenant());
        assert!(Topology::VmDynamic.is_co_tenant());
        assert!(!Topology::Native.is_co_tenant());
        assert!(!Topology::VmFixed.is_co_tenant());
        assert!(!Topology::Cloud.is_co_tenant());
    }

    #[test]
    fn forced_topology_overrides_detection_and_describes_itself() {
        for (value, expected) in [
            ("wsl2", Topology::Wsl2),
            ("container", Topology::Container),
            ("vm-fixed", Topology::VmFixed),
            ("vm-dynamic", Topology::VmDynamic),
            ("cloud", Topology::Cloud),
            ("native", Topology::Native),
        ] {
            with_forced(value, || {
                assert_eq!(detect(), expected);
                assert_eq!(detect().name(), value);
                assert!(!expected.describe().is_empty());
            });
        }
        // The bare `vm` alias resolves to the isolated fixed case.
        with_forced("vm", || assert_eq!(detect(), Topology::VmFixed));
    }

    #[test]
    fn a_local_directory_is_host_local_co_residency_proof() {
        assert!(channel_is_host_local(&env::temp_dir()));
    }

    #[test]
    fn native_filesystem_signals_reject_network_channels() {
        assert!(!linux_filesystem_is_host_local(0x6969));
        assert!(!linux_filesystem_is_host_local(0xFF53_4D42));
        assert!(linux_filesystem_is_host_local(0x9FA0)); // WSL2 9p
        assert!(!macos_filesystem_is_host_local(0));
        assert!(macos_filesystem_is_host_local(0x1000));
        assert!(!windows_drive_is_host_local(4));
        assert!(windows_drive_is_host_local(3));
    }
}
