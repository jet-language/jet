//! Static contracts for the direct C/C++ driver integrations.
//!
//! The executable Make/CMake matrix is intentionally not run by this focused
//! test. The proof commands in the reference document run the real aliases
//! against a provisioned signed toolchain; this test keeps the fixture contract
//! visible even on machines without Make, CMake, or the Hangar bundle.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn make_driver_fixture_requires_explicit_tool_aliases_and_scopes_paths() {
    let root = repo_root().join("tests/fixtures/foreign_build_hosts/make-cc");
    let makefile = read(&root.join("Makefile"));
    for declaration in [
        "ifeq ($(origin CC),command line)",
        "ifeq ($(origin CXX),command line)",
        "--project-root=\"$(PROJECT_ROOT)\"",
        "--build-root=\"$(BUILD_ROOT)\"",
        "@flags.rsp",
        "-MMD",
        "-MP",
        "-MF \"$(C_DEPFILE)\"",
        "-MT \"$(C_OBJECT)\"",
        "-MF \"$(CXX_DEPFILE)\"",
        "-MT \"$(CXX_OBJECT)\"",
    ] {
        assert!(makefile.contains(declaration), "Make fixture omits {declaration}");
    }
    for fallback in ["CC ?=", "CXX ?=", "CC := cc", "CXX := c++"] {
        assert!(!makefile.contains(fallback), "Make fixture retains {fallback}");
    }
    for file in ["main.c", "main.cpp", "flags.rsp", "include/config.h"] {
        assert!(root.join(file).is_file(), "Make fixture omits {file}");
    }
}

#[test]
fn cmake_driver_fixture_builds_both_languages_through_standard_compiler_slots() {
    let root = repo_root().join("tests/fixtures/foreign_build_hosts/cmake-cc");
    let cmake = read(&root.join("CMakeLists.txt"));
    for declaration in [
        "project(jet_cc_cmake_fixture LANGUAGES C CXX)",
        "add_executable(cc-driver main.c)",
        "add_executable(cxx-driver main.cpp)",
        "target_include_directories",
        "@${CMAKE_CURRENT_SOURCE_DIR}/flags.rsp",
        "C_STANDARD 11",
        "CXX_STANDARD 17",
    ] {
        assert!(cmake.contains(declaration), "CMake fixture omits {declaration}");
    }
    assert!(!cmake.contains("find_program"));
    assert!(!cmake.contains("/usr/bin/cc"));
    for file in ["main.c", "main.cpp", "flags.rsp", "include/config.h"] {
        assert!(root.join(file).is_file(), "CMake fixture omits {file}");
    }
}

#[test]
fn aliases_are_real_installed_bins_and_docs_cover_the_driver_matrix() {
    let cargo = read(&repo_root().join("Cargo.toml"));
    for bin in ["name = \"jet-cc\"", "name = \"jet-cxx\""] {
        assert!(cargo.contains(bin), "Cargo does not build {bin}");
    }
    let main = read(&repo_root().join("Source/main.rs"));
    assert!(main.contains("\"jet-cc\" => Some(\"cc\")"));
    assert!(main.contains("\"jet-cxx\" | \"jet-c++\" => Some(\"c++\")"));

    let release = read(&repo_root().join(".github/workflows/release.yml"));
    assert!(release.contains("--bin jet-cc --bin jet-cxx"));
    assert!(release.contains("jet jetpack jet-cc jet-c++"));
    assert!(release.contains("jet.exe jetpack.exe jet-cc.exe jet-c++.exe"));

    let docs = read(&repo_root().join("docs/reference/cc-driver.md"));
    for term in [
        "signed Nix index",
        "offline",
        "Make",
        "CMake",
        "no PATH fallback",
        "cross-target",
        "clean",
        "no-op",
        "edit",
        "error",
    ] {
        assert!(docs.to_ascii_lowercase().contains(&term.to_ascii_lowercase()), "driver docs omit {term}");
    }
}
