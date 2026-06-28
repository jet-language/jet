//! `workspace.jet` evaluator (D-WORKSPACE1=B, D-WORKSPACE2=A).
//!
//! Parses and evaluates `module workspace { members: <expr> }` from a
//! `workspace.jet` at the repo root. The `members:` expression may be:
//!   - `find("./packages")` — discovers package directories under the path
//!   - A list literal of strings: `["./pkg/a", "./pkg/b"]`
//!   - Any comptime expression that evaluates to a `[String]`
//!
//! The result is a `WorkspacePlan` listing the member packages with their
//! names (read from each member's `pack.jet`) and relative paths.
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

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{Expr, Func, Item, StrPart};

// ──────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────

/// The result of evaluating `workspace.jet`.
#[derive(Debug, Clone, Default)]
pub struct WorkspacePlan {
    /// Member packages in source order (the order `members:` produced them).
    pub members: Vec<WorkspaceMember>,
}

/// One workspace member package.
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    /// Package name read from the member's `pack.jet` (or derived from path).
    pub name: String,
    /// Path to the package directory, relative to the workspace root.
    pub path: String,
}

// ──────────────────────────────────────────────
// Load / evaluate
// ──────────────────────────────────────────────

/// Load and evaluate `workspace.jet` from `dir`. Returns `None` when the file
/// is absent (not an error — a plain project has no workspace). Returns
/// `Err(diagnostic)` when the file exists but is malformed.
pub fn load(dir: &Path) -> Option<Result<WorkspacePlan, Diagnostic>> {
    let path = dir.join(Syntax::WORKSPACE_FILE);
    let src = std::fs::read_to_string(&path).ok()?;
    Some(evaluate(&src, dir))
}

/// Evaluate a `workspace.jet` source string to a `WorkspacePlan`.
pub fn evaluate(src: &str, base_dir: &Path) -> Result<WorkspacePlan, Diagnostic> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
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
                if m.name == Syntax::NS_WORKSPACE && !m.disabled {
                    return Some(m);
                }
            }
            None
        })
        .ok_or_else(|| e0995_no_workspace_module())?;

    // Evaluate each `members:` expression (typically just one).
    let mut members = Vec::new();
    for expr in &ws_module.members {
        let paths = eval_members_expr(expr, src, base_dir)?;
        for rel_path in paths {
            let member = resolve_member(&rel_path, base_dir);
            members.push(member);
        }
    }

    Ok(WorkspacePlan { members })
}

// ──────────────────────────────────────────────
// Expression evaluation
// ──────────────────────────────────────────────

/// Evaluate one `members:` expression to a list of relative package directory
/// paths. Handles `find("./dir")` specially (the common case); falls back to
/// comptime for arbitrary expressions.
fn eval_members_expr(expr: &Expr, _src: &str, base_dir: &Path) -> Result<Vec<String>, Diagnostic> {
    // Fast path: `find("./dir")` — scan for package directories.
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
            let dir_str = extract_literal_string(&arg.expr).ok_or_else(|| {
                Diagnostic::error(
                    "E0996",
                    "`find` path must be a string literal".to_string(),
                    "computed paths can't be used here; write the path inline".to_string(),
                    "example: `members: find(\"./packages\")`".to_string(),
                    Some(arg.expr.span()),
                )
            })?;
            let scan_dir = base_dir.join(&dir_str);
            return find_package_dirs(&scan_dir, base_dir, span);
        }
    }

    // General case: evaluate through comptime, expect a list of strings.
    let funcs: HashMap<String, &Func> = HashMap::new();
    let extern_names: HashSet<String> = HashSet::new();
    let v = crate::Comptime::evaluate(expr, &funcs, &extern_names, base_dir, &HashMap::new())?;
    extract_string_list(v, expr.span())
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
            StrPart::Interp(_) => return None,
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

/// Scan `scan_dir` for immediate subdirectories containing `pkg.jet`.
/// Returns paths relative to `workspace_root`, sorted for determinism.
fn find_package_dirs(
    scan_dir: &Path,
    workspace_root: &Path,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    let entries =
        std::fs::read_dir(scan_dir).map_err(|_| e0997_find_dir_missing(scan_dir, span))?;
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path.join(Syntax::PAYLOAD_FILE).is_file() {
            continue;
        }
        found.push(path);
    }
    found.sort();
    // Make each path relative to the workspace root.
    let mut out = Vec::with_capacity(found.len());
    for abs in found {
        let rel = abs.strip_prefix(workspace_root).map(|p| {
            // Normalise to forward-slash form even on Windows (workspace.lock
            // stores POSIX paths; the platform join handles them on read).
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

/// Resolve a member package: read its `pack.jet` (or `pkg.jet`) to get the
/// package name. Falls back to the directory basename when no manifest exists.
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
    }
}

/// Try to read the package name from a `pack.jet` or `pkg.jet` in `dir`.
/// Uses the simple text-level package name parser — no full evaluation needed.
fn read_package_name(dir: &Path) -> Option<String> {
    let manifest_path = if dir.join(Syntax::PAYLOAD_FILE).is_file() {
        dir.join(Syntax::PAYLOAD_FILE)
    } else {
        return None;
    };

    let src = std::fs::read_to_string(&manifest_path).ok()?;
    // Fast heuristic: find `package: { name: "…" }` or `name: "…"`.
    // This avoids a full parse for the common case.
    for line in src.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("name:") {
            let val = rest.trim().trim_matches('"').trim_matches(',');
            if !val.is_empty() && !val.contains('{') {
                return Some(val.to_string());
            }
        }
    }
    None
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
    fn explicit_string_list() {
        // Paths that don't exist: name falls back to the path basename.
        let src =
            "module workspace {\n    members: [\"./packages/hello\", \"./packages/ranker\"]\n}\n";
        let plan = eval(src);
        assert_eq!(plan.members.len(), 2);
        assert_eq!(plan.members[0].path, "./packages/hello");
        assert_eq!(plan.members[0].name, "hello");
        assert_eq!(plan.members[1].path, "./packages/ranker");
        assert_eq!(plan.members[1].name, "ranker");
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
    fn disabled_workspace_module_is_skipped() {
        // A disabled module doesn't count as the workspace declaration.
        let src = "module _workspace { members: [] }\n";
        let d = eval_err(src);
        assert_eq!(d.code, "E0995");
    }

    #[test]
    fn find_discovers_pack_jet_directories() {
        let tmp = tempdir("workspace-find");
        // packages/hello/pack.jet
        let hello = tmp.join("packages/hello");
        std::fs::create_dir_all(&hello).unwrap();
        std::fs::write(hello.join(Syntax::PAYLOAD_FILE), "name: \"hello\"\n").unwrap();
        // packages/ranker/pkg.jet
        let ranker = tmp.join("packages/ranker");
        std::fs::create_dir_all(&ranker).unwrap();
        std::fs::write(ranker.join(Syntax::PAYLOAD_FILE), "name: \"ranker\"\n").unwrap();
        // packages/lib (no pack.jet — should be ignored)
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

    /// I5: the committed `examples/workspace/workspace.jet` parses + evaluates
    /// with the expected member names when run against the fixture packages.
    #[test]
    fn committed_workspace_example_evaluates_clean() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/workspace");
        if !dir.exists() {
            // Skip when the examples directory hasn't been created yet.
            return;
        }
        let workspace_path = dir.join(crate::Syntax::WORKSPACE_FILE);
        let src = match std::fs::read_to_string(&workspace_path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let plan = evaluate(&src, &dir).expect("example workspace should evaluate clean");
        assert!(
            !plan.members.is_empty(),
            "example workspace must have members"
        );
        let names: Vec<&str> = plan.members.iter().map(|m| m.name.as_str()).collect();
        assert!(
            names.contains(&"hello"),
            "expected `hello` member; got: {names:?}"
        );
    }
}
