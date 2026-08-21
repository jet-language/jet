//! D-META-EFFECT1: the effect facts, owned once.
//!
//! An effect set says what a call touches. That fact does not depend on the
//! stage the call runs at, so it cannot live where only one stage can read
//! it. Sema reads it to check declared bounds; the comptime evaluator reads
//! the same table to decide which tier a call belongs to, instead of keeping
//! a second hard-coded list of its own.
pub use crate::Authority::{builtin_effect, Effect};
/// D-EFFTREE1: an effect set's elements are canonical dotted paths (`"FS"`,
/// `"FS.Read"`) rather than bare `Effect` roots — see the module doc.
pub type EffectSet = crate::Authority::Holds;

/// D-DET1: Core calls whose result depends on ambient wall-clock or PRNG
/// state. This is the one classification used by purity checking and
/// compile-time folding; deterministic constructors such as `random.rng`
/// remain outside it.
pub fn is_nondeterministic_core(module: &str, method: &str) -> bool {
    matches!(
        (module, method),
        (
            "core.time",
            "now" | "now_utc" | "today" | "instant" | "sleep" | "start"
        ) | ("core.task", "timeout")
            | (
                "core.math.random",
                "int"
                    | "float"
                    | "float_range"
                    | "bool"
                    | "normal"
                    | "exponential"
                    | "pick"
                    | "weighted_pick"
                    | "sample"
                    | "shuffle"
                    | "seed"
                    | "split"
                    | "bytes"
            )
            | ("core.crypto.random", "bytes")
    )
}

/// The effect carried by a Core call `module.method`, or `None` if pure.
/// Grounded in the real Core API surface (CheckerCoreLib). The `module` is the
/// fully-resolved name (`core.files`, `core.http`, …); legacy internal ring
/// keys are normalized through the foundation resolver before matching.
pub fn core_effect(module: &str, method: &str) -> Option<Effect> {
    if let Some(row) = crate::Syntax::core_call(module, method) {
        return row.effect;
    }
    core_effect_legacy(module, method)
}

/// Fallback for special calls that do not yet have a plain Core-call row.
/// Plain rows never reach this resolver; their effect is stored on the row.
fn core_effect_legacy(module: &str, method: &str) -> Option<Effect> {
    // #1691 retired the jet.* internal module keys: callers always pass the
    // canonical `core.*` name, so no normalization step remains.
    // D-DET1: the deterministic input constructors carry NO ambient effect —
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
                | "zone"
                | "utc"
                | "zoned"
                | "zoned_local"
        ) | ("core.math.random", "rng")
    ) {
        return None;
    }
    if is_nondeterministic_core(module, method) {
        return Some(match module {
            "core.time" | "core.task" => Effect::Time,
            "core.math.random" | "core.crypto.random" => Effect::Rand,
            _ => return None,
        });
    }
    // D-META-EFFECT1: these read or reshape values the caller already holds —
    // parsing an address, asking a recorded error for its message, reading a
    // packet's own bytes. They reach nothing outside the program, so they carry
    // no effect and both stages may run them. This used to be a second list
    // (`is_pure_tier2_call`) that only the comptime tier consulted; the fact
    // belongs here, where the run tier reads it too.
    if matches!(
        (module, method),
        ("core.term", "style_force")
            | (
                "core.net",
                "ip_addr"
                    | "ip_to_string"
                    | "ip_is_ipv4"
                    | "ip"
                    | "ipv4"
                    | "ipv6"
                    | "parse_ip"
                    | "is_ipv4"
                    | "is_ipv6"
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
    if module == "core.web.browser" {
        return match method {
            "profile" | "timeout" => None,
            "locked" => Some(Effect::FS),
            _ => Some(Effect::Net),
        };
    }
    // Encoding stream constructors own a live file handle.  Whole-value
    // codecs remain pure; only reader/writer construction reaches the host.
    if matches!(
        (module, method),
        (
            "core.encoding.json"
                | "core.encoding.jsonl"
                | "core.encoding.csv"
                | "core.encoding.xml"
                | "core.encoding.cbor",
            "reader" | "writer"
        )
    ) {
        return Some(Effect::FS);
    }
    Some(match module {
        // D-COMPUTE-PLACE1=D: `.Auto` is the beginner placement default and
        // may select an accelerator, so compute operations carry GPU until an
        // explicit CPU placement narrows the call site. The CPU oracle remains
        // the deterministic implementation when no accelerator is installed.
        "core.compute" if method != "device_cpu" => Effect::GPU,
        "core.files" => Effect::FS,
        // D-BROWSER-AUTO1=A: browser automation is a versioned network protocol.
        "core.net"
        | "core.net.tls"
        | "core.http.client"
        | "core.http.server"
        | "core.http.middleware" => Effect::Net,
        // D-RAYLIB1=A: windowing/drawing/input/audio bridge.
        "core.game.raylib" => Effect::GPU,
        "core.time" => Effect::Time,
        "core.math.random" | "core.crypto.random" => Effect::Rand,
        "core.sys" => Effect::Env,
        "core.process" => Effect::Exec,
        "core.term" => Effect::IO,
        "core.db" => Effect::DB,
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
        "core.plugin" | "core.mod" => Effect::Exec,
        "core.log" => Effect::Log,
        "core.ui" | "core.web" | "core.web.storage.local" | "core.web.storage.session" => {
            Effect::Browser
        }
        // U13 (D-JPK-SECRETCRYPTO1): only `core.crypto.vault.get` reads the encrypted
        // store. D-CORE-SECRETS1=A also places pure in-memory lifecycle helpers
        // in this module; those do not acquire the ambient Secret effect.
        "core.crypto.vault"
            if matches!(
                method,
                "get"
                    | "current"
                    | "versions"
                    | "load"
                    | "status"
                    | "prepare_generate"
                    | "prepare_store"
                    | "prepare_rotate"
                    | "prepare_retire"
                    | "prepare_revoke"
                    | "authorize_write"
                    | "commit_generate"
                    | "commit_store"
                    | "commit_rotate"
                    | "commit_retire"
                    | "commit_revoke"
                    | "export_to_recipients"
                    | "export_to_passphrase"
                    | "prepare_import_wrapped"
                    | "authorize_wrapped_import"
                    | "commit_import_wrapped"
                    | "prepare_import_signing"
                    | "prepare_import_x25519"
                    | "commit_import_signing"
                    | "commit_import_x25519"
            ) =>
        {
            Effect::Secret
        }
        _ => return None,
    })
}

/// The shared comptime gate fact for a Core call.
pub fn core_requires_comptime_gate(module: &str, method: &str) -> bool {
    core_effect(module, method).is_some_and(Effect::requires_comptime_gate)
}
/// D-TXN2: the irreversible effects — a network, filesystem, or subprocess
/// effect that, once performed, cannot be rolled back. These are rejected when
/// reached directly inside a `#Transact { … }` block (E0746). The remaining
/// effects (IO/Time/Rand/Env/DB/Log/GPU) are reversible-or-benign for this
/// purpose: reads, clock/RNG reads, and logging leave no committed external
/// state a rollback must undo, and DB rollback is the transaction's own job.
pub fn is_irreversible_effect(e: Effect) -> bool {
    matches!(e, Effect::Net | Effect::FS | Effect::Exec | Effect::FFI)
}
