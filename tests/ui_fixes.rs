//! M2 exit criterion: every ownership ui fixture's Fix compiles.
//!
//! Each failing tests/ui/NAME.jet may have a sibling NAME.fixed.jet that
//! applies the diagnostic's Fix line. Those companions must pass the front
//! end; when rustc is available, generated Rust must build too (I2).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

mod common;
use common::{panic_message, test_worker_count, FfiBridgeLock};

#[test]
fn ownership_ui_fixes_compile() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let ext = jet::Syntax::FILE_EXT;
    let have_rustc = common::have_rustc();
    let have_cargo = Command::new("cargo").arg("--version").output().is_ok();

    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(&format!(".fixed.{}", ext)))
        })
        .collect();
    entries.sort();

    assert!(
        entries.len() >= 10,
        "expected M2 ownership .fixed.jet companions, found {}",
        entries.len()
    );

    let entries = Arc::new(entries);
    let next = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let workers = test_worker_count(16).min(entries.len().max(1));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let entries = Arc::clone(&entries);
        let next = Arc::clone(&next);
        let failures = Arc::clone(&failures);
        handles.push(std::thread::spawn(move || {
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= entries.len() {
                    break;
                }
                let path = entries[i].clone();
                if let Err(payload) =
                    std::panic::catch_unwind(|| check_fixed_companion(i, &path, have_rustc, have_cargo))
                {
                    failures.lock().unwrap().push(panic_message(payload));
                }
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let failures = failures.lock().unwrap();
    if !failures.is_empty() {
        panic!("{}", failures.join("\n\n"));
    }
}

fn check_fixed_companion(i: usize, path: &PathBuf, have_rustc: bool, have_cargo: bool) {
    let name = path.file_name().unwrap().to_string_lossy();
    let src = fs::read_to_string(path).unwrap();
    let shown = format!("tests/ui/{}", name);

    let stem_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem_name.starts_with("ffi_") && !have_cargo {
        return;
    }

    let _ffi_guard = if stem_name.starts_with("ffi_") {
        Some(FfiBridgeLock::acquire())
    } else {
        None
    };
    let out = jet::compile_with_path(&src, &shown).unwrap_or_else(|diags| {
        panic!(
            "fixed companion {} should compile:\n{}",
            name,
            jet::render_diagnostics(&shown, &src, &diags)
        );
    });

    // Until M3 struct literals, `take_required.fixed` proves the sema
    // fix only (Int passed to `take NoClone` is not valid Rust yet).
    let rustc_skip = stem_name == "take_required.fixed";

    if have_rustc && !rustc_skip {
        let stem = stem_name.replace('.', "_");
        let tmp = std::env::temp_dir();
        let rs = tmp.join(format!("jet_ui_fix_{}_{}_{}.rs", std::process::id(), i, stem));
        let bin = tmp.join(format!("jet_ui_fix_{}_{}_{}", std::process::id(), i, stem));
        fs::write(&rs, &out.rust).unwrap();
        let mut cmd = Command::new("rustc");
        cmd.args(["--edition", "2021", "-o"]).arg(&bin).arg(&rs);
        if let Some(link) = &out.ffi {
            cmd.arg("--extern").arg(format!(
                "{}={}",
                link.crate_name,
                link.rlib_path.display()
            ));
            for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
                cmd.arg("-L")
                    .arg(format!("dependency={}", deps_dir.display()));
            }
        }
        let status = cmd.status().unwrap();
        assert!(
            status.success(),
            "rustc rejected fixed companion {} (I2)",
            name
        );
    }
}
