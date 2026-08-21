mod common;
mod tir_support;

use std::fs;
use std::process::Command;

use jet::Sema::EffectSet;
use jetpack::EffectBudget::{self, PackageEffects};

const FFI_LEAF_SOURCE: &str = r#"
@message :: "ffi leaf"

fn run() :[FFI.Py, IO]> {
    print(@message)
}
"#;

#[test]
fn package_deny_ffi_covers_every_language_leaf() {
    let source = r#"
name: "ffi_denied"
version: "0.1.0"
authority: .{ holds: { deny: [FFI] } }
"#;
    let package = jetpack::Package::PackageFacts::parse(source, "package.jet")
        .expect("one-line FFI denial should parse");
    assert_eq!(package.effects_deny, Some(vec!["FFI".to_string()]));

    let leaves = [
        "FFI.Go",
        "FFI.Java",
        "FFI.DotNet",
        "FFI.Fortran",
        "FFI.Cobol",
        "FFI.Tcl",
        "FFI.Lua",
        "FFI.Ada",
        "FFI.Pascal",
        "FFI.Dart",
        "FFI.PowerShell",
        "FFI.Perl",
        "FFI.Ruby",
        "FFI.Php",
        "FFI.R",
        "FFI.Com",
        "FFI.Cpp",
        "FFI.Py",
        "FFI.Octave",
    ];
    let entry = PackageEffects {
        name: "foreign-dep".to_string(),
        effects: EffectSet::from(leaves.map(str::to_string)),
        panic_sites: Vec::new(),
        boundary_span: None,
    };
    let diagnostics = EffectBudget::enforce(&[entry], &package);
    assert_eq!(diagnostics.len(), leaves.len());
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.code == "E1220"));
    for leaf in leaves {
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.what.contains(leaf))
                .count(),
            1,
            "FFI denial must report the specific leaf {leaf}"
        );
    }
}

#[test]
fn retired_flat_ffi_spellings_are_rejected_by_the_compiler() {
    for root in [
        "Go", "Java", "DotNet", "Fortran", "Cobol", "Tcl", "Lua", "Ada", "Pascal",
        "Dart", "PowerShell", "Perl", "Ruby", "Php", "R", "Com", "Cpp", "Py", "Octave",
    ] {
        let source = format!("fn run() :[{root}]> {{}}\n");
        let diagnostics = jet::compile(&source).expect_err("retired flat effect must not parse");
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == "E0119"),
            "expected E0119 for retired effect {root}, got {diagnostics:#?}"
        );
    }
}

#[test]
fn i9_parser_accepts_the_ffi_leaf_row() {
    let (tokens, diagnostics) = jet::Lexer::lex(FFI_LEAF_SOURCE);
    assert!(
        diagnostics.is_empty(),
        "parser lexed the FFI leaf differently: {diagnostics:#?}"
    );
    let ast = jet::Parser::parse(&tokens).expect("parser accepts FFI.Py");
    assert!(!ast.items.is_empty(), "parser dropped the FFI leaf program");
}

#[test]
fn i9_sema_accepts_the_ffi_leaf_row() {
    jet::compile(FFI_LEAF_SOURCE).expect("sema accepts FFI.Py under FFI");
}

#[test]
fn i9_tir_erases_the_ffi_leaf_row() {
    let output = jet::compile(FFI_LEAF_SOURCE).expect("TIR accepts FFI.Py");
    assert!(
        !output.rust.contains("FFI.Py"),
        "effect leaf leaked into Rust"
    );
}

#[test]
fn i9_aot_runs_the_ffi_leaf_row() {
    let (code, stdout) = tir_support::build_and_run("ffi_leaf_aot", FFI_LEAF_SOURCE);
    assert_eq!(code, 0);
    assert_eq!(stdout, "ffi leaf\n");
}

#[test]
fn i9_jit_runs_the_ffi_leaf_row() {
    let (code, stdout, stderr) = tir_support::jit_run("ffi_leaf_jit", FFI_LEAF_SOURCE);
    assert_eq!(code, 0, "JIT stderr: {stderr}");
    assert_eq!(stdout, "ffi leaf\n");
}

#[test]
fn i9_interpreter_runs_the_ffi_leaf_row() {
    let (code, stdout, stderr) =
        tir_support::interpreter_run("ffi_leaf_interpreter", FFI_LEAF_SOURCE);
    assert_eq!(code, 0, "interpreter stderr: {stderr}");
    assert_eq!(stdout, "ffi leaf\n");
}

#[test]
fn i9_dev_runs_the_ffi_leaf_row() {
    let root = std::env::temp_dir().join(format!("jet_ffi_leaf_dev_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("dev scratch");
    let entry = root.join("main.jet");
    fs::write(&entry, FFI_LEAF_SOURCE).expect("dev source");
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["dev", entry.to_str().unwrap(), "--watch=off"])
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .output()
        .expect("dev command");
    let _ = fs::remove_dir_all(&root);
    assert!(
        output.status.success(),
        "dev stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ffi leaf\n");
}

#[test]
fn i9_comptime_keeps_the_ffi_leaf_row() {
    let output = jet::compile(FFI_LEAF_SOURCE).expect("comptime accepts FFI.Py");
    assert!(output.rust.contains("ffi leaf"), "comptime value was lost");
}

#[test]
fn i9_repl_runs_the_ffi_leaf_row() {
    let transcript = jet::REPL::run_transcript(&[FFI_LEAF_SOURCE, ":run"], None);
    assert!(transcript.contains("ffi leaf"), "REPL output: {transcript}");
}

#[test]
fn i9_web_accepts_the_ffi_leaf_row() {
    let output = jet::compile_web_with_path(FFI_LEAF_SOURCE, "ffi_leaf_web.jet")
        .expect("web accepts FFI.Py");
    assert!(
        output.web.is_some(),
        "web tier dropped the FFI leaf program"
    );
}
