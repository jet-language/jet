//! #648: hostile proofs for the existing allocator families.
//!
//! Runtime-internal tests compile the exact generated `jet_mem` prelude with
//! observation stubs. Language-facing tests keep sema/AOT behavior honest.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

mod common;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(label: &str) -> std::path::PathBuf {
    let id = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "jet_allocator_{label}_{}_{}",
        std::process::id(),
        id
    ))
}

fn compile_rust_harness(body: &str) -> std::process::Output {
    let dir = temp_dir("runtime");
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.rs");
    let binary = dir.join("main");
    let observe = std::fs::read_to_string("crates/jet-codegen/src/Prelude/Observe.rs").unwrap();
    let uninit = std::fs::read_to_string("crates/jet-codegen/src/Prelude/Uninit.rs").unwrap();
    let outcome = std::fs::read_to_string("crates/jet-foundation/src/Outcome.rs").unwrap();
    let fault_injection =
        std::fs::read_to_string("crates/jet-codegen/src/Prelude/FaultInjection.rs").unwrap();
    let sentry = std::fs::read_to_string("crates/jet-foundation/src/MemSentry.rs").unwrap();
    let prelude = std::fs::read_to_string("crates/jet-codegen/src/Prelude/Mem.rs").unwrap();
    // Registry.rs depends on the full Foundation Diagnostics and Policy graph,
    // which this standalone rustc harness intentionally does not embed. Keep
    // Outcome.rs on its canonical typed seam; these memory tests exercise the
    // sentry renderer, so no registered generic runtime row is active here.
    let registry = r#"
#[allow(non_snake_case)]
mod Registry {
    pub fn active_runtime_diagnostic(
        _code: &str,
    ) -> Option<&'static super::JetRuntimeDiagnosticRow> {
        None
    }
}
"#;
    let runtime_stop = r#"
fn jet_sentry_runtime_stop(
    code: &str,
    file: &str,
    line: u32,
    gate: &str,
    operation: &str,
    obligation: &str,
    detail: &str,
) -> ! {
    let report = jet_render_runtime_sentry(
        match code {
            "R0801" => "R0801",
            "R0802" => "R0802",
            "R0803" => "R0803",
            _ => "R0801",
        },
        file,
        line,
        gate,
        operation,
        obligation,
        detail,
    );
    panic!("{}", report.rendered);
}
"#;
    let source_text = format!(
        r#"#![allow(dead_code)]
{observe}
{registry}
{outcome}
{fault_injection}
{runtime_stop}
mod jet_uninit_semantics {{
{uninit}
}}
mod jet_mem {{
    use super::jet_sentry_runtime_stop;
    mod jet_sentry {{
{sentry}
    }}
{prelude}
}}
{body}
"#
    );
    std::fs::write(&source, source_text).unwrap();
    let compiled = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "allocator runtime harness failed to compile:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    Command::new(binary).output().unwrap()
}

fn compile_program_allocator_harness(body: &str) -> std::process::Output {
    let dir = temp_dir("program_allocator");
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.rs");
    let binary = dir.join("main");
    let allocator =
        std::fs::read_to_string("crates/jet-codegen/src/Prelude/ProgramAllocator.rs").unwrap();
    std::fs::write(
        &source,
        format!(
            "#![allow(dead_code)]\nmod allocator {{\n{allocator}\n}}\n{body}\n"
        ),
    )
    .unwrap();
    let compiled = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "program allocator harness failed to compile:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    Command::new(binary).output().unwrap()
}

fn write_program_allocator_project(label: &str, allocator: Option<&str>) -> std::path::PathBuf {
    let root = temp_dir(label);
    std::fs::create_dir_all(&root).unwrap();
    let allocator = allocator
        .map(|value| format!("allocator: {value}\n"))
        .unwrap_or_default();
    std::fs::write(
        root.join("package.jet"),
        format!("name: \"{label}\"\nversion: \"0.1.0\"\n{allocator}"),
    )
    .unwrap();
    let entry = root.join("main.jet");
    std::fs::write(&entry, "fn run() { print(\"ok\") }\n").unwrap();
    entry
}

fn compile_jet(src: &str) -> Result<jet::CompileOutput, Vec<String>> {
    let dir = temp_dir("jet");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    std::fs::write(&path, src).unwrap();
    jet::compile_with_path(src, &path.to_string_lossy())
        .map_err(|diags| diags.into_iter().map(|diag| diag.code).collect())
}

fn run_jet(label: &str, src: &str) -> (i32, String, String) {
    let output = compile_jet(src).unwrap_or_else(|diags| {
        panic!(
            "{label} front end failed: {:?}",
            diags
        )
    });
    let user = common::strip_vetted_prelude_modules(&output.rust);
    let unsafe_lines = user
        .lines()
        .filter(|line| !common::unsafe_keyword_columns(line).is_empty())
        .collect::<Vec<_>>();
    assert!(
        unsafe_lines.is_empty(),
        "unsafe escaped the vetted prelude: {unsafe_lines:?}"
    );
    let dir = temp_dir(label);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.rs");
    let binary = dir.join("main");
    std::fs::write(&source, output.rust).unwrap();
    let compiled = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "rustc rejected generated {label} code:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new(binary).output().unwrap();
    (
        ran.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

#[test]
fn arena_bump_and_slab_have_distinct_aligned_drop_and_retention_laws() {
    if !common::have_rustc() {
        return;
    }
    let ran = compile_rust_harness(
        r#"
use std::sync::Mutex;
static DROPS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
struct Dropped(usize);
impl Drop for Dropped { fn drop(&mut self) { DROPS.lock().unwrap().push(self.0); } }
#[repr(align(64))]
struct Aligned(u8);

fn main() {
    let mut arena = jet_mem::JetArena::with_capacity(64);
    let first_chunk = arena.alloc(1u8) as *mut u8 as usize;
    let aligned = arena.alloc(Aligned(7)) as *mut Aligned as usize;
    let _drop1 = arena.alloc(Dropped(1));
    let _drop2 = arena.alloc(Dropped(2));
    assert_eq!(aligned % 64, 0);
    let before = arena.facts();
    assert!(before.live_bytes >= 1 && before.retained_bytes >= before.live_bytes);
    arena.reset();
    assert_eq!(*DROPS.lock().unwrap(), vec![2, 1]);
    let after = arena.facts();
    assert_eq!(after.live_bytes, 0);
    assert_eq!(after.retained_bytes, before.retained_bytes);
    assert!(after.high_water_bytes >= before.live_bytes);
    let observed = jet_observe_allocator_memory();
    assert!(observed.0 >= after.retained_bytes);
    assert!(observed.1 >= before.live_bytes);
    let reused_first_chunk = arena.alloc(2u8) as *mut u8 as usize;
    assert_eq!(reused_first_chunk, first_chunk);

    let mut bump = jet_mem::JetBump::with_capacity(128);
    let first = bump.alloc(1u8) as *mut u8 as usize;
    let second = bump.alloc(2u64) as *mut u64 as usize;
    assert!(second > first && second - first < 128);
    let exhausted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = bump.alloc([0u8; 256]);
    }));
    assert!(exhausted.is_err());
    let bump_before = bump.facts();
    bump.reset();
    let bump_after = bump.facts();
    assert_eq!(bump_after.retained_bytes, bump_before.retained_bytes);
    assert_eq!(bump_after.high_water_bytes, bump_before.high_water_bytes);

    let mut pool = jet_mem::JetPool::with_slots(2);
    let generation = pool.generation();
    let p0 = pool.alloc(10u64) as *mut u64 as usize;
    let _p1 = pool.alloc(20u64) as *mut u64 as usize;
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = pool.alloc(30u64);
    })).is_err());
    pool.reset();
    assert_ne!(pool.generation(), generation);
    let reused = pool.alloc(40u64) as *mut u64 as usize;
    assert_eq!(reused, p0);
    let large = pool.alloc([7u8; 513]);
    assert_eq!(large[512], 7);
    assert!(pool.facts().retained_bytes >= 521);

    let mut classes = jet_mem::JetPool::with_slots(1);
    let _small = classes.alloc(1u8);
    classes.reset();
    let small_retained = classes.facts().retained_bytes;
    let wide = classes.alloc([9u64; 80]);
    assert_eq!(wide[79], 9);
    assert!(classes.facts().retained_bytes > small_retained);
    println!("ok");
}
"#,
    );
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "ok\n");
}

#[test]
fn fixed_uses_one_borrowed_buffer_for_payload_alignment_and_reverse_drop_metadata() {
    if !common::have_rustc() {
        return;
    }
    let ran = compile_rust_harness(
        r#"
use std::sync::Mutex;
static DROPS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
struct Dropped(usize);
impl Drop for Dropped { fn drop(&mut self) { DROPS.lock().unwrap().push(self.0); } }
#[repr(align(64))]
struct Aligned(u8);

fn main() {
    let mut bytes = [0u8; 256];
    let mut fixed = jet_mem::JetFixed::over(&mut bytes);
    let aligned = fixed.alloc(Aligned(7)) as *mut Aligned as usize;
    let _first = fixed.alloc(Dropped(1));
    let _second = fixed.alloc(Dropped(2));
    assert_eq!(aligned % 64, 0);
    let before = fixed.facts();
    assert_eq!(before.retained_bytes, 256);
    fixed.reset();
    assert_eq!(*DROPS.lock().unwrap(), vec![2, 1]);
    assert_eq!(fixed.facts().retained_bytes, 256);
    assert_eq!(fixed.facts().live_allocations, 0);

    let mut try_bytes = [0u8; 40];
    let try_fixed = jet_mem::JetFixed::over(&mut try_bytes);
    assert_eq!(*try_fixed.try_alloc(7u64).unwrap(), 7);
    let error = match try_fixed.try_alloc(9u64) {
        Ok(_) => panic!("fallible Fixed allocation unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(error.requested_bytes, 8);
    assert_eq!(error.allocator, "Fixed");
    drop(try_fixed);

    let exhausted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = fixed.alloc([0u8; 233]);
    }));
    assert!(exhausted.is_err());
    assert_eq!(fixed.facts().live_allocations, 0);
    assert_eq!(jet_observe_allocator_memory().0, 256);
    drop(fixed);
    assert_eq!(jet_observe_allocator_memory().0, 0);
    println!("ok");
}
"#,
    );
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "ok\n");
}

#[test]
fn fixed_source_synthesizes_inline_backing_and_rejects_dynamic_or_live_reset() {
    let output = compile_jet(
        r#"
use core.mem

fn run() {
    fixed :: mem.Fixed.new(size: 128 + 128)
    value :: fixed.alloc(7)
    print(value)
    close(^fixed)
}
"#,
    )
    .expect("positive comptime Fixed size should compile");
    let user = common::strip_vetted_prelude_modules(&output.rust);
    assert!(user.contains("[std::mem::MaybeUninit::<u8>::uninit(); 256]"), "{user}");
    assert!(user.contains("JetFixed::over_uninit"), "{user}");
    assert!(user.contains("impl __jet_Close for jet_mem::JetFixed"), "{user}");

    let over_src = r#"
use core.mem
fn run() {
    bytes := [U8#128]{ uninit }
    fixed :: mem.Fixed.over(&bytes)
    value :: fixed.alloc(9)
    print(value)
    close(^fixed)
}
"#;
    let over = compile_jet(over_src)
        .expect("Fixed.over should accept one mutable inline byte array");
    let over_user = common::strip_vetted_prelude_modules(&over.rust);
    assert!(
        over_user.contains("JetFixed::over_uninit_fixed(&mut __jet_bytes)"),
        "{over_user}"
    );
    if common::have_rustc() {
        let (code, stdout, stderr) = run_jet("fixed_over", over_src);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "9\n");
    }

    let dynamic = compile_jet(
        r#"
use core.mem
fn make(size: Int) {
    fixed :: mem.Fixed.new(size: size)
    close(^fixed)
}
"#,
    )
    .expect_err("runtime Fixed sizes cannot synthesize inline backing");
    assert!(dynamic.iter().any(|code| code == "E0103"), "{dynamic:?}");

    let live_reset = compile_jet(
        r#"
use core.mem
fn run() {
    fixed :: mem.Fixed.new(size: 128)
    value :: fixed.alloc(1)
    fixed.reset()
    print(value)
}
"#,
    )
    .expect_err("reset cannot invalidate a live Fixed allocation view");
    assert!(live_reset.iter().any(|code| code == "E0212"), "{live_reset:?}");

    let stored = compile_jet(
        r#"
use core.mem
fn run() {
    fixed :: mem.Fixed.new(size: 128)
    handles :: [fixed]
    print(handles.len())
}
"#,
    )
    .expect_err("Fixed handles cannot be stored in aggregates");
    assert!(stored.iter().any(|code| code == "E0631"), "{stored:?}");
}

#[test]
fn allocator_close_impls_require_resolved_core_constructors() {
    let output = compile_jet(
        r#"
use core.mem as mem

fn run() {
    text := "mem.Fixed.new(size: 128); notmem.Fixed.new()"
    // mem.Fixed.new(size: 128)
    // notmem.Fixed.new()
    print(text)
}
"#,
    )
    .expect("non-Core allocator lookalikes should compile");
    let user = common::strip_vetted_prelude_modules(&output.rust);
    assert!(!user.contains("impl __jet_Close for jet_mem::"), "{user}");
}

#[test]
fn fixed_over_exclusively_borrows_one_inline_byte_array() {
    let borrowed = compile_jet(
        r#"
use core.mem
fn run() {
    bytes := [U8#8]{ uninit }
    fixed :: mem.Fixed.over(&bytes)
    bytes[0] = 1
    close(^fixed)
}
"#,
    )
    .expect_err("the backing array stays exclusively borrowed while Fixed is live");
    assert!(borrowed.iter().any(|code| code == "E0212"), "{borrowed:?}");
}

#[test]
fn allocator_handles_are_thread_confined() {
    let src = r#"
use core.mem
fn run() {
    arena :: mem.Arena.new()
    handle :: task {
        value :: arena.alloc(1)
        print(value)
    }
    handle.join() ?? panic("task failed")
}
"#;
    let diags = compile_jet(src).expect_err("allocators must not cross task boundaries");
    assert!(diags.iter().any(|code| code == "E1102"), "{diags:?}");
}

#[test]
fn pool_ids_reuse_slots_without_reviving_stale_generations() {
    if !common::have_rustc() {
        return;
    }
    let src = r#"
struct Item { value: Int }

fn run() {
    pool := Pool<Item>.new()
    first :: pool.add(Item{ value: 10 })
    stale :: first
    removed :: pool.remove(first)
    second :: pool.add(Item{ value: 20 })
    if removed == Val(item) {
        print(item.value)
    } else {
        print(-1)
    }
    print(stale == second)
    print(pool[second].value)
}
"#;
    let (code, stdout, stderr) = run_jet("pool_generation", src);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "10\nfalse\n20\n");
}

#[test]
fn stale_pool_id_panics_instead_of_reading_reused_slot() {
    if !common::have_rustc() {
        return;
    }
    let src = r#"
struct Item { value: Int }

fn run() {
    pool := Pool<Item>.new()
    first :: pool.add(Item{ value: 10 })
    stale :: first
    _removed :: pool.remove(first)
    _second :: pool.add(Item{ value: 20 })
    print(pool[stale].value)
}
"#;
    let (code, _stdout, stderr) = run_jet("pool_stale", src);
    assert_eq!(code, 70, "{stderr}");
    assert!(stderr.contains("no longer refers to a live value"), "{stderr}");
}

#[test]
fn default_dev_runs_allocator_constructors_natively() {
    use jet::Interpreter::RunOutcome;
    use jet::JitBackend::JitBackend;

    if !common::have_rustc() {
        return;
    }
    let src = r#"
use core.mem

fn run() {
    arena :: mem.Arena.new(capacity: 32)
    a :: arena.alloc(1)
    print(a)
    arena.reset()
    again :: arena.alloc(2)
    print(again)
    bump :: mem.Bump.new(capacity: 32)
    b :: bump.alloc(3)
    print(b)
    pool :: mem.Pool.new(slots: 2)
    p :: pool.alloc(4)
    print(p)
    fixed :: mem.Fixed.new(size: 128)
    f :: fixed.alloc(5)
    print(f)
}
"#;
    let root = temp_dir("dev");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    std::fs::write(&path, src).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");

    if jet_jit::cranelift_host_supported() {
        jet_jit::try_compile_bundle(&bundle)
            .expect("allocator constructors must compile natively in resident JIT");
    }

    let mut dev = jet_jit::CraneliftBackend::new();
    jet_jit::reset_jit_trace_for_test();
    match dev.run(&bundle, false) {
        RunOutcome::Ran { stdout, .. } => {
            assert!(
                !jet_jit::deopt_invoked_for_test(),
                "allocator constructors must stay resident-native (no deopt)"
            );
            assert_eq!(stdout, "1\n2\n3\n4\n5\n");
        }
        RunOutcome::Problems(diags) => {
            panic!("resident JIT must run allocator constructors: {diags:?}");
        }
    }

    let (code, stdout, _) = run_jet("pool_generation", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n2\n3\n4\n5\n");
}

#[test]
fn try_allocation_example_reports_exhaustion_as_a_value() {
    let src = include_str!("../examples/features/memory/try_allocation.jet");
    let expected = include_str!("../examples/features/expected/memory/try_allocation.out");
    if common::have_rustc() {
        let (code, stdout, stderr) = run_jet("try_allocation", src);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, expected);

        let dir = temp_dir("try_allocation_dev");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.jet");
        std::fs::write(&path, src).unwrap();
        for force_interpreter in [false, true] {
            let outcome = jet::Interpreter::dev_iteration(
                &path.to_string_lossy(),
                false,
                force_interpreter,
            );
            match outcome {
                jet::Interpreter::RunOutcome::Ran {
                    stdout, exit_code, ..
                } => {
                    assert_eq!(exit_code, 0);
                    assert_eq!(stdout, expected);
                }
                jet::Interpreter::RunOutcome::Problems(diags) => {
                    panic!(
                        "try allocation example failed on {} tier: {diags:?}",
                        if force_interpreter { "interpreter" } else { "default dev" }
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    } else {
        compile_jet(src).expect("try allocation example should pass the front end");
    }
}

#[test]
fn fault_scheduler_reaches_allocator_try_alloc() {
    if !common::have_rustc() {
        return;
    }
    let ran = compile_rust_harness(
        r#"
fn main() {
    let (result, injected, counts) = jet_fault_run_once(
        &["FS.Write"],
        Some(1),
        1,
        &mut || {
            let mut bytes = [0u8; 64];
            let fixed = jet_mem::JetFixed::over(&mut bytes);
            match fixed.try_alloc(7u64) {
                Ok(_) => Err("allocator fault was not injected".to_string()),
                Err(_) => Ok(()),
            }
        },
    )
    .unwrap();
    assert!(result.is_ok());
    assert!(injected);
    assert_eq!(counts[1], 1);
    println!("ok");
}
"#,
    );
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "ok\n");
}

#[test]
fn program_allocator_fact_selects_counting_wrapper_for_aot() {
    let entry = write_program_allocator_project(
        "program_allocator_counting",
        Some("mem.Counting.over(mem.Heap, cap: 2.kb)"),
    );
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    assert!(matches!(
        bundle.program_allocator,
        jet::TargetMachine::AllocatorPolicy::Counting {
            cap: Some(jet::TargetMachine::ByteSize { bytes: 2048 })
        }
    ));
    let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:?}");
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(rust.contains(
        "static __JET_PROGRAM_ALLOCATOR: JetProgramAllocator = JetProgramAllocator::counting(2048)"
    ));
}

#[test]
fn missing_program_allocator_keeps_hidden_system_heap() {
    let entry = write_program_allocator_project("program_allocator_default", None);
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    assert_eq!(
        bundle.program_allocator,
        jet::TargetMachine::AllocatorPolicy::HostedDefault
    );
    let errors = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:?}");
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(!rust.contains("__JET_PROGRAM_ALLOCATOR"));
}

#[test]
fn invalid_program_allocator_fact_is_a_teaching_diagnostic() {
    let entry =
        write_program_allocator_project("program_allocator_invalid", Some("mem.Mystery"));
    let diagnostics = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap_err();
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E1206")
        .expect("invalid allocator fact must use the registered manifest diagnostic");
    assert_eq!(diagnostic.what, "invalid hosted program allocator");
    assert!(diagnostic.fix.contains("mem.Counting.over"));
}

#[test]
fn program_allocator_kernel_caps_fallible_allocations() {
    if !common::have_rustc() {
        return;
    }
    let ran = compile_program_allocator_harness(
        r#"
use std::alloc::{GlobalAlloc, Layout};

fn main() {
    let allocator = allocator::JetProgramAllocator::counting(64);
    let first_layout = Layout::from_size_align(48, 16).unwrap();
    let second_layout = Layout::from_size_align(32, 16).unwrap();
    let first = unsafe { GlobalAlloc::alloc(&allocator, first_layout) };
    assert!(!first.is_null());
    let exhausted = unsafe { GlobalAlloc::alloc(&allocator, second_layout) };
    assert!(exhausted.is_null());
    let facts = allocator.facts();
    assert_eq!(facts.allocations, 1);
    assert_eq!(facts.requested_bytes, 48);
    assert_eq!(facts.live_bytes, 48);
    assert_eq!(facts.high_water_bytes, 48);
    unsafe { GlobalAlloc::dealloc(&allocator, first, first_layout) };
    assert_eq!(allocator.facts().live_bytes, 0);
    println!("ok");
}
"#,
    );
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "ok\n");
}

#[test]
fn hosted_allocator_reservations_are_cumulative_and_released() {
    if !common::have_rustc() {
        return;
    }
    let ran = compile_program_allocator_harness(
        r#"
fn main() {
    let ((), facts) = allocator::jet_with_host_program_allocator(Some(64), || {
        assert!(allocator::jet_host_program_allocator_try_reserve(48));
        assert!(!allocator::jet_host_program_allocator_try_reserve(32));
        assert_eq!(allocator::JET_HOST_PROGRAM_ALLOCATOR.facts().live_bytes, 48);
    });
    assert_eq!(facts.live_bytes, 0);
    assert_eq!(allocator::JET_HOST_PROGRAM_ALLOCATOR.facts().live_bytes, 0);
    println!("ok");
}
"#,
    );
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "ok\n");
}

#[test]
fn program_allocator_default_blocks_survive_counting_swap_without_tracking() {
    if !common::have_rustc() {
        return;
    }
    let ran = compile_program_allocator_harness(
        r#"
use std::alloc::{GlobalAlloc, Layout};

fn main() {
    let allocator = allocator::JetProgramAllocator::system();
    let layout = Layout::from_size_align(32, 16).unwrap();
    let default_block = unsafe { GlobalAlloc::alloc(&allocator, layout) };
    assert!(!default_block.is_null());
    assert_eq!(allocator.facts().allocations, 0);

    let previous = allocator.configure_counting(64);
    let counted_block = unsafe { GlobalAlloc::alloc(&allocator, layout) };
    assert!(!counted_block.is_null());
    assert_eq!(allocator.facts().live_bytes, 32);
    unsafe { GlobalAlloc::dealloc(&allocator, default_block, layout) };
    assert_eq!(allocator.facts().live_bytes, 32);

    allocator.restore(previous);
    unsafe { GlobalAlloc::dealloc(&allocator, counted_block, layout) };
    assert_eq!(allocator.facts().live_bytes, 0);
    println!("ok");
}
"#,
    );
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "ok\n");
}

#[test]
fn program_allocator_example_matches_aot_jit_and_interpreter() {
    if !common::have_rustc() {
        return;
    }
    let project = std::path::Path::new("examples/features/memory/program_allocator");
    let expected =
        std::fs::read_to_string("examples/features/expected/memory/program_allocator.out")
            .unwrap();
    for (name, args) in [
        ("jit", &["run", "main.jet"][..]),
        ("interpreter", &["run", "--interpret", "main.jet"][..]),
        ("aot", &["run", "--release", "main.jet"][..]),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(args)
            .current_dir(project)
            .env("NO_COLOR", "1")
            .env("JET_RUN_CACHE_DIR", temp_dir(&format!("program_allocator_{name}")))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected, "{name}");
    }
    let dossier = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "dossier", "main.jet", "run", "--json"])
        .current_dir(project)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        dossier.status.success(),
        "{}",
        String::from_utf8_lossy(&dossier.stderr)
    );
    let dossier = String::from_utf8(dossier.stdout).unwrap();
    assert!(
        dossier.contains(
            "\"program_allocator\":{\"kind\":\"counting\",\"wraps\":\"system\",\"cap_bytes\":2147483648}"
        ),
        "{dossier}"
    );
}
