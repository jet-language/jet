//! M14 soundness fuzz: sema-checked programs must compile with rustc (I2).
//! Short CI run (N=50); use FUZZ_SEED for reproducibility.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const VARIANTS: usize = 50;

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
    let ex_dir = root.join("examples");
    let ext = jet::syntax::FILE_EXT;
    let mut seeds = Vec::new();
    for e in fs::read_dir(&ex_dir).unwrap().flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) == Some(ext) {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            if stem == "22_ffi" {
                continue;
            }
            let src = fs::read_to_string(&path).unwrap();
            let shown = format!("examples/{}.{}", stem, ext);
            seeds.push((shown, src));
        }
    }
    seeds.sort_by(|a, b| a.0.cmp(&b.0));
    seeds
}

fn mutate_source(rng: &mut Rng, src: &str, variant: usize) -> String {
    let n = variant;
    match rng.next() % 3 {
        0 => format!("{src}\nval _fuzz_{n} = {n};\n"),
        1 => format!("{src}\nfn _fuzz_fn_{n}() {{\n    val _x = {n};\n    return;\n}}\n"),
        _ => format!("{src}\nfn _fuzz_wrap_{n}() -> Int {{\n    return {n};\n}}\n"),
    }
}

fn is_jet_diagnostic(d: &jet::diag::Diagnostic) -> bool {
    let c = d.code;
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
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
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

    for i in 0..VARIANTS {
        let (shown, src) = rng.pick(&seeds).clone();
        let mutated = mutate_source(&mut rng, &src, i);
        let file = fuzz_dir.join(format!("variant_{i}.jet"));
        fs::write(&file, &mutated).unwrap();
        let file_str = file.to_string_lossy().into_owned();

        match jet::compile_with_path(&mutated, &file_str) {
            Ok(out) => {
                assert!(
                    !out.rust.contains("unsafe"),
                    "I1 violated in fuzz variant {i} from {shown}"
                );
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

    let _ = fs::remove_dir_all(&fuzz_dir);
}
