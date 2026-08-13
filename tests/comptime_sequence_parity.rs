//! Card #392 packet A: sequence-method AOT/comptime parity.

use std::fs;
use std::process::Command;

use jet::Interpreter::{dev_iteration, RunOutcome};
use jet::REPL::run_transcript;

mod common;

#[test]
fn parallel_adapters_match_forced_interpreter_and_default_jit_dev() {
    let source = r#"fn run() {
    values :: [1, 2, 3, 4]
    print(values.para_map((n: Int) => n * 2))
    print(values.para_filter((n: Int) => n % 2 == 0))
    split :: values.para_partition((n: Int) => n % 2 == 0)
    print(split.false_)
    print(split.true_)
    print(values.para_fold(() => 0, (acc: Int, n: Int) => acc + n, (left: Int, right: Int) => left + right))
}
"#;
    let dir = common::unique_tmp("jet_para_dev_matrix");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    fs::write(&path, source).unwrap();
    let expected = "[2, 4, 6, 8]\n[2, 4]\n[1, 3]\n[2, 4]\n10\n";
    for (tier, use_interpreter) in [("forced-interpreter", true), ("default-jit-dev", false)] {
        match dev_iteration(path.to_str().unwrap(), false, use_interpreter) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(exit_code, 0, "{tier} exit");
                assert_eq!(stderr, "", "{tier} stderr");
                assert_eq!(stdout, expected, "{tier} stdout");
            }
            RunOutcome::Problems(diags) => panic!("{tier} rejected parallel adapters: {diags:#?}"),
        }
    }
}

#[test]
fn repl_list_and_fixed_list_sequence_methods_are_exact() {
    let out = run_transcript(
        &[
            "[1, 2, 3].all((n: Int) => n > 0)",
            "[1, 2, 3].any((n: Int) => n == 2)",
            "[1, 2, 3, 4, 5].chunks(2)",
            "[1, 2, 3, 4].count_by((n: Int) => \"{n % 2}\")",
            "[1, 1, 2, 2, 1].dedup()",
            "[4, 5].indexed()",
            "[\"1\", \"bad\", \"2\"].filter_map((s: String) => Int.parse(s))",
            "[7, 8].first()",
            "[1, 2, 3].flat_map((n: Int) => [n, n * 10])",
            "[[1, 2], [3], [4, 5]].flatten()",
            "[1, 2, 3].fold(10, (acc: Int, n: Int) => acc + n)",
            "[1, 2, 3, 4].group_by((n: Int) => \"{n % 2}\")",
            "[4, 5, 6].index_of(5)",
            "inserted := [1, 3]",
            "inserted.insert(1, 2)",
            "inserted",
            "[1, 2, 3].intersperse(0)",
            "[7, 8].last()",
            "[3, 9, 2].max()",
            "[\"bbb\", \"a\", \"cc\"].max_by((s: String) => s.len())",
            "[3, 9, 2].min()",
            "[\"bbb\", \"a\", \"cc\"].min_by((s: String) => s.len())",
            "[true, false, true].min()",
            "[true, false, true].max()",
            "[true, false, true].min_by((b: Bool) => b)",
            "[true, false, true].max_by((b: Bool) => b)",
            "[1, 2, 3, 4].para_filter((n: Int) => n % 2 == 0)",
            "[1, 2, 3].para_fold(() => 0, (acc: Int, n: Int) => acc + n, (left: Int, right: Int) => left + right)",
            "[1, 2, 3].para_map((n: Int) => n * 2)",
            "[1, 2, 3, 4].para_partition((n: Int) => n % 2 == 0)",
            "[1, 2, 3, 4].partition((n: Int) => n % 2 == 0)",
            "[4, 5, 6].position((n: Int) => n == 5)",
            "[2, 3, 4].product()",
            "[1, 2, 3].scan(0, (acc: Int, n: Int) => acc + n)",
            "[1, 2, 3, 4].skip(2)",
            "[1, 2, 3, 4].skip_while((n: Int) => n < 3)",
            "[1, 2, 3, 4, 5].step_by(2)",
            "[1, 2, 3, 4].sum()",
            "[1, 2, 3, 4].take(2)",
            "[1, 2, 3, 4].take_while((n: Int) => n < 3)",
            "[\"1\", \"2\"].map((s: String) => Int.parse(s)).try_collect()",
            "[(a: 1, b: \"x\"), (a: 2, b: \"y\")].unzip()",
            "[1, 2, 3, 4].windows(3)",
            "[1, 2, 3].zip([\"a\", \"b\"])",
            "fixed: [Int#4] :: [1, 2, 3, 4]",
            "fixed.all((n: Int) => n > 0)",
            "fixed.chunks(3)",
            "fixed.fold(0, (acc: Int, n: Int) => acc + n)",
            "fixed.zip([9, 8, 7])",
        ],
        None,
    );

    let expected = [
        "true : Bool",
        "true : Bool",
        "[[1, 2], [3, 4], [5]] : List",
        "[0: 2, 1: 2] : Map",
        "[1, 2, 1] : List",
        "[(idx,item)(idx: 0, item: 4), (idx,item)(idx: 1, item: 5)] : List",
        "[1, 2] : List",
        "7 : Option",
        "[1, 10, 2, 20, 3, 30] : List",
        "[1, 2, 3, 4, 5] : List",
        "16 : Int",
        "[0: [2, 4], 1: [1, 3]] : Map",
        "1 : Option",
        "[1, 2, 3] : List",
        "[1, 0, 2, 0, 3] : List",
        "8 : Option",
        "9 : Option",
        "bbb : Option",
        "2 : Option",
        "a : Option",
        "false : Option",
        "true : Option",
        "false : Option",
        "true : Option",
        "[2, 4] : List",
        "6 : Int",
        "[2, 4, 6] : List",
        "(false_,true_)(false_: [1, 3], true_: [2, 4]) : (false_,true_)",
        "(false_,true_)(false_: [1, 3], true_: [2, 4]) : (false_,true_)",
        "1 : Option",
        "24 : Int",
        "[1, 3, 6] : List",
        "[3, 4] : List",
        "[3, 4] : List",
        "[1, 3, 5] : List",
        "10 : Int",
        "[1, 2] : List",
        "[1, 2] : List",
        "[1, 2] : Result",
        "(a,b)(a: [1, 2], b: [x, y]) : (a,b)",
        "[[1, 2, 3], [2, 3, 4]] : List",
        "[(a,b)(a: 1, b: a), (a,b)(a: 2, b: b)] : List",
        "true : Bool",
        "[[1, 2, 3], [4]] : List",
        "10 : Int",
        "[(a,b)(a: 1, b: 9), (a,b)(a: 2, b: 8), (a,b)(a: 3, b: 7)] : List",
    ];
    let actual = out
        .lines()
        .filter(|line| !line.starts_with("inserted:") && !line.starts_with("fixed:"))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "full REPL output:\n{out}");
    assert!(!out.contains("E0956"), "sequence method fell through: {out}");
}

/// D-CORE-EAGER1=A / D-LOOPMAP1=B: the public REPL exposes eager concrete
/// adapters and the same names on an explicit lazy view.
#[test]
fn repl_eager_collection_adapters_and_lazy_opt_in_are_exact() {
    let out = run_transcript(
        &[
            "values := [1, 2, 3]",
            "values.map((n: Int) => n * 2)",
            "values.filter((n: Int) => n > 1)",
            "values.lazy().map((n: Int) => n * 10).to_list()",
        ],
        None,
    );
    let actual = out
        .lines()
        .filter(|line| !line.starts_with("values:"))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            "[2, 4, 6] : List",
            "[2, 3] : List",
            "[10, 20, 30] : List",
        ],
        "REPL eager/lazy output:\n{out}"
    );
}

#[test]
fn repl_view_and_view_mut_sequence_methods_are_exact() {
    let out = run_transcript(
        &[
            "values := [10, 20, 30, 40]",
            "view :: values[1..3]",
            "view.len()",
            "view.is_empty()",
            "view.get(1)",
            "view.first()",
            "view.last()",
            "view.contains(30)",
            "view.index_of(40)",
            "view.fold(0, (acc: Int, n: Int) => acc + n)",
            "view.map((n: Int) => n / 10)",
            "edit :: &values[0..1]",
            "edit.len()",
            "edit.is_empty()",
            "edit.get(1)",
            "edit.first()",
            "edit.last()",
            "edit.contains(20)",
            "edit.index_of(20)",
            "edit.fold(0, (acc: Int, n: Int) => acc + n)",
            "edit.map((n: Int) => n / 10)",
        ],
        None,
    );
    let actual = out
        .lines()
        .filter(|line| {
            !line.starts_with("values:")
                && !line.starts_with("view:")
                && !line.starts_with("edit:")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            "3 : Int", "false : Bool", "30 : Option", "20 : Option", "40 : Option",
            "true : Bool", "2 : Option", "90 : Int", "[2, 3, 4] : List",
            "2 : Int", "false : Bool", "20 : Option", "10 : Option", "20 : Option",
            "true : Bool", "1 : Option", "30 : Int", "[1, 2] : List",
        ],
        "full REPL output:\n{out}"
    );
    assert!(!out.contains("E0956"), "view method fell through: {out}");
}

#[test]
fn repl_non_slice_write_place_is_not_silently_copied() {
    let out = run_transcript(
        &["values := [1, 2]", "edit :: &values", "edit.len()"],
        None,
    );
    assert!(out.contains("E0956"), "non-slice write place was copied: {out}");
}

#[test]
fn sequence_return_shapes_match_rustc_backed_aot() {
    if !common::have_rustc() {
        eprintln!("note: rustc not found; skipping sequence differential battery");
        return;
    }
    let cases = [
        ("scalar", "", "[1, 2, 3].all((n: Int) => n > 0)"),
        (
            "float-literal-receiver",
            "",
            "[1.5, 2.5].all((n: Float) => n > 1.0)",
        ),
        (
            "bool-literal-receiver",
            "",
            "[true, false].any((b: Bool) => b)",
        ),
        (
            "float-fold-seed",
            "",
            "[1.5, 2.5].fold(0.5, (a: Float, n: Float) => a + n)",
        ),
        (
            "bool-fold-seed",
            "",
            "[true, false].fold(true, (a: Bool, b: Bool) => a && b)",
        ),
        (
            "float-reduce-seed",
            "",
            "[1.5, 2.5].reduce(0.5, (a: Float, n: Float) => a + n)",
        ),
        (
            "bool-scan-seed",
            "",
            "[true, false].scan(true, (a: Bool, b: Bool) => a && b)",
        ),
        (
            "float-para-fold-seed",
            "",
            "[1.5, 2.5].para_fold(() => 0.5, (a: Float, n: Float) => a + n, (left: Float, right: Float) => left + right)",
        ),
        (
            "empty-float-sum",
            "fn empty_sum() => Float {\n\
                 empty :: [Float].{}\n\
                 return empty.sum()\n\
             }",
            "empty_sum()",
        ),
        (
            "empty-float-product",
            "fn empty_product() => Float {\n\
                 empty :: [Float].{}\n\
                 return empty.product()\n\
             }",
            "empty_product()",
        ),
        (
            "f32-nonempty-sum",
            "fn f32_values() => [F32] { return [16777217.0, 1.0, 1.0] }",
            "f32_values().sum()",
        ),
        (
            "f32-nonempty-product",
            "fn f32_values() => [F32] { return [16777217.0, 2.0] }",
            "f32_values().product()",
        ),
        (
            "f32-fold",
            "fn f32_values() => [F32] { return [16777217.0, 1.0] }\n\
             fn f32_zero() => F32 { return 0.0 }",
            "f32_values().fold(f32_zero(), (a: F32, n: F32) => a + n)",
        ),
        (
            "f32-map",
            "fn f32_values() => [F32] { return [16777217.0, 1.0] }\n\
             fn f32_one() => F32 { return 1.0 }",
            "f32_values().map((n: F32) => n + f32_one())",
        ),
        (
            "f32-min",
            "fn f32_values() => [F32] { return [16777217.0, 1.0] }",
            "f32_values().min()",
        ),
        (
            "f32-max",
            "fn f32_values() => [F32] { return [16777217.0, 1.0] }",
            "f32_values().max()",
        ),
        (
            "empty-f32-sum",
            "fn empty_f32_sum() => F32 {\n\
                 empty :: [F32].{}\n\
                 return empty.sum()\n\
             }",
            "empty_f32_sum()",
        ),
        (
            "empty-f32-product",
            "fn empty_f32_product() => F32 {\n\
                 empty :: [F32].{}\n\
                 return empty.product()\n\
             }",
            "empty_f32_product()",
        ),
        ("optional", "", "[3, 1, 2].min()"),
        ("owned-list", "", "[1, 2, 3].take(2)"),
        ("nested-list", "", "[1, 2, 3, 4].chunks(3)"),
        ("map", "", "[1, 2, 3].group_by((n: Int) => \"{n % 2}\")"),
        ("tuple", "", "[(a: 1, b: \"x\"), (a: 2, b: \"y\")].unzip().a"),
        (
            "result",
            "",
            "[Int.parse(\"1\"), Int.parse(\"2\")].try_collect() ?? []",
        ),
        (
            "view",
            "fn view_sum(xs: [Int]) => Int {\n\
                 view :: xs[0..1]\n\
                 return view.fold(0, (a: Int, n: Int) => a + n)\n\
             }",
            "view_sum([10, 20, 30])",
        ),
    ];
    for (index, (name, preamble, expression)) in cases.iter().enumerate() {
        check_aot_comptime(index, name, preamble, expression);
    }
}

fn check_aot_comptime(index: usize, name: &str, preamble: &str, expression: &str) {
    let src = format!(
        "{preamble}\n@expected :: {expression}\nfn run() {{\n\
             actual :: {expression}\n\
             print(\"{{expected}}\")\n\
             print(\"{{actual}}\")\n\
         }}\n"
    );
    let compiled = jet::Driver::compile_generated_src(
        &src,
        "comptime_sequence_parity.jet",
        jet::Sema::CompileMode::Run,
    )
    .unwrap_or_else(|diags| {
        panic!(
            "{name} failed front end:\n{}",
            jet::render_diagnostics("comptime_sequence_parity.jet", &src, &diags)
        )
    });
    let user_rust = common::strip_vetted_prelude_modules(&compiled.rust);
    assert!(!user_rust.contains("unsafe"), "{name} generated unsafe");

    let dir = common::unique_tmp("jet_sequence");
    fs::create_dir_all(&dir).unwrap();
    let rs = dir.join(format!("jet_sequence_{}_{}.rs", std::process::id(), index));
    let bin = dir.join(format!("jet_sequence_{}_{}", std::process::id(), index));
    fs::write(&rs, compiled.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "I2 violated for {name}: {}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "{name} runtime failed");
    let lines = String::from_utf8(run.stdout).unwrap();
    let lines = lines.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2, "{name} output shape: {lines:?}");
    assert_eq!(lines[0], lines[1], "{name} comptime/AOT mismatch");
    fs::remove_file(rs).ok();
    fs::remove_file(bin).ok();
    fs::remove_dir(dir).ok();
}
