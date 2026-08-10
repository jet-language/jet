//! D-SHAPE-PLACE1=A: Tensor bracket slices use the same checked window law on
//! owned copies, AOT views, and mutable write-through places.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc};

#[test]
fn tensor_slice_and_mutable_view_run_through_aot() {
    if !have_rustc() {
        return;
    }
    let src = r#"
use core.compute as compute
fn run() {
    tensor := compute.from_list([1.0, 2.0, 3.0, 4.0]) ?? panic("tensor")
        copied :: ~tensor[1..2]
    print(compute.to_list(copied))
    edit :: &tensor[1..2]
    edit[0] = 9.0
    print(compute.to_list(tensor))
}
"#;
    let (code, stdout) = build_and_run("compute_tensor_views", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "[2.0, 3.0]\n[1.0, 9.0, 3.0, 4.0]\n");
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
    assert!(out.rust.contains("jet_compute_slice"), "{}", out.rust);
    assert!(out.rust.contains("struct JetTensor"), "{}", out.rust);
}

#[test]
fn tensor_stored_range_uses_the_same_owned_and_mutable_window_law() {
    if !have_rustc() {
        return;
    }
    let src = r#"
use core.compute as compute
fn run() {
    tensor := compute.from_list([1.0, 2.0, 3.0, 4.0]) ?? panic("tensor")
    window :: 1..2
        copied :: ~tensor[window]
    print(compute.to_list(copied))
    edit :: &tensor[window]
    edit[1] = 8.0
    print(compute.to_list(tensor))
}
"#;
    let (code, stdout) = build_and_run("compute_tensor_stored_range", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "[2.0, 3.0]\n[1.0, 2.0, 8.0, 4.0]\n");
}
