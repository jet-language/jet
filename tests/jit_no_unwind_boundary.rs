//! D-JITUNWIND1 (#1995 / #1997): the mechanical check behind the JIT's
//! no-unwind boundary, plus the cross-tier regression it protects.
//!
//! The invariant: **no Rust panic may be raised while a Cranelift frame is on
//! the stack, and no `extern "C"` frame `crates/jet-jit` exposes to generated
//! code may be reached by an unwind.** `cranelift-jit` registers no unwind
//! information for the code it emits, so a panic above a JIT frame cannot be
//! walked past — libgcc's phase 1 returns `_URC_END_OF_STACK` and the process
//! rtaborts with `fatal runtime error: failed to initiate panic, error 5`
//! before any outer `catch_unwind` can see it. A bare `SIGABRT` with no text is
//! not one of Jet's exit codes.
//!
//! The chosen mechanism is boundary conversion, and its recorded cost is that
//! the guarantee has to hold across ~1.7k host symbols — "so it needs a
//! mechanical check, not review discipline." This file is that check. The
//! decision record is `crates/jet-jit/src/host_seam.rs` and
//! `docs/spec/architecture.md` R13.
//!
//! Why the checks below and not "does each seam catch?": the guarantee is
//! structural, not per-body.
//!
//! 1. A host seam is a plain Rust `fn`. rustc gives an `extern "C"` **body** an
//!    abort-on-unwind shim, so a panic inside one dies at that body's own edge
//!    as `thread caused non-unwinding panic` — a *different* abort from error
//!    5, and one no wrapper above it could ever catch. So a boundary can only
//!    be added by replacing the C frame. **Any** `extern` fn defined in this
//!    crate is therefore an unguardable seam by construction, whatever its ABI
//!    and whatever it is called; the one exception is stated positively, as the
//!    shim that *is* the boundary. The first form of this check banned
//!    `extern "C" fn jet_*` only, and `Ffi.rs`'s `jit_ffi_reporter` — the frame
//!    a foreign library enters when a foreign function has already failed, with
//!    a Cranelift frame below it — sat outside that family for as long as the
//!    family was the rule. There used to be a second exception, the OS signal
//!    handler, because this crate compiled its own copy of it; #2027 deleted
//!    that copy, so the handler is now checked where it actually lives (see
//!    `SIGNAL_HANDLER_OWNER`) and a copy reappearing here is a bug on both
//!    counts.
//! 2. The only `extern "C"` frames generated code can reach are the shims
//!    `host_seam::guarded` builds, and the only ways to reach one are the
//!    single `builder.symbol` call inside `host_fns!` and `guarded_addr`. Any
//!    other route from a host function to an address is an escape.
//! 3. The generator must still generate the guard. Without this, reverting one
//!    line of `host_fns!` would silently unguard every symbol and leave every
//!    other test green — and the same is true of the shim's own body, which is
//!    why check 1's `host_seam.rs` exemption is pinned rather than assumed.
//! 4. An abort may never become a *ledger row*. The structural checks above
//!    cover the frames this crate compiles; the example corpus is where an
//!    escape actually shows up, and every ratcheted section of
//!    `tests/jit_corpus_gate.txt` is shrink-only, so a filed abort is an abort
//!    under permanent protection. `corpus_gate_refuse_abort`
//!    (`tests/dev_parts/support.rs`) refuses to classify one on any tier, using
//!    the `ABORT_MARKERS` list below so the two checks cannot drift.
//!
//! Run: `scripts/agent/jet-env cargo test --test jit_no_unwind_boundary`

mod common;

/// The abort markers come from `tests/common` so this suite and the
/// example-corpus gate cannot drift: this suite proves one cancelled task never
/// aborts, and `corpus_gate_refuse_abort` proves no stem in the whole corpus may
/// be *classified* as one.
use common::ABORT_MARKERS;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Every `.rs` file under `crates/jet-jit/src`, as `(label, contents)`.
fn jit_crate_sources() -> Vec<(String, String)> {
    let root = repo_root().join("crates/jet-jit/src");
    assert!(root.is_dir(), "jet-jit source root is missing: {root:?}");
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);
    files.sort();
    assert!(
        files.len() > 20,
        "the jet-jit scan found only {} files; the scan root moved",
        files.len()
    );
    files
        .into_iter()
        .map(|path| {
            let label = path
                .strip_prefix(&root)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {path:?}: {error}"));
            (label, text)
        })
        .collect()
}

/// Line-comment lines are skipped: this file, `host_seam.rs`, and several
/// module docs all *describe* the banned forms, and describing them is the
/// point. Everything else is scanned verbatim.
fn code_lines(sources: &[(String, String)]) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    for (label, text) in sources {
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            out.push((label.clone(), index + 1, line.to_string()));
        }
    }
    out
}

/// The `extern` fn *definitions* this crate compiles, as
/// `(label, line, name, text)`.
///
/// A `fn`-pointer type (`type Pred = unsafe extern "C" fn(i64) -> i8`) and a
/// foreign declaration inside an `unsafe extern "C" { … }` block both have no
/// name in this position, so neither matches. What is left is exactly the set of
/// C frames this crate itself puts on the stack.
fn extern_fn_definitions(sources: &[(String, String)]) -> Vec<(String, usize, String, String)> {
    let mut out = Vec::new();
    for (label, number, line) in code_lines(sources) {
        let Some(at) = line.find("extern \"") else { continue };
        let rest = &line[at + 8..];
        let Some(close) = rest.find('"') else { continue };
        let tail = rest[close + 1..].trim_start();
        let Some(tail) = tail.strip_prefix("fn ") else { continue };
        // `$` is a name here too: `extern "C" fn $name(…)` inside a macro is the
        // same unguardable body wearing a metavariable, and it must not read as
        // an anonymous `fn`-pointer type and slip past.
        let name: String = tail
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
            .collect();
        if name.is_empty() {
            continue;
        }
        out.push((label, number, name, line.trim().to_string()));
    }
    out
}

/// Check 1: an `extern "C"` seam body cannot be guarded from outside, so a C
/// frame this crate compiles is an unguardable seam unless it *is* the boundary.
///
/// The permitted set is stated positively and is now ONE entry long:
/// `host_seam.rs` holds the generated shim — the boundary itself.
///
/// It used to be two, because this crate compiled its own copy of the OS signal
/// handler. #2027 deleted that copy: the one signal handler lives in
/// [`SIGNAL_HANDLER_OWNER`] and this crate marshals to it, so a C frame named
/// `unix_mark`/`windows_mark`/`jet_interrupt_mark` appearing here again is a
/// re-fork of the handler AND an unguardable seam, and check 1 must say so.
///
/// Every other `extern` fn definition is a hole, whatever it is called. The
/// previous form of this check banned `extern "C" fn jet_*` only, and
/// `Ffi.rs`'s `jit_ffi_reporter` — a C frame the bridge calls when a foreign
/// function already failed, with a Cranelift frame below it — sat outside that
/// name for exactly as long as the name was the rule (#1995).
fn unguardable_seam_definitions(sources: &[(String, String)]) -> Vec<String> {
    extern_fn_definitions(sources)
        .into_iter()
        .filter(|(label, _, _, _)| label.as_str() != "host_seam.rs")
        .map(|(label, number, _, line)| format!("{label}:{number}: {line}"))
        .collect()
}

/// The one signal handler's home (#2027). It is not in this crate and must not
/// come back: a signal handler runs on a borrowed stack that may have a JIT
/// frame under it, and `guard_seam` is the wrong tool there —
/// `jet_scheduler_install_panic_hook` takes the process panic-hook lock and
/// allocates on first call, so catching inside a handler would trade an
/// unreachable panic for a reachable deadlock. The guarantee is that the body
/// cannot panic, and it is checked, not stated.
///
/// One file, because there is one handler: the Prelude owner AOT embeds in the
/// generated program and `jet_codegen::interrupt_runtime` compiles once for the
/// `jet` binary's interpreter ambient and Cranelift host alike.
const SIGNAL_HANDLER_OWNER: &str = "crates/jet-codegen/src/Prelude/CoreLib/Top/Interrupt.rs";

/// The one handler's name. Both platform arms carry it, under mutually
/// exclusive `cfg`s, so the process has one mark whatever it is built for.
const SIGNAL_HANDLER: &str = "jet_interrupt_mark";

/// The complete set of statements a signal handler body may contain. Anything
/// that allocates, locks, formats, or reports would raise a panic in a frame
/// that may sit above JIT code — and would do it from a context where the
/// conversion boundary cannot be used.
const SIGNAL_HANDLER_STATEMENTS: &[&str] = &[
    "JET_INTERRUPT_PENDING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);",
    "const CTRL_C_EVENT: u32 = 0;",
    "if kind == CTRL_C_EVENT {",
    "1",
    "} else {",
    "0",
    "}",
];

/// The signal handler's own file, as the one-entry `(label, contents)` list the
/// scanners above take.
fn signal_handler_owner_source() -> Vec<(String, String)> {
    let path = repo_root().join(SIGNAL_HANDLER_OWNER);
    let text =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path:?}: {error}"));
    vec![(SIGNAL_HANDLER_OWNER.to_string(), text)]
}

/// The statements inside the definition that starts on `line_index`: the text
/// between its opening brace and the matching close, trimmed, blank lines and
/// line comments dropped.
fn definition_body(text: &str, line_index: usize) -> Vec<String> {
    let from: String = text
        .lines()
        .skip(line_index - 1)
        .collect::<Vec<_>>()
        .join("\n");
    let open = from.find('{').expect("an extern fn definition has a body");
    let mut depth = 0usize;
    let mut end = from.len();
    for (at, ch) in from[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + at;
                    break;
                }
            }
            _ => {}
        }
    }
    from[open + 1..end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .map(str::to_string)
        .collect()
}

/// Check 2: a host function reaching an address by any route other than
/// `host_seam::guarded` / `guarded_addr` is an unguarded boundary.
///
/// Both name families this crate uses, not just one: the seams `host_fns!`
/// declares are `jet_*`, and the callbacks it hands to foreign libraries are
/// `jit_*` (`Ffi.rs`). Scanning one prefix let the second family escape (#1995).
fn host_address_escapes(sources: &[(String, String)]) -> Vec<String> {
    code_lines(sources)
        .into_iter()
        .filter(|(label, _, _)| label.as_str() != "host_seam.rs")
        .filter(|(_, _, line)| {
            // Every cast on the line, not just the first: `a as usize as
            // *const u8` and `f(x as usize, jet_seam as *const u8)` both occur.
            line.match_indices(" as ").any(|(at, _)| {
                let tail = line[at + 4..].trim_start();
                if !(tail.starts_with("usize")
                    || tail.starts_with("*const")
                    || tail.starts_with("*mut"))
                {
                    return false;
                }
                // The cast operand is the identifier just before ` as `. A
                // parenthesised operand ends in `)`, which yields an empty
                // token: those already went through `guarded_addr`.
                let operand = line[..at]
                    .trim_end()
                    .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("");
                operand.starts_with("jet_") || operand.starts_with("jit_")
            })
        })
        .map(|(label, number, line)| format!("{label}:{number}: {}", line.trim()))
        .collect()
}

/// Check 3: exactly one place registers a JIT host symbol, and it registers the
/// guard rather than the seam.
fn symbol_registrations(sources: &[(String, String)]) -> Vec<String> {
    code_lines(sources)
        .into_iter()
        .filter(|(_, _, line)| line.contains(".symbol("))
        .map(|(label, number, line)| format!("{label}:{number}: {}", line.trim()))
        .collect()
}

#[test]
fn no_unguardable_extern_c_seam_is_defined_in_the_jit_crate() {
    let offenders = unguardable_seam_definitions(&jit_crate_sources());
    assert!(
        offenders.is_empty(),
        "an `extern \"C\"` frame this crate compiles cannot be guarded from \
         outside: rustc aborts an escaping unwind at the body's own edge \
         (`thread caused non-unwinding panic`), before the shim \
         `host_seam::guarded` builds could catch it. Both directions are bugs, \
         so say which rail the new definition belongs on:\n\
         \x20 - generated code calls it -> make it a plain `fn` and let \
         `host_fns!` generate its boundary;\n\
         \x20 - foreign code calls it -> make it a plain `fn` and hand out \
         `host_seam::guarded(f)`, as `Ffi.rs` does for the bridge reporter;\n\
         \x20 - the kernel calls it -> it is a signal handler, and there is \
         exactly one, in `SIGNAL_HANDLER_OWNER`; marshal to it instead of \
         re-forking it here (#2027).\n\
         (crates/jet-jit/src/host_seam.rs, docs/spec/architecture.md R13.) \
         Offending definitions:\n{}",
        offenders.join("\n")
    );
}

/// The signal handler is entered by the kernel, not by generated code, so it
/// cannot be routed through the boundary — and it may interrupt a JIT frame, so
/// a panic raised in one is the very abort this file exists to prevent. The only
/// guarantee left is that the body cannot panic. Pin it, at the one owner.
#[test]
fn the_os_signal_handler_stays_panic_free_without_a_catch() {
    let sources = signal_handler_owner_source();
    let handlers: Vec<_> = extern_fn_definitions(&sources)
        .into_iter()
        .filter(|(_, _, name, _)| name == SIGNAL_HANDLER)
        .collect();
    assert_eq!(
        handlers.len(),
        2,
        "the one signal handler has exactly two platform arms — `extern \"C\"` \
         for unix and `extern \"system\"` for windows, both named \
         `{SIGNAL_HANDLER}` under mutually exclusive `cfg`s. Found {:?} in \
         {SIGNAL_HANDLER_OWNER}. A third arm, or a missing one, means the mark \
         was renamed or re-forked and this proof stopped covering it.",
        handlers
            .iter()
            .map(|(_, number, name, _)| format!("{name}:{number}"))
            .collect::<Vec<_>>()
    );
    for (label, number, name, _) in handlers {
        for statement in definition_body(&sources[0].1, number) {
            assert!(
                SIGNAL_HANDLER_STATEMENTS.contains(&statement.as_str()),
                "{label}:{number}: `{name}` gained the statement `{statement}`, \
                 which is not one of {:?}. A signal handler runs on a borrowed \
                 stack that may have a JIT frame under it: anything that \
                 allocates, locks, formats or reports can panic there, and \
                 `guard_seam` cannot be used to catch it. Move the work to the \
                 drain (`jet_interrupt_dispatch`) instead of widening this list.",
                SIGNAL_HANDLER_STATEMENTS
            );
        }
    }
}

/// #2027: the mechanism itself is singular, not just panic-free.
///
/// One pending count, one arm path, one mark, and exactly one place that
/// compiles the owner for in-binary use. Before this check the handler existed
/// in three copies with three counts, and because `signal(SIGINT, …)` REPLACES
/// the process handler, whichever tier armed last silently disarmed the others'
/// counts — a divergence nothing could detect. Re-forking it must fail here
/// instead.
#[test]
fn exactly_one_signal_mechanism_owns_the_interrupt_count() {
    let mut sources = Vec::new();
    for root in [repo_root().join("crates"), repo_root().join("Source")] {
        assert!(root.is_dir(), "scan root moved: {root:?}");
        let mut files = Vec::new();
        collect_rs_files(&root, &mut files);
        files.sort();
        for path in files {
            let label = path
                .strip_prefix(repo_root())
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {path:?}: {error}"));
            sources.push((label, text));
        }
    }
    let owner_label = SIGNAL_HANDLER_OWNER.to_string();
    assert!(
        sources.iter().any(|(label, _)| *label == owner_label),
        "the scan did not reach the owner {SIGNAL_HANDLER_OWNER}"
    );

    // The count, the arm state and the mark are declared only by the owner, and
    // each one is actually there — a rename that made the scan find nothing
    // would otherwise pass this check vacuously.
    let mark_definition = format!("fn {SIGNAL_HANDLER}(");
    for (fact, expected) in [
        ("static JET_INTERRUPT_PENDING", 1usize),
        ("static JET_INTERRUPT_ARMED", 1),
        (mark_definition.as_str(), 2),
    ] {
        let declarers: Vec<_> = code_lines(&sources)
            .into_iter()
            .filter(|(_, _, line)| line.contains(fact))
            .map(|(label, number, line)| format!("{label}:{number}: {}", line.trim()))
            .collect();
        assert!(
            declarers.len() == expected
                && declarers
                    .iter()
                    .all(|entry| entry.starts_with(&owner_label)),
            "`{fact}` belongs to the one signal mechanism in \
             {SIGNAL_HANDLER_OWNER} (#2027), {expected} time(s) — the mark has \
             one `cfg`-exclusive arm per platform, everything else is single. A \
             second declaration elsewhere is a second interrupt count, and \
             `signal(SIGINT, …)` REPLACES the process handler, so the two would \
             not merely duplicate: the later arm would silently disarm the \
             earlier one's count. Marshal to \
             `jet_interrupt_arm`/`jet_interrupt_dispatch` instead. Found:\n{}",
            declarers.join("\n")
        );
    }

    // Exactly one module compiles the owner for in-binary use, so the `jet`
    // binary's interpreter ambient and Cranelift host share one count. AOT's
    // copy is a separate process and comes from `Codegen/mod.rs`'s
    // `include_str!`, which embeds text rather than compiling a second instance
    // here.
    let includers: Vec<_> = code_lines(&sources)
        .into_iter()
        .filter(|(_, _, line)| {
            line.contains("include!(") && line.contains("Top/Interrupt.rs")
        })
        .map(|(label, number, line)| format!("{label}:{number}: {}", line.trim()))
        .collect();
    assert_eq!(
        includers.len(),
        1,
        "the owner must be compiled exactly once per binary — \
         `jet_codegen::interrupt_runtime`. Each extra `include!` is another \
         `JET_INTERRUPT_PENDING` static in the same process (#2027). Found:\n{}",
        includers.join("\n")
    );
    assert!(
        includers[0].starts_with("crates/jet-codegen/src/lib.rs:"),
        "the one in-binary instance lives in `jet-codegen/src/lib.rs` as \
         `pub mod interrupt_runtime`; found {}",
        includers[0]
    );
}

/// #2027: what a signal does inside `#Shield` is answered once, by construction.
///
/// A shield defers a *cooperative* interrupt — a cancel or a blown deadline — at
/// the wait points of the shielded task
/// (`Prelude/Scheduler.rs::jet_scheduler_shielded`). An OS signal is not a
/// wait-point outcome: the mark only increments a count, and the drain runs
/// every registered handler once per counted interrupt. So a signal delivered
/// while a task is inside `#Shield { … }` is neither deferred to the region's
/// exit nor discarded — it is counted, and the handlers run on the next drain
/// while the shielded region keeps running.
///
/// All three tiers answer that identically because they call this one drain and
/// it cannot see shield or task state. Pin the "cannot", so the three tiers
/// agree by construction rather than by three copies happening to match.
#[test]
fn the_interrupt_drain_cannot_consult_shield_or_task_state() {
    let owner = signal_handler_owner_source();
    let leaks: Vec<_> = code_lines(&owner)
        .into_iter()
        .filter(|(_, _, line)| {
            ["shield", "SHIELD", "task_cancelled", "current_task", "panicking"]
                .iter()
                .any(|probe| line.contains(probe))
        })
        .map(|(label, number, line)| format!("{label}:{number}: {}", line.trim()))
        .collect();
    assert!(
        leaks.is_empty(),
        "the one interrupt drain read shield or task state. That is how the \
         three tiers stop agreeing: a signal delivered inside `#Shield` would \
         then be deferred on whichever tier happened to be executing, and \
         counted on the others. Shield policy belongs to cooperative wait \
         points (`Prelude/Scheduler.rs`), not to the signal count (#2027). \
         Offending lines:\n{}",
        leaks.join("\n")
    );
}

#[test]
fn no_jit_host_address_escapes_the_generated_boundary() {
    let offenders = host_address_escapes(&jit_crate_sources());
    assert!(
        offenders.is_empty(),
        "a `jet_*` host function reached an address without the no-unwind \
         boundary. Generated code that calls it would have an unguarded seam \
         below it. Use `host_seam::guarded_addr(...)` (#1997). Offending \
         casts:\n{}",
        offenders.join("\n")
    );
}

/// #1997 criterion 9: `Concurrency.rs` is the named call-site inventory for
/// scheduler control transfers. Its production host bodies must not grow a
/// bare panic/expect/unwrap: those either abort below a JIT frame or bypass
/// the shared status conversion. The test module has ordinary Rust assertions
/// by design, so stop the scan at its `#[cfg(test)]` boundary instead of
/// weakening the rule to an allowlist of test line numbers.
#[test]
fn concurrency_host_bodies_have_no_bare_panic_sites() {
    let path = repo_root().join("crates/jet-jit/src/Concurrency.rs");
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {path:?}: {error}"));
    let marker = "\n#[cfg(test)]\nmod tests {";
    let (production, _) = source.split_once(marker).unwrap_or_else(|| {
        panic!(
            "{path:?} lost its cfg(test) boundary; the panic-site check would scan the wrong scope"
        )
    });
    let offenders: Vec<String> = production
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                return None;
            }
            (line.contains("panic!(")
                || line.contains(".expect(")
                || line.contains(".unwrap("))
                .then(|| format!("Concurrency.rs:{}: {}", index + 1, line.trim()))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "a Concurrency host body gained a bare panic site below a JIT frame; \
         route the outcome through `wait_status`, `contain_seam_unwind`, or \
         the generated `host_seam` boundary:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_host_fns_macro_still_generates_the_boundary() {
    let sources = jit_crate_sources();
    let registrations = symbol_registrations(&sources);
    assert_eq!(
        registrations.len(),
        1,
        "JIT host symbol registration must stay in the one `host_fns!` \
         expansion that generates the no-unwind boundary; found:\n{}",
        registrations.join("\n")
    );
    let lib = sources
        .iter()
        .find(|(label, _)| label.as_str() == "lib.rs")
        .map(|(_, text)| text.as_str())
        .expect("crates/jet-jit/src/lib.rs");
    assert!(
        lib.contains("builder.symbol($symbol, $crate::host_seam::guarded($host_fn));"),
        "`host_fns!` must register the generated guard. Registering the seam \
         address directly silently unguards every host symbol at once and no \
         other test can see it (#1997)."
    );
    assert!(
        !lib.contains("builder.symbol($symbol, $host_fn as *const u8)"),
        "the pre-#1997 unguarded registration is back in `host_fns!`"
    );
    // `host_seam.rs` is the one file check 1 exempts, because the shim it
    // generates is the boundary. That exemption is only sound while the shim
    // still runs the seam inside the conversion, so pin the body: without this,
    // dropping `guard_seam` from the shim would unguard every symbol at once and
    // leave the exemption looking like a rule.
    let seam = sources
        .iter()
        .find(|(label, _)| label.as_str() == "host_seam.rs")
        .map(|(_, text)| text.as_str())
        .expect("crates/jet-jit/src/host_seam.rs");
    assert!(
        seam.contains("guard_seam(move || zero_sized_callee::<F>()"),
        "the generated shim must still run the seam inside `guard_seam`; \
         without it every `extern \"C\"` shim is a bare unguarded seam again \
         and check 1's `host_seam.rs` exemption hides it (#1997)."
    );
    assert!(
        seam.contains("jet_scheduler_catch_foreign_boundary"),
        "`guard_seam` must keep converting through the shared Prelude boundary \
         rather than a second engine-local catch (I8/I9)."
    );
}

/// The checks above only earn their keep if they actually trip. Seeded text,
/// not seeded source: this proves the scanning logic without adding and
/// reverting a real hole in the crate.
#[test]
fn the_boundary_scan_trips_on_seeded_violations() {
    let seeded_seam = vec![(
        "Seeded.rs".to_string(),
        "extern \"C\" fn jet_jit_seeded(a: i64) -> i64 { a }\n".to_string(),
    )];
    assert_eq!(
        unguardable_seam_definitions(&seeded_seam).len(),
        1,
        "the seam scan must catch a hand-written `extern \"C\" fn jet_*`"
    );

    let seeded_macro_seam = vec![(
        "Seeded.rs".to_string(),
        "        extern \"C\" fn $name(handle: i64) -> i64 { handle }\n".to_string(),
    )];
    assert_eq!(
        unguardable_seam_definitions(&seeded_macro_seam).len(),
        1,
        "the seam scan must catch a macro-generated `extern \"C\"` seam"
    );

    // The name is not the rule: `Ffi.rs`'s bridge reporter was an `extern "C"`
    // body outside the `jet_*` family for as long as the family was the rule.
    let seeded_foreign_callback = vec![(
        "Seeded.rs".to_string(),
        "extern \"C\" fn jit_ffi_seeded(m: *const u8, n: usize) { report(m, n) }\n".to_string(),
    )];
    assert_eq!(
        unguardable_seam_definitions(&seeded_foreign_callback).len(),
        1,
        "the seam scan must catch an `extern \"C\"` callback handed to a foreign \
         library, whatever it is called (#1995)"
    );

    // A `"system"` handler and a `"C-unwind"` body are the same hole in a
    // different ABI, and both must be seen.
    let seeded_other_abis = vec![(
        "Seeded.rs".to_string(),
        concat!(
            "    unsafe extern \"system\" fn seeded_mark(kind: u32) -> i32 { 0 }\n",
            "    extern \"C-unwind\" fn seeded_unwinding(a: i64) -> i64 { a }\n",
        )
        .to_string(),
    )];
    assert_eq!(
        unguardable_seam_definitions(&seeded_other_abis).len(),
        2,
        "the seam scan must not be ABI-specific"
    );

    // A `fn`-pointer TYPE is not a body: this crate transmutes JIT code
    // addresses to these on every callback, and flagging them would be a wrong
    // refusal, which is equally a bug.
    let type_only = vec![(
        "Seeded.rs".to_string(),
        concat!(
            "type SpawnFn2 = extern \"C\" fn(i64, i64) -> i64;\n",
            "    let f: unsafe extern \"C\" fn(i64) -> i8 = std::mem::transmute(ptr);\n",
        )
        .to_string(),
    )];
    assert!(
        unguardable_seam_definitions(&type_only).is_empty(),
        "the seam scan must not fire on a `fn`-pointer type: {:?}",
        unguardable_seam_definitions(&type_only)
    );

    // The signal-handler body scan must trip on a statement that can panic.
    let grown_handler = vec![(
        SIGNAL_HANDLER_OWNER.to_string(),
        concat!(
            "    extern \"C\" fn jet_interrupt_mark(_: i32) {\n",
            "        eprintln!(\"interrupt\");\n",
            "        JET_INTERRUPT_PENDING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);\n",
            "    }\n",
        )
        .to_string(),
    )];
    let (_, number, _, _) = extern_fn_definitions(&grown_handler)
        .into_iter()
        .find(|(_, _, name, _)| name == SIGNAL_HANDLER)
        .expect("the seeded handler");
    assert!(
        definition_body(&grown_handler[0].1, number)
            .iter()
            .any(|statement| !SIGNAL_HANDLER_STATEMENTS.contains(&statement.as_str())),
        "the handler body scan must catch a statement that can panic"
    );

    let seeded_escape = vec![(
        "Seeded.rs".to_string(),
        "    let ptr = jet_jit_seeded as usize as i64;\n".to_string(),
    )];
    assert_eq!(
        host_address_escapes(&seeded_escape).len(),
        1,
        "the escape scan must catch a raw host address handed to generated code"
    );

    // ... and must not fire on the guarded form, or on the JIT-code pointers
    // the hosts legitimately transmute in the other direction.
    let allowed = vec![(
        "Seeded.rs".to_string(),
        concat!(
            "    let a = crate::host_seam::guarded_addr(jet_jit_seeded) as i64;\n",
            "    let b = cb.fn_ptr as usize as *const u8;\n",
            "    let c = table[idx] as *const u8;\n",
        )
        .to_string(),
    )];
    assert!(
        host_address_escapes(&allowed).is_empty(),
        "the escape scan must not fire on the guarded form or on host->JIT \
         pointer reads: {:?}",
        host_address_escapes(&allowed)
    );

    let seeded_comment = vec![(
        "Seeded.rs".to_string(),
        "//! an `extern \"C\" fn jet_jit_example` in prose is documentation\n".to_string(),
    )];
    assert!(
        unguardable_seam_definitions(&seeded_comment).is_empty(),
        "the seam scan must ignore prose that names the banned form"
    );
}

/// The marker list is now load-bearing twice over: this suite asserts one
/// cancelled task never aborts, and `corpus_gate_refuse_abort` asserts no stem
/// in the example corpus may be *classified* as an abort. So prove the list
/// recognises every abort family by its real text, and — the half that matters
/// more, because this list can fail a whole corpus — prove it stays quiet on an
/// ordinary Jet report.
#[test]
fn the_abort_markers_name_every_abort_and_no_report() {
    for stderr in [
        "fatal runtime error: failed to initiate panic, error 5, aborting\n",
        "thread caused non-unwinding panic. aborting.\n",
        "panicked at library/core/src/panicking.rs: panic in a function that \
         cannot unwind\n",
        "panicked at library/core/src/panicking.rs:233:5:\npanic in a destructor \
         during cleanup\n",
        "thread 'main' has overflowed its stack\nfatal runtime error: stack \
         overflow\n",
        "   2: core::panicking::panic_fmt\n",
    ] {
        assert!(
            common::abort_marker(stderr).is_some(),
            "an abort went unrecognised, so it could be filed as an outcome \
             instead of failing: {stderr:?}"
        );
    }

    // Every shape a real Jet stop takes, including the words `panic` and
    // `runtime`: a marker that fired here would refuse the whole example corpus
    // over correct output, which is the more expensive direction to get wrong.
    for stderr in [
        "",
        "Stop [E3001]: `panic: expected the answer, got none`\n",
        "Error: file not found\n Trail [E3002] (2 hops via ?, origin first):\n  \
         1. load (app.jet:7) — reading config\n  2. run (app.jet:12)\n",
        "Error [E3003]: deadline exceeded while waiting in task join\n",
        "Runtime fault [E3010]: the list has 2 items\n",
        "panic: can't view 1 items from 2 to 2 (inclusive)\n",
    ] {
        assert_eq!(
            common::abort_marker(stderr),
            None,
            "a Jet report was read as an abort: {stderr:?}"
        );
    }
}

fn jet_run(args: &[&str], cache: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("JET_RUN_CACHE_DIR", cache)
        .current_dir(repo_root())
        .output()
        .expect("run the jet driver")
}

#[cfg(unix)]
fn terminating_signal(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn terminating_signal(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

/// #1995 criterion 4. A cancelled task unwinds at its next wait point
/// (`Prelude/Scheduler.rs::jet_task_deliver_cancel`), and under default
/// `jet run` that wait point can sit inside `jet_deopt_call`, i.e. below a
/// Cranelift frame. The outcome must be an exit code and a report, never a
/// signal — on every tier, and with the same observable result on each, because
/// a silent deopt otherwise hands back the right answer from the wrong tier.
///
/// The program is the shipped `task_controls` example rather than an invented
/// one, so the expected bytes are the existing golden: it pauses, resumes and
/// cancels a task handle, and `task.any` cancels the losing sleeper — a cancel
/// delivered at a real wait point.
#[test]
fn a_cancelled_task_reaches_an_exit_code_on_every_tier_never_a_signal() {
    let example = repo_root().join("examples/features/concurrency/task_controls.jet");
    let golden = repo_root().join("examples/features/expected/concurrency/task_controls.out");
    let expected = fs::read_to_string(&golden).expect("task_controls golden");
    let shown = example.to_string_lossy().into_owned();

    let cache_root = std::env::temp_dir().join(format!("jet_no_unwind_{}", std::process::id()));
    let mut observed: Vec<(&str, String, i32)> = Vec::new();

    for (tier, args) in [
        ("default jet run", vec!["run", shown.as_str()]),
        ("forced interpreter", vec!["run", "--interpret", shown.as_str()]),
    ] {
        let cache = cache_root.join(tier.replace(' ', "_"));
        fs::create_dir_all(&cache).expect("cache dir");
        let output = jet_run(&args, &cache);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        assert!(
            terminating_signal(&output.status).is_none(),
            "{tier}: a cancelled task killed the process with signal {:?} \
             instead of producing a report. An abort is never an outcome \
             (#1995/#1997, docs/spec/architecture.md R13).\nstdout:\n{stdout}\nstderr:\n{stderr}",
            terminating_signal(&output.status)
        );
        for marker in ABORT_MARKERS {
            assert!(
                !stderr.contains(marker),
                "{tier}: stderr carries the abort marker `{marker}`, so a \
                 control transfer escaped a host boundary instead of being \
                 converted.\nstderr:\n{stderr}"
            );
        }
        let code = output
            .status
            .code()
            .expect("a non-signalled process always has a code");
        assert_eq!(
            code, 0,
            "{tier}: task_controls must complete.\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert_eq!(
            stdout, expected,
            "{tier}: stdout drifted from the shipped golden"
        );
        observed.push((tier, stdout, code));
    }

    let (first_tier, first_stdout, first_code) = &observed[0];
    for (tier, stdout, code) in &observed[1..] {
        assert_eq!(
            stdout, first_stdout,
            "{tier} disagreed with {first_tier} on stdout"
        );
        assert_eq!(
            code, first_code,
            "{tier} disagreed with {first_tier} on the exit code"
        );
    }

    let _ = fs::remove_dir_all(&cache_root);
}

/// #1997 criterion 8. The cancelled child is deliberately named-deopted by
/// `core.text.casefold` in an interpolated return; its wait point therefore
/// runs inside `jet_deopt_call` below a Cranelift frame. Cancellation must
/// become the ordinary child status consumed by `join` and render as a Jet
/// diagnostic with exit 70. A signal or an abort marker proves the boundary
/// was bypassed.
#[test]
fn cancelled_interpreter_wait_inside_deopt_is_a_diagnostic_not_a_signal() {
    let root = std::env::temp_dir().join(format!(
        "jet_no_unwind_deopt_cancel_{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("deopt cancellation fixture dir");
    let file = root.join("cancel_deopt.jet");
    fs::write(
        &file,
        r#"use core.text as text
use core.time as time

fn slow_deopt() String {
    time.sleep(200ms)
    return "{text.casefold("Straße")}"
}

fn run() {
    task_handle :: task slow_deopt()
    time.sleep(20ms)
    task_handle.cancel()
    print(task_handle.join() ?? panic("cancelled deopt task"))
}
"#,
    )
    .expect("write deopt cancellation fixture");

    let shown = file.to_string_lossy().into_owned();
    let cache = root.join("cache");
    fs::create_dir_all(&cache).expect("deopt cancellation cache dir");
    let output = jet_run(&["run", "--trace-tiers", shown.as_str()], &cache);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        terminating_signal(&output.status).is_none(),
        "deopt cancellation terminated with signal {:?}; stdout:\n{stdout}\nstderr:\n{stderr}",
        terminating_signal(&output.status)
    );
    for marker in ABORT_MARKERS {
        assert!(
            !stderr.contains(marker),
            "deopt cancellation escaped its boundary through `{marker}`:\n{stderr}"
        );
    }
    assert_eq!(
        output.status.code(),
        Some(70),
        "cancelled deopt child must reach the runtime diagnostic rail:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Stop [E3001]") && stderr.contains("task cancelled"),
        "missing cancellation runtime diagnostic:\n{stderr}"
    );
    assert!(
        stderr.contains("slow_deopt") && stderr.contains("tier0 interp"),
        "fixture did not prove the cancelled wait ran through named deopt:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&root);
}
