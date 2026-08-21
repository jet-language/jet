use std::fs;
use std::process::{Command, Output};

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

const SOURCE: &str = r#"fn run() {
    print(Vec2(1.0, 2.0))
    print(Vec3(1.0, 2.0, 3.0))
    print(Vec4(1.0, 2.0, 3.0, 4.0))
    print(Mat3(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0))
    print(Mat4(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0))
}
"#;

const EXPECTED: &str = "\
Vec2 { x: 1.0, y: 2.0 }\n\
Vec3 { x: 1.0, y: 2.0, z: 3.0 }\n\
Vec4 { x: 1.0, y: 2.0, z: 3.0, w: 4.0 }\n\
Mat3 { m00: 1.0, m10: 2.0, m20: 3.0, m01: 4.0, m11: 5.0, m21: 6.0, m02: 7.0, m12: 8.0, m22: 9.0 }\n\
Mat4 { m00: 1.0, m10: 2.0, m20: 3.0, m30: 4.0, m01: 5.0, m11: 6.0, m21: 7.0, m31: 8.0, m02: 9.0, m12: 10.0, m22: 11.0, m32: 12.0, m03: 13.0, m13: 14.0, m23: 15.0, m33: 16.0 }\n";

fn run_lens(file: &std::path::Path, release: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jet"));
    command.arg("run");
    if release {
        command.arg("--release");
    }
    command
        .arg(file)
        .env("NO_COLOR", "1")
        .output()
        .expect("run builtin math display fixture")
}

#[test]
fn builtin_math_family_has_named_field_parity_on_both_lenses() {
    let dir = common::unique_tmp("jet_math_display_parity");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("family.jet");
    fs::write(&file, SOURCE).unwrap();

    let run = run_lens(&file, false);
    let build = run_lens(&file, true);
    assert!(
        run.status.success(),
        "jet run failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        build.status.success(),
        "jet run --release failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert_eq!(
        run.stdout, build.stdout,
        "builtin math family diverged between run and build lenses"
    );
    assert_eq!(String::from_utf8(run.stdout).unwrap(), EXPECTED);

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn measured_values_propagate_first_order_uncertainty() {
    let source = r#"
use core.math as math

fn run() {
    gravity :: measurement(9.8, uncertainty: 0.1)
    time :: measurement(2.0, uncertainty: 0.05)
    squared_time :: time * time
    height :: measurement(0.5, uncertainty: 0.0) * gravity * squared_time
    print(height)
    print(math.sqrt(gravity))
}
"#;
    tir_support::assert_tiers_agree(
        "measurement_first_order_uncertainty",
        source,
        "19.6 ± 0.7212489168102787\n3.1304951684997055 ± 0.015971914124998498\n",
    );
}

#[test]
fn measured_unit_scale_produces_measured_conversion() {
    let source = r#"
fn run() {
    mass :: Kilogram.from_dalton_rounded(6.022140762e26dalton, .NearestEven, digits: 0) ?? panic("measured conversion")
    print(mass)
}
"#;
    tir_support::assert_tiers_agree(
        "measurement_unit_conversion",
        source,
        "1.0 ± 3.1315131919070317e-10\n",
    );
}

#[test]
fn exact_default_numbers_match_all_tiers() {
    let source = r#"
fn run() {
    third :: 1 / 3
    print(third)
    print(third * 3 == 1)
    print(0.1 + 0.2 == 0.3)
    fast :: Float{19.99}
    print(fast)
}
"#;
    tir_support::assert_tiers_agree("exact_default_numbers", source, "1/3\ntrue\ntrue\n19.99\n");
}
