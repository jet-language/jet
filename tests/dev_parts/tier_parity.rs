// Curated per-topic three-way tier batteries (#2020).
//
// Each stem is judged by a fresh child run of its own test, and
// `run_child_stem_battery` spends `test_worker_count(8)` of them at a time.
// Before that these ~160 child builds ran strictly in series, which is most of
// why `--test dev` could not finish inside the suite guard.

#[test]
fn language_callables_and_types_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1215_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_cranelift_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        "basics/bare_lambda_param",
        "basics/default_refs",
        "basics/callbacks",
        "basics/pattern_matching",
        "basics/variadics_spread",
        "effects/effect_higher_order",
        "memory/parameter_modes",
        "patterns/struct_destructure",
        "syntax/trailing_block",
        "types/anonymous_unions",
        "types/generic_types",
        "types/measurement",
        "types/nested_enum_groups",
        "types/no_any_alternatives",
        "types/optional_result_variants",
        "types/patchable",
        "types/refinements",
        "types/renderable-varargs-multi",
        "types/renderable-varargs",
        "types/traits",
        "types/type_alias",
        "types/value_tag_type",
    ];
    run_child_stem_battery(
        "language_callables_and_types_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "language/callable/type parity",
        *DEV_DIFF_TIMEOUT,
        &stems,
        &[],
    );
}

#[test]
fn comptime_effects_and_errors_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1217_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_cranelift_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        "comptime/comptime_if",
        "comptime/comptime_table",
        "devloop/persist",
        "effects/determinism",
        "effects/effect_higher_order",
        "effects/effect_levers",
        "effects/single_use_discard",
        "effects/smart_context",
        "errors/default_err_edge",
        "errors/error_context",
        "errors/must_use",
        "errors/panic",
        // #1967: `?? panic(…)` on a failing carrier. Named here so a tier that
        // reports the stop without ending the program fails by name.
        "errors/qq_panic",
        "errors/rollback_trait",
        "errors/transact",
        "errors/default_error_conversion",
        "errors/typed_error_families",
    ];
    run_child_stem_battery(
        "comptime_effects_and_errors_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "comptime/effect/error parity",
        *DEV_DIFF_TIMEOUT,
        &stems,
        &[],
    );
}

#[test]
fn collections_memory_and_streams_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1216_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_cranelift_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        "collections/index_hook",
        "collections/iter_hook",
        "collections/iter_tools_audit",
        // The `[2,3].join(",")` element-renderer divergence in this stem was
        // caught only by the corpus-wide gate, because no per-stem battery
        // named it. Named here so the next such regression fails by name.
        "collections/lists",
        "memory/arena",
        "memory/arena_parse",
        "memory/arena_regions",
        "memory/entity_tree",
        "memory/entity_world",
        "memory/expiring_secret",
        "memory/ownership",
        "memory/parameter_modes",
        "memory/pool_stale_id",
        "memory/rawptr",
        "memory/returned_views",
        "memory/shared_config",
        "memory/shared_transact",
        "memory/copy_verb",
        "memory/string_view",
        "streams/generators",
    ];
    run_child_stem_battery(
        "collections_memory_and_streams_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "collection/memory/stream parity",
        *DEV_DIFF_TIMEOUT,
        &stems,
        &[],
    );
}

#[test]
fn stream_pull_hostile_matrix_matches_interpreter_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let stem = "streams/generators";
    let file = example_path(stem);
    let expected = golden_stdout(stem);
    match dev_iteration_with_timeout(stem, &file, true) {
        RunOutcome::Ran { stdout, .. } => {
            assert_eq!(stdout, expected, "hostile Stream matrix drifted from its golden");
        }
        RunOutcome::Problems(diags) => {
            panic!("hostile Stream matrix did not run in the interpreter: {diags:?}");
        }
    }
    assert_cranelift_three_way(&file, stem);
}

#[test]
fn stream_producer_failure_matches_interpreter_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let source = r#"fn failing() Stream<Int> {
    yield 1
    if true {
        panic("producer failure")
    }
}

fn run() {
    loop value in failing() {
        print("value: {value}")
    }
}
"#;
    let file = std::env::temp_dir().join(format!("jet_stream_failure_{}.jet", std::process::id()));
    fs::write(&file, source).expect("write Stream failure fixture");
    let file = file.to_string_lossy().into_owned();
    let interpreted = match dev_iteration(&file, false, true) {
        RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => ProgramOutput::ran(stdout, stderr, exit_code),
        RunOutcome::Problems(diags) => {
            panic!("Stream producer failure did not run in the interpreter: {diags:?}");
        }
    };

    let bundle = checked_bundle_from_path(&file);
    jet_jit::reset_jit_trace_for_test();
    let mut backend = CraneliftBackend::new();
    let jit = jet_jit::with_program_args(std::slice::from_ref(&file), || {
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => ProgramOutput::ran(stdout, stderr, exit_code),
            RunOutcome::Problems(diags) => {
                panic!("Stream producer failure did not run in JIT: {diags:?}");
            }
        }
    });
    let aot_dir = std::env::temp_dir().join(format!("jet_stream_failure_aot_{}", std::process::id()));
    let aot = compiled_binary_output(&aot_dir, "stream_failure", 0, "streams/generators", &file);

    assert_eq!(jit, interpreted, "Stream producer failure drifted in JIT");
    assert_eq!(aot, interpreted, "Stream producer failure drifted in AOT");
    assert_eq!(interpreted.exit_code, 70, "producer failure must remain a panic");
    assert_eq!(interpreted.stdout, "value: 1\n");
    assert!(interpreted.stderr.contains("panic: producer failure"));
}

#[test]
fn crypto_auth_and_vault_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1222_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_crypto_auth_vault_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        "crypto/auth_tokens",
        "crypto/crypto_envelope",
        "crypto/crypto_migration",
        "crypto/crypto_sign",
        "crypto/hash",
        "crypto/random_api_split",
        "crypto/typed_crypto",
        "crypto/vault_key_wrap",
        "crypto/vault_keys",
        "crypto/vault_secret",
    ];
    run_child_stem_battery(
        "crypto_auth_and_vault_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "crypto/auth/vault parity",
        *DEV_DIFF_TIMEOUT,
        &stems,
        &[],
    );
}

#[test]
fn network_http_and_browser_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1221_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_network_http_browser_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        "net/browser_bidi_profiles",
        "net/email_dkim",
        "net/email_message",
        "net/http_client",
        "net/http_get",
        "net/http_rest_service",
        "net/http_routes",
        "net/http_server",
        "net/http_server_lifecycle",
        "net/http_server_limits",
        "net/http_server_middleware",
        "net/http_server_tasks",
        "net/http_server_trailers",
        "net/socket_echo",
        "net/url_mime",
        "net/ws_echo",
    ];
    run_child_stem_battery(
        "network_http_and_browser_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "network/http/browser parity",
        *DEV_DIFF_TIMEOUT,
        &stems,
        &[],
    );
}

#[test]
fn concurrency_and_game_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1218_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_concurrency_and_game_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        "concurrency/deadline_context",
        "concurrency/detached_task",
        "concurrency/parallel_iter",
        "concurrency/parallel_scan",
        "concurrency/task_controls",
        "concurrency/task_runtime_audit",
        "game/core_game_headless",
        "game/raylib_window",
    ];
    run_child_stem_battery(
        "concurrency_and_game_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "concurrency/game parity",
        *DEV_DIFF_TIMEOUT,
        &stems,
        &[],
    );
}

#[test]
fn ui_and_web_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1225_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_ui_and_web_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        "ui/events",
        "ui/layout_basic",
        "ui/loadable",
        "ui/reactive",
        "ui/reactive_scope",
        "ui/ui_a11y",
        "ui/ui_component_kit",
        "ui/ui_motion",
        "ui/ui_native_linux",
        "ui/ui_null_backend",
        "ui/ui_tui_reactive",
        "ui/ui_typed_style",
        "ui/ui_view_tree",
        "web/ui_showcase",
        "web/ui_web_click",
        "web/ui_web_reactive",
        "web/web_app",
        "web/web_hello",
        "web/web_wasm_callback",
    ];
    run_child_stem_battery(
        "ui_and_web_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "ui/web parity",
        *DEV_DIFF_TIMEOUT,
        &stems,
        &[("JET_UI_HEADLESS", "1")],
    );
}

#[test]
fn reflection_value_matches_interpreter_jit_aot_and_jet_run() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stem = "reflection/reflect-value";
    let file = example_path(stem);
    assert_data_pipelines_parsing_three_way(&file, stem);
    let cli_run = run_cli_default_resident("run", &file, "reflection_value_cli_run");
    assert_eq!(
        cli_run,
        golden_program_output(stem),
        "default `jet run` drifted from reflection golden output"
    );
}

#[test]
fn typed_fact_reads_match_interpreter_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stem = "reflection/fact_reads";
    assert_data_pipelines_parsing_three_way(&example_path(stem), stem);
}

#[test]
fn data_pipelines_and_parsing_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1223_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_data_pipelines_parsing_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        // #1223 — data pipelines / schemas
        "tooling/data_analysis",
        "tooling/data_bridges",
        "tooling/data_core",
        "tooling/data_hostile",
        "tooling/data_json",
        "tooling/data_pipeline",
        "tooling/data_plot",
        "tooling/data_schema",
        "tooling/data_stream_bounds",
        // #1224 — parsing / reflection / tooling
        "parsing/binary-reader",
        "parsing/binary_pattern",
        "parsing/parse_interpolation",
        "parsing/text-cursor",
        "tooling/debug_native",
        "tooling/fuzz_demo",
        "tooling/panic_report",
        "tooling/property_tests",
        "tooling/provenance_track",
        "tooling/testing_helpers",
        "tooling/todo_hole",
    ];
    run_child_stem_battery(
        "data_pipelines_and_parsing_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "data/parse/tooling parity",
        *DEV_DIFF_TIMEOUT,
        &stems,
        &[],
    );
}

#[test]

fn io_cli_terminal_and_time_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1219_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_io_cli_terminal_time_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        "cli/positionals",
        "cli/subcommands",
        "cli/typed_entry_args",
        "io/db_checked_sql",
        "io/scope_guard",
        "io/stdin_filter",
        "io/stream",
        "io/terminal",
        "io/terminal_parity",
        "io/watcher",
        "text/dates",
        "text/datetime",
        "text/decimal",
        "text/regex",
        "text/time_calendar",
    ];
    run_child_stem_battery(
        "io_cli_terminal_and_time_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "io/cli/terminal/time parity",
        Duration::from_secs(120),
        &stems,
        &[],
    );
}

#[test]
fn core_os_examples_match_interpreter_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(core_os_examples_match_interpreter_jit_and_aot_inner)
        .expect("spawn core.sys parity worker")
        .join()
        .expect("core.sys parity worker must not panic");
}

#[test]
fn explicit_exit_cleanup_matches_interpreter_jit_and_aot() {
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    // The cleanup graph is intentionally deep enough to exceed the small
    // default test-thread stack. Keep this focused proof on the same generous
    // stack used by the resident-JIT coverage tests.
    std::thread::Builder::new()
        .name("explicit-process-exit-cleanup".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            assert_io_cli_terminal_time_three_way(
                &example_path("io/process_exit_cleanup"),
                "io/process_exit_cleanup",
            );
            assert_io_cli_terminal_time_three_way(
                &example_path("io/os_stop_cleanup"),
                "io/os_stop_cleanup",
            );
        })
        .expect("cleanup parity worker")
        .join()
        .expect("cleanup parity worker must not panic");
}

#[test]
fn lowlevel_and_safety_match_interpreter_jit_and_aot() {
    const CHILD_STEM: &str = "JET_1220_STEM";
    if let Ok(stem) = std::env::var(CHILD_STEM) {
        assert_lowlevel_and_safety_three_way(&example_path(&stem), &stem);
        return;
    }
    if skip_if_cranelift_host_unsupported() || !have_rustc() {
        return;
    }
    let _guard = lock_recovered(dev_diff_lock(), "dev_diff_lock");
    let stems = [
        "lowlevel/ffi",
        "lowlevel/freestanding",
        "lowlevel/inline_asm",
        "lowlevel/inline_c",
        "lowlevel/layout_columnar",
        "lowlevel/linalg_simd",
        "lowlevel/lowlevel",
        "lowlevel/os_target_gating",
        "lowlevel/pointer_cast_deref",
        "lowlevel/sized_floats",
        "lowlevel/swizzle",
        "lowlevel/target_machine_board",
        "lowlevel/unsafe_obligations",
        "safety/sh_checked_text",
        "safety/checked_text_sql",
    ];
    run_child_stem_battery(
        "lowlevel_and_safety_match_interpreter_jit_and_aot",
        CHILD_STEM,
        "lowlevel/safety parity",
        Duration::from_secs(180),
        &stems,
        &[],
    );
}
