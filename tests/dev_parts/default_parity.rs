// Whole-corpus default (`tiered Cranelift`) `jet dev` parity battery (#2020).
//
// Own target for the same reason as `interp_parity`: one AOT build per stem
// over the whole corpus does not fit a shared 900s budget.

/// c125 M4 exit gate: the DEFAULT `jet dev` path uses tiered Cranelift. JIT-covered
/// examples must match the AOT binary; uncovered examples deopt to the interpreter
/// with byte-identical output (D-LENS-RUN2=A / #778).
#[test]
fn dev_default_matches_compiled_binary() {
    let handle = std::thread::Builder::new()
        .name("dev-default-battery".into())
        .spawn(|| {
            let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
            let have_rustc = have_rustc();
            if !have_rustc {
                eprintln!(
                    "note: rustc not found; skipping jet dev (default backend) differential battery"
                );
                return;
            }
            let dir = std::env::temp_dir()
                .join(format!("jet_dev_default_diff_{}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            let (_, _, manifested_divergences) = parse_jit_gap_manifest();
            let (stems, sema_rejected) = typechecked_example_stems();
            // #2020: name what left the universe BEFORE measuring anything over
            // it. A stem may not leave this battery by breaking.
            for row in &sema_rejected {
                eprintln!("  sema-rejected, NOT tested by this battery: {row}");
            }
            assert!(
                sema_rejected.len() <= SEMA_REJECTED_CEILING,
                "{} shipped example(s) fail the run-path front end (ceiling \
                 {SEMA_REJECTED_CEILING}), so the default `jet dev` battery never tested them, \
                 and the count may only fall:\n{}",
                sema_rejected.len(),
                sema_rejected.join("\n")
            );
            let stats = run_dev_default_battery_parallel(stems, dir, manifested_divergences);
            eprintln!(
                "c125 default-backend battery: {} ran ({} default==compiled, {} manifested divergences), {} deopt, {} boundary-asserted, {} total",
                stats.ran,
                stats.ran - stats.manifested,
                stats.manifested,
                stats.deopt,
                stats.boundary,
                stats.ran + stats.boundary
            );
            assert!(
                stats.ran > 0,
                "expected at least some examples to run via the default jet dev backend"
            );
            assert!(
                stats.deopt > 0,
                "expected tiered deopt for uncovered examples instead of transparent AOT fallback"
            );
            let mut observed_boundaries = stats.boundary_stems;
            observed_boundaries.sort();
            observed_boundaries.dedup();
            for required in DEFAULT_BACKEND_EXPECTED_BOUNDARIES {
                assert!(
                    observed_boundaries.iter().any(|s| s == required),
                    "missing expected boundary `{required}` in {observed_boundaries:?}"
                );
            }
            // #778: many stems deopt then stop at E0956 — named boundaries, not
            // E2211. Do not pin the full set; corpus gate + jit_gaps ratchet own
            // coverage growth.
            assert_eq!(
                stats.manifested, 0,
                "default jet dev must not carry manifested stdout/stderr/exit-code divergences"
            );
        })
        .expect("spawn default-backend battery");
    handle
        .join()
        .expect("default-backend battery thread panicked");
}
