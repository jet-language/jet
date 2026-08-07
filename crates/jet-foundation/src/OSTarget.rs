//! D-OSTARGET1=A (ratified 2026-07-01, c134): native OS platform gating for
//! `#Extern` backends. A second, mutually-exclusive axis of the same
//! `#Target(...)` marker family that `WebPartition::WebBucket` already uses
//! for the web bucket ceiling (`Wasm`/`JS`) and the default-web-backend
//! marker (`Web`) — `OS.Linux`/`OS.MacOS`/`OS.Windows` picks which native
//! platform an `impl` block compiles for (I8: one marker family, mutually
//! exclusive values, never a second marker meaning the same job).

use crate::Syntax;

/// Native OS bucket an `impl` block is gated to (D-OSTARGET1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OSTarget {
    Linux,
    MacOS,
    Windows,
}

impl OSTarget {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            Syntax::TARGET_OS_LINUX => Some(OSTarget::Linux),
            Syntax::TARGET_OS_MACOS => Some(OSTarget::MacOS),
            Syntax::TARGET_OS_WINDOWS => Some(OSTarget::Windows),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            OSTarget::Linux => Syntax::TARGET_OS_LINUX,
            OSTarget::MacOS => Syntax::TARGET_OS_MACOS,
            OSTarget::Windows => Syntax::TARGET_OS_WINDOWS,
        }
    }

    /// Map a rustc `--target=<triple>` string to the OS it builds for.
    /// `None` for a triple with no recognizable OS component (e.g.
    /// `wasm32-unknown-unknown`, or Jet's own `web` pseudo-target) — the
    /// caller falls back to the host OS in that case.
    pub fn from_triple(triple: &str) -> Option<Self> {
        if triple.contains("windows") {
            Some(OSTarget::Windows)
        } else if triple.contains("apple") || triple.contains("darwin") {
            Some(OSTarget::MacOS)
        } else if triple.contains("linux") {
            Some(OSTarget::Linux)
        } else {
            None
        }
    }

    /// The OS this build of `jet` itself runs on — the default active OS
    /// bucket when `--target=<triple>` is omitted.
    pub fn host() -> Self {
        match std::env::consts::OS {
            "windows" => OSTarget::Windows,
            "macos" => OSTarget::MacOS,
            _ => OSTarget::Linux,
        }
    }

    /// The active OS bucket for codegen: `--target=<triple>` wins when it
    /// names a recognizable OS, else the host OS. Reuses the existing
    /// `--target=` cross-compile flag (E2-M15) — no second flag for this.
    pub fn active(triple: Option<&str>) -> Self {
        triple
            .and_then(Self::from_triple)
            .unwrap_or_else(Self::host)
    }
}

/// E-OSTARGET-MIXED-AXIS: a `#Target(OS.*)`-gated item also carries a
/// web-axis marker (a per-method `#Wasm`/`#JS`/`#WasmExport` override, or an
/// enclosing web bucket ceiling) — two different compilation axes, one item.
pub fn os_target_mixed_axis(
    item: &str,
    os: OSTarget,
    web: &str,
    span: Option<crate::Diagnostics::Span>,
) -> crate::Diagnostics::Diagnostic {
    crate::Diagnostics::Diagnostic::error(
        "E-OSTARGET-MIXED-AXIS",
        format!(
            "`#{}(OS.{})` can't combine with `#{}({})` on `{item}`",
            Syntax::MARKER_TARGET,
            os.name(),
            Syntax::MARKER_TARGET,
            web,
        ),
        "the OS axis (OS.Linux/OS.MacOS/OS.Windows, native platform gating) and the web axis (Wasm/JS/Web, D-WASM1's browser partition) are mutually exclusive — one item can't compile for both a specific native OS and a web bucket"
            .to_string(),
        format!("pick one axis: remove the `#{}(OS.{})` marker or the web-axis marker", Syntax::MARKER_TARGET, os.name()),
        span,
    )
}

/// E-OSTARGET-UNMATCHED-CALL: a function/method not itself gated to match
/// takes or returns a value of a type whose `impl` is `#Target(OS.*)`-gated
/// — reachable from any build, it would call a method that `impl` supplies.
pub fn os_target_unmatched_call(
    caller: &str,
    gated_type: &str,
    os: OSTarget,
    span: Option<crate::Diagnostics::Span>,
) -> crate::Diagnostics::Diagnostic {
    crate::Diagnostics::Diagnostic::error(
        "E-OSTARGET-UNMATCHED-CALL",
        format!(
            "`{caller}` uses `{gated_type}`, whose `impl` is gated to `#{}(OS.{})`, without itself being gated to match",
            Syntax::MARKER_TARGET,
            os.name(),
        ),
        "an OS-gated impl only exists in the build for that OS; code reachable on other platforms would hit a missing method, so this is caught at compile time, not left to fail as a link (or a raw rustc) error"
            .to_string(),
        format!(
            "only use `{gated_type}` from inside an `impl` already gated to `#{}(OS.{})`, or move `{caller}`'s body into one",
            Syntax::MARKER_TARGET,
            os.name(),
        ),
        span,
    )
}

/// E-OSTARGET-BUILD-CONTEXT (D-OSTARGET2=B): a `$if … == { }` OS
/// dispatch whose subject is not `build.os`. The comptime dispatch that reaches
/// OS-gated `impl`s only branches on the compiler-known `build.os` value.
pub fn os_target_build_context(
    span: Option<crate::Diagnostics::Span>,
) -> crate::Diagnostics::Diagnostic {
    crate::Diagnostics::Diagnostic::error(
        "E-OSTARGET-BUILD-CONTEXT",
        format!(
            "a `{} {} … == {{ … }}` dispatch branches on `{}.{}`",
            Syntax::COMPTIME_MARK,
            Syntax::KW_IF,
            Syntax::BUILD_INFO,
            Syntax::BUILD_INFO_OS,
        ),
        format!(
            "`{}.{}` is the one compiler-known value this dispatch folds on — it selects the arm matching the build's target OS at compile time (D-OSTARGET2)",
            Syntax::BUILD_INFO,
            Syntax::BUILD_INFO_OS,
        ),
        format!(
            "write `{} {} {}.{} == {{ .{} -> … .{} -> … .{} -> … }}`, or use a plain runtime `{}` for a value that isn't known at compile time",
            Syntax::COMPTIME_MARK,
            Syntax::KW_IF,
            Syntax::BUILD_INFO,
            Syntax::BUILD_INFO_OS,
            Syntax::TARGET_OS_LINUX,
            Syntax::TARGET_OS_MACOS,
            Syntax::TARGET_OS_WINDOWS,
            Syntax::KW_IF,
        ),
        span,
    )
}

/// E-OSTARGET-DISPATCH-ARM (D-OSTARGET2=B): an arm head of a `#Known if
/// build.os == { }` dispatch is not a bare OS variant (`.Linux`/`.MacOS`/
/// `.Windows`), or repeats one.
pub fn os_target_dispatch_arm(
    found: &str,
    span: Option<crate::Diagnostics::Span>,
) -> crate::Diagnostics::Diagnostic {
    crate::Diagnostics::Diagnostic::error(
        "E-OSTARGET-DISPATCH-ARM",
        format!(
            "`{found}` is not an OS arm — a `{}.{}` dispatch matches `.{}`, `.{}`, or `.{}`",
            Syntax::BUILD_INFO,
            Syntax::BUILD_INFO_OS,
            Syntax::TARGET_OS_LINUX,
            Syntax::TARGET_OS_MACOS,
            Syntax::TARGET_OS_WINDOWS,
        ),
        format!(
            "each arm gates code for exactly one native OS, so its head is a bare, payload-free OS variant — the same set `#{}(OS.*)` uses — and each OS appears at most once",
            Syntax::MARKER_TARGET,
        ),
        format!(
            "write `.{} -> …`, `.{} -> …`, or `.{} -> …` (add an `else -> …` for a shared fallback)",
            Syntax::TARGET_OS_LINUX,
            Syntax::TARGET_OS_MACOS,
            Syntax::TARGET_OS_WINDOWS,
        ),
        span,
    )
}

/// E-OSTARGET-DISPATCH-EXHAUSTIVE (D-OSTARGET2=B): a `#Known if build.os ==
/// { }` dispatch's arms leave some target OS uncovered and there is no `else`.
/// Build-independent: enforced regardless of the current `--target` so the same
/// source compiles (or fails) identically on every platform.
pub fn os_target_dispatch_exhaustive(
    missing: &[&str],
    span: Option<crate::Diagnostics::Span>,
) -> crate::Diagnostics::Diagnostic {
    let list = missing.join(", ");
    crate::Diagnostics::Diagnostic::error(
        "E-OSTARGET-DISPATCH-EXHAUSTIVE",
        format!(
            "this `{}.{}` dispatch doesn't cover every target OS — missing: {list}",
            Syntax::BUILD_INFO,
            Syntax::BUILD_INFO_OS,
        ),
        "a build can target any native OS, so the dispatch must handle each one — otherwise a build for a missing OS would have no arm to run"
            .to_string(),
        format!(
            "add an arm for each missing OS ({list}), or an `else -> …` catch-all",
        ),
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_name_roundtrip() {
        for os in [OSTarget::Linux, OSTarget::MacOS, OSTarget::Windows] {
            assert_eq!(OSTarget::parse(os.name()), Some(os));
        }
        assert_eq!(OSTarget::parse("Bsd"), None);
    }

    #[test]
    fn from_triple_maps_known_platforms() {
        assert_eq!(
            OSTarget::from_triple("x86_64-unknown-linux-gnu"),
            Some(OSTarget::Linux)
        );
        assert_eq!(
            OSTarget::from_triple("aarch64-apple-darwin"),
            Some(OSTarget::MacOS)
        );
        assert_eq!(
            OSTarget::from_triple("x86_64-pc-windows-msvc"),
            Some(OSTarget::Windows)
        );
        assert_eq!(OSTarget::from_triple("wasm32-unknown-unknown"), None);
    }

    #[test]
    fn active_falls_back_to_host_for_unrecognized_triple() {
        assert_eq!(
            OSTarget::active(Some("wasm32-unknown-unknown")),
            OSTarget::host()
        );
        assert_eq!(OSTarget::active(None), OSTarget::host());
    }
}
