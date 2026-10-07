//! #4707: exclusive subranges retain the same owner across bindings and calls.
mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

const CASES: &[(&str, &str, &str)] = &[
    ("returned_write_window", r#"
fn fill(window: ViewMut<U8>, value: Int) {
    loop i in 0..<window.len() -> window[i] = U8{value}
}
fn tail_mut(window: ViewMut<U8>, start: Int) -> ViewMut<U8> from window {
    &window[start..<window.len()]
}
fn run() {
    data := [U8{0}, U8{0}, U8{0}, U8{0}, U8{0}]
    edit :: &data[0..<5]
    rest :: tail_mut(edit, 3)
    fill(rest, 9)
    loop byte in data -> print(Int.from_u8(byte))
}
"#, "0\n0\n0\n9\n9\n"),
    ("nested_returned_write_window", r#"
fn tail_mut(window: ViewMut<U8>, start: Int) -> ViewMut<U8> from window {
    &window[start..<window.len()]
}
fn fill(window: ViewMut<U8>) {
    loop i in 0..<window.len() -> window[i] = U8{6}
}
fn run() {
    data := [U8{0}, U8{0}, U8{0}, U8{0}, U8{0}]
    start :: 1
    end :: data.len()
    edit :: &data[start..<end]
    rest :: tail_mut(edit, 1)
    inner :: tail_mut(rest, 1)
    fill(inner)
    loop byte in data -> print(Int.from_u8(byte))
}
"#, "0\n0\n0\n6\n6\n"),
    ("view_subranges_golden", include_str!("../Examples/features/memory/view_subranges.jet"),
        include_str!("../Examples/features/expected/memory/view_subranges.out")),
];

fn prove(run: impl Fn(&str, &str) -> (i32, String, String)) {
    let mut failures = Vec::new();
    for (name, source, expected) in CASES {
        let (code, stdout, stderr) = run(name, source);
        if code != 0 || stdout != *expected {
            failures.push(format!("{name}: exit={code}, stdout={stdout:?}, expected={expected:?}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn write_subranges_default() {
    prove(|name, source| tir_support::run_default_multi(name, "run.jet", &[("run.jet", source)]));
}

#[test]
fn write_subranges_interpreter() {
    prove(|name, source| tir_support::run_interpret_multi(name, "run.jet", &[("run.jet", source)]));
}

#[test]
fn write_subranges_aot() {
    prove(|name, source| {
        let (code, stdout) = tir_support::build_release_and_run_multi(name, "run.jet", &[("run.jet", source)]);
        (code, stdout, String::new())
    });
}
