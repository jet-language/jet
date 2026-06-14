//! Regression tests for the 2026-06 hardening audit: every case here either
//! ICE'd (rustc rejected generated code, invariant I2) or silently
//! type-checked wrong programs before the fixes. Each test pins the new
//! behavior: a clean front-end diagnostic, or generated Rust that compiles.

use std::fs;
use std::path::PathBuf;

fn expect_error(src: &str, code: &str) {
    let diags = jet::compile(src).expect_err("front end should reject this");
    assert!(
        diags.iter().any(|d| d.code == code),
        "expected {code}, got: {:?}",
        diags.iter().map(|d| (d.code, d.what.clone())).collect::<Vec<_>>()
    );
}

#[test]
fn deeply_nested_expression_gets_diagnostic() {
    let src = format!(
        "fn main() {{\n    print({}1{});\n}}\n",
        "(".repeat(600),
        ")".repeat(600)
    );
    expect_error(&src, "E0035");
}

#[test]
fn list_index_assign_requires_var() {
    expect_error(
        r#"
fn main() {
    val xs = [1, 2, 3];
    xs[0] = 9;
}
"#,
        "E0202",
    );
}

#[test]
fn instance_method_args_keep_read_convention() {
    let src = r#"
struct Greeter {
    prefix: String;

    fn greet(self, name: String) -> String {
        return "{self.prefix} {name}";
    }
}

fn main() {
    val g = Greeter { prefix: "hi" };
    val name = "bob";
    print(g.greet(name));
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("user_greet(&("),
        "read-convention method arg must be borrowed: {}",
        out.rust
    );
}

#[test]
fn field_read_clones_instead_of_moving() {
    let src = r#"
struct P {
    name: String;
}

fn main() {
    val p = P { name: "x" };
    val s = p.name;
    val t = p.name;
    print(s);
    print(t);
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("user_name).clone()"),
        "field reads in owning position must clone: {}",
        out.rust
    );
}

#[test]
fn or_fallback_keeps_sema_rewrites() {
    let src = r#"
fn maybe() -> (Int?) {
    return null;
}

fn main() {
    var m: Map<String, Int> = [:];
    m["k"] = 7;
    val x = maybe() or m["k"];
    print(x);
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("jet_index_map"),
        "map index inside `or` fallback must use the map helper: {}",
        out.rust
    );
}

#[test]
fn bare_question_return_uses_default_error() {
    let src = r#"
fn parse_count(raw: String) -> Int? {
    if raw == "" {
        return err("empty");
    }
    return ok(1);
}

fn main() {
    val n = parse_count("") or 0;
    print(n);
}
"#;
    let out = jet::compile(src).expect("default Error fallible return should compile");
    assert!(
        out.rust.contains("Result<i64, String>"),
        "default Error should lower to String: {}",
        out.rust
    );
}

#[test]
fn map_assign_through_field_uses_map_helper() {
    let src = r#"
struct S {
    scores: Map<String, Int>;
}

fn main() {
    var s = S { scores: [:] };
    s.scores["a"] = 1;
    print(s.scores["a"]);
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("jet_map_insert"),
        "indexed assign through a field must resolve to the map helper: {}",
        out.rust
    );
}

#[test]
fn struct_literal_field_knows_expected_type() {
    // `[:]` in a struct literal used to fail with a spurious E0501.
    let src = r#"
struct S {
    scores: Map<String, Int>;
}

fn main() {
    var s = S { scores: [:] };
    print(s.scores.len());
}
"#;
    jet::compile(src).expect("empty map literal in field position should typecheck");
}

#[test]
fn view_result_cannot_be_stored() {
    expect_error(
        r#"
fn pick(items: List<String>) -> view List<String> {
    return items;
}

fn main() {
    val xs = ["a", "b"];
    val ys = pick(xs);
    print(ys.len());
}
"#,
        "E0206",
    );
}

#[test]
fn mut_self_method_requires_var_receiver() {
    expect_error(
        r#"
struct Bag {
    n: Int;

    fn poke(mut self) {
        val x = self.n;
        print(x);
    }
}

fn main() {
    val b = Bag { n: 1 };
    b.poke();
}
"#,
        "E0202",
    );
}

#[test]
fn take_self_on_borrowed_param_is_error() {
    expect_error(
        r#"
struct Token {
    s: String;

    fn consume(take self) {
        print(0);
    }
}

fn use_it(t: Token) {
    t.consume();
}

fn main() {
    print(0);
}
"#,
        "E0120",
    );
}

#[test]
fn if_pattern_binding_moves_subject() {
    expect_error(
        r#"
fn main() {
    val o: String? = value("hi");
    if o == value(n) {
        print(n);
    }
    print(o or "none");
}
"#,
        "E0121",
    );
}

#[test]
fn switch_pattern_binding_moves_subject() {
    expect_error(
        r#"
enum Shape {
    Circle(String)
    Empty
}

fn main() {
    val s = Shape.Circle("big");
    switch s {
        s == Circle(label) -> {
            print(label);
        };
        s == Empty -> {
            print("empty");
        };
    }
    switch s {
        s == Circle(label2) -> {
            print(label2);
        };
        s == Empty -> {
            print("still empty");
        };
    }
}
"#,
        "E0121",
    );
}

#[test]
fn statement_can_start_with_self() {
    let src = r#"
struct Bag {
    items: List<Int>;

    fn add(mut self, n: Int) {
        self.items.push(n);
    }
}

fn main() {
    var b = Bag { items: [0] };
    b.add(5);
    print(b.items.len());
}
"#;
    jet::compile(src).expect("`self.items.push(n);` should be a valid statement");
}

#[test]
fn builtin_mutator_on_read_self_is_error() {
    expect_error(
        r#"
struct Bag {
    items: List<Int>;

    fn add(self, n: Int) {
        self.items.push(n);
    }
}

fn main() {
    print(0);
}
"#,
        "E0202",
    );
}

#[test]
fn user_struct_named_noclone_is_still_cloneable() {
    // A user type literally named `NoClone` must not hit a hidden magic name.
    let src = r#"
struct NoClone {
    n: Int;
}

fn eat(take v: NoClone) {
    print(v.n);
}

fn main() {
    val v = NoClone { n: 1 };
    eat(v);
}
"#;
    let out = jet::compile(src).expect("an Int-only struct is cloneable whatever its name");
    assert!(out.lints.iter().any(|d| d.code == "L0201"));
}

#[test]
fn int_where_struct_expected_is_error() {
    // This mismatch was silently accepted (and ICE'd in codegen).
    expect_error(
        r#"
struct P {
    n: Int;
}

fn show(p: P) {
    print(p.n);
}

fn main() {
    show(7);
}
"#,
        "E0112",
    );
}

#[test]
fn mut_arg_must_be_plain_name() {
    expect_error(
        r#"
struct S {
    n: Int;
}

fn bump(mut n: Int) {
    n = n + 1;
}

fn main() {
    val s = S { n: 1 };
    bump(mut s.n);
}
"#,
        "E0202",
    );
}

// --- multi-file (loader + bundle) -------------------------------------

fn temp_project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jet_hardening_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile_bundle(entry: &PathBuf) -> Result<String, Vec<jet::diag::Diagnostic>> {
    let src = fs::read_to_string(entry).unwrap();
    jet::compile_with_path(&src, entry.to_str().unwrap()).map(|o| o.rust)
}

#[test]
fn hyphenated_file_name_gets_sane_module_alias() {
    let dir = temp_project("hyphen");
    fs::write(dir.join("my-utils.jet"), "pub fn helper() -> Int {\n    return 42;\n}\n").unwrap();
    fs::write(
        dir.join("main.jet"),
        "import \"my-utils\" as util;\nfn main() {\n    print(util.helper());\n}\n",
    )
    .unwrap();
    let rust = compile_bundle(&dir.join("main.jet")).expect("should compile");
    assert!(
        rust.contains("mod user_my_utils"),
        "module alias must be a valid Rust identifier: {rust}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn imported_struct_constructs_and_reads_fields() {
    let dir = temp_project("structs");
    fs::write(
        dir.join("shapes.jet"),
        "pub struct Point {\n    pub x: Int;\n    pub y: Int;\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.jet"),
        "import \"shapes\";\nfn main() {\n    val p = shapes.Point { x: 1, y: 2 };\n    print(p.x);\n}\n",
    )
    .unwrap();
    let rust = compile_bundle(&dir.join("main.jet")).expect("should compile");
    assert!(
        rust.contains("user_shapes::user_Point { user_x:"),
        "cross-module struct literal must match the declaration: {rust}"
    );
    assert!(
        rust.contains("pub struct user_Point"),
        "module items must be reachable from the entry: {rust}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn duplicate_file_stems_get_unique_module_names() {
    let dir = temp_project("dup");
    fs::create_dir_all(dir.join("a")).unwrap();
    fs::create_dir_all(dir.join("b")).unwrap();
    fs::write(dir.join("a/util.jet"), "pub fn one() -> Int {\n    return 1;\n}\n").unwrap();
    fs::write(dir.join("b/util.jet"), "pub fn two() -> Int {\n    return 2;\n}\n").unwrap();
    fs::write(
        dir.join("main.jet"),
        "import \"a/util\" as autil;\nimport \"b/util\" as butil;\nfn main() {\n    print(autil.one());\n    print(butil.two());\n}\n",
    )
    .unwrap();
    let rust = compile_bundle(&dir.join("main.jet")).expect("should compile");
    assert!(
        rust.contains("mod user_util") && rust.contains("mod user_util_2"),
        "same-stem modules must get unique mod names: {rust}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn value_is_identifier_not_keyword() {
    let src = r#"
fn show(label: String, value: Int) {
    print("{label}: {value}");
}

fn main() {
    show("score", 42);
}
"#;
    jet::compile(src).expect("`value` may name a binding; only `val` is reserved");
}
