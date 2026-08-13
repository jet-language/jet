//! Canonical projection rows for plain Core calls.
//!
//! D-ONCE-LAW1=A: each plain Core call states its module, member, erased
//! calling convention, fallibility authority, and one Prelude/Rust symbol
//! once here. AOT and the TIR coverage gate look the row up; they do not keep
//! second key lists. The typed Jet signature remains a sema fact because the
//! foundation crate cannot depend back on sema; the row records that
//! authority explicitly instead of copying sema's `Type` construction.

use crate::Effects::{core_effect, Effect};
use crate::Syntax::sinks::{sink_row, SinkClass};

/// The erased calling convention needed by all plain projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCallSignature {
    /// Number of positional values passed to the Prelude symbol.
    pub arity: usize,
    /// Inclusive upper bound for optional-argument rows. Plain rows have the
    /// same lower and upper bound.
    pub max_arity: usize,
    /// Whether each argument is rendered as a shared borrow by AOT.
    pub borrow_mask: &'static [bool],
}

/// Exact Jet return/parameter types stay in sema. This marker prevents a
/// consumer from silently inventing a second fallibility table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallFallibility {
    /// Resolve from the canonical sema fixed/resolved signature.
    Sema,
}

/// Pure comptime/REPL projection family. The row owns this routing fact so
/// the evaluator does not keep a second `(module, member)` membership list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallPureRoute {
    None,
    Mime,
    Email,
    EncodingXml,
    Time,
    Math,
    Measurement,
    Date,
    DateTime,
    SketchHll,
    SketchTDigest,
    SketchCms,
    SketchReservoir,
    Ui,
    Raylib,
    Io,
    Net,
    Crypto,
}

/// Where AOT resolves a Core call's Rust symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallSymbol {
    /// Prefix the symbol with the generated program's Prelude root.
    Prelude(&'static str),
    /// Emit the Rust symbol as written.
    Rust(&'static str),
}

impl CoreCallSymbol {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Prelude(name) | Self::Rust(name) => name,
        }
    }
}

/// One plain Core-call record shared by every engine projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCallRecord {
    pub module: &'static str,
    pub member: &'static str,
    /// Non-empty only for receiver/static-method rows in the same registry.
    /// Function rows use `core_call`; receiver rows use `core_receiver_method`.
    pub receiver_types: &'static [&'static str],
    pub signature: CoreCallSignature,
    pub fallibility: CoreCallFallibility,
    pub pure_route: CoreCallPureRoute,
    pub symbol: CoreCallSymbol,
    /// Whether AOT can emit this row as one plain symbol call. Typed rows can
    /// still be canonical records while remaining on their typed emitter.
    pub aot_direct: bool,
    /// Whether the resident JIT can use the direct symbol ABI. `false` keeps
    /// a typed/closure adapter on its existing lowering path.
    pub jit_direct: bool,
    /// Optional resident-JIT symbol when the AOT spelling is not its host
    /// spelling. This keeps the alias in the one record instead of lowering.
    pub jit_symbol: Option<&'static str>,
}

impl CoreCallRecord {
    pub const fn new(
        module: &'static str,
        member: &'static str,
        symbol: &'static str,
        prelude: bool,
        borrow_mask: &'static [bool],
    ) -> Self {
        Self {
            module,
            member,
            receiver_types: &[],
            signature: CoreCallSignature {
                arity: borrow_mask.len(),
                max_arity: borrow_mask.len(),
                borrow_mask,
            },
            fallibility: CoreCallFallibility::Sema,
            pure_route: CoreCallPureRoute::None,
            symbol: if prelude {
                CoreCallSymbol::Prelude(symbol)
            } else {
                CoreCallSymbol::Rust(symbol)
            },
            aot_direct: true,
            jit_direct: true,
            jit_symbol: None,
        }
    }

    /// Add a receiver/static-method row without creating a second registry.
    pub const fn receiver(
        receiver_types: &'static [&'static str],
        member: &'static str,
        borrow_mask: &'static [bool],
    ) -> Self {
        Self {
            module: "",
            member,
            receiver_types,
            signature: CoreCallSignature {
                arity: borrow_mask.len(),
                max_arity: borrow_mask.len(),
                borrow_mask,
            },
            fallibility: CoreCallFallibility::Sema,
            pure_route: CoreCallPureRoute::None,
            symbol: CoreCallSymbol::Rust(""),
            aot_direct: false,
            jit_direct: false,
            jit_symbol: None,
        }
    }

    /// The canonical number of positional arguments for this plain call.
    pub const fn arity(self) -> usize {
        self.signature.arity
    }

    pub const fn accepts_arity(self, count: usize) -> bool {
        count >= self.signature.arity && count <= self.signature.max_arity
    }

    pub const fn with_max_arity(mut self, max_arity: usize) -> Self {
        self.signature.max_arity = max_arity;
        self
    }

    pub const fn with_pure_route(mut self, route: CoreCallPureRoute) -> Self {
        self.pure_route = route;
        self
    }

    /// Read the one effect fact. The record owns the key; `Effects` owns the
    /// value, so this projection cannot drift from sema or comptime.
    pub fn effect(self) -> Option<Effect> {
        if self.is_receiver() {
            return None;
        }
        core_effect(self.module, self.member)
    }

    /// Read the one sink fact. `None` means this call is not a registered sink.
    pub fn sink_class(self) -> Option<SinkClass> {
        sink_row(self.module, self.member).map(|row| row.class)
    }

    /// A plain record has a direct symbol projection in every codegen path;
    /// argument/value eligibility is still checked by each engine's adapter.
    pub const fn has_direct_symbol(self) -> bool {
        self.aot_direct && !self.is_receiver() && !self.symbol.name().is_empty()
    }

    pub const fn is_receiver(self) -> bool {
        !self.receiver_types.is_empty()
    }

    pub const fn without_direct_aot(mut self) -> Self {
        self.aot_direct = false;
        self
    }

    /// Keep a row visible to all metadata consumers while selecting a typed
    /// or closure-shaped JIT adapter instead of the direct host projection.
    pub const fn without_direct_jit(mut self) -> Self {
        self.jit_direct = false;
        self
    }

    pub const fn with_jit_symbol(mut self, symbol: &'static str) -> Self {
        self.jit_symbol = Some(symbol);
        self
    }

    /// Positions whose erased Core ABI takes a filesystem path. Sema accepts
    /// `String | Path`; AOT, JIT, and the interpreter marshal a `Path` through
    /// its canonical string representation at this boundary.
    pub fn path_mask(self) -> &'static [bool] {
        match (self.module, self.member) {
            ("core.files", "read" | "read_bytes" | "exists" | "is_dir" | "remove"
                | "remove_dir" | "remove_all" | "list_dir" | "create_dir"
                | "create_dir_all" | "stat" | "canonicalize" | "absolute" | "walk"
                | "glob" | "fsync" | "lock" | "open" | "create" | "append") => &[true],
            ("core.files", "write" | "append_all" | "write_atomic") => &[true],
            ("core.files", "copy" | "copy_dir" | "rename" | "symlink" | "hard_link") => {
                &[true, true]
            }
            ("core.files", "read_link") => &[true],
            ("core.files", "read_at") => &[true],
            ("core.files", "write_at") => &[true],
            ("core.watcher", "files") => &[true],
            ("core.io", "binread") => &[true],
            ("core.io", "binwrite") => &[true],
            ("core.os", "set_current_dir" | "mkfifo") => &[true],
            ("core.os", "utime") => &[true, false, false],
            _ => &[],
        }
    }

    pub fn path_arg(self, index: usize) -> bool {
        let mask = self.path_mask();
        index < mask.len() && mask[index]
    }

    /// Candidate resident-JIT symbols for this Prelude projection.
    ///
    /// The JIT host is an ABI adapter, so its exported name has the same
    /// suffix as the Prelude symbol with the tier prefix changed. Keeping
    /// that projection here lets lowering ask the host registry for a
    /// function without maintaining a second `(module, member)` map.
    pub fn jit_symbol_candidates(self) -> Vec<String> {
        let name = self.symbol.name();
        let mut candidates = self
            .jit_symbol
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if !name.is_empty() && !candidates.iter().any(|known| known == name) {
            candidates.push(name.to_string());
        }
        for prefix in ["jet_std_", "jet_ring_", "jet_"] {
            let Some(suffix) = name.strip_prefix(prefix) else {
                continue;
            };
            let candidate = format!("jet_jit_{suffix}");
            if !candidates.iter().any(|known| known == &candidate) {
                candidates.push(candidate);
            }
        }
        if !self.is_receiver() {
            // Most resident hosts use the stable `jet_jit_<module>_<member>`
            // spelling, while a few old hosts use only the leaf module or
            // member (`jet_jit_xml_parse`, `jet_jit_interval`). These are
            // deterministic projections of the row key, not a second
            // `(module, member)` registry.
            let module_tail = self.module.strip_prefix("core.").unwrap_or(self.module);
            let module_tail = module_tail.replace('.', "_");
            let leaf = self.module.rsplit('.').next().unwrap_or(self.module);
            for candidate in [
                format!("jet_jit_{module_tail}_{}", self.member),
                format!("jet_jit_{leaf}_{}", self.member),
                format!("jet_jit_{}", self.member),
            ] {
                if !candidates.iter().any(|known| known == &candidate) {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    }
}

/// Every Core call whose form is a plain symbol call.
pub const CORE_CALLS: &[CoreCallRecord] = &[
    CoreCallRecord::new("core.mem", "volatile_read", "std::ptr::read_volatile", false, &[false]),
    CoreCallRecord::new("core.mem", "volatile_write", "std::ptr::write_volatile", false, &[false, false]),
    CoreCallRecord::new("core.tasks", "interval", "jet_std::interval", true, &[false]),
    CoreCallRecord::new("core.tasks", "yield_now", "jet_std::jet_task_yield", true, &[]),
    CoreCallRecord::new("core.tasks", "current_task", "jet_std::jet_task_current_trace", true, &[]),
    CoreCallRecord::new("core.reactive", "signal", "jet_std::JetSignal::new", true, &[false]), // D-REACT1=B: `reactive.signal(initial)` producer → a `JetSignal<T>`.
    CoreCallRecord::new("core.event", "scope", "jet_std::JetEventScope::new", true, &[]), // D-EVENT1=D: first-party typed Event/Hook constructors.
    CoreCallRecord::new("core.event", "policy_sync", "jet_std::JetEventPolicy::sync", true, &[]),
    CoreCallRecord::new("core.science.measurement", "from", "jet_std::JetMeasurement::new", true, &[false, false])
        .with_pure_route(CoreCallPureRoute::Measurement), // D-HONESTNUM1=A: `M.from(value, uncertainty)` → a `JetMeasurement<f64>`.
    CoreCallRecord::new("core.math", "fraction", "jet_fraction_new", true, &[false, false])
        .with_pure_route(CoreCallPureRoute::Math), // D-CORE-NUMERIC1=A: `core.math.decimal(s)` → exact parse.
    CoreCallRecord::new("core.math", "decimal", "jet_decimal_from_str", true, &[true])
        .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.files", "read", "jet_std_fs_read", true, &[true]), // D-FILES-WRITE1 (merge, was `core.fs`): whole-file convenience helpers now // live in `core.files` alongside the streaming handle constructors below. // D-FILES-APPEND1=A: whole-file one-shot is `append_all`, not `append` — // that name stays reserved for the streaming handle's `.append(text)`.
    CoreCallRecord::new("core.files", "read_bytes", "jet_std_fs_read_bytes", true, &[true]),
    CoreCallRecord::new("core.files", "write", "jet_std_fs_write", true, &[true, true]),
    CoreCallRecord::new("core.files", "append_all", "jet_std_fs_append", true, &[true, true]),
    CoreCallRecord::new("core.files", "exists", "jet_std_fs_exists", true, &[true]),
    CoreCallRecord::new("core.files", "remove", "jet_std_fs_remove", true, &[true]),
    CoreCallRecord::new("core.files", "remove_dir", "jet_std_fs_remove_dir", true, &[true]),
    CoreCallRecord::new("core.files", "remove_all", "jet_std_fs_remove_all", true, &[true]),
    CoreCallRecord::new("core.files", "list_dir", "jet_std_fs_list_dir", true, &[true]),
    CoreCallRecord::new("core.files", "create_dir", "jet_std_fs_create_dir", true, &[true]),
    CoreCallRecord::new("core.files", "create_dir_all", "jet_std_fs_create_dir_all", true, &[true]),
    CoreCallRecord::new("core.files", "is_dir", "jet_std_fs_is_dir", true, &[true]),
    CoreCallRecord::new("core.files", "copy", "jet_std_fs_copy", true, &[true, true]),
    CoreCallRecord::new("core.files", "copy_dir", "jet_std_fs_copy_dir", true, &[true, true]),
    CoreCallRecord::new("core.files", "rename", "jet_std_fs_rename", true, &[true, true]),
    CoreCallRecord::new("core.files", "symlink", "jet_std_fs_symlink", true, &[true, true]),
    CoreCallRecord::new("core.files", "read_link", "jet_std_fs_read_link", true, &[true]),
    CoreCallRecord::new("core.files", "hard_link", "jet_std_fs_hard_link", true, &[true, true]),
    CoreCallRecord::new("core.files", "stat", "jet_std_fs_stat", true, &[true]),
    CoreCallRecord::new("core.files", "canonicalize", "jet_std_fs_canonicalize", true, &[true]),
    CoreCallRecord::new("core.files", "absolute", "jet_std_fs_absolute", true, &[true]),
    CoreCallRecord::new("core.files", "walk", "jet_std_fs_walk", true, &[true]),
    CoreCallRecord::new("core.files", "glob", "jet_std_fs_glob", true, &[true]),
    CoreCallRecord::new("core.files", "read_at", "jet_std_fs_read_at", true, &[true, false, false]),
    CoreCallRecord::new("core.files", "write_at", "jet_std_fs_write_at", true, &[true, false, true]),
    CoreCallRecord::new("core.files", "fsync", "jet_std_fs_fsync", true, &[true]),
    CoreCallRecord::new("core.files", "write_atomic", "jet_std_fs_write_atomic", true, &[true, true]),
    CoreCallRecord::new("core.files", "temp_dir", "jet_std_fs_temp_dir", true, &[true]),
    CoreCallRecord::new("core.files", "temp_file", "jet_std_fs_temp_file", true, &[true]),
    CoreCallRecord::new("core.files", "lock", "jet_std_fs_lock", true, &[true]),
    CoreCallRecord::new("core.watcher", "files", "jet_watcher_files", true, &[true]),
    CoreCallRecord::new("core.watcher", "process_pid", "jet_watcher_process_pid", true, &[false]),
    CoreCallRecord::new("core.watcher", "port", "jet_watcher_port", true, &[true, false]),
    CoreCallRecord::new("core.watcher", "set", "jet_watcher_set", true, &[]),
    CoreCallRecord::new("core.io", "args", "jet_std_io_args", true, &[]),
    CoreCallRecord::new("core.args", "spec", "jet_args_spec", true, &[]), // D-ARGS1: `args.spec()` → empty builder.
    CoreCallRecord::new("core.io", "confirm", "jet_std_io_confirm", true, &[true]),
    CoreCallRecord::new("core.io", "choose", "jet_std_io_choose", true, &[true, true]),
    CoreCallRecord::new("core.io", "input_secret", "jet_std_io_input_secret", true, &[true]),
    CoreCallRecord::new("core.io", "read_all_input", "jet_std_io_read_all_input", true, &[]),
    CoreCallRecord::new("core.io", "readline", "jet_std_io_readline", true, &[]),
    CoreCallRecord::new("core.io", "read_until", "jet_std_io_read_until", true, &[true]),
    CoreCallRecord::new("core.io", "take", "jet_std_io_take", true, &[false]),
    CoreCallRecord::new("core.io", "buffered", "jet_std_io_buffered", true, &[]),
    CoreCallRecord::new("core.io", "binread", "jet_std_io_binread", true, &[true]),
    CoreCallRecord::new("core.io", "binwrite", "jet_std_io_binwrite", true, &[true, true]),
    CoreCallRecord::new("core.io", "stdin", "jet_std_io_stdin", true, &[]), // D-STDIN1=A: io.stdin() → JetStdinReader handle.
    CoreCallRecord::new("core.io", "stdout", "jet_std_io_stdout", true, &[]),
    CoreCallRecord::new("core.io", "stderr", "jet_std_io_stderr", true, &[]),
    CoreCallRecord::new("core.io", "terminal_width", "jet_std_io_terminal_width", true, &[]),
    CoreCallRecord::new("core.io", "terminal_height", "jet_std_io_terminal_height", true, &[]),
    CoreCallRecord::new("core.io", "style", "jet_std_io_style", true, &[true, true]),
    CoreCallRecord::new("core.io", "style_force", "jet_std_io_style_force", true, &[true, true])
        .with_pure_route(CoreCallPureRoute::Io),
    CoreCallRecord::new("core.env", "get", "jet_std_env_get", true, &[true]),
    CoreCallRecord::new("core.env", "set", "jet_std_env_set", true, &[true, true]),
    CoreCallRecord::new("core.env", "unset", "jet_std_env_unset", true, &[true]),
    CoreCallRecord::new("core.env", "vars", "jet_std_env_vars", true, &[]),
    CoreCallRecord::new("core.env", "current_dir", "jet_std_env_current_dir", true, &[]),
    CoreCallRecord::new("core.env", "home_dir", "jet_std_env_home_dir", true, &[]),
    CoreCallRecord::new("core.os", "name", "jet_std_os_name", true, &[]),
    CoreCallRecord::new("core.os", "family", "jet_std_os_family", true, &[]),
    CoreCallRecord::new("core.os", "arch", "jet_std_os_arch", true, &[]),
    CoreCallRecord::new("core.os", "cpu_count", "jet_std_os_cpu_count", true, &[]),
    CoreCallRecord::new("core.os", "temp_dir", "jet_std_os_temp_dir", true, &[]),
    CoreCallRecord::new("core.os", "executable", "jet_std_os_executable", true, &[]),
    CoreCallRecord::new("core.os", "pid", "jet_std_os_pid", true, &[]),
    CoreCallRecord::new("core.os", "getpid", "jet_std_os_pid", true, &[]),
    CoreCallRecord::new("core.os", "hostname", "jet_std_os_hostname", true, &[]),
    CoreCallRecord::new("core.os", "username", "jet_std_os_username", true, &[]),
    CoreCallRecord::new("core.os", "release", "jet_std_os_release", true, &[]),
    CoreCallRecord::new("core.os", "version", "jet_std_os_version", true, &[]),
    CoreCallRecord::new("core.os", "expand", "jet_std_os_expand", true, &[true]),
    CoreCallRecord::new("core.os", "getppid", "jet_std_os_getppid", true, &[]),
    CoreCallRecord::new("core.os", "getuid", "jet_std_os_getuid", true, &[]),
    CoreCallRecord::new("core.os", "geteuid", "jet_std_os_geteuid", true, &[]),
    CoreCallRecord::new("core.os", "getgid", "jet_std_os_getgid", true, &[]),
    CoreCallRecord::new("core.os", "getegid", "jet_std_os_getegid", true, &[]),
    CoreCallRecord::new("core.os", "getgroups", "jet_std_os_getgroups", true, &[]),
    CoreCallRecord::new("core.os", "getpgrp", "jet_std_os_getpgrp", true, &[]),
    CoreCallRecord::new("core.os", "uptime", "jet_std_os_uptime", true, &[]),
    CoreCallRecord::new("core.os", "loadavg", "jet_std_os_loadavg", true, &[]),
    CoreCallRecord::new("core.os", "times", "jet_std_os_times", true, &[]),
    CoreCallRecord::new("core.os", "sync", "jet_std_os_sync", true, &[]),
    CoreCallRecord::new("core.os", "getpgid", "jet_std_os_getpgid", true, &[false]),
    CoreCallRecord::new("core.os", "getsid", "jet_std_os_getsid", true, &[false]),
    CoreCallRecord::new("core.os", "exitcode", "jet_std_os_exitcode", true, &[false]),
    CoreCallRecord::new("core.os", "success", "jet_std_os_success", true, &[false]),
    CoreCallRecord::new("core.os", "umask", "jet_std_os_umask", true, &[false]),
    CoreCallRecord::new("core.os", "getpriority", "jet_std_os_getpriority", true, &[false]),
    CoreCallRecord::new("core.os", "setpriority", "jet_std_os_setpriority", true, &[false, false]),
    CoreCallRecord::new("core.os", "utime", "jet_std_os_utime", true, &[true, false, false]),
    CoreCallRecord::new("core.os", "stop", "jet_std_os_stop", true, &[false]),
    CoreCallRecord::new("core.os", "set_current_dir", "jet_std_os_set_current_dir", true, &[true]),
    CoreCallRecord::new("core.os", "on_interrupt", "jet_std_os_on_interrupt", true, &[false])
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.os", "atexit", "jet_std_os_atexit", true, &[false]),
    CoreCallRecord::new("core.os", "fork", "jet_std_os_fork", true, &[]),
    CoreCallRecord::new("core.os", "setuid", "jet_std_os_setuid", true, &[false]),
    CoreCallRecord::new("core.os", "setgid", "jet_std_os_setgid", true, &[false]),
    CoreCallRecord::new("core.os", "setpgid", "jet_std_os_setpgid", true, &[false, false]),
    CoreCallRecord::new("core.os", "setpgrp", "jet_std_os_setpgrp", true, &[]),
    CoreCallRecord::new("core.os", "setsid", "jet_std_os_setsid", true, &[]),
    CoreCallRecord::new("core.os", "initgroups", "jet_std_os_initgroups", true, &[true, false]),
    CoreCallRecord::new("core.os", "kill", "jet_std_os_kill", true, &[false, false]),
    CoreCallRecord::new("core.os", "wait", "jet_std_os_wait", true, &[]),
    CoreCallRecord::new("core.os", "waitpid", "jet_std_os_waitpid", true, &[false, false]),
    CoreCallRecord::new("core.os", "pipe", "jet_std_os_pipe", true, &[]),
    CoreCallRecord::new("core.os", "close_fd", "jet_std_os_close_fd", true, &[false]),
    CoreCallRecord::new("core.os", "mkfifo", "jet_std_os_mkfifo", true, &[true, false]),
    CoreCallRecord::new("core.process", "exit", "jet_std_process_exit", true, &[false]),
    CoreCallRecord::new("core.process", "run", "jet_std_process_run", true, &[true]),
    CoreCallRecord::new("core.process", "cmd", "jet_std_process_cmd", true, &[true]),
    CoreCallRecord::new("core.process", "pipeline", "jet_std_process_pipeline", true, &[true]),
    // D-LIB-CALLGRANT1=A: the loader is a Prelude symbol; sema owns the
    // typed `ModGrant` contract and every engine only marshals into this row.
    CoreCallRecord::new("core.mod", "load", "jet_mod_load", true, &[true, true]),
    CoreCallRecord::new("core.testing", "snap", "jet_testing_snap", true, &[true, true]),
    CoreCallRecord::new("core.testing", "golden", "jet_testing_golden", true, &[true, true]),
    CoreCallRecord::new("core.testing", "fixture", "jet_testing_fixture", true, &[true]),
    CoreCallRecord::new("core.testing", "temp_dir", "jet_testing_temp_dir", true, &[true]),
    CoreCallRecord::new("core.testing", "corpus", "jet_testing_corpus", true, &[true]),
    CoreCallRecord::new("core.testing", "fake_clock", "jet_std_clock_new", true, &[false]),
    CoreCallRecord::new("core.testing", "fake_rng", "jet_std_rng_new", true, &[false]),
    CoreCallRecord::new("core.math", "round", "jet_std_math_round", true, &[false]),
    CoreCallRecord::new("core.math", "isqrt", "jet_std_math_isqrt", true, &[false]),
    CoreCallRecord::new("core.math", "factorial", "jet_std_math_factorial", true, &[false]),
    CoreCallRecord::new("core.math", "erf", "jet_std_math_erf", true, &[false]),
    CoreCallRecord::new("core.math", "erfc", "jet_std_math_erfc", true, &[false]),
    CoreCallRecord::new("core.math", "gamma", "jet_std_math_gamma", true, &[false]),
    CoreCallRecord::new("core.math", "lgamma", "jet_std_math_lgamma", true, &[false]),
    CoreCallRecord::new("core.math", "logb", "jet_std_math_logb", true, &[false]),
    CoreCallRecord::new("core.math", "significand", "jet_std_math_significand", true, &[false]),
    CoreCallRecord::new("core.math", "ulp", "jet_std_math_ulp", true, &[false]),
    CoreCallRecord::new("core.math", "cmp", "jet_std_math_cmp", true, &[false, false]),
    CoreCallRecord::new("core.math", "next_after", "jet_std_math_next_after", true, &[false, false]),
    CoreCallRecord::new("core.math", "ldexp", "jet_std_math_ldexp", true, &[false, false]),
    CoreCallRecord::new("core.math", "scaleb", "jet_std_math_ldexp", true, &[false, false]),
    CoreCallRecord::new("core.math", "ilogb", "jet_std_math_ilogb", true, &[false]),
    CoreCallRecord::new("core.math", "leading_ones", "jet_std_math_leading_ones", true, &[false]),
    CoreCallRecord::new("core.math", "trailing_ones", "jet_std_math_trailing_ones", true, &[false]),
    CoreCallRecord::new("core.math", "digits", "jet_std_math_digits", true, &[false]),
    CoreCallRecord::new("core.math", "binomial", "jet_std_math_binomial", true, &[false, false]),
    CoreCallRecord::new("core.math", "checked_pow", "jet_std_math_checked_pow", true, &[false, false]),
    CoreCallRecord::new("core.math", "int_pow", "jet_std_math_int_pow", true, &[false, false]),
    CoreCallRecord::new("core.math", "gcd", "jet_std_math_gcd", true, &[false, false]),
    CoreCallRecord::new("core.math", "lcm", "jet_std_math_lcm", true, &[false, false]),
    CoreCallRecord::new("core.random", "int", "jet_std_random_int", true, &[false, false]),
    CoreCallRecord::new("core.random", "float", "jet_std_random_float", true, &[]),
    CoreCallRecord::new("core.random", "float_range", "jet_std_random_float_range", true, &[false, false]),
    CoreCallRecord::new("core.random", "bool", "jet_std_random_bool", true, &[false]),
    CoreCallRecord::new("core.random", "normal", "jet_std_random_normal", true, &[false, false]),
    CoreCallRecord::new("core.random", "exponential", "jet_std_random_exponential", true, &[false]),
    CoreCallRecord::new("core.random", "seed", "jet_std_random_seed", true, &[false]),
    CoreCallRecord::new("core.random", "bytes", "jet_std_random_bytes", true, &[false]), // D-RANDSPLIT1=A: PRNG bytes — fast, NOT crypto-safe.
    CoreCallRecord::new("core.crypto.random", "bytes", "jet_std_crypto_random_bytes", true, &[false]), // D-CRYPTO-RNG1=A: shared fail-closed OS CSPRNG provider.
    CoreCallRecord::new("core.random", "rng", "jet_std_rng_new", true, &[false]), // D-DET1: deterministic injected RNG capability constructor.
    CoreCallRecord::new("core.random", "split", "jet_std_random_split", true, &[false]),
    CoreCallRecord::new("core.time", "now", "jet_std_time_now", true, &[]),
    CoreCallRecord::new("core.time", "sleep", "jet_std_time_sleep", true, &[false]),
    CoreCallRecord::new("core.time", "start", "jet_std_time_start", true, &[]),
    CoreCallRecord::new("core.time", "instant", "jet_time_instant_now", true, &[])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "now_utc", "jet_time_now_utc", true, &[]),
    CoreCallRecord::new("core.time", "from_unix_ms", "JetDateTime::from_unix_ms", false, &[false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "today", "jet_time_today", true, &[]),
    CoreCallRecord::new("core.time", "parse_rfc3339", "jet_time_parse_rfc3339", true, &[true])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "datetime", "jet_time_datetime", true, &[false, false, false, false, false, false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "time", "JetLocalTime::new", false, &[false, false, false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "local_time", "JetLocalTime::new", false, &[false, false, false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "days_in_month", "jet_time_days_in_month", true, &[false, false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "is_leap_year", "jet_time_is_leap_year", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "period", "jet_time_period", true, &[false, false, false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "period_days", "jet_time_period_days", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "period_months", "jet_time_period_months", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "period_years", "jet_time_period_years", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "zone", "jet_time_zone_named", true, &[true])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "utc", "jet_time_zone_utc", true, &[])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "zoned", "jet_time_zoned", true, &[true, true])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "zoned_local", "jet_time_zoned_local", true, &[true, true, true])
        .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "clock", "jet_std_clock_new", true, &[false]), // D-DET1: deterministic injected Clock capability constructor.
    CoreCallRecord::new("core.time", "nanoseconds", "jet_duration_from_int", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.time", "microseconds", "jet_duration_from_int", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.time", "milliseconds", "jet_duration_from_int", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.time", "seconds", "jet_duration_from_int", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.time", "minutes", "jet_duration_from_int", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.time", "hours", "jet_duration_from_int", true, &[false])
        .with_pure_route(CoreCallPureRoute::Time)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.encoding.json", "parse", "jet_std_json_parse", true, &[true]), // D-ENC1 + D-JSONVERB1 + D-SERDE6: unified `core.encoding.*`. The dynamic forms // (`JSON` tree / `[[String]]` / `Map`) keep their existing helpers; the typed // forms route through the Encode/Decode model, distinguished by the lowered arg // type (encode) or the resolved return type (decode). `is_json_value` etc. read // those total facts — codegen never re-infers (I3).
    CoreCallRecord::new("core.encoding.json", "events", "jet_std_json_events", true, &[true]),
    CoreCallRecord::new("core.encoding.jsonl", "parse", "jet_std_jsonl_parse", true, &[true]),
    CoreCallRecord::new("core.encoding.jsonl", "to_string", "jet_std_jsonl_render", true, &[true]),
    CoreCallRecord::new("core.encoding.csv", "parse", "jet_ring_csv_parse", true, &[true]),
    CoreCallRecord::new("core.data", "count", "jet_data_count", true, &[true]),
    CoreCallRecord::new("core.compute", "zeros", "jet_compute_zeros", true, &[true]), // D-COMPUTE1=D (#443): Tensor CPU oracle — one Prelude symbol per call.
    CoreCallRecord::new("core.compute", "ones", "jet_compute_ones", true, &[true]),
    CoreCallRecord::new("core.compute", "full", "jet_compute_full", true, &[true, false]),
    CoreCallRecord::new("core.compute", "from_list", "jet_compute_from_list", true, &[true]),
    CoreCallRecord::new("core.compute", "matrix", "jet_compute_matrix", true, &[false, false, false]),
    CoreCallRecord::new("core.compute", "vec", "jet_compute_vec", true, &[false, false]),
    CoreCallRecord::new("core.compute", "add", "jet_compute_add", true, &[true, true]),
    CoreCallRecord::new("core.compute", "mul", "jet_compute_mul", true, &[true, true]),
    CoreCallRecord::new("core.compute", "matmul", "jet_compute_matmul", true, &[true, true]),
    CoreCallRecord::new("core.compute", "reshape", "jet_compute_reshape", true, &[true, true]),
    CoreCallRecord::new("core.compute", "get", "jet_compute_get", true, &[true, true]),
    CoreCallRecord::new("core.compute", "shape", "jet_compute_tensor_shape", true, &[true]),
    CoreCallRecord::new("core.compute", "rank", "jet_compute_tensor_rank", true, &[true]),
    CoreCallRecord::new("core.compute", "numel", "jet_compute_tensor_numel", true, &[true]),
    CoreCallRecord::new("core.compute", "to_list", "jet_compute_tensor_to_list", true, &[true]),
    CoreCallRecord::new("core.compute", "device", "jet_compute_tensor_device", true, &[true]),
    CoreCallRecord::new("core.compute", "placement", "jet_compute_tensor_placement", true, &[true]),
    CoreCallRecord::new("core.compute", "device_cpu", "jet_compute_device_cpu", true, &[]),
    CoreCallRecord::new("core.compute", "device_auto", "jet_compute_device_auto", true, &[]),
    CoreCallRecord::new("core.compute", "on_device", "jet_compute_on_device", true, &[true, false]),
    CoreCallRecord::new("core.compute", "broadcast_to", "jet_compute_broadcast_to", true, &[true, true]),
    CoreCallRecord::new("core.compute", "transpose", "jet_compute_transpose", true, &[true]),
    CoreCallRecord::new("core.compute", "sum_axis", "jet_compute_sum_axis", true, &[true, false]),
    CoreCallRecord::new("core.compute", "eye", "jet_compute_eye", true, &[false]),
    CoreCallRecord::new("core.compute", "det", "jet_compute_det", true, &[true]),
    CoreCallRecord::new("core.compute", "inv", "jet_compute_inv", true, &[true]),
    CoreCallRecord::new("core.compute", "fft", "jet_compute_fft", true, &[true]),
    CoreCallRecord::new("core.compute", "solve", "jet_compute_solve", true, &[true, true]),
    CoreCallRecord::new("core.compute", "stream_new", "jet_compute_stream_new", true, &[]),
    CoreCallRecord::new("core.compute", "stream_sync", "jet_compute_stream_sync", true, &[true]),
    CoreCallRecord::new("core.compute", "stream_show", "jet_compute_stream_show", true, &[true]),
    CoreCallRecord::new("core.compute", "transfer", "jet_compute_transfer", true, &[true, false]),
    CoreCallRecord::new("core.compute", "transfer_show", "jet_compute_transfer_show", true, &[true]),
    CoreCallRecord::new("core.compute", "kernel_bounds_ok", "jet_compute_kernel_bounds_ok", true, &[true, true]),
    CoreCallRecord::new("core.compute", "mse_loss", "jet_compute_mse_loss", true, &[true, true]),
    CoreCallRecord::new("core.compute", "sgd_step", "jet_compute_sgd_step", true, &[true, true, false]),
    CoreCallRecord::new("core.compute", "serialize", "jet_compute_serialize", true, &[true]),
    CoreCallRecord::new("core.compute", "deserialize", "jet_compute_deserialize", true, &[true]),
    CoreCallRecord::new("core.compute", "to_sparse", "jet_compute_to_sparse", true, &[true]),
    CoreCallRecord::new("core.compute", "sparse_nnz", "jet_compute_sparse_nnz", true, &[true]),
    CoreCallRecord::new("core.compute", "sparse_mv", "jet_compute_sparse_mv", true, &[true, true]),
    CoreCallRecord::new("core.compute", "sparse_show", "jet_compute_sparse_show", true, &[true]),
    CoreCallRecord::new("core.compute", "matmul_f32_tile", "jet_compute_matmul_f32_tile", true, &[true, true]),
    CoreCallRecord::new("core.compute", "profile_f32_strict", "jet_compute_profile_f32_strict", true, &[]),
    CoreCallRecord::new("core.compute", "profile_show", "jet_compute_profile_show", true, &[]),
    CoreCallRecord::new("core.services", "restart_one_for_one", "jet_services_restart_one_for_one", true, &[]),
    CoreCallRecord::new("core.services", "restart_one_for_all", "jet_services_restart_one_for_all", true, &[]),
    CoreCallRecord::new("core.services", "restart_rest_for_one", "jet_services_restart_rest_for_one", true, &[]),
    CoreCallRecord::new("core.services", "delivery_at_most_once", "jet_services_delivery_at_most_once", true, &[]),
    CoreCallRecord::new("core.services", "delivery_durable", "jet_services_delivery_durable", true, &[]),
    CoreCallRecord::new("core.services", "mailbox_depth", "jet_services_mailbox_depth", true, &[true, true]),
    CoreCallRecord::new("core.services", "restarts", "jet_services_restarts", true, &[true, true]),
    CoreCallRecord::new("core.services", "dead_letter_count", "jet_services_dead_letter_count", true, &[true]),
    CoreCallRecord::new("core.services", "restore_snapshot", "jet_services_restore_snapshot", true, &[true]),
    CoreCallRecord::new("core.services", "event_count", "jet_services_event_count", true, &[true]),
    CoreCallRecord::new("core.services", "replay_events", "jet_services_replay_events", true, &[true]),
    CoreCallRecord::new("core.services", "workflow_history", "jet_services_workflow_history", true, &[true, false]),
    CoreCallRecord::new("core.services", "directory_resolve", "jet_services_directory_resolve", true, &[true, true]),
    CoreCallRecord::new("core.services", "directory_generation", "jet_services_directory_generation", true, &[true]),
    CoreCallRecord::new("core.services", "upgrade_receipt", "jet_services_upgrade_receipt", true, &[true]),
    CoreCallRecord::new("core.services", "observe", "jet_services_observe", true, &[true]),
    CoreCallRecord::new("core.services", "endpoint_show", "jet_services_endpoint_show", true, &[true]),
    CoreCallRecord::new("core.services", "tree_show", "jet_services_tree_show", true, &[true]),
    CoreCallRecord::new("core.data", "table", "jet_data_table", true, &[true]),
    CoreCallRecord::new("core.data", "rows", "jet_data_rows", true, &[true]),
    CoreCallRecord::new("core.data", "series", "jet_data_series", true, &[true]),
    CoreCallRecord::new("core.data", "values", "jet_data_series_values", true, &[true]),
    CoreCallRecord::new("core.data", "missing_count", "jet_data_missing_count", true, &[true]),
    CoreCallRecord::new("core.data", "lazy", "jet_data_lazy", true, &[true]),
    CoreCallRecord::new("core.data", "plan", "jet_data_plan", true, &[true]),
    CoreCallRecord::new("core.data", "filter", "jet_data_filter", true, &[true, false]),
    CoreCallRecord::new("core.data", "lazy_filter", "jet_data_lazy_filter", true, &[true, false]),
    CoreCallRecord::new("core.data", "lazy_sort_by", "jet_data_lazy_sort_by", true, &[true, false]),
    CoreCallRecord::new("core.data", "status", "jet_data_status", true, &[]),
    CoreCallRecord::new("core.data", "require_bridge", "jet_data_require_bridge", true, &[true]),
    CoreCallRecord::new("core.data", "csv_reader", "jet_data_csv_reader", true, &[false, false]),
    CoreCallRecord::new("core.data", "json_reader", "jet_data_json_reader", true, &[false, false]),
    CoreCallRecord::new("core.fmt", "number", "jet_fmt_number", true, &[false]),
    CoreCallRecord::new("core.fmt", "decimal", "jet_fmt_decimal", true, &[false, false]),
    CoreCallRecord::new("core.fmt", "percent", "jet_fmt_percent", true, &[false, false]),
    CoreCallRecord::new("core.fmt", "bytes", "jet_fmt_bytes", true, &[false]),
    CoreCallRecord::new("core.fmt", "duration", "jet_fmt_duration", true, &[false]),
    CoreCallRecord::new("core.fmt", "ordinal", "jet_fmt_ordinal", true, &[false]),
    CoreCallRecord::new("core.fmt", "plural", "jet_fmt_plural", true, &[false, true, true]),
    CoreCallRecord::new("core.fmt", "pad_left", "jet_fmt_pad_left", true, &[true, false, true]),
    CoreCallRecord::new("core.fmt", "pad_right", "jet_fmt_pad_right", true, &[true, false, true]),
    CoreCallRecord::new("core.fmt", "pad_center", "jet_fmt_pad_center", true, &[true, false, true]),
    CoreCallRecord::new("core.encoding.toml", "parse", "jet_std_toml_parse", true, &[true]),
    CoreCallRecord::new("core.encoding.yaml", "parse", "jet_std_yaml_parse", true, &[true]),
    CoreCallRecord::new("core.encoding.xml", "parse", "jet_std_xml_parse", true, &[true]),
    CoreCallRecord::new("core.encoding.xml", "parse_with", "jet_std_xml_parse_with", true, &[true, true]),
    CoreCallRecord::new("core.encoding.xml", "to_string", "jet_std_xml_render", true, &[true]),
    CoreCallRecord::new("core.encoding.xml", "canonical", "jet_std_xml_canonical", true, &[true, true])
        .with_pure_route(CoreCallPureRoute::EncodingXml),
    CoreCallRecord::new("core.encoding.xml", "root", "jet_std_xml_root", true, &[true]),
    CoreCallRecord::new("core.encoding.xml", "attribute", "jet_std_xml_attribute", true, &[true, true]),
    CoreCallRecord::new("core.encoding.xml", "content", "jet_std_xml_content", true, &[true]),
    CoreCallRecord::new("core.encoding.cbor", "to_bytes", "jet_enc_cbor_to_bytes", true, &[true]),
    CoreCallRecord::new("core.encoding.cbor", "to_bytes_canonical", "jet_enc_cbor_to_bytes_canonical", true, &[true]),
    CoreCallRecord::new("core.encoding.hex", "encode", "jet_std_hex_encode", true, &[true]), // D-UUIDENC1=A: hex and base64 encode/decode.
    CoreCallRecord::new("core.encoding.hex", "decode", "jet_std_hex_decode", true, &[true]),
    CoreCallRecord::new("core.encoding.base64", "encode", "jet_std_b64_encode", true, &[true]),
    CoreCallRecord::new("core.encoding.base64", "decode", "jet_std_b64_decode", true, &[true]),
    CoreCallRecord::new("core.encoding.base64", "encode_url", "jet_std_b64url_encode", true, &[true]),
    CoreCallRecord::new("core.encoding.base64", "decode_url", "jet_std_b64url_decode", true, &[true]),
    CoreCallRecord::new("core.encoding.base32", "encode", "jet_std_base32_encode", true, &[true]),
    CoreCallRecord::new("core.encoding.base32", "decode", "jet_std_base32_decode", true, &[true]),
    CoreCallRecord::new("core.uuid", "v4", "jet_std_uuid_v4", true, &[]), // D-UUIDENC1=A: UUID v4 (CSPRNG) and v7 (injectable Clock).
    CoreCallRecord::new("core.uuid", "v7", "jet_std_uuid_v7", true, &[true]),
    CoreCallRecord::new("core.uuid", "v5", "jet_std_uuid_v5", true, &[true, true]), // #1481: `v5` (namespace+name, deterministic) and `parse` (validate // + normalize) — pure std, same UUID-as-String shape as v4/v7.
    CoreCallRecord::new("core.uuid", "parse", "jet_std_uuid_parse", true, &[true]),
    CoreCallRecord::new("core.files", "open", "jet_std_files_open", true, &[true]),
    CoreCallRecord::new("core.files", "create", "jet_std_files_create", true, &[true]),
    CoreCallRecord::new("core.files", "append", "jet_std_files_append", true, &[true]),
    CoreCallRecord::new("core.url", "parse", "jet_url_parse", true, &[true]),
    CoreCallRecord::new("core.url", "from_parts", "jet_url_from_parts", true, &[true, true, true, true, true]),
    CoreCallRecord::new("core.url", "file", "jet_url_file", true, &[true]),
    CoreCallRecord::new("core.url", "data", "jet_url_data", true, &[true, true]),
    CoreCallRecord::new("core.url", "query", "jet_url_query", true, &[true]),
    CoreCallRecord::new("core.url", "percent_encode", "jet_url_percent_encode_component", true, &[true]),
    CoreCallRecord::new("core.url", "percent_decode", "jet_url_percent_decode_component", true, &[true]),
    CoreCallRecord::new("core.mime", "parse", "jet_mime_parse", true, &[true])
        .with_pure_route(CoreCallPureRoute::Mime),
    CoreCallRecord::new("core.mime", "from_extension", "jet_mime_from_extension", true, &[true])
        .with_pure_route(CoreCallPureRoute::Mime),
    CoreCallRecord::new("core.mime", "extension", "jet_mime_extension", true, &[true])
        .with_pure_route(CoreCallPureRoute::Mime),
    CoreCallRecord::new("core.email", "address", "jet_email::address", true, &[true])
        .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "attachment", "jet_email::attachment", true, &[true, true, true])
        .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "message", "jet_email::message", true, &[true, true, true, true, true, true, true])
        .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "envelope", "jet_email::envelope", true, &[true, true])
        .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "serialize", "jet_email::serialize", true, &[true])
        .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.text.unicode", "scalar_count", "jet_text_unicode_scalar_count", true, &[true]), // D-TEXTUNICODE1: std-only Unicode scalar helpers.
    CoreCallRecord::new("core.text.unicode", "byte_count", "jet_text_unicode_byte_count", true, &[true]),
    CoreCallRecord::new("core.text.unicode", "is_ascii", "jet_text_unicode_is_ascii", true, &[true]),
    CoreCallRecord::new("core.text.unicode", "lower", "jet_text_unicode_lower", true, &[true]),
    CoreCallRecord::new("core.text.unicode", "upper", "jet_text_unicode_upper", true, &[true]),
    CoreCallRecord::new("core.text.unicode", "scalars", "jet_text_unicode_scalars", true, &[true]),
    CoreCallRecord::new("core.text", "nfc", "jet_text_nfc", true, &[true]),
    CoreCallRecord::new("core.text", "nfd", "jet_text_nfd", true, &[true]),
    CoreCallRecord::new("core.text", "nfkc", "jet_text_nfkc", true, &[true]),
    CoreCallRecord::new("core.text", "nfkd", "jet_text_nfkd", true, &[true]),
    CoreCallRecord::new("core.text", "casefold", "jet_text_casefold", true, &[true]),
    CoreCallRecord::new("core.text", "caseless_eq", "jet_text_caseless_eq", true, &[true, true]),
    CoreCallRecord::new("core.text", "lower", "jet_text_lower", true, &[true]),
    CoreCallRecord::new("core.text", "upper", "jet_text_upper", true, &[true]),
    CoreCallRecord::new("core.text", "graphemes", "jet_text_graphemes", true, &[true]),
    CoreCallRecord::new("core.text", "words", "jet_text_words", true, &[true]),
    CoreCallRecord::new("core.text", "sentences", "jet_text_sentences", true, &[true]),
    CoreCallRecord::new("core.text", "scalar_count", "jet_text_unicode_scalar_count", true, &[true]),
    CoreCallRecord::new("core.text", "byte_count", "jet_text_unicode_byte_count", true, &[true]),
    CoreCallRecord::new("core.text", "is_alphabetic", "jet_text_is_alphabetic", true, &[true]),
    CoreCallRecord::new("core.text", "is_numeric", "jet_text_is_numeric", true, &[true]),
    CoreCallRecord::new("core.text", "is_whitespace", "jet_text_is_whitespace", true, &[true]),
    CoreCallRecord::new("core.text", "is_ascii", "jet_text_unicode_is_ascii", true, &[true]),
    CoreCallRecord::new("core.text", "scalars", "jet_text_unicode_scalars", true, &[true]),
    CoreCallRecord::new("core.text", "splitn", "jet_text_splitn", true, &[true, true, false]),
    CoreCallRecord::new("core.text", "rsplitn", "jet_text_rsplitn", true, &[true, true, false]),
    CoreCallRecord::new("core.text", "trim", "jet_text_trim", true, &[true]),
    CoreCallRecord::new("core.text", "trim_start", "jet_text_trim_start", true, &[true]),
    CoreCallRecord::new("core.text", "trim_end", "jet_text_trim_end", true, &[true]),
    CoreCallRecord::new("core.text", "pad_start", "jet_text_pad_start", true, &[true, false, true]),
    CoreCallRecord::new("core.text", "pad_end", "jet_text_pad_end", true, &[true, false, true]),
    CoreCallRecord::new("core.text", "center", "jet_text_center", true, &[true, false, true]),
    CoreCallRecord::new("core.text", "starts_any", "jet_text_starts_any", true, &[true, true]),
    CoreCallRecord::new("core.text", "ends_any", "jet_text_ends_any", true, &[true, true]),
    CoreCallRecord::new("core.text", "inspect", "jet_text_inspect", true, &[true]),
    CoreCallRecord::new("core.text", "char_indices", "jet_text_char_indices", true, &[true]),
    CoreCallRecord::new("core.log", "info", "jet_ring_log_info", true, &[true]), // E2-M9: first-party ring packages.
    CoreCallRecord::new("core.log", "warn", "jet_ring_log_warn", true, &[true]),
    CoreCallRecord::new("core.log", "error", "jet_ring_log_error", true, &[true]),
    CoreCallRecord::new("core.log", "debug", "jet_ring_log_debug", true, &[true]),
    CoreCallRecord::new("core.log", "critical", "jet_ring_log_critical", true, &[true]),
    CoreCallRecord::new("core.log", "fatal", "jet_ring_log_fatal", true, &[true]),
    CoreCallRecord::new("core.log", "disable", "jet_ring_log_disable", true, &[]),
    CoreCallRecord::new("core.log", "flush", "jet_ring_log_flush", true, &[]),
    CoreCallRecord::new("core.log", "enabled", "jet_ring_log_enabled", true, &[true]),
    CoreCallRecord::new("core.log", "field", "jet_ring_log_field", true, &[true, true]),
    CoreCallRecord::new("core.log", "int", "jet_ring_log_int", true, &[true, false]),
    CoreCallRecord::new("core.log", "float", "jet_ring_log_float", true, &[true, false]),
    CoreCallRecord::new("core.log", "bool", "jet_ring_log_bool", true, &[true, false]),
    CoreCallRecord::new("core.log", "redact", "jet_ring_log_redact", true, &[true]),
    CoreCallRecord::new("core.log", "info_fields", "jet_ring_log_info_fields", true, &[true, true]),
    CoreCallRecord::new("core.log", "warn_fields", "jet_ring_log_warn_fields", true, &[true, true]),
    CoreCallRecord::new("core.log", "error_fields", "jet_ring_log_error_fields", true, &[true, true]),
    CoreCallRecord::new("core.log", "debug_fields", "jet_ring_log_debug_fields", true, &[true, true]),
    CoreCallRecord::new("core.log", "span", "jet_ring_log_span", true, &[true]),
    CoreCallRecord::new("core.log", "enter", "jet_ring_log_enter", true, &[true]),
    CoreCallRecord::new("core.log", "close", "jet_ring_log_close", true, &[true]),
    CoreCallRecord::new("core.log", "set_sink", "jet_ring_log_set_sink", true, &[true, true]),
    CoreCallRecord::new("core.log", "sample_every", "jet_ring_log_sample_every", true, &[false]),
    CoreCallRecord::new("core.log", "counter", "jet_ring_log_counter", true, &[true, false]),
    CoreCallRecord::new("core.log", "otlp_file", "jet_ring_log_otlp_file", true, &[true]),
    CoreCallRecord::new("core.log", "set_level", "jet_ring_log_set_level", true, &[true]),
    CoreCallRecord::new("core.log", "set_trace_id", "jet_ring_log_set_trace_id", true, &[true]), // E2-M12 D-OBS3: trace context for structured log records.
    CoreCallRecord::new("core.log", "setup", "jet_ring_log_setup", true, &[true]), // D-LOGFMT1=A: explicit log format override.
    CoreCallRecord::new("core.crypto", "sha256_bytes", "jet_ring_crypto_sha256_bytes", true, &[true]),
    CoreCallRecord::new("core.crypto", "sha1", "jet_crypto_sha1_hex", true, &[true]),
    CoreCallRecord::new("core.crypto", "sha224", "jet_crypto_sha224_hex", true, &[true]),
    CoreCallRecord::new("core.crypto", "sha384", "jet_crypto_sha384_hex", true, &[true]),
    CoreCallRecord::new("core.crypto", "sha3_224", "jet_crypto_sha3_224_hex", true, &[true]),
    CoreCallRecord::new("core.crypto", "sha3_256", "jet_crypto_sha3_256_hex", true, &[true]),
    CoreCallRecord::new("core.crypto", "sha3_384", "jet_crypto_sha3_384_hex", true, &[true]),
    CoreCallRecord::new("core.crypto", "sha3_512", "jet_crypto_sha3_512_hex", true, &[true]),
    CoreCallRecord::new("core.crypto", "pbkdf2_hmac", "jet_crypto_pbkdf2_hmac", true, &[true, true, false, false]),
    CoreCallRecord::new("core.auth", "session_validate", "jet_auth_session_validate", true, &[true, false]),
    CoreCallRecord::new("core.auth", "session_show", "jet_auth_session_show", true, &[true]),
    CoreCallRecord::new("core.auth", "session_user", "jet_auth_session_user", true, &[true]),
    CoreCallRecord::new("core.auth", "session_cookie", "jet_auth_session_cookie", true, &[true]),
    CoreCallRecord::new("core.auth", "session_id", "jet_auth_session_id", true, &[true]),
    CoreCallRecord::new("core.sync", "text_merge", "jet_sync_text_merge", true, &[true, true]),
    CoreCallRecord::new("core.sync", "text_show", "jet_sync_text_show", true, &[true]),
    CoreCallRecord::new("core.sync", "text_metadata", "jet_sync_text_metadata", true, &[true]),
    CoreCallRecord::new("core.sync", "counter_merge", "jet_sync_counter_merge", true, &[true, true]),
    CoreCallRecord::new("core.sync", "counter_value", "jet_sync_counter_value", true, &[true]),
    CoreCallRecord::new("core.sync", "map_new", "jet_sync_map_new", true, &[]),
    CoreCallRecord::new("core.sync", "map_get", "jet_sync_map_get", true, &[true, true]),
    CoreCallRecord::new("core.sync", "map_merge", "jet_sync_map_merge", true, &[true, true]),
    CoreCallRecord::new("core.sync", "map_show", "jet_sync_map_show", true, &[true]),
    CoreCallRecord::new("core.sync", "list_new", "jet_sync_list_new", true, &[]),
    CoreCallRecord::new("core.sync", "list_merge", "jet_sync_list_merge", true, &[true, true]),
    CoreCallRecord::new("core.sync", "list_show", "jet_sync_list_show", true, &[true]),
    CoreCallRecord::new("core.sync", "policy_allows", "jet_db_policy_allows", true, &[true, true, true]),
    CoreCallRecord::new("core.sync", "policy_show", "jet_db_policy_show", true, &[true]),
    CoreCallRecord::new("core.net", "ip_addr", "jet_net_ip_addr", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net), // D-NETSOCKET1=A: core.net — typed addresses, TCP/UDP/Unix/DNS, TLS handle.
    CoreCallRecord::new("core.net", "ip_to_string", "jet_net_ip_to_string", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "ip_is_ipv4", "jet_net_ip_is_ipv4", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "socket_addr", "jet_net_socket_addr", true, &[true, false]),
    CoreCallRecord::new("core.net", "socket_addr_parse", "jet_net_socket_addr_parse", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "socket_host", "jet_net_socket_host", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "socket_port", "jet_net_socket_port", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "socket_to_string", "jet_net_socket_to_string", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "tcp_listen", "jet_net_tcp_listen", true, &[true]),
    CoreCallRecord::new("core.net", "tcp_listen_addr", "jet_net_tcp_listen_addr", true, &[true]),
    CoreCallRecord::new("core.net", "tcp_accept", "jet_net_tcp_accept", true, &[true]),
    CoreCallRecord::new("core.net", "tcp_connect", "jet_net_tcp_connect", true, &[true]),
    CoreCallRecord::new("core.net", "tcp_connect_addr", "jet_net_tcp_connect_addr", true, &[true]),
    CoreCallRecord::new("core.net", "tcp_connect_timeout", "jet_net_tcp_connect_timeout", true, &[true, false]),
    CoreCallRecord::new("core.net", "tcp_connect_happy", "jet_net_tcp_connect_happy", true, &[true, false, false]),
    CoreCallRecord::new("core.net", "ready_readable", "jet_net_ready_readable", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "ready_writable", "jet_net_ready_writable", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "error_operation", "jet_net_error_operation", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "error_address", "jet_net_error_address", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "error_name", "jet_net_error_name", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "error_message", "jet_net_error_message", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "error_os_code", "jet_net_error_os_code", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "tcp_local_addr", "jet_net_tcp_local_addr", true, &[true]),
    CoreCallRecord::new("core.net", "tcp_peer_addr", "jet_net_tcp_peer_addr", true, &[true]),
    CoreCallRecord::new("core.net", "tcp_local_socket_addr", "jet_net_tcp_local_socket_addr", true, &[true]),
    CoreCallRecord::new("core.net", "tcp_peer_socket_addr", "jet_net_tcp_peer_socket_addr", true, &[true]),
    CoreCallRecord::new("core.net", "listener_local_socket_addr", "jet_net_listener_local_socket_addr", true, &[true]),
    CoreCallRecord::new("core.net", "nodelay", "jet_net_nodelay", true, &[true]),
    CoreCallRecord::new("core.net", "set_nodelay", "jet_net_set_nodelay", true, &[true, false]),
    CoreCallRecord::new("core.net", "ttl", "jet_net_ttl", true, &[true]),
    CoreCallRecord::new("core.net", "set_ttl", "jet_net_set_ttl", true, &[true, false]),
    CoreCallRecord::new("core.net", "socket_type", "jet_net_socket_type", true, &[true]),
    CoreCallRecord::new("core.net", "tcp_reply", "jet_net_tcp_reply", true, &[false, true, true]),
    CoreCallRecord::new("core.net", "udp_bind", "jet_net_udp_bind", true, &[true]),
    CoreCallRecord::new("core.net", "udp_bind_addr", "jet_net_udp_bind_addr", true, &[true]),
    CoreCallRecord::new("core.net", "udp_local_addr", "jet_net_udp_local_addr", true, &[true]),
    CoreCallRecord::new("core.net", "udp_set_timeout", "jet_net_udp_set_timeout", true, &[true, false]),
    CoreCallRecord::new("core.net", "udp_send_to", "jet_net_udp_send_to", true, &[true, true, true]),
    CoreCallRecord::new("core.net", "udp_recv_from", "jet_net_udp_recv_from", true, &[true, false]),
    CoreCallRecord::new("core.net", "udp_send_bytes_to", "jet_net_udp_send_bytes_to", true, &[true, true, true]),
    CoreCallRecord::new("core.net", "udp_receive", "jet_net_udp_receive", true, &[true, false]),
    CoreCallRecord::new("core.net", "udp_packet_data", "jet_net_udp_packet_data", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "udp_packet_addr", "jet_net_udp_packet_addr", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "udp_packet_bytes", "jet_net_udp_packet_bytes", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "udp_packet_original_len", "jet_net_udp_packet_original_len", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "udp_packet_truncated", "jet_net_udp_packet_truncated", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "unix_listen", "jet_net_unix_listen", true, &[true]),
    CoreCallRecord::new("core.net", "unix_accept", "jet_net_unix_accept", true, &[true]),
    CoreCallRecord::new("core.net", "getservbyname", "jet_net_getservbyname", true, &[true]),
    CoreCallRecord::new("core.net", "getservbyport", "jet_net_getservbyport", true, &[false]),
    CoreCallRecord::new("core.net", "dns_srv_target", "jet_net_dns_srv_target", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "dns_srv_port", "jet_net_dns_srv_port", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "dns_srv_priority", "jet_net_dns_srv_priority", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "dns_srv_weight", "jet_net_dns_srv_weight", true, &[true])
        .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.http", "router", "jet_http_router_new", true, &[]), // c109 Phase 25: HTTPRouter producer + parse/dispatch (D-ROUTE1=A). `router()` is arg-free; `parse(raw)` borrows the raw string; `dispatch(router, req)` borrows the router and passes the request by value.
    CoreCallRecord::new("core.http", "parse", "jet_http_parse_request", true, &[true]),
    CoreCallRecord::new("core.http", "dispatch", "jet_http_router_dispatch", true, &[true, false]),
    CoreCallRecord::new("core.regex", "flags", "jet_std::jet_regex_flags", true, &[false, false, false]), // D-REGEXENGINE1=A: core.regex — std-only runtime in jet_std, no bridge dep.
    CoreCallRecord::new("core.regex", "escape", "jet_std::jet_regex_escape", true, &[true]),
    CoreCallRecord::new("core.regex", "compile", "jet_std::jet_regex_compile", true, &[true]),
    CoreCallRecord::new("core.regex", "compile_with", "jet_std::jet_regex_compile_with", true, &[true, true]),
    CoreCallRecord::new("core.regex", "literal", "jet_std::jet_regex_literal", true, &[true]),
    CoreCallRecord::new("core.regex", "is_match", "jet_std::jet_regex_is_match", true, &[true, true]),
    CoreCallRecord::new("core.regex", "match", "jet_std::jet_regex_match", true, &[true, true]),
    CoreCallRecord::new("core.regex", "find", "jet_std::jet_regex_find", true, &[true, true]),
    CoreCallRecord::new("core.regex", "find_all", "jet_std::jet_regex_find_all", true, &[true, true]),
    CoreCallRecord::new("core.regex", "matches", "jet_std::jet_regex_matches", true, &[true, true]),
    CoreCallRecord::new("core.regex", "split", "jet_std::jet_regex_split", true, &[true, true]),
    CoreCallRecord::new("core.regex", "split_limit", "jet_std::jet_regex_split_limit", true, &[true, true, false]),
    CoreCallRecord::new("core.regex", "replace", "jet_std::jet_regex_replace", true, &[true, true, true]),
    CoreCallRecord::new("core.regex", "replace_all", "jet_std::jet_regex_replace_all", true, &[true, true, true]),
    CoreCallRecord::new("core.raylib", "window_open", "jet_raylib_window_open", true, &[false, false, true]), // D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A: typed graphics bridge.
    CoreCallRecord::new("core.raylib", "window_should_close", "jet_raylib_window_should_close", true, &[true]),
    CoreCallRecord::new("core.raylib", "window_ready", "jet_raylib_window_ready", true, &[true]),
    CoreCallRecord::new("core.raylib", "begin_drawing", "jet_raylib_begin_drawing", true, &[true]),
    CoreCallRecord::new("core.raylib", "clear_background", "jet_raylib_clear_background", true, &[true]),
    CoreCallRecord::new("core.raylib", "draw_text", "jet_raylib_draw_text", true, &[true, false, false, false, true]),
    CoreCallRecord::new("core.raylib", "draw_rectangle", "jet_raylib_draw_rectangle", true, &[false, false, false, false, true]),
    CoreCallRecord::new("core.raylib", "end_drawing", "jet_raylib_end_drawing", true, &[]),
    CoreCallRecord::new("core.raylib", "close_window", "jet_raylib_close_window", true, &[true]),
    CoreCallRecord::new("core.raylib", "key_down", "jet_raylib_key_down", true, &[true]),
    CoreCallRecord::new("core.raylib", "set_target_fps", "jet_raylib_set_target_fps", true, &[false]),
    CoreCallRecord::new("core.raylib", "load_sound", "jet_raylib_load_sound", true, &[true]),
    CoreCallRecord::new("core.raylib", "play_sound", "jet_raylib_play_sound", true, &[true]),
    CoreCallRecord::new("core.raylib", "color", "jet_raylib_color", true, &[false, false, false, false])
        .with_pure_route(CoreCallPureRoute::Raylib),
    CoreCallRecord::new("core.db", "params", "jet_std::jet_db_params_from_sql", true, &[true]),
    CoreCallRecord::new("core.db", "row_value", "jet_std::jet_db_row_value", true, &[true, true]),
    CoreCallRecord::new("core.db", "row_int", "jet_std::jet_db_row_int", true, &[true, true]),
    CoreCallRecord::new("core.db", "row_float", "jet_std::jet_db_row_float", true, &[true, true]),
    CoreCallRecord::new("core.db", "row_text", "jet_std::jet_db_row_text", true, &[true, true]),
    CoreCallRecord::new("core.db", "row_bool", "jet_std::jet_db_row_bool", true, &[true, true]),
    CoreCallRecord::new("core.db", "transaction", "jet_db_scope_transaction", false, &[true, true, true]),
    CoreCallRecord::new("core.db", "migrate", "jet_db_scope_migrate", false, &[true, true, true]),
    CoreCallRecord::new("core.random", "pick", "jet_std_random_pick", true, &[true]),
    CoreCallRecord::new("core.random", "weighted_pick", "jet_std_random_weighted_pick", true, &[true, true]),
    CoreCallRecord::new("core.random", "sample", "jet_std_random_sample", true, &[true, false]),
    CoreCallRecord::new("core.term", "read_key", "jet_term_read_key", true, &[]), // D-TERM1 (ratified 2026-06-22): terminal direct-input.
    CoreCallRecord::new("core.perf", "fidelity", "jet_perf_fidelity", false, &[]), // D-FIDELITY-API1=A: runtime-global fidelity signal.
    CoreCallRecord::new("core.perf", "default_fidelity", "jet_perf_default_fidelity", false, &[]),
    CoreCallRecord::new("core.perf", "override_fidelity", "jet_perf_override_fidelity", false, &[false]),
    CoreCallRecord::new("core.perf", "reset_fidelity", "jet_perf_reset_fidelity", false, &[]),
    CoreCallRecord::new("core.ui", "null_backend", "jet_ui_null", true, &[]), // D-RENDERTGT2=A (c133 M1): UI backend seam constructors.
    CoreCallRecord::new("core.ui", "tui_backend", "jet_ui_tui", true, &[]),
    CoreCallRecord::new("core.ui", "gtk_backend", "jet_ui_gtk", true, &[]), // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend constructor.
    CoreCallRecord::new("core.ui", "point", "jet_ui_point", true, &[false, false])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "size", "jet_ui_size", true, &[false, false])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "rect", "jet_ui_rect", true, &[false, false, false, false])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "constraint", "jet_ui_constraint", true, &[false, false, false, false])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "node", "jet_ui_node", true, &[true, false, false])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "text", "jet_ui_text", true, &[true])
        .with_pure_route(CoreCallPureRoute::Ui),
    // D-WEB-CLICK-PORT1=D: the optional labeled handler is admitted by the
    // bespoke sema/codegen path after the one-label core row is checked.
    CoreCallRecord::new("core.ui", "button", "jet_ui_button", true, &[true])
        .with_max_arity(2)
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "key_event", "jet_ui_key_event", true, &[true])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "resize_event", "jet_ui_resize_event", true, &[false, false])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "node_role", "jet_ui_node_role", true, &[true, false, false, false])
        .with_pure_route(CoreCallPureRoute::Ui), // D-A11YGATE1=B (c134 Phase 6): accessible-role node + role constants.
    CoreCallRecord::new("core.ui", "node_color", "jet_ui_node_color", true, &[true, false, false, true])
        .with_pure_route(CoreCallPureRoute::Ui), // D-STYLESHAPE1=A wiring: a node carrying an explicit fill color.
    CoreCallRecord::new("core.ui", "aria_role_button", "jet_ui_aria_role_button", true, &[])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "aria_role_text_input", "jet_ui_aria_role_text_input", true, &[])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "aria_role_label", "jet_ui_aria_role_label", true, &[])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "aria_role_container", "jet_ui_aria_role_container", true, &[])
        .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.web", "app", "jet_web_app", true, &[]), // D-WEBAPP1=D: application builder + page helper.
    CoreCallRecord::new("app", "live_get", "jet_app_live_get", true, &[true]),
    CoreCallRecord::new("core.web", "live_get", "jet_app_live_get", true, &[true]),
    CoreCallRecord::new("app", "live_show", "jet_app_live_show", true, &[true]),
    CoreCallRecord::new("core.web", "live_show", "jet_app_live_show", true, &[true]),
    CoreCallRecord::new("app", "live_stats", "jet_app_live_stats", true, &[]),
    CoreCallRecord::new("core.web", "live_stats", "jet_app_live_stats", true, &[]),
    CoreCallRecord::new("app", "auth_routes", "jet_app_auth_routes", true, &[true]),
    CoreCallRecord::new("core.web", "auth_routes", "jet_app_auth_routes", true, &[true]),
    CoreCallRecord::new("app", "auth_show", "jet_app_auth_show", true, &[true]),
    CoreCallRecord::new("core.web", "auth_show", "jet_app_auth_show", true, &[true]),
    CoreCallRecord::new("core.web.devserver", "for_app", "jet_devserver_for_app", true, &[true]), // c-devserver (owner-directed 2026-07-01): `devserver.for_app(file)` // constructor — the builder methods dispatch through // `THandleOp::DevServerMethod` above, not here.
    CoreCallRecord::new("core.web.devserver", "app", "jet_devserver_app", true, &[]),
    CoreCallRecord::new("core.sketch.hll", "new", "JetHyperLogLog::new", false, &[])
        .with_pure_route(CoreCallPureRoute::SketchHll), // D-APPROX1=A: sketch constructors.
    CoreCallRecord::new("core.sketch.tdigest", "new", "JetTDigest::new", false, &[])
        .with_pure_route(CoreCallPureRoute::SketchTDigest),
    CoreCallRecord::new("core.sketch.cms", "new", "JetCountMinSketch::new", false, &[])
        .with_pure_route(CoreCallPureRoute::SketchCms),
    CoreCallRecord::new("core.sketch.reservoir", "new", "JetReservoirSampler::new", false, &[false])
        .with_pure_route(CoreCallPureRoute::SketchReservoir),
    CoreCallRecord::new("core.browser", "profile", "jet_browser_profile", false, &[true]), // D-BROWSER-AUTO1=A: native versioned BiDi entry points.
    CoreCallRecord::new("core.browser", "timeout", "jet_browser_timeout", false, &[false]),
    CoreCallRecord::new("core.browser", "locked", "jet_browser_locked", false, &[true]),
    CoreCallRecord::new("core.browser", "connect", "jet_browser_connect", false, &[true]),
    CoreCallRecord::new("core.browser", "connect_profile", "jet_browser_connect_profile", false, &[true, true, false]),
    CoreCallRecord::new("core.http.server", "mux", "jet_http_mux_new", false, &[]),
    CoreCallRecord::new("core.http.server", "response", "jet_http_srv_response", false, &[false, true]),
    CoreCallRecord::new("core.http.server", "tls", "jet_http_srv_tls", false, &[true, true]),
    CoreCallRecord::new("core.http.server", "sse", "jet_http_srv_sse", false, &[true]),
    CoreCallRecord::new("core.http.server", "json", "jet_http_srv_json", false, &[false, true]), // D-HTTP-JSON1=A: one JSON response with its content type set.
    CoreCallRecord::new("core.http.server", "cors", "jet_http_srv_install_cors", false, &[true, true]),
    CoreCallRecord::new("core.http.server", "access_log", "jet_http_srv_access_log", false, &[true, false]),
    CoreCallRecord::new("core.http.server", "request_id", "jet_http_srv_install_request_id", false, &[true]),
    CoreCallRecord::new("core.ws", "connect", "jet_ws_connect", false, &[true]), // D-WS1=B: cleartext WebSocket client/server.
    CoreCallRecord::new("core.ws", "upgrade", "jet_ws_upgrade", false, &[true]),
    CoreCallRecord::new("core.time.date", "new", "JetDate::new", false, &[false, false, false])
        .with_pure_route(CoreCallPureRoute::Date), // D-TIMEDEPTH1=A: civil-time constructors.
    CoreCallRecord::new("core.time.date", "today", "JetDate::today_utc", false, &[])
        .with_pure_route(CoreCallPureRoute::Date),
    CoreCallRecord::new("core.time.datetime", "from_timestamp", "JetDateTime::from_timestamp", false, &[false])
        .with_pure_route(CoreCallPureRoute::DateTime),
    CoreCallRecord::new("core.time.datetime", "now", "JetDateTime::now", false, &[]),

    // Pure comptime evaluator rows whose AOT form is typed or otherwise
    // requires a bespoke emitter. They remain canonical records so every
    // evaluator entry has a table key without stealing the typed AOT path.
    CoreCallRecord::new("core.time", "parse_time", "JetLocalTime::parse", false, &[true])
        .with_pure_route(CoreCallPureRoute::Time)
        .without_direct_aot()
        .with_jit_symbol("jet_jit_time_parse_time"),
    CoreCallRecord::new("core.time.date", "parse", "JetDate::parse", false, &[true])
        .with_pure_route(CoreCallPureRoute::Date)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.ui", "box", "jet_ui_box", true, &[true])
        .with_pure_route(CoreCallPureRoute::Ui)
        .without_direct_aot()
        .with_jit_symbol("jet_jit_ui_box"),
    CoreCallRecord::new(
        "core.crypto.expert",
        "ed25519_verify_strict",
        "jet_crypto_expert_ed25519_verify_strict_impl",
        false,
        &[true, true, true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .without_direct_jit(),
    CoreCallRecord::new(
        "core.crypto.expert",
        "ed25519_sign",
        "jet_crypto_expert_ed25519_sign_impl",
        false,
        &[true, true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .without_direct_jit(),
    CoreCallRecord::new(
        "core.crypto.expert",
        "hkdf_sha256_raw",
        "jet_crypto_expert_hkdf_sha256_impl",
        false,
        &[true, true, true, false],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .with_jit_symbol("jet_jit_crypto_expert_hkdf_sha256"),
    CoreCallRecord::new(
        "core.crypto.expert",
        "x25519_raw",
        "jet_crypto_expert_x25519_impl",
        false,
        &[true, true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .without_direct_jit(),
    CoreCallRecord::new(
        "core.crypto.expert",
        "xchacha20poly1305_seal",
        "jet_crypto_expert_xchacha20poly1305_seal_impl",
        false,
        &[true, true, true, true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .without_direct_jit(),
    CoreCallRecord::new(
        "core.crypto.expert",
        "xchacha20poly1305_open",
        "jet_crypto_expert_xchacha20poly1305_open_impl",
        false,
        &[true, true, true, true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .without_direct_jit(),
    CoreCallRecord::new(
        "core.crypto.expert",
        "aes256gcm_seal",
        "jet_crypto_expert_aes256gcm_seal_impl",
        false,
        &[true, true, true, true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .with_jit_symbol("jet_jit_crypto_expert_aes256gcm_seal"),
    CoreCallRecord::new(
        "core.crypto.expert",
        "aes256gcm_open",
        "jet_crypto_expert_aes256gcm_open_impl",
        false,
        &[true, true, true, true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .with_jit_symbol("jet_jit_crypto_expert_aes256gcm_open"),
    CoreCallRecord::new(
        "core.crypto.expert",
        "argon2id",
        "jet_crypto_expert_argon2id_cancel_impl",
        false,
        &[true, true, false, false, false, false],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .without_direct_jit(),
    CoreCallRecord::new(
        "core.crypto.expert",
        "secret_bytes",
        "jet_crypto_expert_secret_bytes_impl",
        false,
        &[true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .with_jit_symbol("jet_jit_crypto_expert_secret_bytes"),
    CoreCallRecord::new(
        "core.crypto.expert",
        "signing_key_bytes",
        "jet_crypto_expert_signing_key_bytes_impl",
        false,
        &[true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .without_direct_jit(),
    CoreCallRecord::new(
        "core.crypto.expert",
        "x25519_secret_bytes",
        "jet_crypto_expert_x25519_secret_bytes_impl",
        false,
        &[true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .without_direct_jit(),
    CoreCallRecord::new(
        "core.crypto.expert",
        "shared_secret_bytes",
        "jet_crypto_expert_shared_secret_bytes_impl",
        false,
        &[true],
    )
    .with_pure_route(CoreCallPureRoute::Crypto)
    .without_direct_aot()
    .without_direct_jit(),
    CoreCallRecord::new("core.crypto", "__signature_bytes", "jet_crypto_signature_bytes_impl", false, &[true])
        .with_pure_route(CoreCallPureRoute::Crypto)
        .without_direct_aot()
        .with_jit_symbol("jet_jit_crypto_signature_bytes"),
    CoreCallRecord::new("core.crypto", "__verify_key_bytes", "jet_crypto_verify_key_bytes_impl", false, &[true])
        .with_pure_route(CoreCallPureRoute::Crypto)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.crypto", "__x25519_public_bytes", "jet_crypto_x25519_public_bytes_impl", false, &[true])
        .with_pure_route(CoreCallPureRoute::Crypto)
        .without_direct_aot()
        .with_jit_symbol("jet_jit_crypto_x25519_public_bytes"),
    CoreCallRecord::new("core.crypto", "__sealed_bytes", "jet_crypto_sealed_bytes_impl", false, &[true])
        .with_pure_route(CoreCallPureRoute::Crypto)
        .without_direct_aot()
        .with_jit_symbol("jet_jit_crypto_sealed_bytes"),
    CoreCallRecord::new("core.crypto", "__digest256_bytes", "jet_crypto_digest256_bytes_impl", false, &[true])
        .with_pure_route(CoreCallPureRoute::Crypto)
        .without_direct_aot()
        .with_jit_symbol("jet_jit_crypto_digest256_bytes"),
    CoreCallRecord::new("core.crypto", "__digest512_bytes", "jet_crypto_digest512_bytes_impl", false, &[true])
        .with_pure_route(CoreCallPureRoute::Crypto)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.encoding.xml", "decode", "jet_enc_xml_decode", true, &[true])
        .with_pure_route(CoreCallPureRoute::EncodingXml)
        .with_max_arity(2)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.encoding.xml", "decode_bytes", "jet_enc_xml_decode_bytes", true, &[true])
        .with_pure_route(CoreCallPureRoute::EncodingXml)
        .with_max_arity(2)
        .without_direct_aot()
        .without_direct_jit(),

    // Receiver/static-method projections used by the comptime value adapter.
    CoreCallRecord::receiver(
        &["Signature", "Secret", "SigningKey", "VerifyKey", "X25519SecretKey", "X25519PublicKey", "SharedSecret"],
        "bytes",
        &[],
    ),
    CoreCallRecord::receiver(&["Mime"], "media_type", &[]),
    CoreCallRecord::receiver(&["Mime"], "subtype", &[]),
    CoreCallRecord::receiver(&["Mime"], "essence", &[]),
    CoreCallRecord::receiver(&["Mime"], "to_string", &[]),
    CoreCallRecord::receiver(&["Mime"], "param", &[false]),
    CoreCallRecord::receiver(&["Mime"], "params", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "year", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "month", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "day", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "to_string", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "weekday", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "iso_weekday", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "day_of_year", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "iso_week", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "quarter_of_year", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "days_in_month", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "is_leap_year", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "replace", &[false, false, false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "add_days", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "add_months", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "diff_days", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "add_period", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "truncate", &[true]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "format", &[true]),
    CoreCallRecord::receiver(&["LocalTime"], "hour", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "minute", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "second", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "to_string", &[]),
    CoreCallRecord::receiver(&["DateTime"], "to_timestamp", &[]),
    CoreCallRecord::receiver(&["DateTime"], "to_unix_ms", &[]),
    CoreCallRecord::receiver(&["DateTime"], "to_string", &[]),
    CoreCallRecord::receiver(&["DateTime"], "date", &[]),
    CoreCallRecord::receiver(&["DateTime"], "time", &[]),
    CoreCallRecord::receiver(&["DateTime"], "hour", &[]),
    CoreCallRecord::receiver(&["DateTime"], "minute", &[]),
    CoreCallRecord::receiver(&["DateTime"], "second", &[]),
    CoreCallRecord::receiver(&["DateTime"], "millisecond", &[]),
    CoreCallRecord::receiver(&["DateTime"], "microsecond", &[]),
    CoreCallRecord::receiver(&["DateTime"], "nanosecond", &[]),
    CoreCallRecord::receiver(&["DateTime"], "format_rfc3339", &[]),
    CoreCallRecord::receiver(&["DateTime"], "format", &[true]),
    CoreCallRecord::receiver(&["DateTime"], "plus_duration", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "difference", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "truncate", &[true]),
    CoreCallRecord::receiver(&["DateTime"], "round", &[true]),
    CoreCallRecord::receiver(&["DateTime"], "floor", &[true]),
    CoreCallRecord::receiver(&["DateTime"], "ceil", &[true]),
    CoreCallRecord::receiver(&["DateTime"], "replace", &[false, false, false, false, false, false]),
    CoreCallRecord::receiver(&["DateTime"], "in_zone", &[false]),
    CoreCallRecord::receiver(&["Instant"], "elapsed_millis", &[]),
    CoreCallRecord::receiver(&["Instant"], "elapsed", &[]),
    CoreCallRecord::receiver(&["Zone"], "name", &[]),
    CoreCallRecord::receiver(&["Fraction"], "to_string", &[]),
    CoreCallRecord::receiver(&["Fraction"], "numerator", &[]),
    CoreCallRecord::receiver(&["Fraction"], "denominator", &[]),
    CoreCallRecord::receiver(&["Fraction"], "to_float", &[]),
    CoreCallRecord::receiver(&["Fraction"], "is_zero", &[]),
    CoreCallRecord::receiver(&["Fraction"], "equal", &[false]),
    CoreCallRecord::receiver(&["Fraction"], "add", &[false]),
    CoreCallRecord::receiver(&["Fraction"], "sub", &[false]),
    CoreCallRecord::receiver(&["Fraction"], "mul", &[false]),
    CoreCallRecord::receiver(&["Fraction"], "div", &[false]),
    CoreCallRecord::receiver(&["Decimal"], "to_string", &[]),
    CoreCallRecord::receiver(&["Decimal"], "add", &[false]),
    CoreCallRecord::receiver(&["Decimal"], "sub", &[false]),
    CoreCallRecord::receiver(&["Decimal"], "mul", &[false]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "date", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "time", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "offset_seconds", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "is_dst", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "to_datetime", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "zone", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "to_string", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "format", &[true]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "add_duration", &[false]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "add_period", &[false]),
    CoreCallRecord::receiver(&["Period"], "to_string", &[]),
    CoreCallRecord::receiver(&["Measurement"], "value", &[]),
    CoreCallRecord::receiver(&["Measurement"], "uncertainty", &[]),
    CoreCallRecord::receiver(&["Measurement"], "add", &[false]),
    CoreCallRecord::receiver(&["Measurement"], "sub", &[false]),
    CoreCallRecord::receiver(&["Measurement"], "mul", &[false]),
    CoreCallRecord::receiver(&["Measurement"], "div", &[false]),
    CoreCallRecord::receiver(&["HyperLogLog"], "new", &[]),
    CoreCallRecord::receiver(&["HyperLogLog"], "add", &[true]),
    CoreCallRecord::receiver(&["HyperLogLog"], "count", &[]),
    CoreCallRecord::receiver(&["TDigest"], "new", &[]),
    CoreCallRecord::receiver(&["TDigest"], "add", &[false]),
    CoreCallRecord::receiver(&["TDigest"], "quantile", &[false]),
    CoreCallRecord::receiver(&["CountMinSketch"], "new", &[]),
    CoreCallRecord::receiver(&["CountMinSketch"], "add", &[true]),
    CoreCallRecord::receiver(&["CountMinSketch"], "count", &[true]),
    CoreCallRecord::receiver(&["ReservoirSampler"], "new", &[false]),
    CoreCallRecord::receiver(&["ReservoirSampler"], "add", &[true]),
    CoreCallRecord::receiver(&["ReservoirSampler"], "sample", &[]),
    CoreCallRecord::receiver(&["Solver"], "new", &[false]),
    CoreCallRecord::receiver(&["Solver"], "require", &[false]),
    CoreCallRecord::receiver(&["Solver"], "failure_count", &[]),
    CoreCallRecord::receiver(&["Solver"], "status", &[]),
    CoreCallRecord::receiver(
        &[
            "HyperLogLog",
            "TDigest",
            "CountMinSketch",
            "ReservoirSampler",
            "Solver",
            "ServiceUpgradeReceipt",
            "DataError",
        ],
        "__display",
        &[],
    ),
];

/// The one canonical lookup used by all plain Core-call projections.
pub fn core_call(module: &str, member: &str) -> Option<&'static CoreCallRecord> {
    CORE_CALLS
        .iter()
        .find(|row| row.receiver_types.is_empty() && row.module == module && row.member == member)
}

/// Find one receiver/static-method projection from the same Core registry.
pub fn core_receiver_method(
    receiver_type: &str,
    member: &str,
) -> Option<&'static CoreCallRecord> {
    CORE_CALLS.iter().find(|row| {
        row.member == member && row.receiver_types.iter().any(|name| *name == receiver_type)
    })
}

/// A generic table lookup used by tests and future projections. A new record
/// is therefore data-only: no consumer match arm is needed to make it visible.
pub fn core_call_in<'a>(
    rows: &'a [CoreCallRecord],
    module: &str,
    member: &str,
) -> Option<&'a CoreCallRecord> {
    rows.iter()
        .find(|row| row.module == module && row.member == member)
}
