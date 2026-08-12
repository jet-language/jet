//! Workspace member selection for `jetpack build` / `test` / `run`
//! (D-JPK-SELECTOR1=C).
//!
//! `-p <member>` (exact, repeatable) picks named members; unknown names are
//! E1231 with did-you-mean. `--affected` / `--affected-since <ref>` compute
//! the changed set from member input hashes (same `tree_hash` shape the
//! action cache uses — D-BUILDCACHE1) and always close under dependents.
//! No pnpm-style `--filter` pattern DSL.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Diagnostics::Diagnostic;
use crate::WorkspaceFile::{WorkspaceMember, WorkspacePlan};
use jet_pkg_model::Authority::{AuthorityResolver, CheckedPackage};

/// CLI selection request for a workspace command.
#[derive(Debug, Clone, Default)]
pub struct SelectRequest {
    /// Exact member names from repeated `-p <member>`.
    pub packages: Vec<String>,
    /// `--affected` — diff working tree against recorded input hashes.
    pub affected: bool,
    /// `--affected-since <ref>` — members changed since this git ref.
    pub affected_since: Option<String>,
}

impl SelectRequest {
    pub fn is_restricting(&self) -> bool {
        !self.packages.is_empty() || self.affected || self.affected_since.is_some()
    }
}

/// Resolve which workspace members a command should run.
pub fn select_members(
    root: &Path,
    plan: &WorkspacePlan,
    req: &SelectRequest,
) -> Result<Vec<WorkspaceMember>, Diagnostic> {
    if !req.is_restricting() {
        return Ok(plan.members.clone());
    }

    let mut selected: BTreeSet<String> = BTreeSet::new();

    if !req.packages.is_empty() {
        let index = member_name_index(plan);
        for name in &req.packages {
            if !index.contains_key(name.as_str()) {
                return Err(e1231_unknown_member(name, plan));
            }
            selected.insert(name.clone());
        }
    }

    if req.affected || req.affected_since.is_some() {
        let changed = if let Some(git_ref) = req.affected_since.as_deref() {
            members_changed_since(root, plan, git_ref)?
        } else {
            members_affected_vs_cache(root, plan)
        };
        let with_deps = close_under_dependents(root, plan, &changed)?;
        for name in with_deps {
            selected.insert(name);
        }
    }

    Ok(plan
        .members
        .iter()
        .filter(|m| selected.contains(&m.name))
        .cloned()
        .collect())
}

/// Return selected workspace members in deterministic local-dependency order.
///
/// The workspace index remains the source-order tie breaker.  A member is
/// ordered after any selected member named by its `package.jet` dependency list;
/// external dependencies do not affect this order.  Package resolution owns
/// dependency-cycle diagnostics, so a malformed cycle keeps the stable source
/// order here instead of inventing a second diagnostic authority in the
/// workspace runner.
pub fn dependency_order(
    root: &Path,
    members: &[WorkspaceMember],
) -> Result<Vec<WorkspaceMember>, Diagnostic> {
    let resolver = AuthorityResolver::open(root).map_err(|error| error.diagnostic())?;
    dependency_order_checked(&resolver, members).map_err(|error| error.diagnostic())
}

pub fn dependency_order_checked(
    resolver: &AuthorityResolver,
    members: &[WorkspaceMember],
) -> Result<Vec<WorkspaceMember>, jet_pkg_model::Authority::AuthorityError> {
    Ok(dependency_order_packages_checked(resolver, members)?
        .into_iter()
        .map(|(member, _package)| member)
        .collect())
}

/// Return members in dependency order together with the package snapshots used
/// to inspect their dependencies. The caller must pass these snapshots through
/// to realization; reopening a member by its path after ordering would create a
/// validation/consumption gap.
pub fn dependency_order_packages_checked(
    resolver: &AuthorityResolver,
    members: &[WorkspaceMember],
) -> Result<Vec<(WorkspaceMember, CheckedPackage)>, jet_pkg_model::Authority::AuthorityError> {
    let names: HashMap<&str, usize> = members
        .iter()
        .enumerate()
        .map(|(index, member)| (member.name.as_str(), index))
        .collect();
    let mut dependents = vec![Vec::<usize>::new(); members.len()];
    let mut indegree = vec![0usize; members.len()];
    let mut checked_packages = Vec::with_capacity(members.len());

    for (index, member) in members.iter().enumerate() {
        let checked = resolver.checked_package(Path::new(&member.path))?;
        resolver.revalidate_member(&checked.member)?;
        let manifest = &checked.facts;
        let mut seen = HashSet::new();
        for dependency_name in manifest.deps.keys() {
            let Some(&dependency_index) = names.get(dependency_name.as_str()) else {
                continue;
            };
            if dependency_index == index || !seen.insert(dependency_index) {
                continue;
            }
            dependents[dependency_index].push(index);
            indegree[index] += 1;
        }
        checked_packages.push(Some(checked));
    }

    // `(source position, member index)` makes ready-node selection stable even
    // when future callers construct a plan with duplicate names.
    let mut ready = BTreeSet::new();
    for (index, degree) in indegree.iter().enumerate() {
        if *degree == 0 {
            ready.insert((index, index));
        }
    }
    let mut ordered_indices = Vec::with_capacity(members.len());
    let mut emitted = vec![false; members.len()];
    while let Some((_, index)) = ready.pop_first() {
        emitted[index] = true;
        ordered_indices.push(index);
        for dependent in &dependents[index] {
            indegree[*dependent] -= 1;
            if indegree[*dependent] == 0 {
                ready.insert((*dependent, *dependent));
            }
        }
    }

    // Keep a malformed dependency cycle deterministic and let the package
    // resolver report the actual cycle when realization runs.
    for (index, _member) in members.iter().enumerate() {
        if !emitted[index] {
            ordered_indices.push(index);
        }
    }
    resolver.revalidate_root()?;
    ordered_indices
        .into_iter()
        .map(|index| {
            let package = checked_packages[index]
                .take()
                .ok_or_else(|| jet_pkg_model::Authority::AuthorityError::Unsupported(
                    "workspace member snapshot was consumed before realization".to_string(),
                ))?;
            Ok((members[index].clone(), package))
        })
        .collect()
}

/// Persist member input hashes after a successful workspace build so the next
/// `--affected` run can diff against them (action-cache input keys).
pub fn record_member_input_hashes(root: &Path, members: &[WorkspaceMember]) {
    let path = member_input_hash_path(root);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut existing = load_member_input_hashes(root);
    let Ok(resolver) = AuthorityResolver::open(root) else {
        return;
    };
    for member in members {
        let Ok(checked) = resolver.checked_member(Path::new(&member.path)) else {
            continue;
        };
        let Ok(hash) = resolver.source_tree_hash(&checked.directory) else {
            continue;
        };
        if resolver.revalidate_member(&checked).is_ok() {
            existing.insert(member.name.clone(), hash);
        }
    }
    let mut lines: Vec<String> = existing
        .into_iter()
        .map(|(name, hash)| format!("{name} = \"{hash}\"\n"))
        .collect();
    lines.sort();
    let _ = std::fs::write(path, lines.concat());
}

fn member_input_hash_path(root: &Path) -> PathBuf {
    root.join(".jet")
        .join("cache")
        .join("member-input-hashes")
}

fn load_member_input_hashes(root: &Path) -> BTreeMap<String, String> {
    let path = member_input_hash_path(root);
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let hash = rest.trim().trim_matches('"').to_string();
        if !hash.is_empty() {
            out.insert(name.trim().to_string(), hash);
        }
    }
    out
}

fn members_affected_vs_cache(root: &Path, plan: &WorkspacePlan) -> BTreeSet<String> {
    let cached = load_member_input_hashes(root);
    let mut out = BTreeSet::new();
    if cached.is_empty() {
        // No baseline yet — every member is affected (first run / cold cache).
        for m in &plan.members {
            out.insert(m.name.clone());
        }
        return out;
    }
    let resolver = AuthorityResolver::open(root).ok();
    for m in &plan.members {
        let current = resolver.as_ref().and_then(|resolver| {
            let checked = resolver.checked_member(Path::new(&m.path)).ok()?;
            let hash = resolver.source_tree_hash(&checked.directory).ok()?;
            resolver.revalidate_member(&checked).ok()?;
            Some(hash)
        });
        match cached.get(&m.name) {
            Some(prev) if current.as_ref().is_some_and(|current| prev == current) => {}
            _ => {
                out.insert(m.name.clone());
            }
        }
    }
    out
}

fn members_changed_since(
    root: &Path,
    plan: &WorkspacePlan,
    git_ref: &str,
) -> Result<BTreeSet<String>, Diagnostic> {
    resolve_git_ref(root, git_ref)?;
    let output = Command::new("git")
        .args(["diff", "--name-only", git_ref, "--"])
        .current_dir(root)
        .output();
    let Ok(output) = output else {
        return Err(e1292_bad_ref(git_ref, &[], "git could not be run"));
    };
    if !output.status.success() {
        return Err(e1292_bad_ref(
            git_ref,
            &suggest_git_refs(root, git_ref),
            "git diff against that ref failed",
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let changed_files: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let mut out = BTreeSet::new();
    for m in &plan.members {
        let prefix = normalize_member_prefix(&m.path);
        let hit = changed_files.iter().any(|f| {
            let f = f.trim_start_matches("./");
            f == prefix || f.starts_with(&(prefix.to_string() + "/"))
        });
        if hit {
            out.insert(m.name.clone());
        }
    }
    Ok(out)
}

fn resolve_git_ref(root: &Path, git_ref: &str) -> Result<String, Diagnostic> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", &format!("{git_ref}^{{commit}}")])
        .current_dir(root)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
        }
        _ => Err(e1292_bad_ref(
            git_ref,
            &suggest_git_refs(root, git_ref),
            "no commit matches that ref",
        )),
    }
}

fn suggest_git_refs(root: &Path, query: &str) -> Vec<String> {
    let output = Command::new("git")
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes", "refs/tags"])
        .current_dir(root)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let labels: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    nearest(query, &labels)
}

fn close_under_dependents(
    root: &Path,
    plan: &WorkspacePlan,
    seeds: &BTreeSet<String>,
) -> Result<BTreeSet<String>, Diagnostic> {
    let reverse = reverse_dependents(root, plan)?;
    let mut out = seeds.clone();
    let mut stack: Vec<String> = seeds.iter().cloned().collect();
    while let Some(name) = stack.pop() {
        if let Some(deps) = reverse.get(&name) {
            for d in deps {
                if out.insert(d.clone()) {
                    stack.push(d.clone());
                }
            }
        }
    }
    Ok(out)
}

/// Map member name → members that depend on it (direct deps from `package.jet`).
fn reverse_dependents(
    root: &Path,
    plan: &WorkspacePlan,
) -> Result<HashMap<String, Vec<String>>, Diagnostic> {
    let resolver = AuthorityResolver::open(root).map_err(|error| error.diagnostic())?;
    let names: HashSet<&str> = plan.members.iter().map(|m| m.name.as_str()).collect();
    let mut reverse: HashMap<String, Vec<String>> = HashMap::new();
    for m in &plan.members {
        let checked = resolver
            .checked_member(Path::new(&m.path))
            .map_err(|error| error.diagnostic())?;
        resolver
            .revalidate_member(&checked)
            .map_err(|error| error.diagnostic())?;
        let manifest = checked.manifest.facts;
        for dep_name in manifest.deps.keys() {
            if names.contains(dep_name.as_str()) {
                reverse
                    .entry(dep_name.clone())
                    .or_default()
                    .push(m.name.clone());
            }
        }
    }
    resolver.revalidate_root().map_err(|error| error.diagnostic())?;
    Ok(reverse)
}

fn member_name_index(plan: &WorkspacePlan) -> HashMap<&str, &WorkspaceMember> {
    plan.members
        .iter()
        .map(|m| (m.name.as_str(), m))
        .collect()
}

fn normalize_member_prefix(path: &str) -> String {
    path.trim()
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

fn e1231_unknown_member(query: &str, plan: &WorkspacePlan) -> Diagnostic {
    let labels: Vec<String> = plan.members.iter().map(|m| m.name.clone()).collect();
    let suggestions = nearest(query, &labels);
    let fix = if suggestions.is_empty() {
        "check `workspace.jet` — that name isn't in the member index.".to_string()
    } else {
        format!("did you mean: {}?", suggestions.join(", "))
    };
    Diagnostic::error(
        "E1231",
        format!("`{query}` is not a workspace member"),
        "A `-p` selector must name a member listed in the workspace index \
         (`workspace.jet` `members:`)."
            .to_string(),
        fix,
        None,
    )
}

fn e1292_bad_ref(query: &str, suggestions: &[String], why_detail: &str) -> Diagnostic {
    let fix = if suggestions.is_empty() {
        format!("pass a valid git ref (branch, tag, or commit); {why_detail}")
    } else {
        format!("did you mean: {}?", suggestions.join(", "))
    };
    Diagnostic::error(
        "E1295",
        format!("git ref `{query}` not found"),
        format!(
            "`--affected-since` needs a resolvable git ref so Jet can diff \
             member input hashes against that baseline ({why_detail})."
        ),
        fix,
        None,
    )
}

fn nearest(query: &str, labels: &[String]) -> Vec<String> {
    let mut scored: Vec<(usize, &String)> = labels
        .iter()
        .map(|l| (edit_distance(query, l), l))
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .filter(|(d, l)| *d <= query.len().max(l.len()) / 2 + 1)
        .take(3)
        .map(|(_, l)| l.clone())
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Reject pnpm-style `--filter` pattern DSL (D-JPK-SELECTOR1=C — no pattern language).
pub fn reject_filter_dsl(args: &[String]) -> Option<Diagnostic> {
    for a in args {
        if a == "--filter" || a.starts_with("--filter=") {
            return Some(Diagnostic::error(
                "E1296",
                format!("`{a}` is not a Jet workspace selector"),
                "D-JPK-SELECTOR1 rejects pnpm-style `--filter` pattern DSLs; \
                 Jet uses exact `-p <member>` and computed `--affected` / \
                 `--affected-since <ref>` instead."
                    .to_string(),
                "use `-p <member>` (repeatable) or `--affected` / `--affected-since <ref>`."
                    .to_string(),
                None,
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// I4: pin Exact Error/Why/Fix render to tests/fixtures/jetpack-diagnostics/.
    fn check_jetpack_diagnostic_snapshot(code: &str, diag: &Diagnostic) {
        let rendered = crate::Diagnostics::render_all("<cli>", "", std::slice::from_ref(diag));
        assert!(
            rendered.starts_with(&format!("Error [{code}]:")),
            "unexpected render:\n{rendered}"
        );
        assert!(rendered.contains("\n Why: "), "missing Why:\n{rendered}");
        assert!(rendered.contains("\n Fix: "), "missing Fix:\n{rendered}");
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/jetpack-diagnostics")
            .join(format!("{code}.stderr"));
        if std::env::var_os("UPDATE_EXPECT").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &rendered).unwrap();
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing jetpack diagnostic snapshot {}", path.display()));
        assert_eq!(rendered, expected, "snapshot mismatch for {code}");
    }

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("jpk-select-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_member(root: &Path, name: &str, path: &str, deps: &[&str]) -> WorkspaceMember {
        let abs = root.join(path);
        std::fs::create_dir_all(&abs).unwrap();
        let dep_lines: String = deps
            .iter()
            .map(|d| format!("    {d}: {d}#1.0.0,\n"))
            .collect();
        let deps_block = if deps.is_empty() {
            String::new()
        } else {
            format!("deps: {{\n{dep_lines}}}\n")
        };
        std::fs::write(
            abs.join(crate::Syntax::PACKAGE_FILE),
            format!("name: \"{name}\"\nversion: \"1.0.0\"\n{deps_block}"),
        )
        .unwrap();
        // A source file so tree_hash is non-empty / change-sensitive.
        std::fs::write(abs.join("lib.jet"), format!("module {name} {{ }}\n")).unwrap();
        WorkspaceMember {
            name: name.to_string(),
            path: path.to_string(),
            canonical_path: abs
                .canonicalize()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }

    fn plan(members: Vec<WorkspaceMember>) -> WorkspacePlan {
        WorkspacePlan {
            members,
            ..Default::default()
        }
    }

    #[test]
    fn bare_selects_all_members() {
        let root = unique_dir("bare");
        let plan = plan(vec![
            write_member(&root, "a", "packages/a", &[]),
            write_member(&root, "b", "packages/b", &[]),
        ]);
        let got = select_members(&root, &plan, &SelectRequest::default()).unwrap();
        assert_eq!(got.len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dependency_order_places_local_dependencies_first() {
        let root = unique_dir("dependency-order");
        let dependent = write_member(&root, "app", "packages/app", &["shared"]);
        let dependency = write_member(&root, "shared", "packages/shared", &[]);
        let ordered = dependency_order(&root, &[dependent, dependency]).unwrap();
        let names: Vec<_> = ordered.iter().map(|member| member.name.as_str()).collect();
        assert_eq!(names, ["shared", "app"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dash_p_isolates_named_members() {
        let root = unique_dir("dash-p");
        let plan = plan(vec![
            write_member(&root, "billing", "packages/billing", &[]),
            write_member(&root, "shared", "packages/shared", &[]),
            write_member(&root, "ui", "packages/ui", &[]),
        ]);
        let req = SelectRequest {
            packages: vec!["billing".into(), "shared".into()],
            ..Default::default()
        };
        let got = select_members(&root, &plan, &req).unwrap();
        let names: Vec<_> = got.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["billing", "shared"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_dash_p_is_e1231_with_suggestion() {
        let root = unique_dir("unknown");
        let plan = plan(vec![write_member(&root, "billing", "packages/billing", &[])]);
        let req = SelectRequest {
            packages: vec!["biling".into()],
            ..Default::default()
        };
        let err = select_members(&root, &plan, &req).expect_err("typo must fail");
        assert_eq!(err.code, "E1231");
        assert!(
            err.fix.contains("billing"),
            "expected did-you-mean for billing, got {}",
            err.fix
        );
        check_jetpack_diagnostic_snapshot("E1231", &err);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn affected_since_includes_dependents() {
        let root = unique_dir("deps");
        // shared <- billing (billing depends on shared)
        let plan = plan(vec![
            write_member(&root, "shared", "packages/shared", &[]),
            write_member(&root, "billing", "packages/billing", &["shared"]),
            write_member(&root, "ui", "packages/ui", &[]),
        ]);
        // Init git repo with baseline commit, then change shared.
        assert!(Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["add", "."])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "base"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        let base = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&root)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        // Change shared only.
        std::fs::write(
            root.join("packages/shared/lib.jet"),
            "module shared { pub fn bump() {} }\n",
        )
        .unwrap();

        let req = SelectRequest {
            affected_since: Some(base),
            ..Default::default()
        };
        let got = select_members(&root, &plan, &req).unwrap();
        let names: BTreeSet<_> = got.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains("shared"), "changed member missing: {names:?}");
        assert!(
            names.contains("billing"),
            "dependent billing must be included: {names:?}"
        );
        assert!(!names.contains("ui"), "unrelated ui must stay out: {names:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bad_affected_since_ref_is_e1292_with_suggestion() {
        let root = unique_dir("badref");
        let plan = plan(vec![write_member(&root, "a", "packages/a", &[])]);
        assert!(Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["add", "."])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "base"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        // Also create origin/main-like remote-tracking name locally as a branch
        // suggestion target: `orgin/main` ≈ `main`.
        let req = SelectRequest {
            affected_since: Some("orgin/main".into()),
            ..Default::default()
        };
        let err = select_members(&root, &plan, &req).expect_err("bad ref must fail");
        assert_eq!(err.code, "E1295");
        assert!(
            err.fix.contains("main") || err.what.contains("orgin/main"),
            "expected suggestion or clear what, got what={} fix={}",
            err.what,
            err.fix
        );
        check_jetpack_diagnostic_snapshot("E1295", &err);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn filter_dsl_is_rejected() {
        let d = reject_filter_dsl(&["--filter=billing...".into()]).expect("must reject");
        assert_eq!(d.code, "E1296");
        assert!(d.fix.contains("-p"));
        check_jetpack_diagnostic_snapshot("E1296", &d);
    }

    #[test]
    fn affected_vs_cache_detects_changed_member() {
        let root = unique_dir("cache");
        let plan = plan(vec![
            write_member(&root, "a", "packages/a", &[]),
            write_member(&root, "b", "packages/b", &[]),
        ]);
        record_member_input_hashes(&root, &plan.members);
        // Change only a.
        std::fs::write(
            root.join("packages/a/lib.jet"),
            "module a { pub fn changed() {} }\n",
        )
        .unwrap();
        let req = SelectRequest {
            affected: true,
            ..Default::default()
        };
        let got = select_members(&root, &plan, &req).unwrap();
        let names: Vec<_> = got.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
