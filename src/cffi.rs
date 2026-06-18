//! S59 (E2-M14) — C FFI assembly, merge, and link resolution.
//!
//! The front end owns all C-FFI semantics; rustc only verifies the generated
//! `extern "C"` shims (I2/I3). This module runs after the loader has read every
//! `.jet` file and before sema:
//!
//! 1. Gather every `@extern module c.<lib>` (overlay) and
//!    `@bindgen module c.<lib>.__bindgen__` (generated cache) item.
//! 2. Enforce the location rule: `@bindgen` only in a generated
//!    `.jet/bindings/c/<lib>.jet` file (E3207).
//! 3. Merge per library — **bindgen ∪ overlay, overlay wins** on a clash;
//!    incompatible signatures are E3205 (D-CFFI2-SYN-4).
//! 4. Materialize one synthetic module per library (alias `__c_<lib>`) holding
//!    the merged surface, so the rest of the pipeline treats C calls exactly
//!    like any other namespaced module call.
//! 5. Classify each file's C `use` forms — `use c.<lib>` / `use "<header>.h"` —
//!    one form per lib per file (E3204), and record the synthetic target for
//!    sema's import map.
//!
//! Link discovery (hangar vs pkg-config) lives in `link.rs`-style helpers here
//! and surfaces E3201 when neither finds the library.

use crate::ast::{CModule, CModuleKind, ExternFn, ImportDecl, ImportKind, Item, LoadedModule, ProgramBundle};
use crate::diag::{Diagnostic, Span};
use crate::syntax;
use std::collections::HashMap;
use std::path::Path;

/// The result of resolving one C `use` in one file: which synthetic module it
/// binds to. Keyed for sema's per-module import map.
#[derive(Debug, Clone)]
pub struct CImportLink {
    /// Index of the importing module in `bundle.modules`.
    pub importing_idx: usize,
    /// The alias the user bound (`rl`).
    pub alias: String,
    /// Index of the synthetic merged C module in `bundle.modules`.
    pub target_idx: usize,
}

/// One C library that the program links against, with the discovered surface.
#[derive(Debug, Clone)]
pub struct CLib {
    /// Link key — last `c.<lib>` segment (`raylib`).
    pub lib: String,
    /// Index of the synthetic merged module in `bundle.modules`.
    pub module_idx: usize,
}

/// `Module("c.<lib>")` → `Some("<lib>")` (the logical-module C `use` form).
pub fn c_module_lib(imp: &ImportDecl) -> Option<String> {
    if let ImportKind::Module(name, _) = &imp.kind {
        let mut segs = name.split('.');
        if segs.next() == Some(syntax::C_MODULE_ROOT) {
            let lib = segs.next()?;
            // Reject `c.lib.extra` and bare `c` — only `c.<lib>` is a C use.
            if !lib.is_empty() && segs.next().is_none() {
                return Some(lib.to_string());
            }
        }
    }
    None
}

/// `File("<…>.h")` → `Some((header, lib))` (the header-path C `use` form). The
/// link key is the header basename without directory or extension (D-CFFI2-SYN
/// header→lib rule; alias maps are out of M14 v1).
pub fn c_header_lib(imp: &ImportDecl) -> Option<(String, String)> {
    if let ImportKind::File(path, _) = &imp.kind {
        if path.ends_with(".h") {
            let base = path.rsplit('/').next().unwrap_or(path);
            let lib = base.strip_suffix(".h").unwrap_or(base).to_string();
            return Some((path.clone(), lib));
        }
    }
    None
}

/// Is this import any C `use` form?
pub fn is_c_import(imp: &ImportDecl) -> bool {
    c_module_lib(imp).is_some() || c_header_lib(imp).is_some()
}

/// The synthetic module alias for a C library (`__c_raylib`). Never typeable by
/// users (a `c.` prefix is reserved and `__` mirrors the reserved segment).
fn synthetic_alias(lib: &str) -> String {
    format!("__c_{lib}")
}

/// True when a file path sits under the generated bindings cache
/// `.jet/bindings/c/` (D-CBIND7). `@bindgen` is legal only there (E3207).
fn is_generated_cache_file(display: &str) -> bool {
    let needle = format!("{}/{}/", syntax::SOURCE_ROOT_DIR, syntax::BINDINGS_C_SUBDIR);
    display.replace('\\', "/").contains(&needle)
}

/// Two `ExternFn`s have the same boundary signature (params by type + return).
fn same_signature(a: &ExternFn, b: &ExternFn) -> bool {
    if a.params.len() != b.params.len() {
        return false;
    }
    if a.return_type != b.return_type || a.is_view_return != b.is_view_return {
        return false;
    }
    a.params
        .iter()
        .zip(&b.params)
        .all(|(x, y)| x.convention == y.convention && x.ty == y.ty)
}

/// E3207 — `@bindgen` used outside a generated cache file.
fn e3207(lib: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3207",
        format!("`@{}` is only allowed in generated cache files", syntax::ATTR_BINDGEN),
        format!(
            "`{}/{}/{}.{}` is written by `{} bind`; hand-written sources use `@{} module`",
            syntax::SOURCE_ROOT_DIR,
            syntax::BINDINGS_C_SUBDIR,
            lib,
            syntax::FILE_EXT,
            syntax::BINARY_NAME,
            syntax::ATTR_EXTERN_MODULE,
        ),
        format!(
            "edit your overlay file with `@{} module`, or regenerate the cache with `{} bind`",
            syntax::ATTR_EXTERN_MODULE, syntax::BINARY_NAME,
        ),
        Some(span),
    )
}

/// E3205 — overlay symbol clashes with bindgen with an incompatible signature.
fn e3205(lib: &str, name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3205",
        format!("overlay `{}` disagrees with the generated binding", name),
        format!(
            "user `@{} module {}.{}` may override bindgen symbols, but the signature must stay compatible when replacing",
            syntax::ATTR_EXTERN_MODULE, syntax::C_MODULE_ROOT, lib,
        ),
        "match the generated signature, or rename your overlay function".to_string(),
        Some(span),
    )
}

/// E3204 — two different C `use` forms for the same lib in one file.
fn e3204(lib: &str, header: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3204",
        format!("two different `{}` forms refer to the same C library `{}`", syntax::KW_USE, lib),
        format!(
            "{} allows one bring-in per C lib per file — either `{} \"{}\" as alias` or `{} {}.{} as alias`, not both",
            syntax::LANG_NAME, syntax::KW_USE, header, syntax::KW_USE, syntax::C_MODULE_ROOT, lib,
        ),
        "remove one line; keep the form that matches your workflow".to_string(),
        Some(span),
    )
}

/// Gathered C-FFI artifacts to thread into sema and codegen.
#[derive(Debug, Default, Clone)]
pub struct CFfi {
    /// Per-file C import → synthetic module bindings.
    pub import_links: Vec<CImportLink>,
    /// Libraries the program links against.
    pub libs: Vec<CLib>,
}

impl CFfi {
    /// Synthetic-module index for an importing module + alias, if it is a C use.
    pub fn target_for(&self, importing_idx: usize, alias: &str) -> Option<usize> {
        self.import_links
            .iter()
            .find(|l| l.importing_idx == importing_idx && l.alias == alias)
            .map(|l| l.target_idx)
    }

    /// True when the program links against at least one C library.
    pub fn links_c(&self) -> bool {
        !self.libs.is_empty()
    }

    /// Resolve native-library linker arguments for every C library this program
    /// uses (D-CFFI2). On any unresolved lib, returns the E3201 diagnostics.
    /// The returned strings are ready to append to a `rustc`/`cc` command:
    /// `-L native=<dir>` for lib dirs and `-l <name>` for each link name.
    pub fn rustc_link_args(&self, project_root: &Path) -> Result<Vec<String>, Vec<Diagnostic>> {
        let mut args = Vec::new();
        let mut diags = Vec::new();
        for lib in &self.libs {
            match resolve_link(&lib.lib, project_root) {
                Ok(flags) => {
                    for dir in &flags.lib_dirs {
                        args.push("-L".to_string());
                        args.push(format!("native={dir}"));
                    }
                    for name in &flags.link_names {
                        args.push("-l".to_string());
                        args.push(name.clone());
                    }
                }
                Err(d) => diags.push(d),
            }
        }
        if !diags.is_empty() {
            return Err(diags);
        }
        Ok(args)
    }
}

/// Run the whole C-FFI assembly pass over a freshly loaded bundle. Removes
/// `Item::CModule`s from user files, merges them, appends synthetic modules,
/// and resolves C `use` forms. Returns the artifacts, or diagnostics.
pub fn assemble(bundle: &mut ProgramBundle) -> Result<CFfi, Vec<Diagnostic>> {
    let mut diags = Vec::new();

    // 0. Load any generated bindgen cache files for libraries this program
    //    brings in. Cache files (`.jet/bindings/c/<lib>.jet`) are not `use`d
    //    explicitly; they are discovered here so the bindgen surface is present
    //    before merge. (Phase 3 regenerates stale caches; Phase 1 relies on a
    //    hand-written fixture sitting at that path.)
    load_binding_caches(bundle, &mut diags);
    if !diags.is_empty() {
        return Err(diags);
    }

    // 1. Drain every CModule from every loaded file, validating location.
    //    Grouped per lib into (bindgen funcs, overlay funcs).
    struct LibSurface {
        bindgen: Vec<ExternFn>,
        overlay: Vec<ExternFn>,
    }
    let mut surfaces: HashMap<String, LibSurface> = HashMap::new();
    // Preserve first-seen order for stable synthetic module ordering / output.
    let mut order: Vec<String> = Vec::new();

    for module in &mut bundle.modules {
        let display = module.display.clone();
        let generated = is_generated_cache_file(&display);
        let mut kept = Vec::new();
        for item in module.items.drain(..) {
            let Item::CModule(cm) = item else {
                kept.push(item);
                continue;
            };
            let CModule { kind, lib, path_span, functions, .. } = cm;
            if kind == CModuleKind::Bindgen && !generated {
                diags.push(e3207(&lib, path_span));
                continue;
            }
            if !surfaces.contains_key(&lib) {
                order.push(lib.clone());
                surfaces.insert(
                    lib.clone(),
                    LibSurface { bindgen: Vec::new(), overlay: Vec::new() },
                );
            }
            let surf = surfaces.get_mut(&lib).unwrap();
            match kind {
                CModuleKind::Bindgen => surf.bindgen.extend(functions),
                CModuleKind::Extern => surf.overlay.extend(functions),
            }
        }
        module.items = kept;
    }

    if !diags.is_empty() {
        return Err(diags);
    }

    // 2. Merge per lib (bindgen ∪ overlay; overlay wins; clash → E3205) and
    //    materialize one synthetic module each.
    let mut cffi = CFfi::default();
    let mut lib_to_idx: HashMap<String, usize> = HashMap::new();

    for lib in &order {
        let surf = surfaces.get(lib).unwrap();
        // Start from bindgen, then let overlay add/override.
        let mut merged: Vec<ExternFn> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        for ef in &surf.bindgen {
            // Last bindgen wins on intra-bindgen dup (regen artifact); rare.
            if let Some(&i) = index.get(&ef.name) {
                merged[i] = ef.clone();
            } else {
                index.insert(ef.name.clone(), merged.len());
                merged.push(ef.clone());
            }
        }
        for ef in &surf.overlay {
            if let Some(&i) = index.get(&ef.name) {
                // Override: signatures must stay compatible (D-CFFI2-SYN-4).
                if !same_signature(&merged[i], ef) {
                    diags.push(e3205(lib, &ef.name, ef.name_span));
                    continue;
                }
                merged[i] = ef.clone();
            } else {
                index.insert(ef.name.clone(), merged.len());
                merged.push(ef.clone());
            }
        }

        let alias = synthetic_alias(lib);
        let synth_idx = bundle.modules.len();
        let merged_module = CModule {
            kind: CModuleKind::Extern,
            lib: lib.clone(),
            path_span: Span::new(0, 0),
            functions: merged,
            span: Span::new(0, 0),
        };
        bundle.modules.push(LoadedModule {
            path: std::path::PathBuf::from(format!("<c.{lib}>")),
            display: format!("c.{lib}"),
            source: String::new(),
            alias,
            imports: Vec::new(),
            items: vec![Item::CModule(merged_module)],
        });
        lib_to_idx.insert(lib.clone(), synth_idx);
        cffi.libs.push(CLib { lib: lib.clone(), module_idx: synth_idx });
    }

    if !diags.is_empty() {
        return Err(diags);
    }

    // 3. Resolve each file's C `use` forms (E3204 on duplicate forms per lib).
    //    A `use` of a lib with no declared surface is allowed — the bind backend
    //    (Phase 3) would generate it; for now an empty synthetic module is made
    //    on demand so the alias still resolves and link discovery still runs.
    let n_user_modules = bundle.modules.len();
    for idx in 0..n_user_modules {
        // Track which form (module vs header) first bound each lib in this file.
        let mut seen_form: HashMap<String, (bool, String)> = HashMap::new(); // lib -> (is_header, header)
        let imports = bundle.modules[idx].imports.clone();
        for imp in &imports {
            let (lib, is_header, header) = if let Some(lib) = c_module_lib(imp) {
                (lib, false, String::new())
            } else if let Some((header, lib)) = c_header_lib(imp) {
                (lib, true, header)
            } else {
                continue;
            };
            if let Some((prev_header, _)) = seen_form.get(&lib) {
                if *prev_header != is_header {
                    let h = if is_header { &header } else { "" };
                    diags.push(e3204(&lib, h, imp.span));
                    continue;
                }
            } else {
                seen_form.insert(lib.clone(), (is_header, header.clone()));
            }

            let target_idx = match lib_to_idx.get(&lib) {
                Some(&i) => i,
                None => {
                    // No surface yet: make an empty synthetic module so the
                    // alias resolves and link discovery still names the lib.
                    let synth_idx = bundle.modules.len();
                    bundle.modules.push(LoadedModule {
                        path: std::path::PathBuf::from(format!("<c.{lib}>")),
                        display: format!("c.{lib}"),
                        source: String::new(),
                        alias: synthetic_alias(&lib),
                        imports: Vec::new(),
                        items: vec![Item::CModule(CModule {
                            kind: CModuleKind::Extern,
                            lib: lib.clone(),
                            path_span: Span::new(0, 0),
                            functions: Vec::new(),
                            span: Span::new(0, 0),
                        })],
                    });
                    lib_to_idx.insert(lib.clone(), synth_idx);
                    cffi.libs.push(CLib { lib: lib.clone(), module_idx: synth_idx });
                    synth_idx
                }
            };
            let alias = crate::loader::import_alias(imp);
            cffi.import_links.push(CImportLink {
                importing_idx: idx,
                alias,
                target_idx,
            });
        }
    }

    if !diags.is_empty() {
        return Err(diags);
    }
    Ok(cffi)
}

/// Discover and parse generated bindgen cache files for every C library this
/// program brings in. Each cache lives at `<root>/.jet/bindings/c/<lib>.jet`
/// (D-CBIND7). Parsed `@bindgen` modules are appended as ordinary loaded
/// modules so the main drain/merge pass (step 1) folds them like any other.
fn load_binding_caches(bundle: &mut ProgramBundle, diags: &mut Vec<Diagnostic>) {
    // Collect the set of C libs used anywhere in the program.
    let mut libs: Vec<String> = Vec::new();
    for module in &bundle.modules {
        for imp in &module.imports {
            let lib = c_module_lib(imp).or_else(|| c_header_lib(imp).map(|(_, l)| l));
            if let Some(lib) = lib {
                if !libs.contains(&lib) {
                    libs.push(lib);
                }
            }
        }
    }
    if libs.is_empty() {
        return;
    }

    // Avoid re-loading a cache file already present in the bundle.
    let already: std::collections::HashSet<std::path::PathBuf> =
        bundle.modules.iter().map(|m| m.path.clone()).collect();

    for lib in libs {
        let path = bundle
            .project_root
            .join(syntax::SOURCE_ROOT_DIR)
            .join(syntax::BINDINGS_C_SUBDIR)
            .join(format!("{}.{}", lib, syntax::FILE_EXT));
        if !path.is_file() || already.contains(&path) {
            continue;
        }
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let (toks, lex_diags) = crate::lexer::lex(&source);
        if !lex_diags.is_empty() {
            diags.extend(lex_diags);
            continue;
        }
        let mut prog = match crate::parser::parse(&toks) {
            Ok(p) => p,
            Err(ds) => {
                diags.extend(ds);
                continue;
            }
        };
        let display = path.display().to_string();
        bundle.modules.push(LoadedModule {
            path: path.clone(),
            display,
            source,
            alias: format!("__c_cache_{lib}"),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
        });
    }
}

/// Resolve link flags for one C library (D-CFFI2): hangar dep if the manifest
/// pins it under `[dependencies:c]`, else `pkg-config <lib>`, else E3201.
/// Returns `(include_dirs, lib_dirs, link_names)`.
pub fn resolve_link(
    lib: &str,
    project_root: &Path,
) -> Result<LinkFlags, Diagnostic> {
    // 1. Hangar: a `[dependencies:c]` entry keyed by `<lib>` in pkg.jet.
    if let Some(flags) = hangar_link(lib, project_root) {
        return Ok(flags);
    }
    // 2. pkg-config fallback.
    match pkg_config_link(lib) {
        PkgConfig::Found(flags) => Ok(flags),
        PkgConfig::NotFound | PkgConfig::Unavailable => Err(e3201(lib)),
    }
}

/// Native-library link inputs for rustc (`-l`, `-L`, `-I` analog via cc args).
#[derive(Debug, Clone, Default)]
pub struct LinkFlags {
    pub include_dirs: Vec<String>,
    pub lib_dirs: Vec<String>,
    /// Names passed to `-l` (e.g. `raylib`).
    pub link_names: Vec<String>,
}

/// E3201 — C library not found via hangar or pkg-config.
fn e3201(lib: &str) -> Diagnostic {
    Diagnostic::error(
        "E3201",
        format!("C library `{}` was not found.", lib),
        format!(
            "Jet tried the hangar dep keyed `{}` in `{}`, then `pkg-config {}` on the system; neither provided include/link paths.",
            lib, syntax::PAYLOAD_FILE, lib,
        ),
        format!(
            "Install the system package (e.g. `pacman -S {}`), or add `{}` under `[{}]` with a pinned hangar ref.",
            lib, lib, syntax::DEP_TABLE_C,
        ),
        None,
    )
}

/// Look up a `[dependencies:c]` entry keyed by `<lib>` in `pkg.jet`. The
/// hangar realization of C deps is a Jetpack concern (Phase 2 fixture-backed
/// until jetpack parses C deps); here we only honor an explicit local override
/// directory recorded in the manifest if present.
fn hangar_link(lib: &str, project_root: &Path) -> Option<LinkFlags> {
    let manifest = project_root.join(syntax::PAYLOAD_FILE);
    let raw = std::fs::read_to_string(&manifest).ok()?;
    let entry = parse_c_dep(&raw, lib)?;
    // A bare `<lib> = "nixpkgs:raylib#5.5.0"` has no local paths yet (hangar
    // realization pending); a `<lib> = { path = "…" }` override gives a dir to
    // link from. Either way the lib name is the rustc `-l` name.
    let mut flags = LinkFlags { link_names: vec![lib.to_string()], ..Default::default() };
    if let Some(dir) = entry {
        if !dir.is_empty() {
            flags.lib_dirs.push(dir.clone());
            flags.include_dirs.push(dir);
        }
    }
    Some(flags)
}

/// Minimal `[dependencies:c]` reader: returns `Some(local_dir_or_empty)` when
/// `<lib>` is declared. Recognizes `<lib> = { path = "…" }` for a local dir and
/// `<lib> = "ref"` for a pinned (hangar) dep with no local dir yet.
pub fn parse_c_dep(raw: &str, lib: &str) -> Option<Option<String>> {
    let mut in_table = false;
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_table = t == format!("[{}]", syntax::DEP_TABLE_C);
            continue;
        }
        if !in_table {
            continue;
        }
        let Some((key, value)) = t.split_once('=') else { continue };
        if key.trim() != lib {
            continue;
        }
        let v = value.trim();
        if v.starts_with('{') {
            // Look for `path = "…"`.
            if let Some(p) = v.find("path") {
                if let Some(q1) = v[p..].find('"') {
                    let start = p + q1 + 1;
                    if let Some(q2) = v[start..].find('"') {
                        return Some(Some(v[start..start + q2].to_string()));
                    }
                }
            }
            return Some(None);
        }
        return Some(None);
    }
    None
}

enum PkgConfig {
    Found(LinkFlags),
    NotFound,
    Unavailable,
}

/// Run `pkg-config --cflags --libs <lib>` and parse `-I`/`-L`/`-l` flags.
fn pkg_config_link(lib: &str) -> PkgConfig {
    let out = match std::process::Command::new("pkg-config")
        .arg("--cflags")
        .arg("--libs")
        .arg(lib)
        .output()
    {
        Ok(o) => o,
        Err(_) => return PkgConfig::Unavailable,
    };
    if !out.status.success() {
        return PkgConfig::NotFound;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    PkgConfig::Found(parse_pkg_config(&text, lib))
}

/// Parse a pkg-config flag line into structured `LinkFlags`. Public for tests.
pub fn parse_pkg_config(text: &str, lib: &str) -> LinkFlags {
    let mut flags = LinkFlags::default();
    for tok in text.split_whitespace() {
        if let Some(d) = tok.strip_prefix("-I") {
            flags.include_dirs.push(d.to_string());
        } else if let Some(d) = tok.strip_prefix("-L") {
            flags.lib_dirs.push(d.to_string());
        } else if let Some(n) = tok.strip_prefix("-l") {
            flags.link_names.push(n.to_string());
        }
    }
    if flags.link_names.is_empty() {
        flags.link_names.push(lib.to_string());
    }
    flags
}
