//! M2 exit criterion: every ownership ui fixture's Fix compiles.
//!
//! Each failing tests/ui/NAME.jet may have a sibling NAME.fixed.jet that
//! applies the diagnostic's Fix line. Those companions must pass the front
//! end; when rustc is available, generated Rust must build too (I2).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn ownership_ui_fixes_compile() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let ext = jet::syntax::FILE_EXT;
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();

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

    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy();
        let src = fs::read_to_string(&path).unwrap();
        let shown = format!("tests/ui/{}", name);

        let out = jet::compile(&src).unwrap_or_else(|diags| {
            panic!(
                "fixed companion {} should compile:\n{}",
                name,
                jet::render_diagnostics(&shown, &src, &diags)
            );
        });

        // Until M3 struct literals, `take_required.fixed` proves the sema
        // fix only (Int passed to `take NoClone` is not valid Rust yet).
        let stem_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let rustc_skip = stem_name == "take_required.fixed";

        if have_rustc && !rustc_skip {
            let stem = stem_name.replace('.', "_");
            let tmp = std::env::temp_dir();
            let rs = tmp.join(format!("jet_ui_fix_{}.rs", stem));
            let bin = tmp.join(format!("jet_ui_fix_{}", stem));
            fs::write(&rs, &out.rust).unwrap();
            let status = Command::new("rustc")
                .args(["--edition", "2021", "-o"])
                .arg(&bin)
                .arg(&rs)
                .status()
                .unwrap();
            assert!(
                status.success(),
                "rustc rejected fixed companion {} (I2)",
                name
            );
        }
    }
}
