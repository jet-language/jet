//! Native linker selection for rustc-backed builds.
//!
//! Explicit tool choices win. On the host, the default prefers mold and then
//! lld through a C driver; direct linker invocation is not portable because
//! the driver supplies the system runtime libraries. Cross targets keep
//! rustc's target-specific system linker unless the user names one.

use std::process::{Command, Stdio};
use std::sync::OnceLock;

const FAST_BACKENDS: [&str; 2] = ["mold", "lld"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    driver: Option<String>,
    identity_driver: Option<String>,
    backend: Option<String>,
    backend_program: Option<String>,
    explicit: bool,
}

impl Selection {
    fn system() -> Self {
        Self::system_with_identity(None)
    }

    fn system_with_identity(identity_driver: Option<String>) -> Self {
        Self {
            driver: None,
            identity_driver,
            backend: None,
            backend_program: None,
            explicit: false,
        }
    }

    fn explicit(driver: String) -> Self {
        Self {
            driver: Some(driver.clone()),
            identity_driver: Some(driver),
            backend: None,
            backend_program: None,
            explicit: true,
        }
    }

    fn auto(driver: &str, backend: &str, backend_program: &str) -> Self {
        Self {
            driver: Some(driver.to_string()),
            identity_driver: Some(driver.to_string()),
            backend: Some(backend.to_string()),
            backend_program: Some(backend_program.to_string()),
            explicit: false,
        }
    }

    /// Flags for the user crate. RuntimeCache removes final-link-only flags
    /// before compiling reusable runtime/Core rlibs.
    ///
    /// Live in the `jet` binary (`CmdCompile`) and in this module's unit
    /// tests. The library target pulls `NativeLinker` in only for `label()`
    /// (`BudgetProviders` reports the selected linker), so the lib build sees
    /// no caller for this one.
    #[allow(dead_code)]
    pub fn rustc_args(&self) -> Vec<String> {
        let Some(driver) = self.driver.as_deref() else {
            return Vec::new();
        };
        let mut args = vec!["-C".to_string(), format!("linker={driver}")];
        if let Some(backend) = self.backend.as_deref() {
            args.extend(["-C".to_string(), format!("link-arg=-fuse-ld={backend}")]);
        }
        args
    }

    pub fn identity(&self) -> (Option<&str>, Option<&str>, Option<&str>) {
        (
            self.identity_driver.as_deref(),
            self.backend.as_deref(),
            self.backend_program.as_deref(),
        )
    }

    /// Stable human-readable identity used in timing and cache evidence.
    pub fn label(&self) -> String {
        let (driver, backend, _) = self.identity();
        match (driver, backend) {
            (Some(driver), Some(backend)) => format!("{backend} via {driver}"),
            (Some(driver), None) if self.explicit => format!("explicit:{driver}"),
            (Some(driver), None) => format!("system via {driver}"),
            _ => "system".to_string(),
        }
    }
}

/// Select the linker for one native rustc path.
pub fn for_target(cross_target: Option<&str>) -> Selection {
    if let Some(linker) = explicit_linker() {
        return Selection::explicit(linker);
    }
    if cross_target.is_some() {
        return Selection::system();
    }
    static HOST_SELECTION: OnceLock<Selection> = OnceLock::new();
    HOST_SELECTION.get_or_init(detect_host).clone()
}

fn explicit_linker() -> Option<String> {
    explicit_linker_from(|name| std::env::var(name).ok())
}

fn explicit_linker_from(mut get: impl FnMut(&str) -> Option<String>) -> Option<String> {
    ["RUSTC_LINKER", "CC"]
        .iter()
        .copied()
        .find_map(|name| get(name).filter(|value| !value.trim().is_empty()))
}

/// Identity-only query. An empty `rustc_args()` keeps rustc's target-specific
/// system-linker choice intact when no fast backend is available.
fn system_linker() -> Option<String> {
    Command::new("rustc")
        .args(["--print", "linker"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn detect_host() -> Selection {
    let Some(backend) = preferred_backend(|backend| match backend {
        "mold" => command_available("mold"),
        "lld" => command_available("lld") || command_available("ld.lld"),
        _ => false,
    }) else {
        return Selection::system_with_identity(system_linker());
    };
    let backend_program = if backend == "lld" && !command_available("lld") {
        "ld.lld"
    } else {
        backend
    };
    let drivers = if backend == "mold" {
        ["clang", "cc"]
    } else {
        ["cc", "clang"]
    };
    let Some(driver) = drivers
        .iter()
        .copied()
        .find(|program| command_available(program))
    else {
        return Selection::system();
    };
    Selection::auto(driver, backend, backend_program)
}

fn preferred_backend(available: impl Fn(&str) -> bool) -> Option<&'static str> {
    FAST_BACKENDS
        .iter()
        .copied()
        .find(|backend| available(backend))
}

fn command_available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::{explicit_linker_from, preferred_backend, Selection};

    #[test]
    fn preferred_backend_is_ordered_mold_then_lld() {
        assert_eq!(preferred_backend(|name| name == "lld"), Some("lld"));
        assert_eq!(
            preferred_backend(|name| matches!(name, "mold" | "lld")),
            Some("mold")
        );
        assert_eq!(preferred_backend(|_| false), None);
    }

    #[test]
    fn fast_linker_selection_emits_driver_and_backend_flags() {
        let selection = Selection::auto("cc", "lld", "lld");
        assert_eq!(
            selection.rustc_args(),
            vec![
                "-C".to_string(),
                "linker=cc".to_string(),
                "-C".to_string(),
                "link-arg=-fuse-ld=lld".to_string(),
            ]
        );
        assert_eq!(selection.label(), "lld via cc");
    }

    #[test]
    fn explicit_linker_prefers_rustc_linker_then_cc_and_skips_blanks() {
        let selection = explicit_linker_from(|name| match name {
            "RUSTC_LINKER" => Some("  ".into()),
            "CC" => Some("clang".into()),
            _ => None,
        });
        assert_eq!(selection.as_deref(), Some("clang"));

        let selection = explicit_linker_from(|name| match name {
            "RUSTC_LINKER" => Some("custom-linker".into()),
            "CC" => Some("cc".into()),
            _ => None,
        });
        assert_eq!(selection.as_deref(), Some("custom-linker"));
    }

    #[test]
    fn system_identity_does_not_override_rustc_linker_choice() {
        let selection = Selection::system_with_identity(Some("cc".into()));
        assert!(selection.rustc_args().is_empty());
        assert_eq!(selection.identity(), (Some("cc"), None, None));
        assert_eq!(selection.label(), "system via cc");
    }
}
