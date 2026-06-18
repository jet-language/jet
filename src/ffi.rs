//! M7 hidden cargo FFI bridge (S50).
//!
//! When a program declares `extern rust` blocks with crate dependencies, the
//! driver materializes a cached cargo project under `~/.cache/jet/ffi/` and
//! links the built rlib into the user's generated program.

use crate::ast::{AccessConvention, ExternFn, ExternRustBlock, Item, ProgramBundle, Type};
use crate::diag::Diagnostic;
use std::collections::{hash_map::DefaultHasher, BTreeMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

/// Built FFI bridge artifact paths for rustc linking.
#[derive(Debug, Clone)]
pub struct FfiLink {
    pub crate_name: String,
    pub rlib_path: PathBuf,
    pub deps_dir: PathBuf,
}

/// One foreign function collected from the import graph.
#[derive(Debug, Clone)]
pub struct ExternEntry {
    pub jet_name: String,
    pub rust_path: String,
    pub wrapper_name: String,
    pub params: Vec<(AccessConvention, Type)>,
    pub return_type: Option<Type>,
    pub crate_spec: String,
    /// Human-facing hint for E0705 (`extern` line context).
    pub line_hint: String,
}

/// Gather every `extern rust` function across all modules.
pub fn collect_externs(bundle: &ProgramBundle) -> Vec<ExternEntry> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        for item in &module.items {
            let Item::ExternRust(block) = item else {
                continue;
            };
            for ef in &block.functions {
                out.push(extern_entry(ef, block, &module.display));
            }
        }
    }
    out
}

fn extern_entry(ef: &ExternFn, block: &ExternRustBlock, _file: &str) -> ExternEntry {
    ExternEntry {
        jet_name: ef.name.clone(),
        rust_path: ef.rust_path.clone(),
        wrapper_name: format!("jet_ffi_{}", ef.name),
        params: ef
            .params
            .iter()
            .map(|p| (p.convention, p.ty.clone()))
            .collect(),
        return_type: ef.return_type.clone(),
        crate_spec: block.crate_spec.clone(),
        line_hint: format!("`{}` in `extern rust \"{}\"`", ef.name, block.crate_spec),
    }
}

/// Build (or reuse) the hidden wrapper crate. Returns `Ok(None)` when the
/// program has no `extern rust` declarations.
pub fn prepare(bundle: &ProgramBundle) -> Result<Option<FfiLink>, Vec<Diagnostic>> {
    let entries = collect_externs(bundle);
    if entries.is_empty() {
        return Ok(None);
    }

    build_bridge(&entries).map(Some)
}

pub fn build_bridge(entries: &[ExternEntry]) -> Result<FfiLink, Vec<Diagnostic>> {
    let deps = collect_crate_deps(entries);
    let key = cache_key(entries, &deps);
    let cache_root = cache_dir().join(format!("{:016x}", key));
    let crate_name = format!("jet_ffi_{:016x}", key);

    if !command_exists("cargo") {
        return Err(vec![Diagnostic::error(
            "E0703",
            "can't call foreign Rust crates without `cargo`".to_string(),
            "Jet builds a small helper crate for each `extern rust` dependency set".to_string(),
            "install Rust from https://rustup.rs (this includes `cargo`), then try again"
                .to_string(),
            None,
        )]);
    }

    fs::create_dir_all(&cache_root)
        .map_err(|e| tool_error(&format!("couldn't create the FFI cache folder: {}", e)))?;

    let src_dir = cache_root.join("src");
    fs::create_dir_all(&src_dir)
        .map_err(|e| tool_error(&format!("couldn't create the FFI build folder: {}", e)))?;

    let manifest = cache_root.join("Cargo.toml");
    let lib_rs = src_dir.join("lib.rs");
    fs::write(&manifest, emit_cargo_toml(&crate_name, &deps))
        .map_err(|e| tool_error(&format!("couldn't write the FFI manifest: {}", e)))?;
    fs::write(&lib_rs, emit_wrapper_lib(entries))
        .map_err(|e| tool_error(&format!("couldn't write the FFI wrappers: {}", e)))?;

    let target_dir = cache_root.join("target");
    let out = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .map_err(|e| {
            vec![Diagnostic::error(
                "E0703",
                format!("couldn't run `cargo`: {}", e),
                "Jet needs `cargo` to build foreign crate wrappers".to_string(),
                "install Rust from https://rustup.rs, then try again".to_string(),
                None,
            )]
        })?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if looks_like_signature_mismatch(&stderr) {
            let hint = entries
                .first()
                .map(|e| e.line_hint.clone())
                .unwrap_or_else(|| "an `extern rust` declaration".to_string());
            return Err(vec![Diagnostic::error(
                "E0705",
                format!(
                    "the Rust item doesn't match the Jet signature at {}",
                    hint
                ),
                "the `= \"rust::path\"` on an `extern rust` line must name a safe Rust function whose parameters and return type match what you wrote"
                    .to_string(),
                format!(
                    "check the path and types on {}, or pick a different Rust function",
                    hint
                ),
                None,
            )
            .with_detail(format!("  cargo said:\n{}", stable_cargo_detail(&stderr)))]);
        }
        let dep = deps
            .keys()
            .next()
            .map(|k| format!("{}@{}", k, deps[k]))
            .unwrap_or_else(|| "a foreign crate".to_string());
        return Err(vec![Diagnostic::error(
            "E0704",
            format!("couldn't fetch or build `{}`", dep),
            "pure-Rust crates only — crates that need system libraries or a build script aren't supported yet"
                .to_string(),
            "try a different crate version, check your network, or pick another crate"
                .to_string(),
            None,
        )
        .with_detail(format!("  cargo said:\n{}", stable_cargo_detail(&stderr)))]);
    }

    let target = target_dir.join("release");
    let rlib = target.join(format!("lib{}.rlib", crate_name));
    if !rlib.is_file() {
        return Err(tool_error(&format!(
            "FFI build finished but `{}` is missing",
            rlib.display()
        )));
    }
    Ok(FfiLink {
        crate_name,
        rlib_path: rlib,
        deps_dir: target.join("deps"),
    })
}

fn collect_crate_deps(entries: &[ExternEntry]) -> BTreeMap<String, String> {
    let mut deps = BTreeMap::new();
    for e in entries {
        if e.crate_spec == "std" {
            continue;
        }
        if let Some((name, ver)) = parse_crate_spec(&e.crate_spec) {
            deps.insert(name, ver);
        }
    }
    deps
}

/// `"std"` or `Some((name, version))` for `"name@version"`.
pub fn parse_crate_spec(spec: &str) -> Option<(String, String)> {
    if spec == "std" {
        return None;
    }
    let (name, ver) = spec.split_once('@')?;
    if name.is_empty() || ver.is_empty() {
        return None;
    }
    Some((name.to_string(), ver.to_string()))
}

pub fn crate_spec_needs_version(spec: &str) -> bool {
    spec != "std" && spec.split_once('@').is_none()
}

fn cache_key(entries: &[ExternEntry], deps: &BTreeMap<String, String>) -> u64 {
    let mut h = DefaultHasher::new();
    for (k, v) in deps {
        k.hash(&mut h);
        v.hash(&mut h);
    }
    for e in entries {
        e.wrapper_name.hash(&mut h);
        e.rust_path.hash(&mut h);
        e.crate_spec.hash(&mut h);
        for (c, t) in &e.params {
            format!("{:?}", c).hash(&mut h);
            type_key(t).hash(&mut h);
        }
        if let Some(rt) = &e.return_type {
            type_key(rt).hash(&mut h);
        }
    }
    h.finish()
}

fn type_key(ty: &Type) -> String {
    match ty {
        Type::Int => "Int".into(),
        Type::Float => "Float".into(),
        Type::Bool => "Bool".into(),
        Type::String => "String".into(),
        Type::Char => "Char".into(),
        Type::List(inner) => format!("List<{}>", type_key(inner)),
        Type::Map { key, value } => format!("Map<{},{}>", type_key(key), type_key(value)),
        Type::Shared(inner) => format!("Shared<{}>", type_key(inner)),
        Type::Option(inner) => format!("{}?", type_key(inner)),
        Type::Result { ok, err } => format!("Result<{},{}>", type_key(ok), type_key(err)),
        Type::Fn { params, ret } => {
            let ps = params.iter().map(type_key).collect::<Vec<_>>().join(",");
            let r = ret.as_ref().map(|t| type_key(t)).unwrap_or_default();
            format!("fn({ps})->{r}")
        }
        Type::Named(n) => n.clone(),
        Type::Apply { name, args } => format!(
            "{name}<{}>",
            args.iter().map(type_key).collect::<Vec<_>>().join(",")
        ),
        Type::TraitObject(t) => format!("dyn {t}"),
        Type::Tuple(fields) => format!(
            "({})",
            fields
                .iter()
                .map(|(n, t)| format!("{n}:{}", type_key(t)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::FixedList { elem, len } => format!("List<{}#{}>", type_key(elem), len),
    }
}

fn cache_dir() -> PathBuf {
    dirs_home().join(".cache").join("jet").join("ffi")
}

fn dirs_home() -> PathBuf {
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h);
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        return PathBuf::from(h);
    }
    PathBuf::from("/tmp")
}

fn command_exists(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").output().is_ok()
}

fn emit_cargo_toml(crate_name: &str, deps: &BTreeMap<String, String>) -> String {
    let mut s = format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"rlib\"]\n\n"
    );
    if !deps.is_empty() {
        s.push_str("[dependencies]\n");
        for (name, ver) in deps {
            s.push_str(&format!("{name} = \"{ver}\"\n"));
        }
    }
    s
}

fn emit_wrapper_lib(entries: &[ExternEntry]) -> String {
    let mut out = String::from(
        "// Auto-generated FFI wrappers — do not edit.\n#![allow(warnings)]\n\nfn ffi_panic() -> ! {\n    eprintln!(\"panic: a foreign function panicked\");\n    std::process::exit(70);\n}\n\n",
    );
    let mut names: HashSet<String> = HashSet::new();
    for e in entries {
        names.insert(e.jet_name.clone());
    }
    for e in entries {
        out.push_str(&emit_wrapper_fn(e, &names));
        out.push('\n');
    }
    out
}

fn emit_wrapper_fn(entry: &ExternEntry, user_types: &HashSet<String>) -> String {
    let params: Vec<String> = entry
        .params
        .iter()
        .enumerate()
        .map(|(i, (_, ty))| format!("p{i}: {}", rust_type(ty, user_types)))
        .collect();
    let ret = entry
        .return_type
        .as_ref()
        .map(|t| rust_type(t, user_types))
        .unwrap_or_else(|| "()".to_string());
    let call_args: Vec<String> = (0..entry.params.len()).map(|i| format!("p{i}")).collect();
    let rust_call = if ret == "()" {
        format!("{}({});", entry.rust_path, call_args.join(", "))
    } else {
        format!("{}({})", entry.rust_path, call_args.join(", "))
    };
    let body = if ret == "()" {
        format!(
            "match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{\n        {rust_call}\n    }})) {{\n        Ok(()) => (),\n        Err(_) => ffi_panic(),\n    }}"
        )
    } else {
        format!(
            "match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {rust_call})) {{\n        Ok(v) => v,\n        Err(_) => ffi_panic(),\n    }}"
        )
    };
    format!(
        "pub fn {}({}){} {{\n    {body}\n}}\n",
        entry.wrapper_name,
        params.join(", "),
        if ret == "()" {
            String::new()
        } else {
            format!(" -> {ret}")
        }
    )
}

fn rust_type(ty: &Type, user_types: &HashSet<String>) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "String".to_string(),
        Type::Char => "char".to_string(),
        Type::List(inner) => format!("Vec<{}>", rust_type(inner, user_types)),
        Type::Map { key, value } => format!(
            "std::collections::BTreeMap<{}, {}>",
            rust_type(key, user_types),
            rust_type(value, user_types)
        ),
        Type::Shared(inner) => format!("std::sync::Arc<{}>", rust_type(inner, user_types)),
        Type::Option(inner) => format!("Option<{}>", rust_type(inner, user_types)),
        Type::Result { ok, err } => format!(
            "Result<{}, {}>",
            rust_type(ok, user_types),
            rust_type(err, user_types)
        ),
        Type::Fn { .. } => "Box<dyn std::any::Any>".to_string(),
        Type::Named(name) if name == "Error" => "String".to_string(),
        Type::Named(name) if user_types.contains(name) => format!("user_{name}"),
        Type::Named(name) => name.clone(),
        Type::Apply { name, args } if user_types.contains(name) => format!(
            "user_{name}<{}>",
            args.iter()
                .map(|a| rust_type(a, user_types))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Apply { .. } | Type::TraitObject(_) | Type::Tuple(_) => {
            "Box<dyn std::any::Any>".to_string()
        }
        Type::FixedList { elem, .. } => format!("Vec<{}>", rust_type(elem, user_types)),
    }
}

fn looks_like_signature_mismatch(stderr: &str) -> bool {
    stderr.contains("E0308")
        || stderr.contains("E0277")
        || stderr.contains("E0061")
        || stderr.contains("E0425")
        || stderr.contains("cannot find")
        || stderr.contains("mismatched types")
        || stderr.contains("arguments to this function")
}

fn indent_block(s: &str) -> String {
    s.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keep cargo output stable for ui snapshots — drop fetch noise and parallel-build chatter.
fn stable_cargo_detail(stderr: &str) -> String {
    let kept: Vec<String> = stderr
        .lines()
        .filter(|line| {
            let t = line.trim();
            !t.is_empty()
                && !t.starts_with("Updating crates.io")
                && !t.starts_with("Locking ")
                && !t.contains("waiting for other jobs")
        })
        .map(|line| normalize_ffi_cache_path(line))
        .collect();
    indent_block(&kept.join("\n"))
}

/// Keep ui snapshots stable across machines (`/home/…/.cache/jet/ffi/…` → `~/.cache/jet/ffi/…`).
fn normalize_ffi_cache_path(line: &str) -> String {
    let marker = ".cache/jet/ffi/";
    let Some(idx) = line.find(marker) else {
        return line.to_string();
    };
    let path_start = line[..idx]
        .rfind('/')
        .and_then(|slash| {
            let pre = &line[..slash];
            pre.rfind(|c| c == '(' || c == ' ')
                .map(|j| j + 1)
                .or(Some(0))
        })
        .unwrap_or(idx);
    let rest = &line[idx + marker.len()..];
    let hash_len = rest.chars().take_while(|c| c.is_ascii_hexdigit()).count();
    if hash_len != 16 {
        return line.to_string();
    }
    let hash = &rest[..hash_len];
    let suffix = &rest[hash_len..];
    format!("{}~/.cache/jet/ffi/{}{}", &line[..path_start], hash, suffix)
}

fn tool_error(msg: &str) -> Vec<Diagnostic> {
    vec![Diagnostic::error(
        "E0704",
        msg.to_string(),
        "building the foreign crate bridge failed".to_string(),
        "check disk permissions and try again".to_string(),
        None,
    )]
}
