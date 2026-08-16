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
//!    be added by replacing the C frame. An `extern "C" fn jet_*` in this crate
//!    is therefore an unguardable seam by construction.
//! 2. The only `extern "C"` frames generated code can reach are the shims
//!    `host_seam::guarded` builds, and the only ways to reach one are the
//!    single `builder.symbol` call inside `host_fns!` and `guarded_addr`. Any
//!    other route from a `jet_*` function to an address is an escape.
//! 3. The generator must still generate the guard. Without this, reverting one
//!    line of `host_fns!` would silently unguard every symbol and leave every
//!    other test green.
//!
//! Run: `scripts/agent/jet-env cargo test --test jit_no_unwind_boundary`

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

/// Check 1: an `extern "C"` seam body cannot be guarded from outside, so this
/// crate may not define one for a `jet_*` symbol — or for a macro-generated
/// name, which is the same hole wearing a metavariable.
fn unguardable_seam_definitions(sources: &[(String, String)]) -> Vec<String> {
    code_lines(sources)
        .into_iter()
        .filter(|(_, _, line)| {
            line.contains("extern \"C\" fn jet_")
                || line.contains("extern \"C-unwind\" fn jet_")
                || line.contains("extern \"C\" fn $")
                || line.contains("extern \"C-unwind\" fn $")
        })
        .map(|(label, number, line)| format!("{label}:{number}: {}", line.trim()))
        .collect()
}

/// Check 2: a `jet_*` function reaching an address by any route other than
/// `host_seam::guarded` / `guarded_addr` is an unguarded boundary.
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
                line[..at]
                    .trim_end()
                    .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("")
                    .starts_with("jet_")
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
        "an `extern \"C\"` host seam cannot be guarded: rustc aborts an escaping \
         unwind at the body's own edge (`thread caused non-unwinding panic`), \
         before the shim `host_seam::guarded` builds could catch it. Make the \
         seam a plain `fn` and let `host_fns!` generate its `extern \"C\"` \
         boundary (crates/jet-jit/src/host_seam.rs, docs/spec/architecture.md \
         R13). Offending definitions:\n{}",
        offenders.join("\n")
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

/// Text that only appears when a control transfer killed the process instead of
/// becoming a report. `error 5` is the FDE-less JIT frame; the non-unwinding
/// panic is the `extern "C"` body edge; the stack-overflow pair is the third,
/// unrelated abort and is listed so a failure names which one it saw.
const ABORT_MARKERS: &[&str] = &[
    "failed to initiate panic",
    "non-unwinding panic",
    "panic in a function that cannot unwind",
    "core::panicking",
    "fatal runtime error: stack overflow",
];

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
