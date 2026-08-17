// Whole-corpus pure-interpreter parity batteries (#2020).
//
// Own target: each stem here is compared against a fresh optimized `rustc`
// build of the same program, so this slice cannot share a 900s budget with any
// other whole-corpus battery.

/// c77 widened battery: EVERY example either runs (interpreted stdout/stderr/exit
/// code == compiled-binary stdout/stderr/exit code, byte for byte — I2) or stops at a named boundary
/// (E2201/E2202/E0956 — never a silent skip). Reports the run/boundary split so
/// the coverage can't quietly shrink.
#[test]
fn interpreter_matches_compiled_binary() {
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping jet dev differential battery");
        return;
    }
    let dir = std::env::temp_dir().join(format!("jet_dev_diff_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (_, _, manifested_divergences) = parse_jit_gap_manifest();
    let stats =
        run_interpreter_battery_parallel(interpreter_example_stems(), dir, manifested_divergences);
    eprintln!(
        "c77 battery: {} ran ({} interp==compiled, {} manifested divergences), {} boundary-asserted, {} total",
        stats.ran,
        stats.ran - stats.manifested,
        stats.manifested,
        stats.boundary,
        stats.ran + stats.boundary
    );
    assert!(
        stats.ran > 0,
        "expected at least some examples to run in the interpreter"
    );
}

/// Every example that runs in the interpreter and has a checked-in
/// `expected/*.out` golden (the executable spec, I5) must match it byte for
/// byte — a cheap check that needs no rustc. Examples that hit a boundary, or
/// that have no golden (error/panic demos), are asserted as boundaries here too
/// so nothing is silently skipped.
#[test]
fn interpreter_matches_expected_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0usize;
    for stem in interpreter_example_stems() {
        let file = example_path(&stem);
        // D-JPK-TASKRUN1 / R12 (card #476): job_runner's meaningful entries are
        // its `#Job` fns, not the `fn run()` usage hint. Mirror golden.rs's
        // AOT job battery on the interpreter tier via `run_named_job`,
        // proving the same TIR dispatches each job identically. The bare
        // `fn run()` output is not a golden.
        if stem == "devloop/job_runner" {
            check_job_runner_interpreter(&root, &file);
            checked += 2;
            continue;
        }
        if uses_ffi_bridge(&stem) {
            continue;
        }
        // `examples/features/expected/web/web_wasm_*.out` are Node/browser
        // harness goldens (web_build), not interpreter `fn run()` print shape.
        if stem.starts_with("web/web_wasm_") {
            continue;
        }
        // #2017: an interactive example's golden was recorded WITH answers, and
        // this battery fed none, so it compared a no-input run against a
        // fed-input transcript — agreement by shared absence, not agreement.
        // The in-process interpreter reads the test binary's own fd 0, which
        // every thread of this suite shares, so the fed run is a child on the
        // forced tier-0 CLI (`jet run --interpret`, pinned as tier 0 by
        // tests/run_interpret.rs::run_interpret_forces_tier_zero_without_watch).
        // The answers come from `common::example_stdin`, their one home (I8).
        if let Some(answers) = common::example_stdin(&stem) {
            let interpreted = cli_tier_program_output(&file, &stem, answers.piped, true);
            assert_eq!(
                interpreted.stdout,
                golden_stdout(&stem),
                "`{stem}`: forced interpreter differs from the golden recorded with its answers"
            );
            checked += 1;
            continue;
        }
        let expected_path = root.join(format!("examples/features/expected/{}.out", stem));
        match dev_iteration_with_timeout(&stem, &file, true) {
            RunOutcome::Ran { stdout, .. } => {
                if let Some(expected) = host_expected_stdout(&stem) {
                    assert_eq!(
                        stdout, expected,
                        "`{}`: interpreter output differs from host expected output",
                        stem
                    );
                    checked += 1;
                } else if let Ok(expected) = fs::read_to_string(&expected_path) {
                    assert_eq!(
                        stdout, expected,
                        "`{}`: interpreter output differs from expected golden",
                        stem
                    );
                    checked += 1;
                }
                // No golden (e.g. a panic demo) → nothing to compare; the
                // compiled-binary battery still covers it.
            }
            RunOutcome::Problems(diags) => {
                assert!(
                    is_named_dev_boundary(&stem, &diags),
                    "`{}` neither ran nor stopped at a named boundary; codes were {:?}",
                    stem,
                    diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
                );
            }
        }
    }
    assert!(checked > 0, "expected at least some golden comparisons");
}
