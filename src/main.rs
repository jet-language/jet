//! jet CLI: check / build / run.
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

flags:
  --emit-rust                  also print the generated Rust code
",
        bin = jet::syntax::BINARY_NAME,
        lang = jet::syntax::LANG_NAME,
        ext = jet::syntax::FILE_EXT,
    )
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let emit_rust = raw.iter().any(|a| a == "--emit-rust");
    let args: Vec<&String> = raw.iter().filter(|a| !a.starts_with("--")).collect();

    let (cmd, file) = match (args.first(), args.get(1)) {
        (Some(c), Some(f)) => (c.as_str(), f.as_str()),
        _ => {
            eprint!("{}", usage());
            exit(2);
        }
    };

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

    let rust_code = match jet::compile(&src) {
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
            build(file, &rust_code);
            println!("built: {}", bin_path(file).display());
        }
        "run" => {
            build(file, &rust_code);
            let status = Command::new(bin_path(file)).status().unwrap_or_else(|e| {
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

fn stem(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string())
}

fn bin_path(file: &str) -> PathBuf {
    PathBuf::from("build").join(stem(file))
}

fn build(file: &str, rust_code: &str) {
    fs::create_dir_all("build").unwrap_or_else(|e| {
        eprintln!("error: couldn't create the build/ folder: {}", e);
        exit(1);
    });
    let rs_path = PathBuf::from("build").join(format!("{}.rs", stem(file)));
    fs::write(&rs_path, rust_code).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", rs_path.display(), e);
        exit(1);
    });

    // Size strategy (docs/03 R8, S15 ratified): default keeps unwinding.
    // `jet build --small` (opt-level "z", panic=abort) arrives in M6.
    // `-O` is opt-level 2; strip + thin LTO drop unused code.
    let out = Command::new("rustc")
        .args([
            "--edition", "2021",
            "-O",
            "-C", "strip=symbols",
            "-C", "lto=thin",
        ])
        .arg(&rs_path)
        .arg("-o")
        .arg(bin_path(file))
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
        // Invariant I2: rustc errors are never the user's fault.
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
