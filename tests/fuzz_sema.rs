//! M14 soundness fuzz: sema-checked programs must compile with rustc (I2).
//! Short CI run (N=50 default); use FUZZ_SEED for reproducibility and
//! FUZZ_VARIANTS to raise N (D-CI3: nightly lane runs N>=1000 with a
//! rotating seed).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

mod common;
use common::{have_rustc, panic_message, strip_vetted_prelude_modules, test_worker_count};

const DEFAULT_VARIANTS: usize = 50;

fn fuzz_variants() -> usize {
    std::env::var("FUZZ_VARIANTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_VARIANTS)
}

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next() as usize) % items.len()]
    }
}

fn fuzz_seed() -> u64 {
    std::env::var("FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(42)
}

fn load_example_seeds(root: &PathBuf) -> Vec<(String, String)> {
    let ex_dir = root.join("examples/features");
    let ext = jet::Syntax::FILE_EXT;
    let mut seeds = Vec::new();
    // examples live one level down in topic directories (D-REPO-EXAMPLES1)
    let mut files = Vec::new();
    for t in fs::read_dir(&ex_dir).unwrap().flatten() {
        let tp = t.path();
        if !tp.is_dir() || tp.file_name().unwrap() == "expected" {
            continue;
        }
        for e in fs::read_dir(&tp).unwrap().flatten() {
            files.push(e.path());
        }
    }
    for path in files {
        if path.extension().and_then(|x| x.to_str()) == Some(ext) {
            let stem = format!(
                "{}/{}",
                path.parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy(),
                path.file_stem().unwrap().to_string_lossy()
            );
            if stem == "lowlevel/ffi" {
                continue;
            }
            let src = fs::read_to_string(&path).unwrap();
            let shown = format!("examples/features/{}.{}", stem, ext);
            if src.contains("#Unsafe") {
                continue;
            }
            // Skip examples whose *baseline* output legitimately contains vetted
            // `unsafe` from a prelude/runtime-support module (smart context's
            // `#Context` pointer cell, mem/Ptr, etc.). The fuzz body's I1 guard is
            // a crude `contains("unsafe")` substring check that can't tell vetted
            // prelude `unsafe` from user-visible `unsafe`, so such a seed would
            // false-trip it regardless of the mutation. The mutation only appends a
            // trivial binding/fn, so it never introduces new `unsafe` of its own.
            let baseline_has_unsafe = jet::compile_with_path(&src, &path.to_string_lossy())
                .map(|out| strip_vetted_prelude_modules(&out.rust).contains("unsafe"))
                .unwrap_or(false);
            if baseline_has_unsafe {
                continue;
            }
            seeds.push((shown, src));
        }
    }
    seeds.sort_by(|a, b| a.0.cmp(&b.0));
    seeds.extend(curated_soundness_seeds());
    seeds
}

fn curated_soundness_seeds() -> Vec<(String, String)> {
    vec![
        (
            "curated/generic_identity".to_string(),
            r#"
fn id<T>(x: T) -> T {
    return x
}

fn run() {
    print(id<Int>(7))
}
"#
            .to_string(),
        ),
        (
            "curated/fixed_list_literal_index".to_string(),
            r#"
fn second(xs: [Int#3]) -> Int {
    return xs[1]
}

fn run() {
    xs: [Int#3] :: [1, 2, 3]
    print(second(xs))
}
"#
            .to_string(),
        ),
        (
            "curated/refined_index".to_string(),
            r#"
#Invariant("value >= 0 && value < 3")
Index3 :: distinct Int

fn pick(xs: [Int#3], i: Index3) -> Int {
    return xs[i]
}

fn run() {
    xs: [Int#3] :: [1, 2, 3]
    print(pick(xs, Index3.from_int(2)))
}
"#
            .to_string(),
        ),
        (
            "curated/fanout_fixed_list".to_string(),
            r#"
fn inc(x: Int) -> Int {
    return x + 1
}

fn run() {
    ys: [Int#3] :: inc.[1, 2, 3]
    print(ys[2])
}
"#
            .to_string(),
        ),
        (
            "curated/pure_boundary".to_string(),
            r#"
fn add1(x: Int) --[]-> Int {
    return x + 1
}

fn run() {
    print(add1(4))
}
"#
            .to_string(),
        ),
    ]
}

fn mutate_source(rng: &mut Rng, src: &str, variant: usize) -> String {
    let n = variant;
    match rng.next() % 7 {
        0 => format!("{src}\n_fuzz_{n} :: {n};\n"),
        1 => format!("{src}\nfn _fuzz_fn_{n}() {{\n    _x :: {n};\n    return;\n}}\n"),
        2 => format!("{src}\nfn _fuzz_wrap_{n}() -> Int {{\n    return {n};\n}}\n"),
        3 => format!("{src}\nfn _fuzz_id_{n}<T>(x: T) -> T {{\n    return x\n}}\n"),
        4 => format!("{src}\nfn _fuzz_fixed_{n}(xs: [Int#3]) -> Int {{\n    return xs[1]\n}}\n"),
        5 => format!(
            "{src}\n#Invariant(\"value >= 0 && value < 3\")\n_FuzzIndex{n} :: distinct Int\nfn _fuzz_refined_{n}(xs: [Int#3], i: _FuzzIndex{n}) -> Int {{\n    return xs[i]\n}}\n"
        ),
        _ => format!(
            "{src}\nfn _fuzz_inc_{n}(x: Int) -> Int {{\n    return x + 1\n}}\nfn _fuzz_fanout_{n}() -> [Int#3] {{\n    return _fuzz_inc_{n}.[1, 2, 3]\n}}\n"
        ),
    }
}

fn is_jet_diagnostic(d: &jet::Diagnostics::Diagnostic) -> bool {
    let c = d.code.as_str();
    c.starts_with('E') || c.starts_with('L') || c.starts_with('W')
}

fn rustc_accepts(stem: &str, rust_code: &str) -> Result<(), String> {
    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_fuzz_{stem}.rs"));
    let bin = dir.join(format!("jet_fuzz_{stem}"));
    fs::write(&rs, rust_code).unwrap();
    let out = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    if out.status.success() {
        let _ = fs::remove_file(&rs);
        let _ = fs::remove_file(&bin);
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

#[test]
fn fuzz_sema_rustc_agreement() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !have_rustc() {
        eprintln!("note: rustc not found; skipping fuzz_sema");
        return;
    }

    let seeds = load_example_seeds(&root);
    assert!(
        seeds.len() >= 2,
        "expected at least 2 example seeds, found {}",
        seeds.len()
    );

    let seed = fuzz_seed();
    let mut rng = Rng::new(seed);
    let fuzz_dir = std::env::temp_dir().join(format!("jet_fuzz_{}", std::process::id()));
    let _ = fs::remove_dir_all(&fuzz_dir);
    fs::create_dir_all(&fuzz_dir).unwrap();

    let mut variants = Vec::new();
    for i in 0..fuzz_variants() {
        let (shown, src) = rng.pick(&seeds).clone();
        let mutated = mutate_source(&mut rng, &src, i);
        let file = fuzz_dir.join(format!("variant_{i}.jet"));
        fs::write(&file, &mutated).unwrap();
        let file_str = file.to_string_lossy().into_owned();
        variants.push((i, shown, mutated, file_str));
    }

    let jobs = Arc::new(Mutex::new(std::collections::VecDeque::from(variants)));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut handles = Vec::new();
    for _ in 0..test_worker_count(16) {
        let jobs = Arc::clone(&jobs);
        let failures = Arc::clone(&failures);
        handles.push(std::thread::spawn(move || loop {
            let Some((i, shown, mutated, file_str)) = jobs.lock().unwrap().pop_front() else {
                break;
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                check_fuzz_variant(i, &shown, &mutated, &file_str)
            }));
            if let Err(payload) = result {
                failures
                    .lock()
                    .unwrap()
                    .push(format!("variant {i} from {shown}: {}", panic_message(payload)));
            }
        }));
    }
    for handle in handles {
        handle.join().expect("fuzz worker panicked outside harness");
    }
    let failures = failures.lock().unwrap();
    assert!(
        failures.is_empty(),
        "fuzz sema failures:\n{}",
        failures.join("\n\n")
    );

    let _ = fs::remove_dir_all(&fuzz_dir);
}

// ---------------------------------------------------------------------------
// I1 pin (card #447 / durability W2): the ungated-unsafe grep must apply to
// fuzz_sema-generated programs, not just the examples corpus (tests/golden.rs
// checks examples only). Unlike `fuzz_sema_rustc_agreement` above, this test
// does NOT gate on `have_rustc()` — the I1 (no ungated `#[Unsafe]`-less
// `unsafe`) check must run even when rustc is unavailable, since I1 is a sema
// invariant, not an I2 rustc-agreement check.
// ---------------------------------------------------------------------------
#[test]
fn fuzz_sema_i1_unsafe_gate_is_ungated_on_rustc() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let seeds = load_example_seeds(&root);
    assert!(
        seeds.len() >= 2,
        "expected at least 2 example seeds, found {}",
        seeds.len()
    );

    let seed = fuzz_seed();
    let mut rng = Rng::new(seed);
    let mut violations = Vec::new();
    for i in 0..fuzz_variants() {
        let (shown, src) = rng.pick(&seeds).clone();
        let mutated = mutate_source(&mut rng, &src, i);
        if mutated.contains("#Unsafe") {
            continue;
        }
        if let Ok(out) = jet::compile_with_path(&mutated, &format!("fuzz_variant_{i}.jet")) {
            if strip_vetted_prelude_modules(&out.rust).contains("unsafe") {
                violations.push(format!(
                    "variant {i} from {shown}: source has no #Unsafe but generated code contains `unsafe`"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "I1 violated in fuzz_sema-generated programs (checked independently of \
         rustc availability):\n{}",
        violations.join("\n")
    );
}

fn check_fuzz_variant(i: usize, shown: &str, mutated: &str, file_str: &str) {
    match jet::compile_with_path(mutated, file_str) {
        Ok(out) => {
            if !mutated.contains("#Unsafe") {
                assert!(
                    !strip_vetted_prelude_modules(&out.rust).contains("unsafe"),
                    "I1 violated in fuzz variant {i} from {shown}: source has no #Unsafe but generated code contains `unsafe`"
                );
            }
            if out.rust.contains("extern crate jet_ffi_") {
                return;
            }
            if let Err(rustc_err) = rustc_accepts(&format!("v{i}"), &out.rust) {
                panic!(
                    "I2 violated: sema accepted variant {i} from {shown} but rustc rejected:\n{rustc_err}\n--- generated ---\n{}",
                    out.rust
                );
            }
        }
        Err(diags) => {
            assert!(
                !diags.is_empty(),
                "compile_with_path returned empty diags for variant {i} from {shown}"
            );
            for d in &diags {
                assert!(
                    is_jet_diagnostic(d),
                    "variant {i} from {shown}: non-jet diagnostic code `{}`",
                    d.code
                );
            }
        }
    }
}
