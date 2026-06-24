//! check / build / run / test / new / fmt / fix subcommand handlers + the
//! rustc bridge.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use jet::ExitCodes;

use crate::{report_problems, usage, BuildProfile, OutputMode};

pub(crate) fn run_compile_cmd(
    cmd: &str,
    file: &str,
    emit_rust: bool,
    small: bool,
    freestanding: bool,
    cross_target: Option<&str>,
    verbose: bool,
    capabilities_json: bool,
    program_args: &[&String],
    mode: OutputMode,
) {
    let profile = if freestanding {
        BuildProfile::Freestanding
    } else if small {
        BuildProfile::Small
    } else {
        BuildProfile::Default
    };

    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(
                " fix: check the spelling, or run {} from the folder that contains it",
                jet::Syntax::BINARY_NAME
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    if cmd == "check" {
        let diags: Vec<_> = jet::check_with_path(file)
            .into_iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .collect();
        if !diags.is_empty() {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
        if mode.json {
            println!("{}", jet::render_all_json(file, &src, &[]).trim_end());
        } else {
            println!("ok: `{}` has no problems", file);
        }
        return;
    }

    // E2-M15: validate cross-compilation target before invoking rustc.
    if let Some(triple) = cross_target {
        validate_target(triple, mode);
    }

    let compile_result = if freestanding {
        jet::compile_freestanding(file)
    } else {
        jet::compile_with_path(&src, file)
    };
    let (rust_code, ffi_link, clinks, capabilities) = match compile_result {
        Ok(out) => {
            if !out.lints.is_empty() {
                if mode.json {
                    eprint!("{}", jet::render_all_json(file, &src, &out.lints));
                } else {
                    eprint!(
                        "{}",
                        jet::render_all_colored(file, &src, &out.lints, mode.color_stderr())
                    );
                    let n = out.lints.len();
                    eprintln!(
                        "\n{} warning{} emitted (compilation continues)",
                        n,
                        if n == 1 { "" } else { "s" }
                    );
                }
            }
            // S59 (E2-M14): resolve native C link flags at build time; E3201
            // (unresolved C lib) surfaces here, not during front-end checking.
            let clinks = match jet::resolve_c_links(file) {
                Ok(args) => args,
                Err(diags) => {
                    report_problems(mode, file, &src, &diags);
                    exit(ExitCodes::USER_ERROR);
                }
            };
            (out.rust, out.ffi, clinks, out.capabilities)
        }
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };

    if emit_rust {
        print!("{}", rust_code);
    }

    match cmd {
        "build" => {
            build(file, &rust_code, bin_path(file), profile, ffi_link.as_ref(), &clinks, verbose, cross_target, mode);
            println!("built: {}", bin_path(file).display());
            if let Some(triple) = cross_target {
                println!("target: {}", triple);
            }
            // D-TOOL5 (E2-M11): print capability summary after a successful build.
            if capabilities_json {
                println!("{}", capabilities.to_json());
            } else {
                println!("{}", capabilities.summary());
            }
        }
        "run" => {
            let out = bin_path(file);
            build(file, &rust_code, out.clone(), profile, ffi_link.as_ref(), &clinks, verbose, cross_target, mode);
            if cross_target.is_some() {
                eprintln!("note: cross-compiled binary cannot run on this host — use emulation (see docs/embedded.md)");
                exit(ExitCodes::OK);
            }
            let mut run_cmd = Command::new(&out);
            for arg in program_args {
                run_cmd.arg(arg.as_str());
            }
            let status = run_cmd.status().unwrap_or_else(|e| {
                eprintln!("error: couldn't run the built program: {}", e);
                exit(ExitCodes::USER_ERROR);
            });
            exit(status.code().unwrap_or(ExitCodes::OK));
        }
        other => {
            eprintln!(
                "error: `{}` isn't a {} command",
                other,
                jet::Syntax::BINARY_NAME
            );
            eprint!("{}", usage());
            exit(ExitCodes::USAGE);
        }
    }
}

/// Apply all auto-fixable diagnostics in a source file in place (D-LSP7 / M13).
/// Goes through `jet::LSP::collect_fixes` / `apply_all` — the SAME unified fix
/// engine the LSP code-action layer uses — so a fix on the command line and a
/// fix in the editor are byte-identical. `--dry-run` shows the diff without
/// writing.
pub(crate) fn run_fix(file: &str, dry_run: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(" fix: check the spelling");
            exit(ExitCodes::USER_ERROR);
        }
    };
    let fixes = jet::LSP::collect_fixes(file, &src);
    if fixes.is_empty() {
        println!("{}: no auto-fixable problems found", file);
        return;
    }
    let fixed = jet::LSP::apply_all(&src, &fixes);
    if fixed == src {
        println!("{}: no changes made", file);
        return;
    }
    let n = fixes.len();
    if dry_run {
        print!("{}", jet::Formatter::unified_diff(file, &src, &fixed));
        println!(
            "{}: would apply {} fix{} (dry run; nothing written)",
            file,
            n,
            if n == 1 { "" } else { "es" }
        );
        return;
    }
    fs::write(file, &fixed).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });
    println!("{}: applied {} fix{}", file, n, if n == 1 { "" } else { "es" });
}

pub(crate) fn run_new(name: &str, annotated: bool) {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        eprintln!("error: project name must be a simple folder name");
        eprintln!(" fix: try: {} new my_app", jet::Syntax::BINARY_NAME);
        exit(ExitCodes::USER_ERROR);
    }
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("error: `{}` already exists", name);
        exit(ExitCodes::USER_ERROR);
    }
    // Create: <name>/pkg.jet, <name>/.jet/main.jet, <name>/.gitignore
    let jet_dir = dir.join(".jet");
    fs::create_dir_all(&jet_dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`/.jet: {}", name, e);
        exit(ExitCodes::USER_ERROR);
    });
    let manifest_text = jet::Manifest::new_template(name, annotated);
    fs::write(dir.join(jet::Syntax::PAYLOAD_FILE), manifest_text).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", jet::Syntax::PAYLOAD_FILE, e);
        exit(ExitCodes::USER_ERROR);
    });
    let main_src = "fn main() {\n    print(\"hello, world\");\n}\n";
    fs::write(jet_dir.join("main.jet"), main_src).unwrap_or_else(|e| {
        eprintln!("error: couldn't write .jet/main.jet: {}", e);
        exit(ExitCodes::USER_ERROR);
    });
    fs::write(dir.join(".gitignore"), "build/\n.jet-build/\n.jet/lock\n.jet/cache/\n").unwrap_or_else(|e| {
        eprintln!("error: couldn't write .gitignore: {}", e);
        exit(ExitCodes::USER_ERROR);
    });
    println!("created {}/", name);
    println!("  {}", jet::Syntax::PAYLOAD_FILE);
    println!("  .jet/main.jet");
    println!("  .gitignore");
    println!("next: cd {} && {} run", name, jet::Syntax::BINARY_NAME);
}

pub(crate) fn run_test(path: &str, _update_snapshots: bool, mode: OutputMode) {
    let p = Path::new(path);
    if !p.exists() {
        eprintln!("error: can't find `{}`", path);
        exit(ExitCodes::USER_ERROR);
    }
    if p.is_dir() {
        let ext = jet::Syntax::FILE_EXT;
        let mut files: Vec<PathBuf> = fs::read_dir(p)
            .unwrap_or_else(|e| {
                eprintln!("error: couldn't read `{}`: {}", path, e);
                exit(ExitCodes::USER_ERROR);
            })
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| f.extension().and_then(|e| e.to_str()) == Some(ext))
            .collect();
        files.sort();
        if files.is_empty() {
            eprintln!("error: no .{} files in `{}`", ext, path);
            exit(ExitCodes::USER_ERROR);
        }
        let mut any_fail = false;
        for f in files {
            if !run_test_file(&f, mode) {
                any_fail = true;
            }
        }
        exit(if any_fail {
            ExitCodes::USER_ERROR
        } else {
            ExitCodes::OK
        });
    }
    exit(if run_test_file(p, mode) {
        ExitCodes::OK
    } else {
        ExitCodes::USER_ERROR
    });
}

fn run_test_file(path: &Path, mode: OutputMode) -> bool {
    let shown = path.to_string_lossy();
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read `{}`: {}", shown, e);
            return false;
        }
    };
    let (rust_code, ffi_link) = match jet::compile_tests_with_path(&src, &shown) {
        Ok(r) => r,
        Err(diags) => {
            report_problems(mode, &shown, &src, &diags);
            return false;
        }
    };
    let bin = test_bin_path(path);
    build(
        &shown,
        &rust_code,
        bin.clone(),
        BuildProfile::Default,
        ffi_link.as_ref(),
        &[],
        false,
        None,
        mode,
    );
    let out = Command::new(&bin).output().unwrap_or_else(|e| {
        eprintln!("error: couldn't run tests in `{}`: {}", shown, e);
        exit(ExitCodes::USER_ERROR);
    });
    print!("{}", String::from_utf8_lossy(&out.stdout));
    if !out.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }
    out.status.success()
}

pub(crate) fn run_fmt(file: &str, check_only: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let formatted = match jet::format_source(&src) {
        Ok(s) => s,
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            exit(ExitCodes::USER_ERROR);
        }
    };
    if formatted == src {
        return;
    }
    if check_only {
        print!("{}", jet::Formatter::unified_diff(file, &src, &formatted));
        exit(ExitCodes::USER_ERROR);
    }
    fs::write(file, &formatted).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });
}

pub(crate) fn stem(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string())
        .replace('.', "_")
}

fn bin_path(file: &str) -> PathBuf {
    PathBuf::from("build").join(stem(file))
}

fn test_bin_path(path: &Path) -> PathBuf {
    PathBuf::from("build").join(format!("test_{}", stem(&path.to_string_lossy())))
}

/// E2-M15 / E3302: check that rustc knows the requested cross-compilation target.
/// Runs `rustc --print target-list` and exits with E3302 if the triple is absent.
fn validate_target(triple: &str, mode: OutputMode) {
    // rustc --print target-list gives the full list; if the output contains
    // the triple exactly (one per line), the target is known.
    let out = Command::new("rustc").arg("--print").arg("target-list").output();
    let known = match out {
        Ok(o) if o.status.success() => {
            let list = String::from_utf8_lossy(&o.stdout);
            list.lines().any(|l| l.trim() == triple)
        }
        _ => false, // rustc not found or failed; will fail later during compile
    };
    if !known {
        let diag = jet::Sema::e3302(triple);
        let src = format!("// cross-build for {}", triple);
        report_problems(mode, "<target>", &src, &[diag]);
        exit(ExitCodes::USER_ERROR);
    }
    // Check that the std library is installed for this target.
    // `rustc --print sysroot` + check for lib/<triple>/ directory.
    let sysroot = Command::new("rustc").arg("--print").arg("sysroot").output();
    if let Ok(o) = sysroot {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        let target_lib = PathBuf::from(&root).join("lib").join("rustlib").join(triple);
        if !target_lib.exists() {
            let diag = jet::Sema::e3302(triple);
            let src = format!("// cross-build for {}", triple);
            report_problems(mode, "<target>", &src, &[diag]);
            eprintln!(
                " why: `rustup target add {}` to install the standard library for this target",
                triple
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

pub(crate) fn build(
    file: &str,
    rust_code: &str,
    bin: PathBuf,
    profile: BuildProfile,
    ffi: Option<&jet::FFI::FfiLink>,
    clinks: &[String],
    verbose: bool,
    cross_target: Option<&str>,
    mode: OutputMode,
) {
    // D-BUILD2: `jet build -v` makes the hidden Jet→Rust→native bridge honest.
    // Step labels are deterministic so they can be golden-tested.
    let step = |msg: String| {
        if verbose {
            eprintln!("[build] {}", msg);
        }
    };

    fs::create_dir_all("build").unwrap_or_else(|e| {
        eprintln!("error: couldn't create the build/ folder: {}", e);
        exit(ExitCodes::USER_ERROR);
    });
    let rs_path = PathBuf::from("build").join(format!("{}.rs", stem(file)));
    step(format!("emit Rust  -> {}", rs_path.display()));
    fs::write(&rs_path, rust_code).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", rs_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    let small = matches!(profile, BuildProfile::Small | BuildProfile::Freestanding);
    // Cross-compiled or freestanding builds bypass the host binary cache
    // (the binary is not executable on this host, and the target triple
    // affects codegen choices that aren't captured by the source hash).
    let use_cache = ffi.is_none() && clinks.is_empty() && cross_target.is_none();
    let cache_key = if use_cache {
        Some(jet::BuildCache::cache_key(rust_code, small))
    } else {
        None
    };
    if let Some(ref key) = cache_key {
        if jet::BuildCache::try_copy_cached(key, &bin) {
            step("cache hit -> reused cached binary".to_string());
            // c121: still report size on a cache hit so the dashboard always
            // has a binary-size data point.
            if jet::PhaseTiming::enabled() {
                if let Ok(meta) = std::fs::metadata(&bin) {
                    eprintln!("jet-timing binary_bytes={}", meta.len());
                }
            }
            return;
        }
    }
    if verbose {
        if cache_key.is_some() {
            step("cache miss -> compiling".to_string());
        } else if cross_target.is_some() {
            step("cache bypassed (cross-compiled build)".to_string());
        } else {
            step("cache bypassed (C-linked build)".to_string());
        }
    }

    step(format!("rustc      {} -> {}", rs_path.display(), bin.display()));
    let mut cmd = Command::new("rustc");
    cmd.arg("--edition").arg("2021");
    // E2-M15: cross-compilation target triple.
    if let Some(triple) = cross_target {
        cmd.arg("--target").arg(triple);
    }
    match profile {
        BuildProfile::Default => {
            cmd.arg("-O").arg("-C").arg("strip=symbols");
            // FFI rlibs come from a separate cargo build without LTO bitcode.
            if ffi.is_none() {
                cmd.arg("-C").arg("lto=thin");
            }
        }
        BuildProfile::Small => {
            cmd.arg("-C")
                .arg("opt-level=z")
                .arg("-C")
                .arg("panic=abort")
                .arg("-C")
                .arg("strip=symbols");
            if ffi.is_none() {
                cmd.arg("-C").arg("lto=fat");
            }
        }
        // E2-M15: freestanding — like --small but std-gated APIs already
        // rejected in sema; panic=abort matches D-CROSS2.
        BuildProfile::Freestanding => {
            cmd.arg("-C")
                .arg("opt-level=z")
                .arg("-C")
                .arg("panic=abort")
                .arg("-C")
                .arg("strip=symbols");
            if ffi.is_none() {
                cmd.arg("-C").arg("lto=fat");
            }
        }
    }
    cmd.arg(&rs_path).arg("-o").arg(&bin);
    if let Some(link) = ffi {
        cmd.arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        if link.deps_dir.is_dir() {
            cmd.arg("-L")
                .arg(format!("dependency={}", link.deps_dir.display()));
        }
    }
    // S59 (E2-M14): native C library link flags (`-L native=…`, `-l <name>`).
    for arg in clinks {
        cmd.arg(arg);
    }

    let out = match cmd.output() {
        Ok(o) => o,
        Err(_) => {
            eprintln!("error: couldn't find `rustc` on this machine");
            eprintln!(
                " why: v1 of this language uses Rust as its backend (docs/spec/architecture.md)"
            );
            eprintln!(" fix: install Rust from https://rustup.rs, then try again");
            exit(ExitCodes::USER_ERROR);
        }
    };

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // I2: a *missing C library* is a user/system problem, not generated-code
        // rejection. Detect a linker "cannot find -l<name>" and print a clean
        // E3209 diagnostic naming the lib + fix — NOT the bug-in-jet banner. The
        // ICE banner stays only for genuine codegen-rejected-by-rustc.
        if let Some(missing) = missing_c_lib(&stderr) {
            let diag = jet::CFFI::e3209(&missing);
            report_problems(mode, file, "", &[diag]);
            exit(ExitCodes::USER_ERROR);
        }
        eprintln!("internal compiler error: the generated Rust did not compile.");
        eprintln!(
            "This is a bug in {}, NOT in your program. Please report it,",
            jet::Syntax::BINARY_NAME
        );
        eprintln!("attaching your source file and the generated file below.");
        eprintln!("  generated: {}", rs_path.display());
        eprintln!("--- rustc said ---");
        eprintln!("{}", stderr);
        exit(ExitCodes::ICE);
    }

    step(format!("link       -> {}", bin.display()));

    // c121: report final binary size when timing is requested. The dashboard
    // tool captures this `binary_bytes=` line from build stderr.
    if jet::PhaseTiming::enabled() {
        if let Ok(meta) = std::fs::metadata(&bin) {
            eprintln!("jet-timing binary_bytes={}", meta.len());
        }
    }

    if let Some(key) = cache_key {
        jet::BuildCache::store_cached(&key, &bin);
        step("cache store -> saved binary for next time".to_string());
    }
}

/// Scan a failed rustc/linker stderr for a missing C library and return its
/// link name. Matches the GNU ld / lld phrasing `cannot find -l<name>` (and the
/// `-l<name>` form some linkers print). Used to keep a missing *system library*
/// off the I2 ICE path — it's a user/system problem, not generated-code being
/// rejected.
fn missing_c_lib(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        // e.g. "cannot find -lraylib" / "cannot find -lraylib: No such file"
        if let Some(rest) = line.split("cannot find -l").nth(1) {
            let name: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != ':' && *c != '\'' && *c != '"')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

#[cfg(test)]
mod missing_c_lib_tests {
    use super::missing_c_lib;

    #[test]
    fn detects_ld_cannot_find() {
        // GNU ld / lld phrasing — must be routed to E3209, not the I2 ICE banner.
        let stderr = "  = note: /usr/bin/ld: cannot find -lraylib: No such file or directory\n  collect2: error: ld returned 1 exit status\n";
        assert_eq!(missing_c_lib(stderr).as_deref(), Some("raylib"));
    }

    #[test]
    fn detects_bare_form() {
        assert_eq!(missing_c_lib("ld: cannot find -lsqlite3\n").as_deref(), Some("sqlite3"));
    }

    #[test]
    fn genuine_codegen_error_is_not_a_missing_lib() {
        // A real rustc type error must keep the ICE path (returns None here).
        let stderr = "error[E0308]: mismatched types\n --> build/main.rs:3:5\n";
        assert_eq!(missing_c_lib(stderr), None);
    }
}
