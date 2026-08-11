//! D-SHAPE-PLACE1=A: Tensor bracket slices use the same checked window law on
//! owned copies, AOT views, and mutable write-through places.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{
    build_and_run, build_and_run_full, have_rustc, run_default_multi,
    strip_vetted_prelude_modules,
};

#[test]
fn tensor_mutable_window_write_uses_shared_prelude_policy() {
    let src = r#"
use core.compute as compute
fn run() {
    tensor := compute.from_list([1.0, 2.0, 3.0, 4.0]) ?? panic("tensor")
        copied :: ~tensor[1..2]
    edit :: &tensor[1..2]
    edit[0] = 9.0
    print(compute.to_list(copied))
    print(compute.to_list(tensor))
}
"#;
    assert!(
        have_rustc(),
        "compute view AOT coverage requires rustc; do not skip this proof"
    );
    let (code, stdout) = build_and_run("compute_tensor_views", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "[2.0, 3.0]\n[1.0, 9.0, 3.0, 4.0]\n");
    let (code, stdout, stderr) =
        run_default_multi("compute_tensor_views_tir", "main.jet", &[("main.jet", src)]);
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert!(
        stderr.contains("tier1 native"),
        "compute views must run in resident JIT:\n{stderr}"
    );
    assert!(
        !stderr.contains("tier0 interp"),
        "compute views unexpectedly fell back to the interpreter:\n{stderr}"
    );
    assert_eq!(stdout, "[2.0, 3.0]\n[1.0, 9.0, 3.0, 4.0]\n");
}

#[test]
fn tensor_explicit_copy_is_deep_but_implicit_clone_shares_storage() {
    let src = r#"
use core.compute as compute

struct Holder { value: Tensor }

fn run() {
    tensor := compute.from_list([1.0, 2.0]) ?? panic("tensor")
    copied :: ~tensor
    copied_edit :: &copied[0..1]
    copied_edit[0] = 9.0
    print(compute.to_list(tensor))
    print(compute.to_list(copied))

    holder := Holder.{ value: ~tensor }
    shared :: holder.value
    shared_edit :: &shared[0..1]
    shared_edit[0] = 8.0
}
"#;
    assert!(
        have_rustc(),
        "Tensor copy/clone AOT coverage requires rustc; do not skip this proof"
    );
    let (code, stdout, stderr) = build_and_run_full(
        "compute_tensor_copy_clone_aot",
        "explicit_copy_vs_implicit_clone_aot",
        src,
    );
    assert_ne!(code, 0, "an implicit Tensor clone must retain shared storage");
    assert_eq!(stdout, "[1.0, 2.0]\n[9.0, 2.0]\n");
    assert!(
        stderr.contains("Tensor mutable view requires exclusive backing storage"),
        "AOT used the wrong Tensor clone policy:\n{stderr}"
    );

    let (code, stdout, stderr) = run_default_multi(
        "compute_tensor_copy_clone_tir",
        "main.jet",
        &[("main.jet", src)],
    );
    assert_ne!(code, 0, "an implicit Tensor clone must retain shared storage");
    assert_eq!(stdout, "[1.0, 2.0]\n[9.0, 2.0]\n");
    assert!(
        stderr.contains("Tensor mutable view requires exclusive backing storage"),
        "resident JIT used the wrong Tensor clone policy:\n{stderr}"
    );
}

#[test]
fn tensor_slice_is_represented_by_the_compute_prelude() {
    let src = r#"
use core.compute as compute
fn run() {
    tensor :: compute.from_list([1.0, 2.0]) ?? panic("tensor")
        read :: ~tensor[0..1]
    print(compute.to_list(read))
}
"#;
    let out = jet::compile(src).expect("Tensor bracket slices must type-check");
    assert!(out.rust.contains("struct JetTensor"), "{}", out.rust);
    let rust = strip_vetted_prelude_modules(&out.rust);
    assert!(rust.contains("jet_compute_slice(&("), "{}", rust);
    assert!(rust.contains("jet_compute_copy(&("), "{}", rust);
}

#[test]
fn tensor_stored_range_uses_the_same_owned_and_mutable_window_law() {
    let src = r#"
use core.compute as compute
fn run() {
    tensor := compute.from_list([1.0, 2.0, 3.0, 4.0]) ?? panic("tensor")
    window :: 1..2
        copied :: ~tensor[window]
    edit :: &tensor[window]
    edit[1] = 8.0
    print(compute.to_list(copied))
    print(compute.to_list(tensor))
}
"#;
    assert!(
        have_rustc(),
        "stored compute view AOT coverage requires rustc; do not skip this proof"
    );
    let (code, stdout) = build_and_run("compute_tensor_stored_range", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "[2.0, 3.0]\n[1.0, 2.0, 8.0, 4.0]\n");
    let (code, stdout, stderr) = run_default_multi(
        "compute_tensor_stored_range_tir",
        "main.jet",
        &[("main.jet", src)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert!(
        stderr.contains("tier1 native"),
        "stored compute views must run in resident JIT:\n{stderr}"
    );
    assert!(
        !stderr.contains("tier0 interp"),
        "stored compute views unexpectedly fell back to the interpreter:\n{stderr}"
    );
    assert_eq!(stdout, "[2.0, 3.0]\n[1.0, 2.0, 8.0, 4.0]\n");
}

#[test]
fn tensor_bare_range_is_a_borrowed_read_view() {
    let src = r#"
use core.compute as compute
fn run() {
    tensor := compute.from_list([1.0, 2.0, 3.0, 4.0]) ?? panic("tensor")
    read :: tensor[1..2]
    print(read[0])
    print(read[1])
}
"#;
    assert!(
        have_rustc(),
        "borrowed compute view AOT coverage requires rustc; do not skip this proof"
    );
    let (code, stdout) = build_and_run("compute_tensor_borrowed_read_view", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2.0\n3.0\n");
    let (code, stdout, stderr) = run_default_multi(
        "compute_tensor_borrowed_read_view_tir",
        "main.jet",
        &[("main.jet", src)],
    );
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert!(
        stderr.contains("tier1 native"),
        "borrowed compute read view must run in resident JIT:\n{stderr}"
    );
    assert!(
        !stderr.contains("tier0 interp"),
        "borrowed compute read view unexpectedly fell back to the interpreter:\n{stderr}"
    );
    assert_eq!(stdout, "2.0\n3.0\n");
}

#[test]
fn tensor_empty_half_open_window_is_kept_but_element_read_and_write_are_rejected() {
    let cases = [
        (
            "empty_window_read",
            r#"
use core.compute as compute
fn run() {
    tensor := compute.from_list([1.0, 2.0]) ?? panic("tensor")
    empty :: &tensor[0..<0]
    print(empty.len())
    print(empty[0])
}
"#,
        ),
        (
            "empty_window_write",
            r#"
use core.compute as compute
fn run() {
    tensor := compute.from_list([1.0, 2.0]) ?? panic("tensor")
    empty :: &tensor[0..<0]
    print(empty.len())
    empty[0] = 9.0
}
"#,
        ),
    ];
    assert!(
        have_rustc(),
        "empty Tensor window AOT coverage requires rustc; do not skip this proof"
    );
    for (name, src) in cases {
        let (code, stdout, stderr) = build_and_run_full(
            "compute_tensor_empty_window_aot",
            name,
            src,
        );
        assert_ne!(code, 0, "an empty Tensor element operation must fail");
        assert_eq!(stdout, "0\n");
        assert!(
            stderr.contains("the list has 0 items, so position 0 doesn't exist"),
            "AOT used a non-canonical empty-window element error:\n{stderr}"
        );

        let (code, stdout, stderr) = run_default_multi(
            &format!("compute_tensor_empty_window_tir_{name}"),
            "main.jet",
            &[("main.jet", src)],
        );
        assert_ne!(code, 0, "an empty Tensor element operation must fail");
        assert_eq!(stdout, "0\n");
        assert!(
            stderr.contains("the list has 0 items, so position 0 doesn't exist"),
            "resident JIT used a non-canonical empty-window element error:\n{stderr}"
        );
    }
}
