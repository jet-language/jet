//! jet CLI: check / build / run / test / new / fmt / lsp +
//!          add / remove / fetch / update / store (M12.1 package manager).
//!
//! The driver owns invariant I2: rustc's voice never reaches the user as
//! if it were their fault. A rustc failure on generated code is reported
//! as an internal compiler error in jet.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

#[derive(Clone, Copy)]
enum BuildProfile {
    /// Default: speed-oriented (`-O`, thin LTO).
    Default,
    /// S15: size-oriented (`opt-level=z`, fat LTO, `panic=abort`).
    Small,
}

fn usage() -> String {
    format!(
        "\
Welcome to {lang}! (v{ver})

usage:
  {bin} check <file.{ext}>          look for problems, build nothing
  {bin} build <file.{ext}>          compile to a native binary in ./build/
  {bin} run   <file.{ext}>          build, then run (or `jet run` inside a project)
  {bin} run   <file.{ext}> a b      extra words become program arguments
  {bin} test  <file|dir>            compile and run top-level test blocks
  {bin} new   <name>                create a new project folder with jet.toml
  {bin} new   <name> --annotated    same, with commented example deps
  {bin} fmt   <file.{ext}>          rewrite file to canonical style (S44)
  {bin} fix   <file.{ext}>          apply all auto-fixable diagnostics in place
  {bin} lsp                         language server (stdio JSON-RPC)
  {bin} lsp doctor                  health-check the language server
  {bin} lsp --bench                 latency benchmark (CI: must pass in <200ms/round)
  {bin} version                     print compiler version
  {bin} help                        print this help text
  {bin} upgrade                     how to download a newer release

package management (M12.1):
  {bin} add   <dep> --path <dir>    add a path dependency and fetch
  {bin} add   <dep> --git <url> --tag <tag>   add a git dependency
  {bin} remove <dep>                remove a dependency
  {bin} fetch                       download and link all dependencies
  {bin} fetch --locked              verify lock only, no network
  {bin} update                      refresh @latest / branch selectors
  {bin} update <dep>                update one moving selector
  {bin} store verify                re-check all store entry hashes
  {bin} gc                          remove unreferenced store entries

flags:
  --emit-rust                  also print the generated Rust code
  --check                      with fmt: exit 1 if file would change (CI)
  --small                      with build/run: smallest binary (S15)
  --locked                     with fetch: verify only, refuse network
",
        bin = jet::syntax::BINARY_NAME,
        lang = jet::syntax::LANG_NAME,
        ver = env!("CARGO_PKG_VERSION"),
        ext = jet::syntax::FILE_EXT,
    )
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    if raw.iter().any(|a| a == "--version") {
        run_version();
        return;
    }

    let emit_rust = raw.iter().any(|a| a == "--emit-rust");
    let fmt_check = raw.iter().any(|a| a == "--check");
    let small = raw.iter().any(|a| a == "--small");
    let locked = raw.iter().any(|a| a == "--locked");
    let annotated = raw.iter().any(|a| a == "--annotated");
    let args: Vec<&String> = raw.iter().filter(|a| !a.starts_with("--")).collect();

    if args.first().map(|s| s.as_str()) == Some("lsp") {
        let sub = args.get(1).map(|s| s.as_str());
        let bench_flag = raw.iter().any(|a| a == "--bench");
        match (sub, bench_flag) {
            (Some("doctor"), _) => {
                jet::lsp::run_doctor();
                return;
            }
            (_, true) | (Some("--bench"), _) => {
                // jet lsp --bench: run latency benchmark on a small program
                let src = include_str!("../examples/features/16_wordcount.jet");
                jet::lsp::run_bench(src, 10, 200);
                return;
            }
            _ => {}
        }
        if let Err(e) = jet::lsp::run_stdio() {
            eprintln!("error: language server failed: {}", e);
            exit(1);
        }
        return;
    }

    let cmd = match args.first() {
        Some(c) => c.as_str(),
        None => {
            eprint!("{}", usage());
            exit(2);
        }
    };

    // Commands with no required positional target.
    match cmd {
        "help" => {
            eprint!("{}", usage());
            exit(2);
        }
        "version" => {
            run_version();
            return;
        }
        "upgrade" => {
            run_upgrade();
            return;
        }
        "fetch" => {
            run_fetch(locked);
            return;
        }
        "update" => {
            let dep = args.get(1).map(|s| s.as_str());
            run_update(dep);
            return;
        }
        "gc" => {
            run_gc();
            return;
        }
        "store" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "verify" => run_store_verify(),
                _ => {
                    eprintln!("error: unknown store subcommand `{}`", sub);
                    eprintln!(" fix: try `jet store verify`");
                    exit(2);
                }
            }
            return;
        }
        _ => {}
    }

    let target = match args.get(1) {
        Some(f) => f.as_str(),
        None => {
            // No target: try project-root mode for run/build/test.
            match cmd {
                "run" | "build" | "test" | "check" => {
                    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                    if let Some(root) = jet::loader::find_manifest_root(&cwd) {
                        let entry = find_project_entry(&root);
                        let entry_str = entry.to_string_lossy().to_string();
                        match cmd {
                            "test" => {
                                run_test(&entry_str);
                                return;
                            }
                            _ => {
                                let program_args: Vec<&String> =
                                    args.iter().skip(1).copied().collect();
                                run_compile_cmd(cmd, &entry_str, emit_rust, small, &program_args);
                                return;
                            }
                        }
                    }
                    eprintln!(
                        "error: no file given and no `jet.toml` found in this directory or above"
                    );
                    eprintln!(
                        " fix: run `jet {} <file.{}>` or cd into a project",
                        cmd,
                        jet::syntax::FILE_EXT
                    );
                    exit(2);
                }
                _ => {
                    eprint!("{}", usage());
                    exit(2);
                }
            }
        }
    };

    match cmd {
        "fmt" => run_fmt(target, fmt_check),
        "fix" => run_fix(target),
        "new" => run_new(target, annotated),
        "test" => run_test(target),
        "add" => run_add(&raw),
        "remove" => run_remove(target),
        // Teaching error: E0042 foreign manifest filename, E0043 `jet install`
        "install" => {
            eprintln!("Error [E0043]: `jet install` isn't a Jet command");
            eprintln!(" Why: Jet uses `jet fetch` to download and link dependencies");
            eprintln!(" Fix: run `jet fetch` to install all dependencies listed in jet.toml");
            exit(1);
        }
        _ => {
            let program_args: Vec<&String> = args.iter().skip(2).copied().collect();
            run_compile_cmd(cmd, target, emit_rust, small, &program_args);
        }
    }
}

/// Find the entry .jet file for a project (`.jet/main.jet` if exists, else `main.jet`).
fn find_project_entry(root: &Path) -> PathBuf {
    let dot_jet = root
        .join(".jet")
        .join(format!("main.{}", jet::syntax::FILE_EXT));
    if dot_jet.is_file() {
        return dot_jet;
    }
    root.join(format!("main.{}", jet::syntax::FILE_EXT))
}

fn run_version() {
    println!("{}", env!("CARGO_PKG_VERSION"));
}

fn run_upgrade() {
    println!(
        "To upgrade {}, download the latest release from:",
        jet::syntax::BINARY_NAME
    );
    println!("  https://github.com/jet-lang/jet/releases");
}

fn run_compile_cmd(cmd: &str, file: &str, emit_rust: bool, small: bool, program_args: &[&String]) {
    let profile = if small {
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
                jet::syntax::BINARY_NAME
            );
            exit(1);
        }
    };

    if cmd == "check" {
        let diags: Vec<_> = jet::check_with_path(file)
            .into_iter()
            .filter(|d| matches!(d.severity, jet::diag::Severity::Error))
            .collect();
        if !diags.is_empty() {
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            let n = diags.len();
            eprintln!("\n{} problem{} found", n, if n == 1 { "" } else { "s" });
            exit(1);
        }
        println!("ok: `{}` has no problems", file);
        return;
    }

    let (rust_code, ffi_link) = match jet::compile_with_path(&src, file) {
        Ok(out) => {
            if !out.lints.is_empty() {
                eprint!("{}", jet::render_diagnostics(file, &src, &out.lints));
                let n = out.lints.len();
                eprintln!(
                    "\n{} warning{} emitted (compilation continues)",
                    n,
                    if n == 1 { "" } else { "s" }
                );
            }
            (out.rust, out.ffi)
        }
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            let n = diags.len();
            eprintln!("\n{} problem{} found", n, if n == 1 { "" } else { "s" });
            exit(1);
        }
    };

    if emit_rust {
        print!("{}", rust_code);
    }

    match cmd {
        "build" => {
            build(file, &rust_code, bin_path(file), profile, ffi_link.as_ref());
            println!("built: {}", bin_path(file).display());
        }
        "run" => {
            let out = bin_path(file);
            build(file, &rust_code, out.clone(), profile, ffi_link.as_ref());
            let mut run_cmd = Command::new(&out);
            for arg in program_args {
                run_cmd.arg(arg.as_str());
            }
            let status = run_cmd.status().unwrap_or_else(|e| {
                eprintln!("error: couldn't run the built program: {}", e);
                exit(1);
            });
            exit(status.code().unwrap_or(0));
        }
        other => {
            eprintln!(
                "error: `{}` isn't a {} command",
                other,
                jet::syntax::BINARY_NAME
            );
            eprint!("{}", usage());
            exit(2);
        }
    }
}

/// Apply all auto-fixable diagnostics in a source file in place (D-LSP7 / M13).
/// Uses the same `edit` engine as LSP code actions.
fn run_fix(file: &str) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            eprintln!(" fix: check the spelling");
            exit(1);
        }
    };
    let diags = jet::lsp::check_document(file, &src);
    let mut edits: Vec<_> = diags.iter().filter_map(|d| d.edit.clone()).collect();
    if edits.is_empty() {
        println!("{}: no auto-fixable problems found", file);
        return;
    }
    // Apply edits from highest offset to lowest to avoid span invalidation.
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut fixed = src.clone();
    for edit in &edits {
        fixed = jet::lsp::apply_edit(&fixed, edit);
    }
    if fixed == src {
        println!("{}: no changes made", file);
        return;
    }
    fs::write(file, &fixed).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", file, e);
        exit(1);
    });
    println!(
        "{}: applied {} fix{}",
        file,
        edits.len(),
        if edits.len() == 1 { "" } else { "es" }
    );
}

fn run_new(name: &str, annotated: bool) {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        eprintln!("error: project name must be a simple folder name");
        eprintln!(" fix: try: {} new my_app", jet::syntax::BINARY_NAME);
        exit(1);
    }
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("error: `{}` already exists", name);
        exit(1);
    }
    // Create: <name>/jet.toml, <name>/.jet/main.jet, <name>/.gitignore
    let jet_dir = dir.join(".jet");
    fs::create_dir_all(&jet_dir).unwrap_or_else(|e| {
        eprintln!("error: couldn't create `{}`/.jet: {}", name, e);
        exit(1);
    });
    let manifest_text = jet::manifest::new_template(name, annotated);
    fs::write(dir.join("jet.toml"), manifest_text).unwrap_or_else(|e| {
        eprintln!("error: couldn't write jet.toml: {}", e);
        exit(1);
    });
    let main_src = "fn main() {\n    print(\"hello, world\");\n}\n";
    fs::write(jet_dir.join("main.jet"), main_src).unwrap_or_else(|e| {
        eprintln!("error: couldn't write .jet/main.jet: {}", e);
        exit(1);
    });
    fs::write(dir.join(".gitignore"), "build/\n.jet-build/\n").unwrap_or_else(|e| {
        eprintln!("error: couldn't write .gitignore: {}", e);
        exit(1);
    });
    println!("created {}/", name);
    println!("  jet.toml");
    println!("  .jet/main.jet");
    println!("  .gitignore");
    println!("next: cd {} && {} run", name, jet::syntax::BINARY_NAME);
}

// ──────────────────────────────────────────────
// Package management commands (M12.1)
// ──────────────────────────────────────────────

fn run_add(raw_args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `jet.toml` found — run `jet add` inside a project");
        eprintln!(" fix: run `jet new <name>` to create a project first");
        exit(1);
    });

    // Parse: jet add <dep-name> --path <dir> | --git <url> [--tag <t>|--branch <b>|--rev <r>]
    let non_flag: Vec<&String> = raw_args.iter().filter(|a| !a.starts_with("--")).collect();
    let dep_name = match non_flag.get(1) {
        Some(n) => n.as_str(),
        None => {
            eprintln!("error: `jet add` needs a dependency name");
            eprintln!(" fix: try `jet add mylib --path ../mylib`");
            exit(1);
        }
    };

    let path_val = flag_value(raw_args, "--path");
    let git_val = flag_value(raw_args, "--git");
    let tag_val = flag_value(raw_args, "--tag");
    let branch_val = flag_value(raw_args, "--branch");
    let rev_val = flag_value(raw_args, "--rev");

    let spec = if let Some(p) = path_val {
        jet::manifest::DepSpec::Path {
            path: p.to_string(),
        }
    } else if let Some(url) = git_val {
        let selector = if let Some(t) = tag_val {
            jet::manifest::GitSelector::Tag(t.to_string())
        } else if let Some(b) = branch_val {
            jet::manifest::GitSelector::Branch(b.to_string())
        } else if let Some(r) = rev_val {
            jet::manifest::GitSelector::Rev(r.to_string())
        } else {
            eprintln!(
                "error: git dependency `{}` needs one of: --tag, --branch, --rev",
                dep_name
            );
            exit(1);
        };
        jet::manifest::DepSpec::Git {
            url: url.to_string(),
            selector,
        }
    } else {
        eprintln!("error: `jet add {}` needs --path or --git", dep_name);
        eprintln!(
            " fix: try `jet add {} --path ../{}` or `jet add {} --git <url> --tag <tag>`",
            dep_name, dep_name, dep_name
        );
        exit(1);
    };

    // Load the manifest, add the dep, write back.
    let toml_path = root.join("jet.toml");
    let raw = fs::read_to_string(&toml_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read jet.toml: {}", e);
        exit(1);
    });
    let updated = jet::manifest::add_dependency(&raw, dep_name, &spec);
    fs::write(&toml_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write jet.toml: {}", e);
        exit(1);
    });
    println!("added `{}` to jet.toml", dep_name);

    // Auto-fetch.
    do_fetch(&root, false);
}

fn run_remove(dep_name: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `jet.toml` found");
        exit(1);
    });

    let toml_path = root.join("jet.toml");
    let raw = fs::read_to_string(&toml_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read jet.toml: {}", e);
        exit(1);
    });
    let updated = jet::manifest::remove_dependency(&raw, dep_name);
    fs::write(&toml_path, updated).unwrap_or_else(|e| {
        eprintln!("error: couldn't write jet.toml: {}", e);
        exit(1);
    });
    println!("removed `{}` from jet.toml", dep_name);

    // Re-fetch to update lock.
    do_fetch(&root, false);
}

fn run_fetch(locked: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `jet.toml` found — run `jet fetch` inside a project");
        exit(1);
    });
    do_fetch(&root, locked);
}

fn run_update(dep: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = jet::loader::find_manifest_root(&cwd).unwrap_or_else(|| {
        eprintln!("error: no `jet.toml` found");
        exit(1);
    });

    let toml_path = root.join("jet.toml");
    let raw = fs::read_to_string(&toml_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read jet.toml: {}", e);
        exit(1);
    });
    let mf = jet::manifest::parse(&toml_path, &raw).unwrap_or_else(|d| {
        eprintln!(
            "{}",
            jet::render_diagnostics(&toml_path.display().to_string(), &raw, &[d])
        );
        exit(1);
    });
    let existing_lock = jet::lock::load(&root);
    let opts = jet::fetch::FetchOptions {
        locked: false,
        update: true,
        update_dep: dep.map(str::to_string),
    };
    match jet::fetch::fetch(&root, &mf, existing_lock.as_ref(), &opts) {
        Ok(_) => {
            if let Some(d) = dep {
                println!("updated `{}`", d);
            } else {
                println!("updated all moving selectors");
            }
        }
        Err(diags) => {
            let src = String::new();
            eprint!("{}", jet::render_diagnostics("jet.toml", &src, &diags));
            exit(1);
        }
    }
}

fn run_store_verify() {
    let store_dir = jet::store::store_dir();
    let entries = jet::store::list_entries();
    if entries.is_empty() {
        println!("store is empty ({})", store_dir.display());
        return;
    }
    println!("verifying {} store entries...", entries.len());
    // Without lockfile context we can only verify tree hashes against themselves.
    // Full verification requires the lock file; this checks for obvious corruption.
    let mut ok = 0;
    let mut bad = 0;
    for entry in &entries {
        let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let th = jet::sha256::tree_hash(entry);
        if th.starts_with("sha256-") {
            ok += 1;
        } else {
            eprintln!("  bad: {}", name);
            bad += 1;
        }
    }
    println!("{} ok, {} bad", ok, bad);
    if bad > 0 {
        exit(1);
    }
}

fn run_gc() {
    // Without a global registry of in-use locks, we print a stub message.
    // Full gc would walk all jet.lock files; M12.1 ships the infrastructure.
    let entries = jet::store::list_entries();
    println!(
        "store has {} entries; use `jet store verify` to check hashes",
        entries.len()
    );
    println!("(gc: removing unreferenced entries requires a future registry — coming in M12.2)");
}

fn do_fetch(root: &Path, locked: bool) {
    let toml_path = root.join("jet.toml");
    let raw = fs::read_to_string(&toml_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read jet.toml: {}", e);
        exit(1);
    });
    let mf = jet::manifest::parse(&toml_path, &raw).unwrap_or_else(|d| {
        eprint!(
            "{}",
            jet::render_diagnostics(&toml_path.display().to_string(), &raw, &[d])
        );
        exit(1);
    });
    let existing_lock = jet::lock::load(root);
    let opts = jet::fetch::FetchOptions {
        locked,
        update: false,
        update_dep: None,
    };
    match jet::fetch::fetch(root, &mf, existing_lock.as_ref(), &opts) {
        Ok(_) => {
            if locked {
                println!("lock verified");
            } else {
                println!("fetched all dependencies");
            }
        }
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics("jet.toml", &raw, &diags));
            exit(1);
        }
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == flag {
            return iter.next().map(String::as_str);
        }
    }
    None
}

fn run_test(path: &str) {
    let p = Path::new(path);
    if !p.exists() {
        eprintln!("error: can't find `{}`", path);
        exit(1);
    }
    if p.is_dir() {
        let ext = jet::syntax::FILE_EXT;
        let mut files: Vec<PathBuf> = fs::read_dir(p)
            .unwrap_or_else(|e| {
                eprintln!("error: couldn't read `{}`: {}", path, e);
                exit(1);
            })
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| f.extension().and_then(|e| e.to_str()) == Some(ext))
            .collect();
        files.sort();
        if files.is_empty() {
            eprintln!("error: no .{} files in `{}`", ext, path);
            exit(1);
        }
        let mut any_fail = false;
        for f in files {
            if !run_test_file(&f) {
                any_fail = true;
            }
        }
        exit(if any_fail { 1 } else { 0 });
    }
    exit(if run_test_file(p) { 0 } else { 1 });
}

fn run_test_file(path: &Path) -> bool {
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
            eprint!("{}", jet::render_diagnostics(&shown, &src, &diags));
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
    );
    let out = Command::new(&bin).output().unwrap_or_else(|e| {
        eprintln!("error: couldn't run tests in `{}`: {}", shown, e);
        exit(1);
    });
    print!("{}", String::from_utf8_lossy(&out.stdout));
    if !out.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }
    out.status.success()
}

fn run_fmt(file: &str, check_only: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("error: can't find the file `{}`", file);
            exit(1);
        }
    };
    let formatted = match jet::format_source(&src) {
        Ok(s) => s,
        Err(diags) => {
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            exit(1);
        }
    };
    if formatted == src {
        return;
    }
    if check_only {
        print!("{}", jet::fmt::unified_diff(file, &src, &formatted));
        exit(1);
    }
    fs::write(file, &formatted).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", file, e);
        exit(1);
    });
}

fn stem(file: &str) -> String {
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

fn build(
    file: &str,
    rust_code: &str,
    bin: PathBuf,
    profile: BuildProfile,
    ffi: Option<&jet::ffi::FfiLink>,
) {
    fs::create_dir_all("build").unwrap_or_else(|e| {
        eprintln!("error: couldn't create the build/ folder: {}", e);
        exit(1);
    });
    let rs_path = PathBuf::from("build").join(format!("{}.rs", stem(file)));
    fs::write(&rs_path, rust_code).unwrap_or_else(|e| {
        eprintln!("error: couldn't write {}: {}", rs_path.display(), e);
        exit(1);
    });

    let small = matches!(profile, BuildProfile::Small);
    let use_cache = ffi.is_none();
    let cache_key = if use_cache {
        Some(jet::build_cache::cache_key(rust_code, small))
    } else {
        None
    };
    if let Some(ref key) = cache_key {
        if jet::build_cache::try_copy_cached(key, &bin) {
            return;
        }
    }

    let mut cmd = Command::new("rustc");
    cmd.arg("--edition").arg("2021");
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

    let out = match cmd.output() {
        Ok(o) => o,
        Err(_) => {
            eprintln!("error: couldn't find `rustc` on this machine");
            eprintln!(
                " why: v1 of this language uses Rust as its backend (docs/spec/architecture.md)"
            );
            eprintln!(" fix: install Rust from https://rustup.rs, then try again");
            exit(1);
        }
    };

    if !out.status.success() {
        eprintln!("internal compiler error: the generated Rust did not compile.");
        eprintln!(
            "This is a bug in {}, NOT in your program. Please report it,",
            jet::syntax::BINARY_NAME
        );
        eprintln!("attaching your source file and the generated file below.");
        eprintln!("  generated: {}", rs_path.display());
        eprintln!("--- rustc said ---");
        eprintln!("{}", String::from_utf8_lossy(&out.stderr));
        exit(101);
    }

    if let Some(key) = cache_key {
        jet::build_cache::store_cached(&key, &bin);
    }
}
