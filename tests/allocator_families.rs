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
    let prelude = std::fs::read_to_string("crates/jet-codegen/src/Prelude/Mem.rs").unwrap();
    let source_text = format!(
        r#"#![allow(dead_code)]
{observe}
{prelude}
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
    if label == "pool_generation" {
        let _ = std::fs::write("/tmp/781-pool-user.rs", &user);
        eprintln!("DUMP_POOL_USER bytes={} unsafe={}", user.len(), user.lines().filter(|l| l.contains("unsafe")).count());
    }
    let unsafe_lines = user
        .lines()
        .filter(|line| {
            line.contains("unsafe")
                && !(
                    (src.contains(":= uninit") || src.contains(".{ uninit }") || src.contains(".{uninit}"))
                        && line.contains("MaybeUninit")
                )
        })
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
    if Command::new("rustc").arg("--version").output().is_err() {
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
    if Command::new("rustc").arg("--version").output().is_err() {
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
    assert!(user.contains("impl user_Close for jet_mem::JetFixed"), "{user}");

    let over_src = r#"
use core.mem
fn run() {
    bytes := [U8#128].{ uninit }
    fixed :: mem.Fixed.over(&bytes)
    value :: fixed.alloc(9)
    print(value)
    close(^fixed)
}
"#;
    let over = compile_jet(over_src)
        .expect("Fixed.over should accept one mutable inline byte array");
    let over_user = common::strip_vetted_prelude_modules(&over.rust);
    assert!(over_user.contains("JetFixed::over(&mut user_bytes)"), "{over_user}");
    if Command::new("rustc").arg("--version").output().is_ok() {
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
    assert!(!user.contains("impl user_Close for jet_mem::"), "{user}");
}

#[test]
fn fixed_over_exclusively_borrows_one_inline_byte_array() {
    let borrowed = compile_jet(
        r#"
use core.mem
fn run() {
    bytes := [U8#8].{ uninit }
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
use core.tasks as tasks

fn run() {
    arena :: mem.Arena.new()
    task :: tasks.spawn(take(arena) () => {
        value :: arena.alloc(1)
        print(value)
    })
    task.join()
}
"#;
    let diags = compile_jet(src).expect_err("allocators must not cross task boundaries");
    assert!(diags.iter().any(|code| code == "E1102"), "{diags:?}");
}

#[test]
fn pool_ids_reuse_slots_without_reviving_stale_generations() {
    if Command::new("rustc").arg("--version").output().is_err() {
        return;
    }
    let src = r#"
struct Item { value: Int }

fn run() {
    pool := Pool<Item>.new()
    first :: pool.add(Item.{ value: 10 })
    stale :: first
    removed :: pool.remove(first)
    second :: pool.add(Item.{ value: 20 })
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
    if Command::new("rustc").arg("--version").output().is_err() {
        return;
    }
    let src = r#"
struct Item { value: Int }

fn run() {
    pool := Pool<Item>.new()
    first :: pool.add(Item.{ value: 10 })
    stale :: first
    _removed :: pool.remove(first)
    _second :: pool.add(Item.{ value: 20 })
    print(pool[stale].value)
}
"#;
    let (code, _stdout, stderr) = run_jet("pool_stale", src);
    assert_eq!(code, 70, "{stderr}");
    assert!(stderr.contains("no longer refers to a live value"), "{stderr}");
}

#[test]
fn default_dev_reports_e2211_for_allocator_constructors() {
    use jet::Interpreter::RunOutcome;
    use jet::JitBackend::JitBackend;

    if Command::new("rustc").arg("--version").output().is_err() {
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
        let gap = jet_jit::try_compile_bundle(&bundle)
            .expect_err("allocator constructors must name their JIT boundary");
        assert!(
            gap.contains("automatic resource cleanup") || gap.contains("jit "),
            "{gap}"
        );
    }

    let mut dev = jet_jit::CraneliftBackend::new();
    match dev.run(&bundle, false) {
        RunOutcome::Problems(diags) => {
            assert!(jet_jit::is_e2211(&diags), "expected E2211, got {diags:?}");
        }
        RunOutcome::Ran { .. } => panic!("strict JIT must not AOT-fallback allocator constructors"),
    }

    let (code, stdout, _) = run_jet("pool_generation", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n2\n3\n4\n5\n");
}
