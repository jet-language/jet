//! D-META-EFFECT1: the effect facts, owned once.
//!
//! An effect set says what a call touches. That fact does not depend on the
//! stage the call runs at, so it cannot live where only one stage can read
//! it. Sema reads it to check declared bounds; the comptime evaluator reads
//! the same table to decide which tier a call belongs to, instead of keeping
//! a second hard-coded list of its own.
use std::collections::BTreeSet;
/// A primitive effect. Closed, compiler-known set; each Core operation
/// contributes exactly one. Ordered for deterministic diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    Net,
    FS,
    IO,
    DB,
    Time,
    Rand,
    Env,
    Exec,
    Log,
    GPU,
    /// D-FFI-GO1=A: an in-process Go runtime call may block in Go code.
    Go,
    /// D-FFI-JVM1=A: an embedded JVM invocation.
    Java,
    /// D-FFI-DOTNET1=A: an embedded CoreCLR invocation.
    DotNet,
    /// D-FFI-FORTRAN1=A: an in-process ISO_C_BINDING call.
    Fortran,
    /// D-FFI-COBOL1=A: an in-process GnuCOBOL C-ABI call.
    Cobol,
    /// D-FFI-TCL1=A: synchronous in-process Tcl evaluation.
    Tcl,
    /// D-FFI-LUA1=A: synchronous evaluation in a session-owned Lua VM.
    Lua,
    /// D-FFI-ADA1=A: checked call into a GNAT C-ABI export.
    Ada,
    /// D-FFI-PASCAL1=A: call into a FreePascal cdecl library.
    Pascal,
    /// D-FFI-DART1=A: synchronous callback into a Dart-owned isolate.
    Dart,
    /// D-FFI-PWSH1=A: request through a supervised PowerShell worker.
    PowerShell,
    /// D-FFI-PERL1=A: request through a supervised Perl worker.
    Perl,
    /// D-FFI-RUBY1=A: request through a supervised Ruby worker.
    Ruby,
    /// D-FFI-PHP1=A: request through a supervised PHP worker pool.
    Php,
    /// D-FFI-R1=A: request through a supervised R worker.
    R,
    /// D-FFI-COM1=A: Windows COM apartment automation call.
    Com,
    /// D-FFI-PY1=A: a supervised CPython sidecar or opt-in embedded runtime.
    Py,
    /// D-WASM1=A: browser/DOM API use — implies JS partition for web targets.
    Browser,
    /// U13 (D-JPK-SECRETCRYPTO1): reading a decrypted repo secret
    /// (`core.vault.get`). Denied by default even with no declared bound at
    /// all — see `check_secret_grants` — and always denied in a comptime
    /// build-tier context (E1265), with no `#Impure` escape hatch.
    Secret,
}
impl Effect {
    /// The PascalCase surface spelling (D-CASING1).
    pub fn name(self) -> &'static str {
        match self {
            Effect::Net => "Net",
            Effect::FS => "FS",
            Effect::IO => "IO",
            Effect::DB => "DB",
            Effect::Time => "Time",
            Effect::Rand => "Rand",
            Effect::Env => "Env",
            Effect::Exec => "Exec",
            Effect::Log => "Log",
            Effect::GPU => "GPU",
            Effect::Go => "Go",
            Effect::Java => "Java",
            Effect::DotNet => "DotNet",
            Effect::Fortran => "Fortran",
            Effect::Cobol => "Cobol",
            Effect::Tcl => "Tcl",
            Effect::Lua => "Lua",
            Effect::Ada => "Ada",
            Effect::Pascal => "Pascal",
            Effect::Dart => "Dart",
            Effect::PowerShell => "PowerShell",
            Effect::Perl => "Perl",
            Effect::Ruby => "Ruby",
            Effect::Php => "Php",
            Effect::R => "R",
            Effect::Com => "Com",
            Effect::Py => "Py",
            Effect::Browser => "Browser",
            Effect::Secret => "Secret",
        }
    }
    /// Parse a user-written effect name; `None` if it is not a known effect.
    pub fn parse(s: &str) -> Option<Effect> {
        Some(match s {
            "Net" => Effect::Net,
            "FS" => Effect::FS,
            "IO" => Effect::IO,
            "DB" => Effect::DB,
            "Time" => Effect::Time,
            "Rand" => Effect::Rand,
            "Env" => Effect::Env,
            "Exec" => Effect::Exec,
            "Log" => Effect::Log,
            "GPU" => Effect::GPU,
            "Go" => Effect::Go,
            "Java" => Effect::Java,
            "DotNet" => Effect::DotNet,
            "Fortran" => Effect::Fortran,
            "Cobol" => Effect::Cobol,
            "Tcl" => Effect::Tcl,
            "Lua" => Effect::Lua,
            "Ada" => Effect::Ada,
            "Pascal" => Effect::Pascal,
            "Dart" => Effect::Dart,
            "PowerShell" => Effect::PowerShell,
            "Perl" => Effect::Perl,
            "Ruby" => Effect::Ruby,
            "Php" => Effect::Php,
            "R" => Effect::R,
            "Com" => Effect::Com,
            "Py" => Effect::Py,
            "Browser" => Effect::Browser,
            "Secret" => Effect::Secret,
            _ => return None,
        })
    }
    /// Every effect — the maximal set, used for foreign (`extern`) calls whose
    /// body the compiler cannot inspect and for escaping function values. Each
    /// entry is a bare root, which (D-EFFTREE1 ancestor subsumption) covers
    /// its whole subtree — so this is still the true maximal set.
    pub fn all() -> EffectSet {
        crate::Facts::EFFECT_ROOTS
            .iter()
            .map(|effect| (*effect).to_string())
            .collect()
    }
}
/// D-EFFTREE1: an effect set's elements are canonical dotted paths (`"FS"`,
/// `"FS.Read"`) rather than bare `Effect` roots — see the module doc.
pub type EffectSet = BTreeSet<String>;
/// The effect carried by a Core call `module.method`, or `None` if pure.
/// Grounded in the real Core API surface (CheckerCoreLib). The `module` is the
/// fully-resolved name (`core.files`, `core.http`, …); legacy internal ring
/// keys are normalized through the foundation resolver before matching.
pub fn core_effect(module: &str, method: &str) -> Option<Effect> {
    // #1691 retired the jet.* internal module keys: callers always pass the
    // canonical `core.*` name, so no normalization step remains.
    // D-DET1: the deterministic capability constructors carry NO ambient effect —
    // `Clock.new(seed)` / `random.rng(seed)` build a reproducible `Clock`/`Rng`
    // from a caller-supplied seed (a pure value). Reading time/randomness THROUGH
    // the resulting handle (`clock.now()` / `rng.int(…)`) is a method call on a
    // value, not a module call, so it never reaches `core_effect`. This lets a
    // `#Pure fn` take and use an injected `Clock`/`Rng` while ambient `time.now()`
    // / `random.int(…)` stay rejected (E3403).
    // Civil constructors mint deterministic values, so (like `time.clock`) they
    // carry no effect.
    if matches!(
        (module, method),
        (
            "core.time",
            "clock"
                | "time"
                | "parse_time"
                | "period"
                | "period_days"
                | "period_months"
                | "period_years"
                | "utc"
                | "zoned"
                | "zoned_local"
        ) | ("core.random", "rng")
    ) {
        return None;
    }
    // D-META-EFFECT1: these read or reshape values the caller already holds —
    // parsing an address, asking a recorded error for its message, reading a
    // packet's own bytes. They reach nothing outside the program, so they carry
    // no effect and both stages may run them. This used to be a second list
    // (`is_pure_tier2_call`) that only the comptime tier consulted; the fact
    // belongs here, where the run tier reads it too.
    if matches!(
        (module, method),
        ("core.io", "style_force")
            | (
                "core.net",
                "ip_addr" | "ip_to_string" | "ip_is_ipv4" | "ip" | "ipv4" | "ipv6" | "parse_ip"
                    | "is_ipv4" | "is_ipv6"
            )
            | (
                "core.net",
                "socket_addr_parse" | "socket_host" | "socket_port" | "socket_to_string"
            )
            | ("core.net", "ready_readable" | "ready_writable")
            | (
                "core.net",
                "error_operation"
                    | "error_address"
                    | "error_name"
                    | "error_message"
                    | "error_os_code"
            )
            | (
                "core.net",
                "dns_srv_target" | "dns_srv_port" | "dns_srv_priority" | "dns_srv_weight"
            )
            | (
                "core.net",
                "udp_packet_data"
                    | "udp_packet_addr"
                    | "udp_packet_bytes"
                    | "udp_packet_original_len"
                    | "udp_packet_truncated"
            )
    ) {
        return None;
    }
    if module == "core.watcher" {
        return match method {
            "files" => Some(Effect::FS),
            "process_pid" => Some(Effect::Exec),
            "port" => Some(Effect::Net),
            "set" => None,
            _ => None,
        };
    }
    // D-BROWSER-AUTO1=A: profile/timeout validate pure values; locked reads the
    // project lock (FS). Connecting and handle I/O remain Net effects.
    if module == "core.browser" {
        return match method {
            "profile" | "timeout" => None,
            "locked" => Some(Effect::FS),
            _ => Some(Effect::Net),
        };
    }
    Some(match module {
        // D-COMPUTE-PLACE1=D: `.Auto` is the beginner placement default and
        // may select an accelerator, so compute operations carry GPU until an
        // explicit CPU placement narrows the call site. The CPU oracle remains
        // the deterministic implementation when no accelerator is installed.
        "core.compute" if method != "device_cpu" => Effect::GPU,
        "core.files" => Effect::FS,
        // D-BROWSER-AUTO1=A: browser automation is a versioned network protocol.
        "core.net" | "core.tls" | "jet.http" | "core.http.client" | "core.http.server" | "core.http.middleware" => Effect::Net,
        // D-RAYLIB1=A: windowing/drawing/input/audio bridge.
        "core.raylib" => Effect::GPU,
        "core.time" => Effect::Time,
        "core.random" | "core.crypto.random" => Effect::Rand,
        "core.env" => Effect::Env,
        "core.process" => Effect::Exec,
        "core.io" => Effect::IO,
        "jet.db" | "jet.sql" => Effect::DB,
        // D-AUTH1: the storeful session APIs read and write a live user store.
        // Declared here so the comptime tier reads the same fact the run tier
        // does, instead of naming these seven methods in a list of its own.
        "core.auth"
            if matches!(
                method,
                "register_user"
                    | "password_login"
                    | "session_validate"
                    | "magic_link_issue"
                    | "magic_link_consume"
                    | "oauth_begin"
                    | "oauth_finish"
            ) =>
        {
            Effect::DB
        }
        // D-DEP-WASM1=A (c81): loading a sandboxed plugin executes foreign
        // code, even though the sandbox makes it memory-safe — same bucket as
        // `core.process` (an effects-budget `deny: [Exec]` also denies plugins).
        "core.plugin" | "jet.plugin" => Effect::Exec,
        "jet.log" => Effect::Log,
        "core.ui" | "core.web" | "core.web.storage.local" | "core.web.storage.session" => {
            Effect::Browser
        }
        // U13 (D-JPK-SECRETCRYPTO1): only `core.vault.get` reads the encrypted
        // store. D-CORE-SECRETS1=A also places pure in-memory lifecycle helpers
        // in this module; those do not acquire the ambient Secret effect.
        "core.vault" if matches!(method,
            "get" | "current" | "versions" | "load" | "status"
            | "prepare_generate" | "prepare_store" | "prepare_rotate" | "prepare_retire" | "prepare_revoke"
            | "authorize_write" | "commit_generate" | "commit_store" | "commit_rotate" | "commit_retire" | "commit_revoke"
            | "export_to_recipients" | "export_to_passphrase" | "prepare_import_wrapped"
            | "authorize_wrapped_import" | "commit_import_wrapped"
        ) => Effect::Secret,
        "core.vault.expert" => Effect::Secret,
        _ => return None,
    })
}
/// D-TXN2: the irreversible effects — a network, filesystem, or subprocess
/// effect that, once performed, cannot be rolled back. These are rejected when
/// reached directly inside a `#Transact { … }` block (E0746). The remaining
/// effects (IO/Time/Rand/Env/DB/Log/GPU) are reversible-or-benign for this
/// purpose: reads, clock/RNG reads, and logging leave no committed external
/// state a rollback must undo, and DB rollback is the transaction's own job.
pub fn is_irreversible_effect(e: Effect) -> bool {
    matches!(e, Effect::Net | Effect::FS | Effect::Exec)
}
/// The effect carried by an ambient builtin call (`print`, `input`, …).
pub fn builtin_effect(name: &str) -> Option<Effect> {
    if crate::Syntax::IMPURE_BUILTINS.contains(&name) {
        Some(Effect::IO)
    } else {
        None
    }
}
