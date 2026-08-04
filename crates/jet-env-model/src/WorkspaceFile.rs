//! `workspace.jet` evaluator (D-WORKSPACE1=B, D-WORKSPACE2=A).
//!
//! Parses and evaluates `module workspace { members: <expr> }` from a
//! `workspace.jet` at the repo root. The `members:` expression may be:
//!   - `find("./packages")` — discovers package directories under the path
//!   - A list literal of strings: `["./pkg/a", "./pkg/b"]`
//!   - Any comptime expression that evaluates to a `[String]`
//!
//! The result is a `WorkspacePlan` listing the member packages with their
//! names (read from each member's `package.jet`, with `pkg.jet` retained only
//! as an explicit migration fallback) and relative paths.
//!
//! This replaces the `[packages]` table in `jetpack.toml` (D-WORKSPACE1=B
//! clean break: when `workspace.jet` is present, it is the sole index).
//!
//! Diagnostics:
//!   E0995 — the file has no `module workspace { … }` declaration
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
pub use jet_pkg_model::WorkspacePlan::{WorkspaceMember, WorkspacePlan};

// ──────────────────────────────────────────────
// Load / evaluate
// ──────────────────────────────────────────────

/// Load and evaluate the workspace index from `dir`. Returns `None` when no
/// file declares one (not an error — a plain project has no workspace).
/// Returns `Err(diagnostic)` when a declaring file exists but is malformed.
///
/// D-JPK-FILENAME2=B (A2): `workspace.jet` is a convention, not a reserved
/// name — `module workspace { … }` is discovered by declaration and may live
/// in `pkg.jet` or any top-level `.jet` file. `workspace.jet` is checked
/// first (the canonical home, cheap fast path); otherwise every top-level
/// `.jet` file (including `pkg.jet` — the one reserved filename) is scanned
/// for the declaration. Two files both declaring `module workspace` is E1239.
pub fn load(dir: &Path) -> Option<Result<WorkspacePlan, Diagnostic>> {
    let path = dir.join(Syntax::WORKSPACE_FILE);
    if path.exists() {
        let src = match std::fs::read_to_string(&path) {
            Ok(src) => src,
            Err(error) => {
                return Some(Err(Diagnostic::error(
                    "E1239",
                    format!("couldn't read `{}`", path.display()),
                    format!("the canonical workspace source is present but unavailable: {error}"),
                    "restore read access to `workspace.jet`; do not rely on a stale `.jet/lock`".to_string(),
                    None,
                )))
            }
        };
        return Some(evaluate(&src, dir));
    }
    // Discovery-by-declaration over the other top-level `.jet` files.
    let mut declaring: Vec<(PathBuf, String)> = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let p = entry.path();
        if p.extension().and_then(|x| x.to_str()) != Some(Syntax::FILE_EXT) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&p) else {
            continue;
        };
        if declares_workspace_module(&src) {
            declaring.push((p, src));
        }
    }
    match declaring.len() {
        0 => None,
        1 => {
            let (_, src) = declaring.remove(0);
            Some(evaluate(&src, dir))
        }
        _ => Some(Err(e1239_ambiguous_workspace(
            &declaring
                .iter()
                .map(|(p, _)| p.as_path())
                .collect::<Vec<_>>(),
        ))),
    }
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

/// Cheap token probe: does `src` declare a top-level, enabled
/// `module workspace { … }`? Full parse/eval happens only on the one match.
///
/// Must not call `Parser::parse` — discovery walks every ancestor directory
/// (including `/tmp` in tests) and full-parsing unrelated deep `.jet` files
/// overflows the default test-thread stack before `MAX_SOURCE_NESTING` fires.
fn declares_workspace_module(src: &str) -> bool {
    let (toks, _lex_diags) = crate::Lexer::lex(src);
    let toks = crate::Lexer::without_comments(&toks);
    let mut brace_depth = 0i32;
    let mut i = 0;
    while i + 1 < toks.len() {
        match &toks[i].kind {
            crate::Lexer::TokKind::LBrace => {
                brace_depth += 1;
                i += 1;
            }
            crate::Lexer::TokKind::RBrace => {
                brace_depth -= 1;
                i += 1;
            }
            crate::Lexer::TokKind::KwModule if brace_depth == 0 => {
                match &toks[i + 1].kind {
                    crate::Lexer::TokKind::Ident(name)
                        if name == Syntax::NS_WORKSPACE
                            && !name.starts_with(Syntax::MODULE_INTERNAL_PREFIX) =>
                    {
                        return true;
                    }
                    _ => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    false
}

/// E1239: two or more files declare `module workspace` — the index must be
/// unambiguous.
fn e1239_ambiguous_workspace(paths: &[&Path]) -> Diagnostic {
    let list = paths
        .iter()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string())
        })
        .collect::<Vec<_>>()
        .join("`, `");
    Diagnostic::error(
        "E1239",
        format!("`module workspace` is declared in more than one file: `{list}`"),
        "the workspace index is discovered by declaration, so exactly one file may declare \
         `module workspace { … }`"
            .to_string(),
        "keep one declaration (conventionally in `workspace.jet`) and delete the others"
            .to_string(),
        None,
    )
}

/// Evaluate a `workspace.jet` source string to a `WorkspacePlan`.
pub fn evaluate(src: &str, base_dir: &Path) -> Result<WorkspacePlan, Diagnostic> {
    // `members:` may call comptime helpers; Canvas/tests invoke `load` outside
    // `jet`/`jetpack` mains that normally install this bridge.
    jet_codegen::Codegen::TIR::install_comptime_bridge();
    let overlay_policy = Overlay::parse_workspace_policy(src).map_err(|e| {
        Diagnostic::error(
            "E0998",
            "workspace overlay policy is malformed".to_string(),
            e.message().to_string(),
            "write `overlay <name> { provider: Provider.nixpkgs(channel: \"...\"); package(\"pkg\").patches += [patch(\"path.patch\")] }`".to_string(),
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

    // Find `module workspace { … }`.
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
        })
        .ok_or_else(|| e0995_no_workspace_module())?;

    // Build the comptime context a `members:` expression evaluates against:
    // every top-level `fn` (callable) and every top-level `const`/`comptime`
    // binding (referenceable), evaluated in source order so a later binding can
    // build on an earlier one. This is what lets `members:` compose a sibling
    // `#Known packages :: […]` or call a local helper `fn`, rather than being
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
            validate_member_path(&rel_path, base_dir, &members, Some(span))?;
            let member = resolve_member(&rel_path, base_dir);
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
) -> Result<(), Diagnostic> {
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
    let path = package_file(&abs).expect("package file checked above");
    // Diagnostics use the same physical identity as membership validation.
    // This removes harmless `./` spelling from the reported manifest path.
    let path = path.canonicalize().unwrap_or(path);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Diagnostic::error(
            "E1334",
            format!("couldn't read workspace member `{rel_path}` Package file: {error}"),
            "workspace membership must validate the complete member manifest before accepting it".to_string(),
            "fix the member manifest permissions or contents and try again".to_string(),
            span,
        )
    })?;
    let has_members = if path.file_name().and_then(|name| name.to_str())
        == Some(Syntax::PACKAGE_FILE)
    {
        match PackageFacts::parse_uncomposed(&text, path.display().to_string()) {
            Ok(facts) => !facts.members.is_empty(),
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
        text.lines().any(|line| line.trim_start().starts_with("members:"))
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
    let name = resolve_member(rel_path, base_dir).name;
    if members.iter().any(|member| member.name == name) {
        return Err(Diagnostic::error(
            "E1325",
            format!("workspace member name `{name}` is declared more than once"),
            "Package references use a stable name; two physical members cannot claim the same name".to_string(),
            "rename one package or remove the duplicate member reference".to_string(),
            span,
        ));
    }
    Ok(())
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
            "`members:` describes the packages in this workspace; it must be a `[String]` list of relative paths or a `find(\"…\")` call".to_string(),
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
    let candidate = workspace_root.join(path);
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

/// Resolve a member package: read its Package source to get the package name.
/// Falls back to the directory basename when no manifest exists.
fn resolve_member(rel_path: &str, base_dir: &Path) -> WorkspaceMember {
    let abs = base_dir.join(rel_path);
    let name = read_package_name(&abs)
        .or_else(|| {
            PathBuf::from(rel_path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| rel_path.to_string());
    WorkspaceMember {
        name,
        path: rel_path.to_string(),
        canonical_path: abs
            .canonicalize()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// Try to read the package name from a Package source in `dir`. Uses the simple
/// text-level package name parser — no full evaluation needed.
fn read_package_name(dir: &Path) -> Option<String> {
    let manifest_path = package_file(dir)?;

    let src = std::fs::read_to_string(&manifest_path).ok()?;
    if manifest_path.file_name().and_then(|name| name.to_str()) == Some(Syntax::PACKAGE_FILE) {
        if let Ok(facts) = PackageFacts::parse_uncomposed(&src, manifest_path.display().to_string()) {
            return Some(facts.name);
        }
    }
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
    let canonical = dir.join(Syntax::PACKAGE_FILE);
    if canonical.is_file() {
        Some(canonical)
    } else if dir.join(Syntax::PAYLOAD_FILE).is_file() {
        Some(dir.join(Syntax::PAYLOAD_FILE))
    } else {
        None
    }
}

fn has_package_file(dir: &Path) -> bool {
    package_file(dir).is_some()
}

// ──────────────────────────────────────────────
// Diagnostics
// ──────────────────────────────────────────────

/// E0995: workspace.jet has no `module workspace { … }` declaration.
fn e0995_no_workspace_module() -> Diagnostic {
    Diagnostic::error(
        "E0995",
        format!(
            "`{}` must declare `module {} {{ … }}`",
            Syntax::WORKSPACE_FILE,
            Syntax::NS_WORKSPACE
        ),
        format!(
            "`{}` is the monorepo workspace index (D-WORKSPACE2=A); it must contain exactly one \
             `module workspace {{ members: … }}` declaration",
            Syntax::WORKSPACE_FILE
        ),
        format!(
            "write `module workspace {{ members: find(\"./packages\") }}` in `{}`",
            Syntax::WORKSPACE_FILE
        ),
        None,
    )
}

/// E0997: `find("…")` in `members:` points at a directory that doesn't exist.
fn e0997_find_dir_missing(dir: &Path, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0997",
        format!("`find` can't read the directory `{}`", dir.display()),
        "`members: find(\"<dir>\")` scans that directory for package subdirectories; \
         it must exist relative to this file"
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
    fn members_references_sibling_comptime_const() {
        // Slice A: a `members:` expression can name a top-level `comptime`
        // binding declared in the same file — not just inline literals.
        let src = "#Known pkgs :: [\"./packages/hello\", \"./packages/ranker\"]\n\
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
        let src = "#Known base :: \"./workspace-packages\"\n\
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

    // D-JPK-FILENAME2=B (A2): `module workspace` is discovered by declaration
    // in any top-level `.jet` file — only `pkg.jet` is a reserved filename.

    #[test]
    fn workspace_module_discovered_in_arbitrary_filename() {
        let dir = tempdir("ws-arbitrary");
        std::fs::write(
            dir.join("repo-index.jet"),
            "module workspace { members: [] }\n",
        )
        .unwrap();
        let plan = load(&dir)
            .expect("declaration should be discovered")
            .expect("should evaluate clean");
        assert!(plan.members.is_empty());
    }

    #[test]
    fn workspace_module_discovered_in_pkg_jet() {
        let dir = tempdir("ws-in-pkg");
        std::fs::write(
            dir.join(Syntax::PAYLOAD_FILE),
            "module workspace { members: [] }\n",
        )
        .unwrap();
        let plan = load(&dir)
            .expect("pkg.jet declaration should be discovered")
            .expect("should evaluate clean");
        assert!(plan.members.is_empty());
    }

    #[test]
    fn workspace_jet_wins_over_discovered_files() {
        let dir = tempdir("ws-canonical-wins");
        let packages = dir.join("packages/hello");
        std::fs::create_dir_all(&packages).unwrap();
        std::fs::write(packages.join(Syntax::PAYLOAD_FILE), "name: \"hello\"\n").unwrap();
        std::fs::write(
            dir.join(Syntax::WORKSPACE_FILE),
            "module workspace { members: find(\"./packages\") }\n",
        )
        .unwrap();
        // A second declaration elsewhere is shadowed by the canonical file,
        // never scanned — no E1239.
        std::fs::write(dir.join("other.jet"), "module workspace { members: [] }\n").unwrap();
        let plan = load(&dir).unwrap().unwrap();
        assert_eq!(plan.members.len(), 1);
    }

    #[test]
    fn two_discovered_workspace_declarations_are_e1239() {
        let dir = tempdir("ws-ambiguous");
        std::fs::write(dir.join("a.jet"), "module workspace { members: [] }\n").unwrap();
        std::fs::write(dir.join("b.jet"), "module workspace { members: [] }\n").unwrap();
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
