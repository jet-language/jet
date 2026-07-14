//! S59 (E2-M14) — C FFI assembly, merge, and link resolution.
//!
//! The front end owns all C-FFI semantics; rustc only verifies the generated
//! `extern "C"` shims (I2/I3). This module runs after the loader has read every
//! `.jet` file and before sema:
//!
//! 1. Gather every `#Extern module c.<lib>` (overlay) and
//!    `#Bindgen module c.<lib>.__bindgen__` (generated cache) item.
//! 2. Enforce the location rule: `#Bindgen` only in a generated
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
//! Link discovery lives in helpers here: a declared `<lib>: c@…` dep in the
//! `deps:` block of `pkg.jet` (S59/D-CFFI2) takes precedence, else `pkg-config
//! <lib>`, else E3201.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{
    CModule, CModuleKind, ExternFn, ForeignLanguage, ImportDecl, ImportKind, Item, LoadedModule,
    ProgramBundle,
};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::path::Path;

// Struct defs live in AST for cross-seam sharing; re-export for callers.
pub use crate::AST::{CFfi, CImportLink, CLib};

/// `Module("c.<lib>")` → `Some("<lib>")` (the logical-module C `use` form).
pub fn c_module_lib(imp: &ImportDecl) -> Option<String> {
    let ns = imp.foreign_namespace()?;
    (ns.language == ForeignLanguage::C).then_some(ns.lib)
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

fn import_alias(imp: &ImportDecl) -> String {
    imp.import_alias()
}

fn binding_cache_file(project_root: &Path, language: ForeignLanguage, lib: &str) -> std::path::PathBuf {
    project_root
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(language.bindings_subdir())
        .join(format!("{}.{}", lib, Syntax::FILE_EXT))
}

/// The synthetic module alias for a C library (`__c_raylib`). Never typeable by
/// users (a `c.` prefix is reserved and `__` mirrors the reserved segment).
fn synthetic_alias(lib: &str) -> String {
    format!("__c_{lib}")
}

/// True when a file path sits under the generated bindings cache
/// `.jet/bindings/c/` (D-CBIND7). `#Bindgen` is legal only there (E3207).
fn is_generated_cache_file(display: &str) -> bool {
    let needle = format!(
        "{}/{}/",
        Syntax::SOURCE_ROOT_DIR,
        ForeignLanguage::C.bindings_subdir()
    );
    display.replace('\\', "/").contains(&needle)
}

/// Two `ExternFn`s have the same boundary signature (params by type + return).
fn same_signature(a: &ExternFn, b: &ExternFn) -> bool {
    if a.abi.as_ref().map(|(name, _)| name) != b.abi.as_ref().map(|(name, _)| name) {
        return false;
    }
    if a.params.len() != b.params.len() {
        return false;
    }
    if a.return_type != b.return_type {
        return false;
    }
    a.params
        .iter()
        .zip(&b.params)
        .all(|(x, y)| x.convention == y.convention && x.ty == y.ty)
}

/// E3207 — `#Bindgen` used outside a generated cache file.
fn e3207(lib: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3207",
        format!(
            "`#{}` is only allowed in generated cache files",
            Syntax::ATTR_BINDGEN
        ),
        format!(
            "`{}/{}/{}.{}` is written by `{} inspect bind`; hand-written sources use `#{} module`",
            Syntax::SOURCE_ROOT_DIR,
            ForeignLanguage::C.bindings_subdir(),
            lib,
            Syntax::FILE_EXT,
            Syntax::BINARY_NAME,
            Syntax::ATTR_EXTERN_MODULE,
        ),
        format!(
            "edit your overlay file with `#{} module`, or regenerate the cache with `{} inspect bind`",
            Syntax::ATTR_EXTERN_MODULE,
            Syntax::BINARY_NAME,
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
            "user `#{} module {}.{}` may override bindgen symbols, but the signature must stay compatible when replacing",
            Syntax::ATTR_EXTERN_MODULE, Syntax::C_MODULE_ROOT, lib,
        ),
        "match the generated signature, or rename your overlay function".to_string(),
        Some(span),
    )
}

/// E3204 — two different C `use` forms for the same lib in one file.
fn e3204(lib: &str, header: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3204",
        format!("two different `{}` forms refer to the same C library `{}`", Syntax::KW_USE, lib),
        format!(
            "{} allows one bring-in per C lib per file — either `{} \"{}\" as alias` or `{} {}.{} as alias`, not both",
            Syntax::LANG_NAME, Syntax::KW_USE, header, Syntax::KW_USE, Syntax::C_MODULE_ROOT, lib,
        ),
        "remove one line; keep the form that matches your workflow".to_string(),
        Some(span),
    )
}

/// Resolve native-library linker arguments for every C library this program
/// uses (D-CFFI2). On any unresolved lib, returns the E3201 diagnostics.
/// The returned strings are ready to append to a `rustc`/`cc` command.
pub fn rustc_link_args(cffi: &CFfi, project_root: &Path) -> Result<Vec<String>, Vec<Diagnostic>> {
    let mut args = Vec::new();
    let mut diags = Vec::new();
    for lib in &cffi.libs {
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
                for dir in &flags.rpath_dirs {
                    args.push("-C".to_string());
                    args.push(format!("link-arg=-Wl,-rpath,{dir}"));
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
            let CModule {
                kind,
                lib,
                path_span,
                functions,
                ..
            } = cm;
            if kind == CModuleKind::Bindgen && !generated {
                diags.push(e3207(&lib, path_span));
                continue;
            }
            let surf = match surfaces.entry(lib.clone()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    order.push(lib.clone());
                    entry.insert(LibSurface {
                        bindgen: Vec::new(),
                        overlay: Vec::new(),
                    })
                }
            };
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
        // Every key enters `order` only beside its vacant `surfaces` insertion
        // above, and no surface is removed before this ordered merge pass.
        let Some(surf) = surfaces.get(lib) else {
            continue;
        };
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

        if lib.starts_with("jet_go_") {
            for function in &mut merged {
                function.effect_root = Some("Go".to_string());
            }
        }
        if lib.starts_with("jet_java_") {
            for function in &mut merged {
                function.effect_root = Some("Java".to_string());
            }
        }
        if lib.starts_with("jet_tcl_") {
            for function in &mut merged { function.effect_root=Some("Tcl".to_string()); }
        }
        if lib.starts_with("jet_ada_") {
            for function in &mut merged { function.effect_root=Some("Ada".to_string()); }
        }
        if lib.starts_with("jet_pascal_") {
            for function in &mut merged { function.effect_root=Some("Pascal".to_string()); }
        }
        if lib.starts_with("jet_dart_") {
            for function in &mut merged { function.effect_root=Some("Dart".to_string()); }
        }
        if lib.starts_with("jet_pwsh_") {
            for function in &mut merged { function.effect_root=Some("PowerShell".to_string()); }
        }
        if lib.starts_with("jet_com_") {
            for function in &mut merged { function.effect_root=Some("Com".to_string()); }
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
            web_target_ceiling: None,
            pub_file: false,
            no_prelude: false,
            html_path: None,
            no_alloc_policy: None,
        });
        lib_to_idx.insert(lib.clone(), synth_idx);
        cffi.libs.push(CLib {
            lib: lib.clone(),
            module_idx: synth_idx,
        });
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
                        web_target_ceiling: None,
                        pub_file: false,
                        no_prelude: false,
                        html_path: None,
                        no_alloc_policy: None,
                    });
                    lib_to_idx.insert(lib.clone(), synth_idx);
                    cffi.libs.push(CLib {
                        lib: lib.clone(),
                        module_idx: synth_idx,
                    });
                    synth_idx
                }
            };
            let alias = import_alias(imp);
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
/// (D-CBIND7). When a cache is absent and the program uses the header-path form
/// (`use "lib.h" as l`), the bind backend is invoked automatically (D-CBIND2
/// auto half, E3 deferred piece). When the cache exists, its sidecar `.hash`
/// file (Phase 3) is checked; a hash mismatch triggers re-bind before loading.
/// Parsed `#Bindgen` modules are appended as ordinary loaded modules so the
/// main drain/merge pass (step 1) folds them like any other.
fn load_binding_caches(bundle: &mut ProgramBundle, diags: &mut Vec<Diagnostic>) {
    // Collect libs and, for the header-path `use "x.h"` form, the header path.
    // A single lib can be brought in from multiple modules; first-seen header wins.
    let mut libs: Vec<String> = Vec::new();
    let mut lib_header: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for module in &bundle.modules {
        for imp in &module.imports {
            if let Some(lib) = c_module_lib(imp) {
                if !libs.contains(&lib) {
                    libs.push(lib);
                }
            } else if let Some((header_path, lib)) = c_header_lib(imp) {
                if !libs.contains(&lib) {
                    libs.push(lib.clone());
                }
                lib_header.entry(lib).or_insert(header_path);
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
        let cache_path = binding_cache_file(&bundle.project_root, ForeignLanguage::C, &lib);

        if already.contains(&cache_path) {
            continue;
        }

        // --- Phase 3: hash invalidation ---
        // If the cache exists and we know the header path, check whether the
        // header content has changed since the cache was generated. On a
        // mismatch, re-run the bind backend before loading.
        let need_rebind = if cache_path.is_file() {
            if let Some(header_path) = lib_header.get(&lib) {
                // Resolve header relative to project root (for `use "x.h"` forms
                // where the path is relative to the importing file's directory;
                // here we accept absolute or project-root-relative).
                let header_abs = resolve_header_path(header_path, &bundle.project_root);
                if let Ok(header_src) = std::fs::read_to_string(&header_abs) {
                    let current_hash = crate::CBind::compute_bind_hash(&header_src, "");
                    let stored = crate::CBind::read_stored_hash(&cache_path);
                    stored.map(|s| s != current_hash).unwrap_or(false)
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // --- D-CBIND2 auto-invoke on cache miss / hash mismatch ---
        if !cache_path.is_file() || need_rebind {
            let Some(header_path) = lib_header.get(&lib) else {
                // No header path known (use c.<lib> form, no file to parse) —
                // skip silently; an empty synthetic module is made in step 3.
                if cache_path.is_file() && !need_rebind {
                    // Present and fresh — handled below.
                }
                if !cache_path.is_file() {
                    continue; // no cache, no header → nothing to auto-bind
                }
                // need_rebind but no header path: can't rebind, use stale cache.
                // fall through to load existing cache
                let source = match std::fs::read_to_string(&cache_path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                load_cache_source(&source, &cache_path, &lib, diags, &mut bundle.modules);
                continue;
            };
            let header_abs = resolve_header_path(header_path, &bundle.project_root);
            let header_src = match std::fs::read_to_string(&header_abs) {
                Ok(s) => s,
                Err(_) => {
                    // Header not readable — can't auto-bind; if cache exists load it.
                    if cache_path.is_file() {
                        let source = match std::fs::read_to_string(&cache_path) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        load_cache_source(&source, &cache_path, &lib, diags, &mut bundle.modules);
                    }
                    continue;
                }
            };
            let result = match crate::CBind::generate(&header_src, &lib) {
                Ok(r) => r,
                Err(_) => {
                    // Bind failed — if there's a stale cache, use it silently.
                    if cache_path.is_file() {
                        let source = match std::fs::read_to_string(&cache_path) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        load_cache_source(&source, &cache_path, &lib, diags, &mut bundle.modules);
                    }
                    continue;
                }
            };
            // Write cache + hash sidecar.
            if let Some(parent) = cache_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&cache_path, &result.source).is_ok() {
                let _ = crate::CBind::write_bind_hash(&cache_path, &header_src, "");
            }
            load_cache_source(
                &result.source,
                &cache_path,
                &lib,
                diags,
                &mut bundle.modules,
            );
            continue;
        }

        // Cache present and fresh — load it.
        let source = match std::fs::read_to_string(&cache_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        load_cache_source(&source, &cache_path, &lib, diags, &mut bundle.modules);
    }
}

/// Resolve a header path: if absolute use as-is, else try relative to
/// the project root (common for `use "raylib.h"` in single-file mode).
fn resolve_header_path(header_path: &str, project_root: &std::path::Path) -> std::path::PathBuf {
    let p = std::path::Path::new(header_path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_root.join(header_path)
    }
}

/// Parse a cache source string and push a `LoadedModule` into `modules`.
fn load_cache_source(
    source: &str,
    path: &std::path::Path,
    lib: &str,
    diags: &mut Vec<Diagnostic>,
    modules: &mut Vec<LoadedModule>,
) {
    let (toks, lex_diags) = crate::Lexer::lex(source);
    if !lex_diags.is_empty() {
        diags.extend(lex_diags);
        return;
    }
    let mut prog = match crate::Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => {
            diags.extend(ds);
            return;
        }
    };
    let display = path.display().to_string();
    modules.push(LoadedModule {
        path: path.to_path_buf(),
        display,
        source: source.to_string(),
        alias: format!("__c_cache_{lib}"),
        imports: std::mem::take(&mut prog.imports),
        items: std::mem::take(&mut prog.items),
        web_target_ceiling: prog.web_target_ceiling,
        pub_file: prog.pub_file,
        no_prelude: prog.no_prelude,
        html_path: prog.html_path.clone(),
        no_alloc_policy: prog.no_alloc_policy,
    });
}

/// Resolve link flags for one C library (S59/D-CFFI2). Order:
///   1. A declared `<lib>: c@…` dep in the `deps: { … }` block of `pkg.jet`:
///      `c@system` → pkg-config (with a bare `-l <lib>` fallback when there is
///      no `.pc`, e.g. libc); `c@"<path>"` → local dir (`-L`/`-I`/`-l`).
///   2. Else `pkg-config <lib>` (an undeclared `use c.<lib>` keeps this path).
///   3. Else E3201.
pub fn resolve_link(lib: &str, project_root: &Path) -> Result<LinkFlags, Diagnostic> {
    if let Some(actual) = lib.strip_prefix("jet_go_") {
        let archive = project_root
            .join(Syntax::SOURCE_ROOT_DIR)
            .join(ForeignLanguage::Go.bindings_subdir())
            .join(format!("libjet_go_{actual}.a"));
        if archive.is_file() {
            return Ok(LinkFlags {
                lib_dirs: vec![archive.parent().unwrap_or(project_root).display().to_string()],
                link_names: vec![format!("static=jet_go_{actual}"), "pthread".into(), "dl".into(), "m".into()],
                ..Default::default()
            });
        }
        return Err(e3201(lib));
    }
    if let Some(actual) = lib.strip_prefix("jet_java_") {
        let dir = project_root.join(Syntax::SOURCE_ROOT_DIR).join(ForeignLanguage::Java.bindings_subdir());
        let archive = dir.join(format!("libjet_java_{actual}.a"));
        let path_file = dir.join(format!("{actual}.jvm-path"));
        let Ok(jvm_dir) = std::fs::read_to_string(path_file) else { return Err(e3201(lib)); };
        let jvm_dir = jvm_dir.trim();
        if archive.is_file() && std::path::Path::new(jvm_dir).is_absolute() && std::path::Path::new(jvm_dir).join(if cfg!(target_os="macos") { "libjvm.dylib" } else { "libjvm.so" }).is_file() {
            return Ok(LinkFlags {
                lib_dirs: vec![dir.display().to_string(), jvm_dir.to_string()],
                link_names: vec![format!("static=jet_java_{actual}"), "jvm".into(), "pthread".into(), "dl".into()],
                rpath_dirs: vec![jvm_dir.to_string()],
                ..Default::default()
            });
        }
        return Err(e3201(lib));
    }
    if let Some(actual)=lib.strip_prefix("jet_tcl_") {
        let dir=project_root.join(Syntax::SOURCE_ROOT_DIR).join(ForeignLanguage::Tcl.bindings_subdir());
        let archive=dir.join(format!("libjet_tcl_{actual}.a")); let path_file=dir.join(format!("{actual}.tcl-path"));
        let Ok(tcl_dir)=std::fs::read_to_string(path_file) else{return Err(e3201(lib))}; let tcl_dir=tcl_dir.trim();
        if archive.is_file()&&std::path::Path::new(tcl_dir).is_absolute()&&std::path::Path::new(tcl_dir).join(if cfg!(target_os="macos"){"libtcl.dylib"}else{"libtcl.so"}).is_file(){return Ok(LinkFlags{lib_dirs:vec![dir.display().to_string(),tcl_dir.into()],link_names:vec![format!("static=jet_tcl_{actual}"),"tcl".into(),"pthread".into(),"dl".into()],rpath_dirs:vec![tcl_dir.into()],..Default::default()})}
        return Err(e3201(lib));
    }
    if let Some(actual)=lib.strip_prefix("jet_ada_") {
        let dir=project_root.join(Syntax::SOURCE_ROOT_DIR).join(ForeignLanguage::Ada.bindings_subdir());
        let archive=dir.join(format!("libjet_ada_{actual}.a"));let path_file=dir.join(format!("{actual}.ada-path"));
        let Ok(runtime_dir)=std::fs::read_to_string(path_file) else{return Err(e3201(lib))};let runtime_dir=runtime_dir.trim();
        if archive.is_file()&&std::path::Path::new(runtime_dir).is_absolute()&&std::path::Path::new(runtime_dir).join(if cfg!(target_os="macos"){"libgnat.dylib"}else{"libgnat.so"}).is_file(){return Ok(LinkFlags{lib_dirs:vec![dir.display().to_string(),runtime_dir.into()],link_names:vec![format!("static=jet_ada_{actual}"),"gnat".into(),"pthread".into(),"dl".into(),"m".into()],rpath_dirs:vec![runtime_dir.into()],..Default::default()})}
        return Err(e3201(lib));
    }
    if let Some(actual)=lib.strip_prefix("jet_pascal_") {
        let dir=project_root.join(Syntax::SOURCE_ROOT_DIR).join(ForeignLanguage::Pascal.bindings_subdir());
        let archive=dir.join(format!("libjet_pascal_{actual}.a"));let runtime=dir.join(format!("libjet_pascal_{actual}_runtime{}",if cfg!(target_os="macos"){".dylib"}else if cfg!(target_os="windows"){".dll"}else{".so"}));
        if archive.is_file()&&runtime.is_file(){let path=dir.display().to_string();return Ok(LinkFlags{lib_dirs:vec![path.clone()],link_names:vec![format!("static=jet_pascal_{actual}"),format!("dylib=jet_pascal_{actual}_runtime"),"pthread".into(),"dl".into(),"m".into()],rpath_dirs:vec![path],..Default::default()})}
        return Err(e3201(lib));
    }
    if let Some(actual)=lib.strip_prefix("jet_dart_") {
        let dir=project_root.join(Syntax::SOURCE_ROOT_DIR).join(ForeignLanguage::Dart.bindings_subdir());
        let archive=dir.join(format!("libjet_dart_{actual}.a"));
        if archive.is_file(){return Ok(LinkFlags{lib_dirs:vec![dir.display().to_string()],link_names:vec![format!("static=jet_dart_{actual}")],..Default::default()})}
        return Err(e3201(lib));
    }
    if let Some(actual)=lib.strip_prefix("jet_pwsh_") {
        let dir=project_root.join(Syntax::SOURCE_ROOT_DIR).join(ForeignLanguage::PowerShell.bindings_subdir());
        let archive=dir.join(format!("libjet_pwsh_{actual}.a"));
        if archive.is_file(){return Ok(LinkFlags{lib_dirs:vec![dir.display().to_string()],link_names:vec![format!("static=jet_pwsh_{actual}"),"pthread".into()],..Default::default()})}
        return Err(e3201(lib));
    }
    if let Some(actual)=lib.strip_prefix("jet_com_") {
        let dir=project_root.join(Syntax::SOURCE_ROOT_DIR).join(ForeignLanguage::Com.bindings_subdir());
        let archive=dir.join(format!("libjet_com_{actual}.a"));
        if cfg!(target_os="windows")&&archive.is_file(){return Ok(LinkFlags{lib_dirs:vec![dir.display().to_string()],link_names:vec![format!("static=jet_com_{actual}"),"ole32".into(),"oleaut32".into()],..Default::default()})}
        return Err(e3201(lib));
    }
    if let Some(actual) = lib.strip_prefix("jet_fortran_") {
        let archive = project_root
            .join(Syntax::SOURCE_ROOT_DIR)
            .join(ForeignLanguage::Fortran.bindings_subdir())
            .join(format!("libjet_fortran_{actual}.a"));
        if archive.is_file() {
            return Ok(LinkFlags {
                lib_dirs: vec![archive.parent().unwrap_or(project_root).display().to_string()],
                link_names: vec![format!("static=jet_fortran_{actual}")],
                ..Default::default()
            });
        }
        return Err(e3201(lib));
    }
    // 1. A declared `<lib>: c@…` dep in the manifest's `deps:` block.
    if let Some(target) = declared_c_dep(lib, project_root) {
        return clib_link(lib, &target, project_root);
    }
    // 2. pkg-config fallback (undeclared `use c.<lib>`).
    match pkg_config_link(lib) {
        PkgConfig::Found(flags) => Ok(flags),
        PkgConfig::NotFound | PkgConfig::Unavailable => Err(e3201(lib)),
    }
}

/// Look up a declared `<lib>: c@<target>` dep in the package manifest at
/// `project_root`, returning its target (`"system"` or a local path) when
/// present. Uses the real PackageManifest parser — the same one that produces
/// `pm.deps` — not an ad-hoc reader.
fn declared_c_dep(lib: &str, project_root: &Path) -> Option<String> {
    use crate::PackageManifest::{DepSource, PackManifest};
    let pm: PackManifest = PackManifest::load(project_root)?.ok()?;
    pm.deps.into_iter().find_map(|dep| {
        if dep.name != lib {
            return None;
        }
        match dep.source {
            DepSource::CLib { target } => Some(target),
            _ => None,
        }
    })
}

/// Libc-family names that the platform always links and that have no `.pc`
/// file: a bare `-l <lib>` on the default search path is correct for these.
const ALWAYS_LINKED_LIBC: &[&str] = &["c", "m", "pthread", "dl", "rt", "util"];

/// Resolve a declared `c@<target>` dep into link flags (S59/D-CFFI2).
///   - `target == "system"` → (a) `pkg-config <lib>`; else (b) a bare `-l <lib>`
///     for the always-linked libc set (no `.pc`); else (c) auto-provision
///     `nixpkgs#<lib>`; else (d) E3201.
///   - `target` of the form `nixpkgs:<attr>` → always auto-provision
///     `nixpkgs#<attr>` (the nix attr may differ from the link name).
///   - any other `target` → a local dir: `-L <path>`, `-I <path>`, `-l <lib>`.
fn clib_link(lib: &str, target: &str, project_root: &Path) -> Result<LinkFlags, Diagnostic> {
    // Explicit nixpkgs attr: `c@nixpkgs:<attr>`.
    if let Some(attr) = target.strip_prefix(NIXPKGS_TARGET_PREFIX) {
        return provision_from_nixpkgs(lib, attr, project_root);
    }

    if target == Syntax::SYSTEM_LIB_TARGET {
        // (a) host pkg-config.
        if let PkgConfig::Found(flags) = pkg_config_link(lib) {
            return Ok(flags);
        }
        // (b) always-linked libc: a bare `-l <lib>` is the declared intent.
        if ALWAYS_LINKED_LIBC.contains(&lib) {
            return Ok(LinkFlags {
                link_names: vec![lib.to_string()],
                ..Default::default()
            });
        }
        // (c) auto-provision from nixpkgs (attr == link name).
        return provision_from_nixpkgs(lib, lib, project_root);
    }

    // Local dir: link from `<path>` with the lib's own name.
    Ok(LinkFlags {
        include_dirs: vec![target.to_string()],
        lib_dirs: vec![target.to_string()],
        link_names: vec![lib.to_string()],
        rpath_dirs: Vec::new(),
    })
}

/// `target` prefix selecting an explicit nixpkgs attribute for a C dep.
const NIXPKGS_TARGET_PREFIX: &str = "nixpkgs:";

/// Realize `nixpkgs#<attr>` to a store path and build link flags from it.
/// The store path is cached at `<root>/.jet/clinks/<lib>` so repeat builds skip
/// the nix call; a stale/missing cached path re-realizes.
fn provision_from_nixpkgs(
    lib: &str,
    attr: &str,
    project_root: &Path,
) -> Result<LinkFlags, Diagnostic> {
    let store = match cached_store_path(lib, project_root) {
        Some(p) => p,
        None => {
            let p = realize_nixpkgs(attr).map_err(|reason| e3210(lib, attr, &reason))?;
            write_cached_store_path(lib, &p, project_root);
            p
        }
    };
    let lib_dir = format!("{store}/lib");
    let include_dir = format!("{store}/include");
    Ok(LinkFlags {
        include_dirs: vec![include_dir],
        lib_dirs: vec![lib_dir.clone()],
        link_names: vec![lib.to_string()],
        rpath_dirs: vec![lib_dir],
    })
}

/// `<root>/.jet/clinks/<lib>` — the per-lib resolved-store-path cache file.
fn clink_cache_file(lib: &str, project_root: &Path) -> std::path::PathBuf {
    project_root
        .join(Syntax::SOURCE_ROOT_DIR)
        .join("clinks")
        .join(lib)
}

/// Read a cached store path if present and still a valid store dir.
fn cached_store_path(lib: &str, project_root: &Path) -> Option<String> {
    let f = clink_cache_file(lib, project_root);
    let p = std::fs::read_to_string(&f).ok()?;
    let p = p.trim().to_string();
    if !p.is_empty() && Path::new(&p).is_dir() {
        Some(p)
    } else {
        None
    }
}

/// Persist a resolved store path for next time (best-effort).
fn write_cached_store_path(lib: &str, store: &str, project_root: &Path) {
    let f = clink_cache_file(lib, project_root);
    if let Some(parent) = f.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&f, store);
}

/// Run `nix build --no-link --print-out-paths nixpkgs#<attr>`; return the store
/// path (first line of stdout). On any failure return a trimmed reason string.
fn realize_nixpkgs(attr: &str) -> Result<String, String> {
    let flake = format!("nixpkgs#{attr}");
    let out = std::process::Command::new("nix")
        .args(["build", "--no-link", "--print-out-paths"])
        .arg(&flake)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "`nix` is not installed".to_string()
            } else {
                e.to_string()
            }
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr
            .trim()
            .lines()
            .last()
            .unwrap_or("nix build failed")
            .to_string();
        return Err(reason);
    }
    let path = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if path.is_empty() {
        return Err("nix build produced no store path".to_string());
    }
    Ok(path)
}

/// E3210 — a declared C dep could not be auto-provisioned from nixpkgs.
pub fn e3210(lib: &str, attr: &str, reason: &str) -> Diagnostic {
    Diagnostic::error(
        "E3210",
        format!("Couldn't fetch C library `{}` from nixpkgs.", lib),
        format!(
            "`{lib}: {}@{}` asked Jet to provision `nixpkgs#{attr}`, but `nix build` failed: {reason}",
            Syntax::DEP_PROVIDER_C, Syntax::SYSTEM_LIB_TARGET,
        ),
        format!(
            "Check the attr exists (`nix build nixpkgs#{attr}`), or point at a local build with `{lib}: {}@\"<path>\"`, or install it and use `{}`.",
            Syntax::DEP_PROVIDER_C, Syntax::SYSTEM_LIB_TARGET,
        ),
        None,
    )
}

/// Native-library link inputs for rustc (`-l`, `-L`, `-I` analog via cc args).
#[derive(Debug, Clone, Default)]
pub struct LinkFlags {
    pub include_dirs: Vec<String>,
    pub lib_dirs: Vec<String>,
    /// Names passed to `-l` (e.g. `raylib`).
    pub link_names: Vec<String>,
    /// Dirs to bake into the binary's runtime search path (`-Wl,-rpath`), so a
    /// shared library realized into `/nix/store` is found at run time with no
    /// `LD_LIBRARY_PATH`.
    pub rpath_dirs: Vec<String>,
}

/// E3201 — C library not found via a declared `c@…` dep or pkg-config.
fn e3201(lib: &str) -> Diagnostic {
    Diagnostic::error(
        "E3201",
        format!("C library `{}` was not found.", lib),
        format!(
            "Jet looked for a `{lib}: {}@…` dep in `{}`, then tried `pkg-config {lib}` on the system; neither provided include/link paths.",
            Syntax::DEP_PROVIDER_C, Syntax::PAYLOAD_FILE,
        ),
        format!(
            "Install the system package (e.g. `pacman -S {lib}`), or declare it as `{lib}: {}@{}` in `deps:`.",
            Syntax::DEP_PROVIDER_C, Syntax::SYSTEM_LIB_TARGET,
        ),
        None,
    )
}

/// E3209 — the linker could not find a C library at link time. Distinct from
/// E3201 (resolution found no paths up front): here resolution produced a
/// `-l<name>` but the library is not on the link search path. Kept off the I2
/// ICE banner — a missing system library is a user/system problem, not
/// generated code being rejected by rustc.
pub fn e3209(lib: &str) -> Diagnostic {
    Diagnostic::error(
        "E3209",
        format!("The linker couldn't find C library `{}`.", lib),
        format!(
            "Your program links against `{lib}`, but the linker reported `cannot find -l{lib}` — the library isn't on the link search path.",
        ),
        format!(
            "Declare it in `deps:` so Jet provisions it: `{lib}: {}@{}` (host pkg-config, else fetched from nixpkgs), or `{lib}: {}@nixpkgs:<attr>` to pick the nixpkgs attribute, or install the system package.",
            Syntax::DEP_PROVIDER_C, Syntax::SYSTEM_LIB_TARGET,
            Syntax::DEP_PROVIDER_C,
        ),
        None,
    )
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
