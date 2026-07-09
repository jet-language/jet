const CACHYOS_KERNEL_PACKAGE: &str = "cachyos-kernel";
const SYSTEMD_INIT_PACKAGE: &str = "systemd";
const GNOME_DESKTOP_PACKAGES: [&str; 3] = ["gdm", "gnome-session", "gnome-shell"];
const VM_TOOLS: [&str; 11] = [
    "qemu-system-x86_64",
    "qemu-img",
    "xorriso",
    "limine",
    "sfdisk",
    "blockdev",
    "mkfs.ext4",
    "mkfs.vfat",
    "mmd",
    "mcopy",
    "zstd",
];
const VM_GUEST_PROOF_MARKER: &str = "JETOS_GUEST_PROOF:";
const VM_PROOF_TIMEOUT_MS: u64 = 300_000;

#[derive(Clone)]
pub struct OsFlags {
    pub fixtures: Option<PathBuf>,
    pub offline: bool,
    pub name: Option<String>,
    pub manual_disk: Option<String>,
    pub disk: Option<String>,
    pub json: bool,
    pub assume_yes: bool,
    /// `--real` VM tier: the hidden system backend realizes kernel/init/
    /// desktop from the pinned package set, so the plumbing generation skips
    /// its first-party boot-package auto-requirements for defaulted options.
    pub real_tier: bool,
}

struct Target {
    config: PathBuf,
    host: String,
}

struct Generation {
    name: String,
    host: String,
    path: PathBuf,
    created_at: u64,
}

struct BootProfile {
    loader: String,
    kernel: String,
    init: String,
    initrd_modules: Vec<String>,
}

impl BootProfile {
    fn to_json(&self) -> String {
        let modules = self
            .initrd_modules
            .iter()
            .map(|m| JSON::quote(m))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"loader\":{},\"kernel\":{},\"init\":{},\"initrd_modules\":[{}]}}",
            JSON::quote(&self.loader),
            JSON::quote(&self.kernel),
            JSON::quote(&self.init),
            modules
        )
    }
}
