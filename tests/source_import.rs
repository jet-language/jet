mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NONCE: AtomicU64 = AtomicU64::new(0);

fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn workspace(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jet-source-import-{tag}-{}-{}",
        std::process::id(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("python/app")).unwrap();
    root
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(jet())
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn python_source_import_translates_real_subset_and_reports_every_gap() {
    let root = workspace("vertical");
    fs::write(
        root.join("python/app/math.py"),
        r#"import decimal

def add(x: int, y: int) -> int:
    total = x + y
    return total

def test_add() -> None:
    assert add(2, 3) == 5

def risky(x: int) -> int:
    if x > 0:
        return x
    raise RuntimeError("negative")
"#,
    )
    .unwrap();

    let output = run(&root, &["import", "py", "python/app"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.replace(&root.display().to_string(), "$ROOT"),
        include_str!("fixtures/source_import/vertical.stdout")
    );
    assert!(
        stdout.contains("2 functions, 1 carried tests, 2 TODO diagnostics"),
        "{stdout}"
    );
    assert!(stdout.contains("JT0101"), "{stdout}");

    let generated = fs::read_to_string(root.join("jet/app/math.jet")).unwrap();
    assert!(
        generated.contains("fn add(x: Int, y: Int) Int ->"),
        "{generated}"
    );
    assert!(generated.contains("total := x + y"), "{generated}");
    assert!(generated.contains("#Test fn test_add()"), "{generated}");
    assert!(generated.contains("assert_eq(add(2, 3), 5)"), "{generated}");
    assert!(
        !generated.contains("fn risky"),
        "unsupported body became fake code: {generated}"
    );
    assert!(
        !generated.contains("decimal"),
        "unsupported import leaked into output: {generated}"
    );
    let checked = run(&root, &["check", "jet/app/math.jet"]);
    assert_eq!(
        checked.status.code(),
        Some(0),
        "generated Jet failed its own front end:\n{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let report = fs::read_to_string(root.join("jet/app/import-report.json")).unwrap();
    assert!(jet_foundation::Registry::diagnostic("JT0101").is_some());
    assert!(
        report.contains("\"schema\":\"jet.source-import.v1\""),
        "{report}"
    );
    assert!(report.contains("\"code\":\"JT0101\""), "{report}");
    assert!(report.contains("\"what\":"), "{report}");
    assert!(report.contains("\"why\":"), "{report}");
    assert!(report.contains("\"fix\":"), "{report}");
    assert!(report.contains("\"source\":"), "{report}");
    assert!(report.contains("\"generated_target\":"), "{report}");
    assert!(
        report.contains("\"migration_status\":\"omitted-reported\""),
        "{report}"
    );
}

#[test]
fn source_import_reports_typed_operation_failure() {
    let root = workspace("operation");
    let output = run(&root, &["import", "py", "missing"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        include_str!("fixtures/source_import/operation.stderr")
    );
    assert!(!root.join("jet").exists());
}

#[test]
fn python_source_import_matches_python_oracle_fixed_corpus() {
    let root = workspace("differential");
    fs::write(
        root.join("python/app/corpus.py"),
        include_str!("fixtures/source_import/corpus.py"),
    )
    .unwrap();

    let imported = run(&root, &["import", "py", "python/app"]);
    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let generated = root.join("jet/app/corpus.jet");
    assert_eq!(
        fs::read_to_string(&generated).unwrap(),
        include_str!("fixtures/source_import/corpus.jet")
    );

    let checked = run(&root, &["check", "jet/app/corpus.jet"]);
    assert_eq!(
        checked.status.code(),
        Some(0),
        "generated Jet failed its own front end:\n{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let oracle = Command::new("python3")
        .current_dir(&root)
        .args([
            "-c",
            "import runpy; runpy.run_path('python/app/corpus.py')['run']()",
        ])
        .output()
        .expect("Python 3 is the source-language differential oracle");
    assert_eq!(
        oracle.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    assert_eq!(
        oracle.stdout.as_slice(),
        include_bytes!("fixtures/source_import/corpus.out").as_slice()
    );

    let translated = run(&root, &["run", "jet/app/corpus.jet"]);
    assert_eq!(
        translated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&translated.stderr)
    );
    assert_eq!(translated.stderr, oracle.stderr);
    assert_eq!(
        translated.stdout, oracle.stdout,
        "imported output diverged from Python oracle"
    );
}

#[test]
fn pascal_source_import_preserves_source_and_emits_binder_todo() {
    let root = workspace("pascal");
    let source_dir = root.join("pascal/app");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(
        source_dir.join("inventory.pas"),
        r#"library inventory;
function add_scalar(A, B: Int64): Int64; cdecl;
begin Result := A + B; end;
exports add_scalar;
begin end.
"#,
    )
    .unwrap();

    let output = run(&root, &["import", "pascal", "pascal/app"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("0 functions, 0 carried tests, 1 TODO diagnostics"),
        "{stdout}"
    );
    assert!(stdout.contains("JT0101"), "{stdout}");

    let generated = fs::read_to_string(root.join("jet/app/inventory.jet")).unwrap();
    assert!(
        generated.contains("Generated by jet import pascal"),
        "{generated}"
    );
    assert!(
        generated.contains("use pascal.inventory as inventory"),
        "{generated}"
    );
    assert!(generated.contains("TODO JT0101"), "{generated}");
    assert!(
        generated.contains("canonical source of truth"),
        "{generated}"
    );
    assert!(
        !generated.contains("fn add_scalar"),
        "Pascal source became guessed Jet: {generated}"
    );
    let checked = run(&root, &["check", "jet/app/inventory.jet"]);
    assert_eq!(
        checked.status.code(),
        Some(0),
        "binder stub failed its own front end:\n{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let report = fs::read_to_string(root.join("jet/app/import-report.json")).unwrap();
    assert!(report.contains("\"language\":\"pascal\""), "{report}");
    assert!(report.contains("inventory.pas"), "{report}");
    assert!(report.contains("\"code\":\"JT0101\""), "{report}");
    assert!(
        report.contains("\"migration_status\":\"omitted-reported\""),
        "{report}"
    );
}

#[test]
fn ada_source_import_preserves_package_and_emits_binder_todo() {
    let root = workspace("ada");
    let source_dir = root.join("ada/app");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(
        source_dir.join("geodesy.ads"),
        r#"with Interfaces.C;
package Geodesy is
   subtype Latitude is Interfaces.C.double range -90.0 .. 90.0;
   function Double_Lat (Lat : Latitude) return Interfaces.C.double
     with Export, Convention => C, External_Name => "geo_double";
end Geodesy;
"#,
    )
    .unwrap();
    fs::write(
        source_dir.join("geodesy.adb"),
        "package body Geodesy is begin null; end Geodesy;\n",
    )
    .unwrap();

    let output = run(&root, &["import", "ada", "ada/app"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("0 functions, 0 carried tests, 1 TODO diagnostics"),
        "{stdout}"
    );
    let generated = fs::read_to_string(root.join("jet/app/geodesy.jet")).unwrap();
    assert!(
        generated.contains("Generated by jet import ada"),
        "{generated}"
    );
    assert!(
        generated.contains("use ada.geodesy as geodesy"),
        "{generated}"
    );
    assert!(generated.contains("TODO JT0101"), "{generated}");
    assert!(
        generated.contains("canonical source of truth"),
        "{generated}"
    );
    assert!(
        !generated.contains("fn double_lat"),
        "Ada source became guessed Jet: {generated}"
    );
    let checked = run(&root, &["check", "jet/app/geodesy.jet"]);
    assert_eq!(
        checked.status.code(),
        Some(0),
        "generated Ada stub failed its own front end:\n{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let report = fs::read_to_string(root.join("jet/app/import-report.json")).unwrap();
    assert!(report.contains("\"language\":\"ada\""), "{report}");
    assert!(report.contains("geodesy.ads"), "{report}");
    assert!(report.contains("\"code\":\"JT0101\""), "{report}");
    assert!(
        report.contains("\"migration_status\":\"omitted-reported\""),
        "{report}"
    );
}

#[test]
fn source_import_dry_run_idempotence_and_three_way_update_are_honest() {
    let root = workspace("rerun");
    let source = root.join("python/app/main.py");
    fs::write(&source, "def answer() -> int:\n    return 42\n").unwrap();

    let preview = run(&root, &["import", "py", "python/app", "--dry-run"]);
    assert_eq!(preview.status.code(), Some(0));
    assert!(!root.join("jet").exists(), "dry-run wrote output");

    let first = run(&root, &["import", "py", "python/app"]);
    assert_eq!(first.status.code(), Some(0));
    let generated = root.join("jet/app/main.jet");
    let original = fs::read_to_string(&generated).unwrap();

    let rerun = run(&root, &["import", "py", "python/app"]);
    assert_eq!(
        rerun.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert_eq!(fs::read_to_string(&generated).unwrap(), original);

    let edited = format!("{original}\nfn owner_edit() {{}}\n");
    fs::write(&generated, &edited).unwrap();
    let create_conflict = run(&root, &["import", "py", "python/app"]);
    assert_eq!(create_conflict.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&create_conflict.stderr)
            .replace(&root.display().to_string(), "$ROOT"),
        include_str!("fixtures/source_import/conflict.stderr")
    );

    let preserve = run(&root, &["import", "py", "python/app", "--update"]);
    assert_eq!(
        preserve.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&preserve.stderr)
    );
    assert_eq!(fs::read_to_string(&generated).unwrap(), edited);

    let report = root.join("jet/app/import-report.json");
    let report_original = fs::read_to_string(&report).unwrap();
    let report_edited = format!("{report_original}\n{{\"owner_note\":true}}\n");
    fs::write(&report, &report_edited).unwrap();
    let report_preserve = run(&root, &["import", "py", "python/app", "--update"]);
    assert_eq!(report_preserve.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&report_preserve.stderr).contains("JT0199"));
    assert_eq!(fs::read_to_string(&report).unwrap(), report_edited);

    fs::write(&source, "def answer() -> int:\n    return 43\n").unwrap();
    let both_changed = run(&root, &["import", "py", "python/app", "--update"]);
    assert_eq!(both_changed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&both_changed.stderr).contains("JT0199"));
    assert_eq!(fs::read_to_string(&generated).unwrap(), edited);
    assert_eq!(fs::read_to_string(&report).unwrap(), report_edited);
}

#[test]
fn source_import_rejects_unknown_language_and_does_not_follow_symlinks() {
    let root = workspace("hostile");
    fs::write(
        root.join("python/app/good.py"),
        "def Ok() -> int:\n    return 1\n",
    )
    .unwrap();
    let unknown = run(&root, &["import", "ruby", "python/app"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(!root.join("jet").exists());

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc", root.join("python/app/outside")).unwrap();
        let output = run(&root, &["import", "py", "python/app"]);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            fs::read_dir(root.join("jet/app"))
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().and_then(|v| v.to_str()) == Some("jet"))
                .count(),
            1
        );
    }
}

#[test]
fn enterprise_importers_match_scalar_behavior_fixtures_and_keep_source_unchanged() {
    let cases = [
        (
            "java",
            "java",
            include_str!("fixtures/source_import/enterprise/java/Math.java"),
            "fn add(left: Int, right: Int) Int ->",
            "5\n",
        ),
        (
            "csharp",
            "cs",
            include_str!("fixtures/source_import/enterprise/csharp/Math.cs"),
            "fn add(left: Int, right: Int) Int ->",
            "5\n",
        ),
        (
            "ts",
            "ts",
            include_str!("fixtures/source_import/enterprise/ts/math.ts"),
            "fn add(left: Float, right: Float) Float ->",
            "5.0\n",
        ),
        (
            "js",
            "js",
            include_str!("fixtures/source_import/enterprise/js/math.js"),
            "fn add(left: Float, right: Float) Float ->",
            "5.0\n",
        ),
        (
            "go",
            "go",
            include_str!("fixtures/source_import/enterprise/go/math.go"),
            "fn add(left: Int, right: Int) Int ->",
            "5\n",
        ),
    ];

    for (language, extension, source, signature, expected_output) in cases {
        let root = workspace(&format!("enterprise-{language}"));
        let source_dir = root.join(language).join("app");
        fs::create_dir_all(&source_dir).unwrap();
        let source_path = source_dir.join(format!("program.{extension}"));
        fs::write(&source_path, source).unwrap();
        let source_arg = format!("{language}/app");

        let imported = run(&root, &["import", language, &source_arg]);
        assert_eq!(
            imported.status.code(),
            Some(0),
            "{language}: {}",
            String::from_utf8_lossy(&imported.stderr)
        );
        let generated_path = root.join("jet/app/program.jet");
        let generated = fs::read_to_string(&generated_path).unwrap();
        assert!(generated.contains(signature), "{language}: {generated}");
        assert!(
            generated.contains("// Source span:"),
            "{language}: {generated}"
        );
        assert!(
            generated.contains("D-MIGRATE-SRC1"),
            "{language}: {generated}"
        );
        assert!(
            !generated.contains("fn unsupported"),
            "{language}: {generated}"
        );
        assert_eq!(fs::read_to_string(&source_path).unwrap(), source);

        let report = fs::read_to_string(root.join("jet/app/import-report.json")).unwrap();
        assert!(
            report.contains("\"law\":\"D-MIGRATE-SRC1\""),
            "{language}: {report}"
        );
        assert!(report.contains("\"source_span\":"), "{language}: {report}");
        assert!(
            report.contains("\"generated_target\":"),
            "{language}: {report}"
        );
        assert!(
            report.contains("\"code\":\"JT0101\""),
            "{language}: {report}"
        );

        let checked = run(&root, &["check", "jet/app/program.jet"]);
        assert_eq!(
            checked.status.code(),
            Some(0),
            "{language}: generated Jet failed its own front end:\n{}",
            String::from_utf8_lossy(&checked.stderr)
        );
        let translated = run(&root, &["run", "jet/app/program.jet"]);
        assert_eq!(
            translated.status.code(),
            Some(0),
            "{language}: {}",
            String::from_utf8_lossy(&translated.stderr)
        );
        assert_eq!(translated.stdout, expected_output.as_bytes(), "{language}");

        let rerun = run(&root, &["import", language, &source_arg]);
        assert_eq!(rerun.status.code(), Some(0), "{language}: rerun failed");
        assert_eq!(fs::read_to_string(generated_path).unwrap(), generated);
    }
}

#[test]
fn enterprise_import_reports_ambiguity_malformed_input_and_no_cpp_importer() {
    let root = workspace("enterprise-failures");
    let source_dir = root.join("java/app");
    fs::create_dir_all(&source_dir).unwrap();
    let source_path = source_dir.join("Ambiguous.java");
    let source = include_str!("fixtures/source_import/enterprise/failures/ambiguous.java");
    fs::write(&source_path, source).unwrap();

    let imported = run(&root, &["import", "java", "java/app"]);
    assert_eq!(imported.status.code(), Some(0));
    let generated_path = root.join("jet/app/Ambiguous.jet");
    let generated = fs::read_to_string(&generated_path).unwrap();
    assert!(
        generated.contains("fn keep(value: Int) Int ->"),
        "{generated}"
    );
    assert!(
        !generated.contains("fn add"),
        "ambiguous overload was guessed: {generated}"
    );
    assert!(
        !generated.contains("fn quotient"),
        "foreign division was guessed: {generated}"
    );
    assert!(
        !generated.contains("fn broken"),
        "malformed body was guessed: {generated}"
    );
    assert_eq!(fs::read_to_string(&source_path).unwrap(), source);

    let report = fs::read_to_string(root.join("jet/app/import-report.json")).unwrap();
    assert!(
        report.contains("unsupported java dependency declaration"),
        "{report}"
    );
    assert!(
        report.contains("ambiguous overloaded java function"),
        "{report}"
    );
    assert!(
        report.contains("foreign division or remainder semantics"),
        "{report}"
    );
    assert!(report.contains("malformed java function"), "{report}");
    assert!(
        report.contains("\"migration_status\":\"omitted-reported\""),
        "{report}"
    );

    let checked = run(&root, &["check", "jet/app/Ambiguous.jet"]);
    assert_eq!(
        checked.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    for language in ["c", "c++"] {
        let unavailable = run(&root, &["import", language, "java/app"]);
        assert_eq!(unavailable.status.code(), Some(2), "{language}");
        let stderr = String::from_utf8_lossy(&unavailable.stderr);
        assert!(
            stderr.contains("intentionally unavailable"),
            "{language}: {stderr}"
        );
        assert!(stderr.contains("binder"), "{language}: {stderr}");
    }
}

#[test]
fn javascript_import_does_not_rewrite_foreign_names_inside_strings() {
    let root = workspace("javascript-string");
    let source_dir = root.join("javascript/app");
    fs::create_dir_all(&source_dir).unwrap();
    let source = r#"/** @returns {string} */
function label() {
    return "console.log(";
}
"#;
    let source_path = source_dir.join("labels.js");
    fs::write(&source_path, source).unwrap();

    let imported = run(&root, &["import", "js", "javascript/app"]);
    assert_eq!(imported.status.code(), Some(0));
    let generated = fs::read_to_string(root.join("jet/app/labels.jet")).unwrap();
    assert!(generated.contains("return \"console.log(\""), "{generated}");
    assert_eq!(fs::read_to_string(source_path).unwrap(), source);

    let translated = run(&root, &["run", "jet/app/labels.jet"]);
    assert_eq!(translated.status.code(), Some(0));
    assert_eq!(translated.stdout, b"console.log(\n");
}

#[test]
fn enterprise_import_preserves_source_for_partial_failure() {
    let root = workspace("enterprise-partial-failure");
    let source_dir = root.join("ts/app");
    fs::create_dir_all(&source_dir).unwrap();
    let source = include_str!("fixtures/source_import/enterprise/failures/partial.ts");
    let source_path = source_dir.join("partial.ts");
    fs::write(&source_path, source).unwrap();

    let imported = run(&root, &["import", "ts", "ts/app"]);
    assert_eq!(
        imported.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let generated_path = root.join("jet/app/partial.jet");
    let generated = fs::read_to_string(&generated_path).unwrap();
    assert!(
        generated.contains("fn keep(value: Float) Float ->"),
        "{generated}"
    );
    assert!(
        generated.contains("increment := value + 1.0"),
        "{generated}"
    );
    for omitted in ["unsupported", "broken", "duplicate", "readFile"] {
        assert!(
            !generated.contains(&format!("fn {omitted}")),
            "unproven function `{omitted}` was guessed: {generated}"
        );
    }
    assert_eq!(fs::read_to_string(&source_path).unwrap(), source);

    let report = fs::read_to_string(root.join("jet/app/import-report.json")).unwrap();
    for reason in [
        "unsupported ts dependency declaration",
        "foreign type `number[]` is outside the scalar importer subset",
        "ts body `broken` was not translated",
        "ambiguous overloaded ts function `duplicate`",
    ] {
        assert!(report.contains(reason), "{reason}: {report}");
    }
    assert!(
        report
            .matches("\"migration_status\":\"omitted-reported\"")
            .count()
            >= 4,
        "{report}"
    );

    let checked = run(&root, &["check", "jet/app/partial.jet"]);
    assert_eq!(
        checked.status.code(),
        Some(0),
        "partial import generated invalid Jet:\n{}",
        String::from_utf8_lossy(&checked.stderr)
    );
}
