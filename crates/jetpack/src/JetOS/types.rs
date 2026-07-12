use crate::JSON;
use std::path::PathBuf;

pub(super) const CACHYOS_KERNEL_PACKAGE: &str = "cachyos-kernel";
pub(super) const SYSTEMD_INIT_PACKAGE: &str = "systemd";
pub(super) const GNOME_DESKTOP_PACKAGES: [&str; 3] = ["gdm", "gnome-session", "gnome-shell"];
pub(super) const VM_TOOLS: [&str; 11] = [
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
pub(super) const VM_GUEST_PROOF_MARKER: &str = "JETOS_GUEST_PROOF:";
pub(super) const VM_PROOF_TIMEOUT_MS: u64 = 300_000;

#[derive(Clone)]
pub struct OsFlags {
    pub fixtures: Option<PathBuf>,
    pub offline: bool,
    pub name: Option<String>,
    pub manual_disk: Option<String>,
    pub disk: Option<String>,
    pub json: bool,
    pub assume_yes: bool,
    /// `--host <name>` — consumed by the global flag parser (shared with the
    /// Studio surface), threaded here so `jet os import` can see it.
    pub host: Option<String>,
    /// `--real` VM tier: the hidden system backend realizes kernel/init/
    /// desktop from the pinned package set, so the plumbing generation skips
    /// its first-party boot-package auto-requirements for defaulted options.
    pub real_tier: bool,
}

pub(super) struct Target {
    pub(super) config: PathBuf,
    pub(super) host: String,
}

pub(super) struct Generation {
    pub(super) name: String,
    pub(super) host: String,
    pub(super) path: PathBuf,
    pub(super) created_at: u64,
}

pub(super) struct BootProfile {
    pub(super) loader: String,
    pub(super) kernel: String,
    pub(super) init: String,
    pub(super) initrd_modules: Vec<String>,
}

impl BootProfile {
    pub(super) fn to_json(&self) -> String {
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
