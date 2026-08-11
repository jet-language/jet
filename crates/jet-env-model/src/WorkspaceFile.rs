//! Declaration-resolved workspace evaluator (D-WORKSPACE1=B, D-WORKSPACE2=A).
//!
//! Parses and evaluates the canonical `workspace.jet` index declaration
//! `module workspace { members: <expr> }`. Arbitrary top-level authority
//! declarations are resolved separately for policy and boundary purposes; they
//! do not supply workspace members. The index `members:` expression may be:
//!   - `find("./packages")` — discovers package directories under the path
//!   - A list literal of strings: `["./pkg/a", "./pkg/b"]`
//!   - Any comptime expression that evaluates to a `[String]`
//!
//! The result is a `WorkspacePlan` listing the member packages with their
//! names (read from each member's `package.jet`, with `pkg.jet` retained only
//! as an explicit migration fallback) and relative paths.
//!
//! This replaces the `[packages]` table in `jetpack.toml` (D-WORKSPACE1=B
//! clean break: one canonical declaration is the sole index).
//!
//! Diagnostics:
//!   E0995 — the file has no `module workspace { … }` body
//!   E0996 — `members:` evaluated to something other than a list of strings
//!   E0997 — `find("…")` in `members:` points at a missing directory

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::Overlay;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{ComptimeInput, Expr, Func, Item, StrPart};
use jet_pkg_model::Package::PackageFacts;

// Re-export types so callers can use `jet_env_model::WorkspaceFile::WorkspacePlan` etc.
pub use jet_pkg_model::WorkspacePlan::{
    resolve_workspace_source, WorkspaceMember, WorkspacePlan, WorkspaceSource,
    WorkspaceSourceRole,
};

// ──────────────────────────────────────────────
// Load / evaluate
// ──────────────────────────────────────────────

/// Load and evaluate the canonical workspace index from `dir`. Returns `None`
/// when no file declares an index (an authority-only source is not an index).
/// Returns `Err(diagnostic)` when a declaring file exists but is malformed.
///
/// D-JPK-FILENAME2=B (A2): `module workspace { … }` is discovered by
/// declaration and may live in any top-level `.jet` file. The shared resolver
/// owns candidate scanning, ambiguity, and source I/O diagnostics.
pub fn load(dir: &Path) -> Option<Result<WorkspacePlan, Diagnostic>> {
    let source = resolve_workspace_source(dir)?;
    Some(match source {
        Ok(source) => match evaluate_source(&source.source, dir, source.role) {
            Ok(plan) if source.role == WorkspaceSourceRole::Index => Ok(plan),
            Ok(_) => return None,
            Err(diagnostic) => Err(diagnostic),
        },
        Err(diagnostic) => Err(diagnostic),
    })
}

/// Return whether a workspace source declares the optional top-level build
/// authority.  This is a discovery helper for the CLI; full diagnostics and
/// execution still go through the normal Driver loader and sema pass.
pub fn has_build_entry(src: &str) -> bool {
    // Workspace policy is an evaluated overlay, not a Jet item. Keep this
    // discovery probe on the same source shape as `evaluate`, otherwise a
    // valid workspace build entry disappears as soon as policy is present.
    let source = Overlay::strip_overlay_policy(src);
    let (tokens, lex_diags) = crate::Lexer::lex(&source);
    if !lex_diags.is_empty() {
        return false;
    }
    crate::Parser::parse(&tokens)
        .map(|program| {
            program.items.iter().any(|item| {
                matches!(item, Item::Func(func) if func.name == "build")
            })
        })
        .unwrap_or(false)
}

/// Evaluate a resolved workspace source string to a `WorkspacePlan`.
pub fn evaluate(src: &str, base_dir: &Path) -> Result<WorkspacePlan, Diagnostic> {
    evaluate_source(src, base_dir, WorkspaceSourceRole::Index)
}

/// Evaluate a resolved source according to its classified role. Arbitrary
/// authority modules contribute policy and a boundary only; they never become
/// the D-WORKSPACE2 member index.
pub fn evaluate_source(
    src: &str,
    base_dir: &Path,
    role: WorkspaceSourceRole,
) -> Result<WorkspacePlan, Diagnostic> {
    evaluate_with_role(src, base_dir, role)
}

fn evaluate_with_role(
    src: &str,
    base_dir: &Path,
    role: WorkspaceSourceRole,
) -> Result<WorkspacePlan, Diagnostic> {
    // `members:` may call comptime helpers; Canvas/tests invoke `load` outside
    // `jet`/`jetpack` mains that normally install this bridge.
    jet_codegen::Codegen::TIR::install_comptime_bridge();
    let overlay_policy = Overlay::parse_workspace_policy(src).map_err(|e| {
        let unsupported_policy = matches!(
            &e,
            Overlay::OverlayError::UnsupportedPolicy(_)
        );
        Diagnostic::error(
            if unsupported_policy { "E3503" } else { "E0998" },
            if unsupported_policy {
                "workspace build policy contains an unsupported field".to_string()
            } else {
                "workspace overlay policy is malformed".to_string()
            },
            e.message().to_string(),
            if unsupported_policy {
                "use `policy: .{ deny: #(…) }` or subject-scoped `policy: .{ grants: .{ \"package\": #(…) } }`".to_string()
            } else {
                "write `overlay <name> { provider: Provider.nixpkgs(channel: \"...\"); package(\"pkg\").patches += [patch(\"path.patch\")] }`".to_string()
            },
            None,
        )
    })?;
    let eval_src = Overlay::strip_overlay_policy(src);
    let (toks, lex_diags) = crate::Lexer::lex(&eval_src);
    if let Some(d) = lex_diags.into_iter().next() {
        return Err(d);
    }
    let program = crate::Parser::parse(&toks).map_err(|mut diags| {
        diags.pop().unwrap_or_else(|| {
            Diagnostic::error(
                "E0000",
                "parse failed".into(),
                String::new(),
                String::new(),
                None,
            )
        })
    })?;

    // Find `module workspace { … }`. An authority-only source has no required
    // `members:` field, so policy stripping may leave it as an ordinary code
    // module (`module workspace {}`); that still proves the declaration was
    // syntactically present without turning it into an index.
    let ws_module = program
        .items
        .iter()
        .find_map(|item| {
            if let Item::Module(m) = item {
                if m.name == Syntax::NS_WORKSPACE && m.is_auto_discovered() {
                    return Some(m);
                }
            }
            None
        });
    let code_workspace = program.items.iter().any(|item| {
        matches!(
            item,
            Item::CodeModule(module)
                if module.name == Syntax::NS_WORKSPACE && module.body.is_some()
        )
    });
    let workspace_declarations = program
        .items
        .iter()
        .filter(|item| match item {
            Item::Module(module) => {
                module.name == Syntax::NS_WORKSPACE && module.is_auto_discovered()
            }
            Item::CodeModule(module) => module.name == Syntax::NS_WORKSPACE,
            _ => false,
        })
        .count();

    if role == WorkspaceSourceRole::Authority {
        if workspace_declarations != 1 || (ws_module.is_none() && !code_workspace) {
            return Err(jet_pkg_model::WorkspacePlan::e0995_no_workspace_module());
        }
        return Ok(WorkspacePlan {
            members: Vec::new(),
            comptime_inputs: Vec::new(),
            overlay_policy,
            source_digest: jet_pkg_model::SHA256::sha256_hex(src.as_bytes()),
        });
    }

    if workspace_declarations != 1 {
        return Err(jet_pkg_model::WorkspacePlan::e0995_no_workspace_module());
    }
    let ws_module = ws_module
        .filter(|module| !module.members.is_empty())
        .ok_or_else(jet_pkg_model::WorkspacePlan::e0995_no_workspace_module)?;

    // Build the comptime context a `members:` expression evaluates against:
    // every top-level `fn` (callable) and every top-level `const`/`comptime`
    // binding (referenceable), evaluated in source order so a later binding can
    // build on an earlier one. This is what lets `members:` compose a sibling
    // `$packages :: […]` or call a local helper `fn`, rather than being
    // limited to inline literals + the `find("./dir")` fast path.
    let funcs: HashMap<String, &Func> = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Func(f) => Some((f.name.clone(), f)),
            _ => None,
        })
        .collect();
    let extern_names: HashSet<String> = HashSet::new();
    let mut globals: HashMap<String, crate::Comptime::CtValue> = HashMap::new();
    for item in &program.items {
        if let Item::Const(c) = item {
            let v = crate::Comptime::evaluate(&c.value, &funcs, &extern_names, base_dir, &globals)?;
            globals.insert(c.name.clone(), v);
        }
    }

    // Evaluate each `members:` expression (typically just one).
    let mut members = Vec::new();
    let mut comptime_inputs = Vec::new();
    for expr in &ws_module.members {
        let (paths, inputs) =
            eval_members_expr(expr, src, base_dir, &funcs, &extern_names, &globals)?;
        comptime_inputs.extend(inputs);
        for (rel_path, span) in paths {
            let (name, canonical_path) =
                validate_member_path(&rel_path, base_dir, &members, Some(span))?;
            let member = resolve_member(&rel_path, name, canonical_path);
            members.push(member);
        }
    }

    Ok(WorkspacePlan {
        members,
        comptime_inputs,
        overlay_policy,
        source_digest: jet_pkg_model::SHA256::sha256_hex(src.as_bytes()),
    })
}

/// D-ECO membership law: member paths are physical identities inside the
/// root, names are unique, and a member cannot introduce another member list.
fn validate_member_path(
    rel_path: &str,
    base_dir: &Path,
    members: &[WorkspaceMember],
    span: Option<Span>,
) -> Result<(String, String), Diagnostic> {
    let raw = Path::new(rel_path);
    if raw.is_absolute()
        || raw
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(Diagnostic::error(
            "E1322",
            format!("workspace member `{rel_path}` escapes the workspace root"),
            "Package membership is rooted in the workspace and cannot follow an absolute or `..` path".to_string(),
            "use a relative member path below the workspace root, or use `find(\"./packages\")`".to_string(),
            span,
        ));
    }
    let abs = base_dir.join(raw);
    if !abs.is_dir() || package_file(&abs).is_none() {
        return Err(Diagnostic::error(
            "E1334",
            format!("workspace member `{rel_path}` is not a Package directory"),
            "an explicit workspace member must exist and contain `package.jet` or the migration-era `pkg.jet`".to_string(),
            "create the Package file, correct the member path, or use `find(\"./packages\")` for discovery".to_string(),
            span,
        ));
    }
    let root = std::fs::canonicalize(base_dir).map_err(|error| {
        Diagnostic::error(
            "E1322",
            format!("couldn't resolve the workspace root: {error}"),
            "workspace membership uses real directory identity, so the workspace root must be canonicalizable".to_string(),
            "fix the workspace root permissions or path before declaring members".to_string(),
            span,
        )
    })?;
    let real = std::fs::canonicalize(&abs).map_err(|error| {
        Diagnostic::error(
            "E1334",
            format!("couldn't resolve workspace member `{rel_path}`: {error}"),
            "a member's physical identity must be known before the workspace accepts it".to_string(),
            "fix the member path or its symlink and try again".to_string(),
            span,
        )
    })?;
    if !real.starts_with(&root) {
        return Err(Diagnostic::error(
            "E1322",
            format!("workspace member `{rel_path}` resolves outside the workspace root"),
            "member identity follows the real path, including symlinks; an escaping target is not a workspace member".to_string(),
            "move the member under the workspace root or remove the escaping symlink".to_string(),
            span,
        ));
    }
    for member in members {
        let existing = std::fs::canonicalize(base_dir.join(&member.path)).map_err(|error| {
            Diagnostic::error(
                "E1324",
                format!("couldn't resolve existing workspace member `{}`: {error}", member.path),
                "duplicate detection uses physical member identity and cannot ignore an unresolved existing path".to_string(),
                "fix the earlier member path before adding another member".to_string(),
                span,
            )
        })?;
        if existing == real {
            return Err(Diagnostic::error(
                "E1324",
                format!("workspace member `{rel_path}` has the same physical identity as another member"),
                "a workspace member is identified by its real directory, not by two spelling variants".to_string(),
                "keep one member path for this directory".to_string(),
                span,
            ));
        }
    }
    let path = package_file(&abs).ok_or_else(|| {
        Diagnostic::error(
            "E1334",
            format!("workspace member `{rel_path}` Package metadata disappeared"),
            "workspace membership must validate the complete member manifest before accepting it".to_string(),
            "restore the member manifest and try again".to_string(),
            span,
        )
    })?;
    let is_package_file = path.file_name().and_then(|name| name.to_str())
        == Some(Syntax::PACKAGE_FILE);
    // Diagnostics use the same physical identity as membership validation.
    // This removes harmless `./` spelling from the reported manifest path.
    let path = path.canonicalize().map_err(|error| {
        Diagnostic::error(
            "E1334",
            format!("couldn't resolve workspace member `{rel_path}` Package file: {error}"),
            "workspace membership must know the real Package file before accepting it".to_string(),
            "fix the member manifest path or its symlink and try again".to_string(),
            span,
        )
    })?;
    if !path.starts_with(&root) {
        return Err(Diagnostic::error(
            "E1322",
            format!("workspace member `{rel_path}` Package file resolves outside the workspace root"),
            "member metadata follows the real path, including symlinks; an escaping target is not a workspace member".to_string(),
            "move the Package file under the workspace root or remove the escaping symlink".to_string(),
            span,
        ));
    }
    let metadata = std::fs::metadata(&path).map_err(|error| {
        Diagnostic::error(
            "E1334",
            format!("couldn't inspect workspace member `{rel_path}` Package file: {error}"),
            "workspace membership must validate a regular Package file before accepting it".to_string(),
            "restore the member manifest and try again".to_string(),
            span,
        )
    })?;
    if !metadata.is_file() {
        return Err(Diagnostic::error(
            "E1334",
            format!("workspace member `{rel_path}` Package metadata is not a regular file"),
            "workspace membership cannot read a directory or special file as Package metadata".to_string(),
            "replace the member metadata with a regular `package.jet` or `pkg.jet` file".to_string(),
            span,
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Diagnostic::error(
            "E1334",
            format!("couldn't read workspace member `{rel_path}` Package file: {error}"),
            "workspace membership must validate the complete member manifest before accepting it".to_string(),
            "fix the member manifest permissions or contents and try again".to_string(),
            span,
        )
    })?;
    let (has_members, name) = if is_package_file {
        match PackageFacts::parse_uncomposed(&text, path.display().to_string()) {
            Ok(facts) => (!facts.members.is_empty(), facts.name),
            Err(error) => {
                return Err(Diagnostic::error(
                    "E1334",
                    format!("workspace member `{rel_path}` has an invalid Package file"),
                    error.to_string(),
                    "fix the member's `package.jet` fields before adding it to the workspace".to_string(),
                    span,
                ));
            }
        }
    } else {
        let name = legacy_package_name(&text).ok_or_else(|| {
            Diagnostic::error(
                "E1334",
                format!("workspace member `{rel_path}` has an invalid Package file"),
                "the migration-era `pkg.jet` must contain a non-empty `name:` field".to_string(),
                "fix the member's `pkg.jet` fields before adding it to the workspace".to_string(),
                span,
            )
        })?;
        (
            text.lines().any(|line| line.trim_start().starts_with("members:")),
            name,
        )
    };
    if has_members {
        return Err(Diagnostic::error(
            "E1323",
            format!(
                "member package `{rel_path}` at `{}` declares `members`",
                path.display()
            ),
            format!(
                "Package membership has depth cap one: only the workspace root may list members; the declaration comes from `{}`",
                path.display()
            ),
            format!(
                "remove the inner `members:` field from `{}` and lift its references into the workspace root",
                path.display()
            ),
            span,
        ));
    }
    if members.iter().any(|member| member.name == name) {
        return Err(Diagnostic::error(
            "E1325",
            format!("workspace member name `{name}` is declared more than once"),
            "Package references use a stable name; two physical members cannot claim the same name".to_string(),
            "rename one package or remove the duplicate member reference".to_string(),
            span,
        ));
    }
    let canonical_path = real
        .strip_prefix(&root)
        .map_err(|_| {
            Diagnostic::error(
                "E1322",
                format!("workspace member `{rel_path}` has no relative identity"),
                "workspace membership must keep the member's physical identity below the workspace root".to_string(),
                "move the member under the workspace root or remove the escaping symlink".to_string(),
                span,
            )
        })?
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    let canonical_path = if canonical_path.is_empty() {
        ".".to_string()
    } else {
        canonical_path
    };
    Ok((name, canonical_path))
}

// ──────────────────────────────────────────────
// Expression evaluation
// ──────────────────────────────────────────────

/// Evaluate one `members:` expression to a list of relative package directory
/// paths plus any Tier-1 comptime inputs it recorded. Handles `find("./dir")`
/// specially (the common case, no comptime inputs); otherwise evaluates through
/// comptime against the workspace file's top-level `fn`s and `const`s.
fn eval_members_expr(
    expr: &Expr,
    _src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
) -> Result<(Vec<(String, Span)>, Vec<ComptimeInput>), Diagnostic> {
    // Fast path: workspace `find("./dir")` scans for package directories. This
    // manifest helper is separate from comptime `find(glob) -> [String]`, which
    // records matched file hashes for ordinary Jet comptime bindings.
    if let Expr::Call(call) = expr {
        if call.name == Syntax::BUILTIN_FIND {
            let span = call.name_span;
            let arg = call.args.first().ok_or_else(|| {
                Diagnostic::error(
                    "E0996",
                    format!("`find` in `members:` needs a path argument"),
                    "write `find(\"./packages\")` to discover all packages under that directory"
                        .to_string(),
                    "example: `members: find(\"./packages\")`".to_string(),
                    Some(span),
                )
            })?;
            if let Some(dir_str) = extract_literal_string(&arg.expr) {
                let scan_dir = validate_find_scan_dir(&dir_str, base_dir, span)?;
                let paths = find_package_dirs(&scan_dir, base_dir, span)?
                    .into_iter()
                    .map(|path| (path, arg.expr.span()))
                    .collect();
                return Ok((paths, Vec::new()));
            }
            // A non-literal `find` argument (e.g. `find(base + "/pkgs")`) is a
            // computed path: fall through to the comptime evaluator, which
            // resolves the argument against `globals`/`funcs` before scanning.
        }
    }

    // General case: evaluate through comptime, expect a list of strings, and
    // capture any Tier-1 inputs (`@embed`, `fetch`) the expression pulled in so
    // the workspace lock can record them (D-CTEFFECT1).
    let (v, inputs) = crate::Comptime::evaluate_with_imports_opts_collecting(
        expr,
        funcs,
        extern_names,
        base_dir,
        globals,
        &HashMap::new(),
        false,
        0,
        None,
    )?;
    let paths = extract_string_list(v, expr.span())?
        .into_iter()
        .map(|path| (path, expr.span()))
        .collect();
    Ok((paths, inputs))
}

/// Extract a plain string literal from an `Expr::Str` with no interpolation.
fn extract_literal_string(expr: &Expr) -> Option<String> {
    let Expr::Str(parts, _) = expr else {
        return None;
    };
    let mut s = String::new();
    for part in parts {
        match part {
            StrPart::Lit(lit) => s.push_str(lit),
            StrPart::Interp(..) => return None,
        }
    }
    Some(s)
}

/// Extract `[String]` from a `CtValue::List` of `CtValue::Str`. Any non-string
/// element or a non-list value is E0996.
fn extract_string_list(v: crate::Comptime::CtValue, span: Span) -> Result<Vec<String>, Diagnostic> {
    match v {
        crate::Comptime::CtValue::List(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                match x {
                    crate::Comptime::CtValue::Str(s) => out.push(s),
                    _ => {
                        return Err(Diagnostic::error(
                            "E0996",
                            "`members:` list must contain strings (package paths)".to_string(),
                            "each element should be a relative path to a package directory"
                                .to_string(),
                            "example: `members: [\"./packages/hello\", \"./packages/ranker\"]`"
                                .to_string(),
                            Some(span),
                        ))
                    }
                }
            }
            Ok(out)
        }
        _ => Err(Diagnostic::error(
            "E0996",
            "`members:` must evaluate to a list of package paths".to_string(),
            "The `members:` value must evaluate to a `[String]` — a list of relative package directory paths".to_string(),
            "example: `members: find(\"./packages\")` or `members: [\"./pkg/hello\"]`".to_string(),
            Some(span),
        )),
    }
}

// ──────────────────────────────────────────────
// find("./dir") — package discovery
// ──────────────────────────────────────────────

fn validate_find_scan_dir(
    raw: &str,
    workspace_root: &Path,
    span: Span,
) -> Result<PathBuf, Diagnostic> {
    let path = Path::new(raw);
    if raw.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(Diagnostic::error(
            "E1322",
            format!("`find` path `{raw}` escapes the workspace root"),
            "workspace discovery follows only relative paths below the workspace root; absolute and `..` paths are rejected".to_string(),
            "use a relative path such as `find(\"./packages\")`".to_string(),
            Some(span),
        ));
    }
    let root = workspace_root.canonicalize().map_err(|error| {
        Diagnostic::error(
            "E0997",
            format!("couldn't resolve the workspace root `{}`: {error}", workspace_root.display()),
            "workspace discovery uses real paths and cannot safely scan an unresolved root".to_string(),
            "fix the workspace root permissions or path before using `find`".to_string(),
            Some(span),
        )
    })?;
    let candidate = workspace_root.join(path.strip_prefix("./").unwrap_or(path));
    let canonical = candidate
        .canonicalize()
        .map_err(|_| e0997_find_dir_missing(&candidate, span))?;
    if !canonical.starts_with(&root) {
        return Err(Diagnostic::error(
            "E1322",
            format!("`find` path `{raw}` resolves outside the workspace root"),
            "workspace discovery follows real paths, including symlinks, and cannot scan outside the workspace".to_string(),
            "move the target below the workspace root or remove the escaping symlink".to_string(),
            Some(span),
        ));
    }
    Ok(canonical)
}

/// Scan `scan_dir` for immediate subdirectories containing `package.jet` or
/// the migration-era `pkg.jet`.
/// Returns paths relative to `workspace_root`, sorted for determinism.
fn find_package_dirs(
    scan_dir: &Path,
    workspace_root: &Path,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    let workspace_root = workspace_root.canonicalize().map_err(|error| {
        Diagnostic::error(
            "E0997",
            format!("couldn't resolve the workspace root `{}`: {error}", workspace_root.display()),
            "workspace discovery uses real paths and cannot safely report member paths from an unresolved root".to_string(),
            "fix the workspace root permissions or path before using `find`".to_string(),
            Some(span),
        )
    })?;
    let entries =
        std::fs::read_dir(scan_dir).map_err(|_| e0997_find_dir_missing(scan_dir, span))?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| e0997_find_dir_missing(scan_dir, span))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !has_package_file(&path) {
            continue;
        }
        found.push(path);
    }
    found.sort();
    // Make each path relative to the workspace root.
    let mut out = Vec::with_capacity(found.len());
    for abs in found {
        let rel = abs.strip_prefix(&workspace_root).map(|p| {
            // Normalise to forward-slash form even on Windows; `.jet/lock`
            // stores POSIX paths and platform joins handle them on read.
            p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
        });
        match rel {
            Ok(r) => out.push(r),
            Err(_) => out.push(abs.to_string_lossy().into_owned()),
        }
    }
    Ok(out)
}

// ──────────────────────────────────────────────
// Member resolution
// ──────────────────────────────────────────────

/// Resolve a member package after its metadata has been validated.
fn resolve_member(rel_path: &str, name: String, canonical_path: String) -> WorkspaceMember {
    WorkspaceMember {
        name,
        path: rel_path.to_string(),
        canonical_path,
    }
}

/// Read a package name from the migration-era `pkg.jet` text shape.
fn legacy_package_name(src: &str) -> Option<String> {
    // Fast heuristic: find `package: { name: "…" }` or `name: "…"`.
    // This avoids a full parse for the common case.
    for line in src.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("name:") {
            let val = rest.trim().trim_end_matches(',').trim().trim_matches('"');
            if !val.is_empty() && !val.contains('{') {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn package_file(dir: &Path) -> Option<PathBuf> {
    for name in [Syntax::PACKAGE_FILE, Syntax::PAYLOAD_FILE] {
        let path = dir.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => return Some(path),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Some(path),
            Err(_) => {}
        }
    }
    None
}

fn has_package_file(dir: &Path) -> bool {
    package_file(dir).is_some()
}

// ──────────────────────────────────────────────
// Diagnostics
// ──────────────────────────────────────────────

/// E0997: `find("…")` in `members:` points at a directory that doesn't exist.
fn e0997_find_dir_missing(dir: &Path, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0997",
        format!("`find` can't read the directory `{}`", dir.display()),
        "`find` scans that directory for subdirectories containing `package.jet`; \
         the directory must exist relative to `workspace.jet`"
            .to_string(),
        "create the directory, or fix the path so it points at your packages folder".to_string(),
        Some(span),
    )
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(src: &str) -> WorkspacePlan {
        evaluate(src, &std::env::temp_dir()).expect("evaluation should succeed")
    }

    fn eval_err(src: &str) -> Diagnostic {
        evaluate(src, &std::env::temp_dir()).expect_err("evaluation should fail")
    }

    #[test]
    fn empty_members_list() {
        let src = "module workspace {\n    members: []\n}\n";
        let plan = eval(src);
        assert!(plan.members.is_empty());
    }

    #[test]
    fn overlay_policy_survives_workspace_eval() {
        let src = r#"
module workspace {
    members: []
    overlay plasma_beta {
        provider: Provider.nixpkgs(channel: "plasma-beta")
        package("foo").patches += [patch("patches/foo.patch")]
    }
    policy.allowUnfree: ["discord"]
}
"#;
        let plan = eval(src);
        assert!(plan.members.is_empty());
        assert_eq!(plan.overlay_policy.allow_unfree, vec!["discord"]);
        assert_eq!(
            plan.overlay_policy
                .package_override("plasma_beta", "foo")
                .unwrap()
                .patches,
            vec!["patches/foo.patch"]
        );
    }

    #[test]
    fn explicit_string_list() {
        let tmp = tempdir("explicit-list");
        for (relative, name) in [("packages/hello", "hello"), ("packages/ranker", "ranker")] {
            let dir = tmp.join(relative);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(Syntax::PACKAGE_FILE), format!("name: \"{name}\"\n")).unwrap();
        }
        let src =
            "module workspace {\n    members: [\"./packages/hello\", \"./packages/ranker\"]\n}\n";
        let plan = evaluate(src, &tmp).unwrap();
        assert_eq!(plan.members.len(), 2);
        assert_eq!(plan.members[0].path, "./packages/hello");
        assert_eq!(plan.members[0].name, "hello");
        assert_eq!(plan.members[1].path, "./packages/ranker");
        assert_eq!(plan.members[1].name, "ranker");
        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn member_can_be_the_workspace_root() {
        let tmp = tempdir("member-root");
        std::fs::write(tmp.join(Syntax::PACKAGE_FILE), "name: \"root\"\n").unwrap();
        let plan = evaluate(
            "module workspace { members: [\".\"] }\n",
            &tmp,
        )
        .expect("the workspace root may be an explicit member");
        assert_eq!(plan.members.len(), 1);
        assert_eq!(plan.members[0].path, ".");
        assert_eq!(plan.members[0].name, "root");
        assert_eq!(plan.members[0].canonical_path, ".");
        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn member_paths_cannot_duplicate_one_physical_directory() {
        let tmp = tempdir("member-physical-duplicate");
        let package = tmp.join("packages/app");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join(Syntax::PACKAGE_FILE), "name: \"app\"\n").unwrap();
        let error = evaluate(
            "module workspace { members: [\"./packages/app\", \"./packages/app/.\"] }\n",
            &tmp,
        )
        .expect_err("two spellings cannot create two member identities");
        assert_eq!(error.code, "E1324");
        assert!(error.span.is_some());
        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn member_names_are_unique() {
        let tmp = tempdir("member-name-duplicate");
        for path in ["packages/one", "packages/two"] {
            let package = tmp.join(path);
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(package.join(Syntax::PACKAGE_FILE), "name: \"same\"\n").unwrap();
        }
        let error = evaluate(
            "module workspace { members: [\"./packages/one\", \"./packages/two\"] }\n",
            &tmp,
        )
        .expect_err("stable Package names must be unique");
        assert_eq!(error.code, "E1325");
        assert!(error.span.is_some());
        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn member_package_cannot_declare_nested_members() {
        let tmp = tempdir("member-nested");
        let package = tmp.join("packages/app");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join(Syntax::PACKAGE_FILE),
            "name: \"app\"\nmembers: [\"./child\"]\n",
        )
        .unwrap();
        let error = evaluate(
            "module workspace { members: [\"./packages/app\"] }\n",
            &tmp,
        )
        .expect_err("nested Package membership is not allowed");
        assert_eq!(error.code, "E1323");
        assert!(error
            .what
            .contains(&package.join(Syntax::PACKAGE_FILE).display().to_string()));
        assert!(error.span.is_some());
        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn malformed_member_metadata_is_not_replaced_by_path_name() {
        let tmp = tempdir("member-malformed-metadata");
        let package = tmp.join("packages/app");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join(Syntax::PAYLOAD_FILE), "not package metadata\n").unwrap();
        let error = evaluate(
            "module workspace { members: [\"./packages/app\"] }\n",
            &tmp,
        )
        .expect_err("malformed member metadata must be surfaced");
        assert_eq!(error.code, "E1334");
        std::fs::remove_dir_all(tmp).ok();
    }

    #[cfg(unix)]
    #[test]
    fn member_symlink_cannot_escape_workspace_root() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir("member-symlink-escape");
        let outside = tempdir("member-symlink-target");
        std::fs::write(outside.join(Syntax::PACKAGE_FILE), "name: \"outside\"\n").unwrap();
        std::fs::create_dir_all(tmp.join("packages")).unwrap();
        symlink(&outside, tmp.join("packages/escape")).unwrap();

        let error = evaluate(
            "module workspace { members: [\"./packages/escape\"] }\n",
            &tmp,
        )
        .expect_err("an escaping member symlink must be rejected");
        assert_eq!(error.code, "E1322");
        std::fs::remove_dir_all(tmp).ok();
        std::fs::remove_dir_all(outside).ok();
    }

    #[test]
    fn members_references_sibling_comptime_const() {
        // Slice A: a `members:` expression can name a top-level `comptime`
        // binding declared in the same file — not just inline literals.
        let src = "$pkgs :: [\"./packages/hello\", \"./packages/ranker\"]\n\
                   module workspace {\n    members: pkgs\n}\n";
        let tmp = tempdir("comptime-list");
        for (relative, name) in [("packages/hello", "hello"), ("packages/ranker", "ranker")] {
            let dir = tmp.join(relative);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(Syntax::PACKAGE_FILE), format!("name: \"{name}\"\n")).unwrap();
        }
        let plan = evaluate(src, &tmp).unwrap();
        let names: Vec<&str> = plan.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["hello", "ranker"]);
        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn members_composes_comptime_const_with_string_ops() {
        // A binding can be composed inside the list expression.
        let src = "$base :: \"./workspace-packages\"\n\
                   module workspace {\n    members: [\"{base}/a\", \"{base}/b\"]\n}\n";
        let tmp = tempdir("comptime-strings");
        for (relative, name) in [("workspace-packages/a", "a"), ("workspace-packages/b", "b")] {
            let dir = tmp.join(relative);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(Syntax::PACKAGE_FILE), format!("name: \"{name}\"\n")).unwrap();
        }
        let plan = evaluate(src, &tmp).unwrap();
        let paths: Vec<&str> = plan.members.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["./workspace-packages/a", "./workspace-packages/b"]);
        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn members_calls_top_level_fn() {
        // A `members:` expression can call a top-level helper `fn`.
        let src = "fn member(name: String) => String { return \"./pkgs/{name}\" }\n\
                   module workspace {\n    members: [member(\"hello\"), member(\"ranker\")]\n}\n";
        let tmp = tempdir("comptime-function");
        for (relative, name) in [("pkgs/hello", "hello"), ("pkgs/ranker", "ranker")] {
            let dir = tmp.join(relative);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(Syntax::PACKAGE_FILE), format!("name: \"{name}\"\n")).unwrap();
        }
        let plan = evaluate(src, &tmp).unwrap();
        let paths: Vec<&str> = plan.members.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["./pkgs/hello", "./pkgs/ranker"]);
        std::fs::remove_dir_all(tmp).ok();
    }

    #[test]
    fn e0995_no_workspace_module() {
        let src = "module dev { env.dev: Env.{ packages: [] } }\n";
        let d = eval_err(src);
        assert_eq!(d.code, "E0995");
    }

    #[test]
    fn e0996_members_not_a_list() {
        let src = "module workspace { members: 42 }\n";
        let d = eval_err(src);
        assert_eq!(d.code, "E0996");
    }

    #[test]
    fn e0996_members_list_non_string() {
        let src = "module workspace { members: [1, 2] }\n";
        let d = eval_err(src);
        assert_eq!(d.code, "E0996");
    }

    #[test]
    fn e0997_find_missing_dir() {
        let src = "module workspace { members: find(\"./no-such-packages\") }\n";
        let d = eval_err(src);
        assert_eq!(d.code, "E0997");
    }

    #[test]
    fn internal_workspace_module_is_skipped_by_discovery() {
        // An internal module doesn't count as the discovered workspace
        // declaration.
        let src = "module _workspace { members: [] }\n";
        let d = eval_err(src);
        assert_eq!(d.code, "E0995");
    }

    #[test]
    fn find_discovers_pkg_jet_directories() {
        let tmp = tempdir("workspace-find");
        // packages/hello/pkg.jet
        let hello = tmp.join("packages/hello");
        std::fs::create_dir_all(&hello).unwrap();
        std::fs::write(hello.join(Syntax::PAYLOAD_FILE), "name: \"hello\"\n").unwrap();
        // packages/ranker/pkg.jet
        let ranker = tmp.join("packages/ranker");
        std::fs::create_dir_all(&ranker).unwrap();
        std::fs::write(ranker.join(Syntax::PAYLOAD_FILE), "name: \"ranker\"\n").unwrap();
        // packages/lib (no pkg.jet — should be ignored)
        let lib = tmp.join("packages/lib");
        std::fs::create_dir_all(lib).unwrap();

        let src = "module workspace { members: find(\"./packages\") }\n";
        let plan = evaluate(src, &tmp).expect("should succeed");
        let names: Vec<&str> = plan.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["hello", "ranker"]);
        let paths: Vec<&str> = plan.members.iter().map(|m| m.path.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.contains("hello")),
            "paths: {paths:?}"
        );
        assert!(
            paths.iter().any(|p| p.contains("ranker")),
            "paths: {paths:?}"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[cfg(unix)]
    #[test]
    fn find_does_not_skip_unreadable_member_metadata() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir("workspace-find-bad-metadata");
        let package = tmp.join("packages/app");
        std::fs::create_dir_all(&package).unwrap();
        symlink("missing-package.jet", package.join(Syntax::PACKAGE_FILE)).unwrap();

        let error = evaluate(
            "module workspace { members: find(\"./packages\") }\n",
            &tmp,
        )
        .expect_err("find must surface unreadable member metadata");
        assert_eq!(error.code, "E1334");
        std::fs::remove_dir_all(tmp).ok();
    }

    #[cfg(unix)]
    #[test]
    fn workspace_metadata_must_be_regular_and_not_symlinked() {
        use std::os::unix::fs::symlink;

        let symlinked = tempdir("workspace-source-symlink");
        let target = symlinked.join("workspace-source.txt");
        std::fs::write(&target, "module workspace { members: [] }\n").unwrap();
        symlink(&target, symlinked.join(Syntax::WORKSPACE_FILE)).unwrap();
        let error = load(&symlinked)
            .expect("the symlinked metadata must be surfaced")
            .expect_err("symlinked workspace metadata must fail closed");
        assert_eq!(error.code, "E1239");
        std::fs::remove_dir_all(symlinked).ok();

        let nonregular = tempdir("workspace-source-directory");
        std::fs::create_dir(nonregular.join(Syntax::WORKSPACE_FILE)).unwrap();
        let error = load(&nonregular)
            .expect("the non-regular metadata must be surfaced")
            .expect_err("non-regular workspace metadata must fail closed");
        assert_eq!(error.code, "E1239");
        std::fs::remove_dir_all(nonregular).ok();
    }

    fn tempdir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("workspace-{tag}-{nanos}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// I5: a workspace.jet with find() discovers package members.
    #[test]
    fn committed_workspace_example_evaluates_clean() {
        let dir = tempdir("ws-example");
        let packages = dir.join("packages");
        let hello = packages.join("hello");
        let ranker = packages.join("ranker");
        std::fs::create_dir_all(&hello).unwrap();
        std::fs::create_dir_all(&ranker).unwrap();
        std::fs::write(hello.join("pkg.jet"), "name: \"hello\"\n").unwrap();
        std::fs::write(ranker.join("pkg.jet"), "name: \"ranker\"\n").unwrap();
        std::fs::write(
            dir.join(crate::Syntax::WORKSPACE_FILE),
            "module workspace {\n    members: find(\"./packages\")\n}\n",
        )
        .unwrap();

        let workspace_path = dir.join(crate::Syntax::WORKSPACE_FILE);
        let src = std::fs::read_to_string(&workspace_path).unwrap();
        let plan = evaluate(&src, &dir).expect("workspace fixture should evaluate clean");
        let names: Vec<&str> = plan.members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"ranker"));
    }

    // D-JPK-FILENAME2=B (A2): an arbitrary top-level declaration may provide
    // authority, while only `workspace.jet` provides the member index.

    #[test]
    fn workspace_module_discovered_in_arbitrary_filename() {
        let dir = tempdir("ws-arbitrary");
        std::fs::write(
            dir.join("repo-index.jet"),
            "module workspace { policy: .{ deny: #(Exec) } }\n",
        )
        .unwrap();
        assert!(load(&dir).is_none(), "authority metadata is not the index");

        // An arbitrary authority cannot repair a malformed reserved index.
        std::fs::write(dir.join(Syntax::WORKSPACE_FILE), "module env {}\n").unwrap();
        let diagnostic = load(&dir)
            .expect("the malformed canonical source must be surfaced")
            .expect_err("workspace.jet remains the strict index role");
        assert_eq!(diagnostic.code, "E0995");
    }

    #[test]
    fn malformed_arbitrary_workspace_declaration_is_not_absent() {
        let dir = tempdir("ws-arbitrary-malformed");
        std::fs::write(
            dir.join("repo-index.jet"),
            "module workspace { members: [ }\n",
        )
        .unwrap();
        assert!(load(&dir)
            .expect("a workspace declaration candidate must be surfaced")
            .is_err());
    }

    #[test]
    fn workspace_module_discovered_in_pkg_jet() {
        let dir = tempdir("ws-in-pkg");
        std::fs::write(
            dir.join(Syntax::PAYLOAD_FILE),
            "module workspace { policy: .{ deny: #(FS) } }\n",
        )
        .unwrap();
        assert!(load(&dir).is_none(), "authority metadata is not the index");
    }

    #[test]
    fn workspace_jet_and_discovered_file_are_e1239() {
        let dir = tempdir("ws-canonical-wins");
        let packages = dir.join("packages/hello");
        std::fs::create_dir_all(&packages).unwrap();
        std::fs::write(packages.join(Syntax::PAYLOAD_FILE), "name: \"hello\"\n").unwrap();
        std::fs::write(
            dir.join(Syntax::WORKSPACE_FILE),
            "module workspace { members: find(\"./packages\") }\n",
        )
        .unwrap();
        // A canonical filename never shadows another declaration.
        std::fs::write(
            dir.join("other.jet"),
            "module workspace { policy: .{ deny: #(FS) } }\n",
        )
        .unwrap();
        let diagnostic = load(&dir)
            .expect("declarations should be discovered")
            .expect_err("two declarations must remain ambiguous");
        assert_eq!(diagnostic.code, "E1239");
        assert!(diagnostic.what.contains(Syntax::WORKSPACE_FILE));
        assert!(diagnostic.what.contains("other.jet"));
    }

    #[test]
    fn two_discovered_workspace_declarations_are_e1239() {
        let dir = tempdir("ws-ambiguous");
        std::fs::write(
            dir.join("a.jet"),
            "module workspace { policy: .{ deny: #(FS) } }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("b.jet"),
            "module workspace { policy: .{ deny: #(FS) } }\n",
        )
        .unwrap();
        let d = load(&dir).expect("should be Some").expect_err("ambiguous");
        assert_eq!(d.code, "E1239");
        assert!(d.what.contains("a.jet") && d.what.contains("b.jet"));
    }

    #[test]
    fn no_workspace_anywhere_is_none() {
        let dir = tempdir("ws-none");
        std::fs::write(dir.join("env.jet"), "module env.dev { }\n").unwrap();
        assert!(load(&dir).is_none());
    }
}
