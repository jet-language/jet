//! jet CLI: check / build / run / test / new / fmt.
//!
//! The driver owns invariant I2: rustc's voice never reaches the user as
//! if it were their fault. A rustc failure on generated code is reported
//! as an internal compiler error in jet.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

fn usage() -> String {
    format!(
        "\
{bin} — compiler for {lang}

usage:
  {bin} check <file.{ext}>     look for problems, build nothing
  {bin} build <file.{ext}>     compile to a native binary in ./build/
  {bin} run   <file.{ext}>     build, then run
  {bin} test  <file|dir>       compile and run top-level test blocks
  {bin} new   <name>           create a new project folder
  {bin} fmt   <file.{ext}>     rewrite file to canonical style (S44)

flags:
  --emit-rust                  also print the generated Rust code
  --check                      with fmt: exit 1 if file would change (CI)
",
        bin = jet::syntax::BINARY_NAME,
        lang = jet::syntax::LANG_NAME,
        ext = jet::syntax::FILE_EXT,
    )
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let emit_rust = raw.iter().any(|a| a == "--emit-rust");
    let fmt_check = raw.iter().any(|a| a == "--check");
    let args: Vec<&String> = raw.iter().filter(|a| !a.starts_with("--")).collect();

    let (cmd, target) = match (args.first(), args.get(1)) {
        (Some(c), Some(f)) => (c.as_str(), f.as_str()),
        _ => {
            eprint!("{}", usage());
            exit(2);
        }
    };

    match cmd {
        "fmt" => run_fmt(target, fmt_check),
        "new" => run_new(target),
        "test" => run_test(target),
        _ => run_compile_cmd(cmd, target, emit_rust),
    }
}

fn run_compile_cmd(cmd: &str, file: &str, emit_rust: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(
                " fix: check the spelling, or run {} from the folder that contains it",
                jet::syntax::BINARY_NAME
            );
            exit(1);
        }
    };

    let rust_code = match jet::compile_with_path(&src, file) {
        Ok(out) => {
            if !out.lints.is_empty() {
                eprint!("{}", jet::render_diagnostics(file, &src, &out.lints));
                let n = out.lints.len();
                eprintln!(
                    "\n{} warning{} emitted (compilation continues)",
                    n,
                    if n == 1 { "" } else { "s" }
                );
            }
            out.rust
        }
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            let n = diags.len();
            eprintln!("\n{} problem{} found", n, if n == 1 { "" } else { "s" });
            exit(1);
        }
    };

    if emit_rust {
        print!("{}", rust_code);
    }

    match cmd {
        "check" => {
            println!("ok: `{}` has no problems", file);
        }
        "build" => {
            build(file, &rust_code, bin_path(file));
            println!("built: {}", bin_path(file).display());
        }
        "run" => {
            let out = bin_path(file);
            build(file, &rust_code, out.clone());
            let status = Command::new(&out).status().unwrap_or_else(|e| {
                eprintln!("error: couldn't run the built program: {}", e);
                exit(1);
            });
            exit(status.code().unwrap_or(0));
        }
        other => {
            eprintln!(
                "error: `{}` isn't a {} command",
                other,
                jet::syntax::BINARY_NAME
            );
            eprint!("{}", usage());
            exit(2);
        }
    }
}

fn run_new(name: &str) {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        eprintln!("error: project name must be a simple folder name");
        eprintln!(" fix: try: {} new my_app", jet::syntax::BINARY_NAME);
        exit(1);
    }
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("error: `{}` already exists", name);
        exit(1);
    }
    fs::create_dir_all(dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`: {}", name, e);
        exit(1);
    });
    let main_src = format!(
        "fn main() {{\n    print(\"hello, world\");\n}}\n"
    );
    fs::write(dir.join("main.jet"), main_src).unwrap_or_else(|e| {
        eprintln!("error: couldn't write main.jet: {}", e);
        exit(1);
    });
    fs::write(dir.join(".gitignore"), "build/\n").unwrap_or_else(|e| {
        eprintln!("error: couldn't write .gitignore: {}", e);
        exit(1);
    });
    println!("created {}/", name);
    println!("  main.jet");
    println!("  .gitignore");
    println!("next: {} run {}/main.jet", jet::syntax::BINARY_NAME, name);
}

fn run_test(path: &str) {
    let p = Path::new(path);
    if !p.exists() {
        eprintln!("error: can't find `{}`", path);
        exit(1);
    }
    if p.is_dir() {
        let ext = jet::syntax::FILE_EXT;
        let mut files: Vec<PathBuf> = fs::read_dir(p)
            .unwrap_or_else(|e| {
                eprintln!("error: couldn't read `{}`: {}", path, e);
                exit(1);
            })
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| f.extension().and_then(|e| e.to_str()) == Some(ext))
            .collect();
        files.sort();
        if files.is_empty() {
            eprintln!("error: no .{} files in `{}`", ext, path);
            exit(1);
        }
        let mut any_fail = false;
        for f in files {
            if !run_test_file(&f) {
                any_fail = true;
            }
        }
        exit(if any_fail { 1 } else { 0 });
    }
    exit(if run_test_file(p) { 0 } else { 1 });
}

fn run_test_file(path: &Path) -> bool {
    let shown = path.to_string_lossy();
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read `{}`: {}", shown, e);
            return false;
        }
    };
    let rust_code = match jet::compile_tests_with_path(&src, &shown) {
        Ok(r) => r,
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(&shown, &src, &diags));
            return false;
        }
    };
    let bin = test_bin_path(path);
    build(&shown, &rust_code, bin.clone());
    let out = Command::new(&bin).output().unwrap_or_else(|e| {
        eprintln!("error: couldn't run tests in `{}`: {}", shown, e);
        exit(1);
    });
    print!("{}", String::from_utf8_lossy(&out.stdout));
    if !out.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }
    out.status.success()
}

fn run_fmt(file: &str, check_only: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            exit(1);
        }
    };
    let formatted = match jet::format_source(&src) {
        Ok(s) => s,
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            exit(1);
        }
    };
    if formatted == src {
        return;
    }
    if check_only {
        print!("{}", jet::fmt::unified_diff(file, &src, &formatted));
        exit(1);
    }
    fs::write(file, &formatted).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", file, e);
        exit(1);
    });
}

fn stem(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string())
        .replace('.', "_")
}

fn bin_path(file: &str) -> PathBuf {
    PathBuf::from("build").join(stem(file))
}

fn test_bin_path(path: &Path) -> PathBuf {
    PathBuf::from("build").join(format!("test_{}", stem(&path.to_string_lossy())))
}

fn build(file: &str, rust_code: &str, bin: PathBuf) {
    fs::create_dir_all("build").unwrap_or_else(|e| {
        eprintln!("error: couldn't create the build/ folder: {}", e);
        exit(1);
    });
    let rs_path = PathBuf::from("build").join(format!("{}.rs", stem(file)));
    fs::write(&rs_path, rust_code).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", rs_path.display(), e);
        exit(1);
    });

    let out = Command::new("rustc")
        .args([
            "--edition", "2021",
            "-O",
            "-C", "strip=symbols",
            "-C", "lto=thin",
        ])
        .arg(&rs_path)
        .arg("-o")
        .arg(&bin)
        .output();

    let out = match out {
        Ok(o) => o,
        Err(_) => {
            eprintln!("error: couldn't find `rustc` on this machine");
            eprintln!(" why: v1 of this language uses Rust as its backend (docs/03-architecture.md)");
            eprintln!(" fix: install Rust from https://rustup.rs, then try again");
            exit(1);
        }
    };

    if !out.status.success() {
        eprintln!("internal compiler error: the generated Rust did not compile.");
        eprintln!(
            "This is a bug in {}, NOT in your program. Please report it,",
            jet::syntax::BINARY_NAME
        );
        eprintln!("attaching your source file and the generated file below.");
        eprintln!("  generated: {}", rs_path.display());
        eprintln!("--- rustc said ---");
        eprintln!("{}", String::from_utf8_lossy(&out.stderr));
        exit(101);
    }
}
