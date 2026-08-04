//! U25 (D-JPK-PLATFORM1=A): jetpack's tier-1 platform contract.
//!
//! jetos stays Linux-only for now, but jetpack itself is a native product on
//! Linux, macOS, and Windows. Keep target spelling, executable lookup, and PATH
//! composition in one small module so platform handling does not drift across
//! providers, the hangar envelope, services, secrets, and trust code.

use std::process::Command;

pub const OS_LINUX: &str = "linux";
pub const OS_MACOS: &str = "macos";
pub const OS_WINDOWS: &str = "windows";

pub const ARCH_X64: &str = "x86_64";
pub const ARCH_ARM64: &str = "aarch64";

pub const TIER_ONE_OSES: &[&str] = &[OS_LINUX, OS_MACOS, OS_WINDOWS];
pub const TIER_ONE_ARCHES: &[&str] = &[ARCH_X64, ARCH_ARM64];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformKey {
    pub arch: String,
    pub os: String,
}

impl PlatformKey {
    pub fn host() -> PlatformKey {
        PlatformKey {
            arch: std::env::consts::ARCH.to_string(),
            os: std::env::consts::OS.to_string(),
        }
    }

    pub fn new(arch: &str, os: &str) -> Option<PlatformKey> {
        if !TIER_ONE_ARCHES.contains(&arch) || !TIER_ONE_OSES.contains(&os) {
            return None;
        }
        Some(PlatformKey {
            arch: arch.to_string(),
            os: os.to_string(),
        })
    }

    pub fn envelope_key(&self) -> String {
        format!("{}-{}", self.arch, self.os)
    }

    pub fn is_tier_one(&self) -> bool {
        TIER_ONE_ARCHES.contains(&self.arch.as_str()) && TIER_ONE_OSES.contains(&self.os.as_str())
    }
}

pub fn host_key() -> String {
    PlatformKey::host().envelope_key()
}

pub fn path_separator_for_os(os: &str) -> char {
    if os == OS_WINDOWS {
        ';'
    } else {
        ':'
    }
}

pub fn path_separator() -> char {
    path_separator_for_os(std::env::consts::OS)
}

/// The minimal host-independent PATH available to clean child processes.
/// This is platform plumbing, not inherited user state; declared Jet bins are
/// prepended by the caller.
pub fn clean_path() -> &'static str {
    if cfg!(windows) {
        r"C:\Windows\System32;C:\Windows"
    } else {
        "/usr/bin:/bin"
    }
}

pub fn exe_suffix_for_os(os: &str) -> &'static str {
    if os == OS_WINDOWS {
        ".exe"
    } else {
        ""
    }
}

pub fn exe_suffix() -> &'static str {
    exe_suffix_for_os(std::env::consts::OS)
}

pub fn shell_command(script: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", script]);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_one_os_set_is_linux_macos_windows() {
        assert_eq!(TIER_ONE_OSES, [OS_LINUX, OS_MACOS, OS_WINDOWS]);
    }

    #[test]
    fn platform_key_renders_arch_dash_os() {
        let key = PlatformKey::new(ARCH_X64, OS_WINDOWS).unwrap();
        assert_eq!(key.envelope_key(), "x86_64-windows");
        assert!(key.is_tier_one());
        assert!(PlatformKey::new("sparc", OS_LINUX).is_none());
    }

    #[test]
    fn platform_path_and_exe_rules_are_explicit() {
        assert_eq!(path_separator_for_os(OS_WINDOWS), ';');
        assert_eq!(path_separator_for_os(OS_LINUX), ':');
        assert_eq!(path_separator_for_os(OS_MACOS), ':');
        assert_eq!(exe_suffix_for_os(OS_WINDOWS), ".exe");
        assert_eq!(exe_suffix_for_os(OS_LINUX), "");
        assert_eq!(exe_suffix_for_os(OS_MACOS), "");
    }
}
