//! D-MERGE-* (Tower #143): semantic-index-backed structural diff and merge.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{exit, Command};

use jet::Diagnostics::json_str as json_string;
use jet_foundation::ExitCodes;
use jet_foundation::Report::render_status_json;
use jet_semindex::{
    open_structural_with_overlays, semantic_ops_for_file, DefinitionFact, SemIndexError,
    SemanticOp,
};

#[derive(Clone)]
struct Unit {
    fact: DefinitionFact,
    source: String,
    leading: String,
}

struct Document {
    units: Vec<Unit>,
    suffix: String,
    source_hash: String,
    semantic_ops: Vec<SemanticOp>,
}

struct UnitMatches {
    matched: BTreeMap<usize, Option<usize>>,
    ambiguous: BTreeMap<usize, Vec<usize>>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ChangeKind { Added, Removed, Renamed, Moved, Signature, Body }

impl ChangeKind {
    fn name(&self) -> &'static str { match self { Self::Added => "added", Self::Removed => "removed", Self::Renamed => "renamed", Self::Moved => "moved", Self::Signature => "signature_changed", Self::Body => "body_changed" } }
}

#[derive(Clone)]
struct Change {
    kind: ChangeKind,
    stable_id: String,
    before: Option<String>,
    after: Option<String>,
    semantic_op: Option<SemanticOp>,
}

#[derive(Clone)]
struct Conflict {
    kind: &'static str,
    stable_id: String,
    human_identity: String,
    ours: String,
    theirs: String,
}

pub(crate) fn run_diff(args: &[String]) {
    if wants_help(args) {
        print!("{}", diff_help());
        return;
    }
    if !args.iter().any(|arg| arg == "--structural") {
        fail("`jet diff` currently requires `--structural`", "jet diff --structural before.jet after.jet");
    }
    let paths = positional(args, "diff");
    if paths.len() != 2 { fail("`jet diff --structural` needs two checked Jet files", "jet diff --structural before.jet after.jet"); }
    let before = load(Path::new(&paths[0]));
    let after = load(Path::new(&paths[1]));
    let ops = transition_ops(&before, &after);
    let changes = structural_diff(&before.units, &after.units, &ops);
    let report = report_mode(args);
    if report == "text" {
        if changes.is_empty() { println!("no structural changes"); }
        for change in changes { println!("{}: {} [{}]", change.kind.name(), change.after.as_deref().or(change.before.as_deref()).unwrap_or("definition"), change.stable_id); }
    } else {
        let payload = format!(
            "{{\"changes\":[{}]}}",
            changes.iter().map(change_json).collect::<Vec<_>>().join(",")
        );
        println!(
            "{}",
            render_status_json("ok", true, "diff.structural", &format!(",\"structural_diff\":{payload}"))
        );
    }
}

pub(crate) fn run_merge(args: &[String]) {
    if wants_help(args) {
        print!("{}", merge_help());
        return;
    }
    if args.get(1).map(String::as_str) == Some("install-driver") {
        install_driver(args);
        return;
    }
    if !args.iter().any(|arg| arg == "--structural") {
        fail("`jet merge` currently requires `--structural`", "jet merge --structural base.jet ours.jet theirs.jet --out merged.jet");
    }
    let paths = positional(args, "merge");
    if paths.len() != 3 { fail("`jet merge --structural` needs base, ours, and theirs", "jet merge --structural base.jet ours.jet theirs.jet --out merged.jet"); }
    let base_path = Path::new(&paths[0]);
    let ours_path = Path::new(&paths[1]);
    let theirs_path = Path::new(&paths[2]);
    let base = load(base_path);
    let ours = load(ours_path);
    let theirs = load(theirs_path);
    let (candidate, conflicts) = merge_units(&base, &ours, &theirs);
    if !conflicts.is_empty() {
        render_conflicts(&conflicts, report_mode(args));
        exit(ExitCodes::USER_ERROR);
    }
    let formatted = jet::format_source(&candidate).unwrap_or_else(|_| fail("structural merge produced source that does not parse", "resolve edits manually; no output was written"));
    let output_path = absolute_normalized(
        &flag_value(args, "--out").map(PathBuf::from).unwrap_or_else(|| ours_path.to_path_buf()),
    );
    let overlays = [(output_path.as_path(), formatted.as_str())];
    if let Err(err) = open_structural_with_overlays(&output_path, &overlays) { render_index_error("merged output did not pass parser and sema", err); }
    if let Err(err) = fs::write(&output_path, formatted.as_bytes()) { fail(&format!("could not write `{}`: {err}", output_path.display()), "choose a writable --out path"); }
    if report_mode(args) == "text" { println!("merged: {}", output_path.display()); }
    else {
        let payload = format!(
            "{{\"output\":{}}}",
            json_string(&output_path.display().to_string())
        );
        println!(
            "{}",
            render_status_json("merged", true, "merge.structural", &format!(",\"structural_merge\":{payload}"))
        );
    }
}

pub(crate) fn structural_help(command: &str) -> Option<&'static str> {
    match command {
        "diff" => Some(diff_help()),
        "merge" => Some(merge_help()),
        _ => None,
    }
}

fn load(path: &Path) -> Document {
    let source = fs::read_to_string(path).unwrap_or_else(|err| fail(&format!("could not read `{}`: {err}", path.display()), "pass a readable Jet source file"));
    let index = open_structural_with_overlays(path, &[]).unwrap_or_else(|err| render_index_error(&format!("`{}` did not pass parser and sema", path.display()), err));
    let mut units = Vec::new();
    for fact in index.definition_facts().iter().filter(|fact| same_module(path, &fact.module_path)) {
        let Some(slice) = source.get(fact.span.start..fact.span.end) else { fail("semantic index returned an invalid source span", "run `jet check` and report this compiler bug"); };
        units.push(Unit { fact: fact.clone(), source: slice.trim().to_string(), leading: String::new() });
    }
    units.sort_by_key(|unit| unit.fact.span.start);
    let mut cursor = 0;
    for unit in &mut units {
        unit.leading = source.get(cursor..unit.fact.span.start).unwrap_or("").to_string();
        cursor = unit.fact.span.end;
    }
    let source_hash = jet::SHA256::sha256_hex(source.as_bytes());
    let semantic_ops = semantic_ops_for_file(path, &source_hash);
    Document {
        units,
        suffix: source.get(cursor..).unwrap_or("").to_string(),
        source_hash,
        semantic_ops,
    }
}

fn structural_diff(before: &[Unit], after: &[Unit], ops: &[SemanticOp]) -> Vec<Change> {
    let matches = match_units(before, after, ops);
    let mut used = BTreeSet::new();
    let mut changes = Vec::new();
    for (left, right) in before.iter().enumerate().map(|(i, unit)| (unit, matches.matched.get(&i).copied().flatten())) {
        match right {
            None => changes.push(change(ChangeKind::Removed, left, None)),
            Some(index) => {
                used.insert(index);
                let right = &after[index];
                let rename = semantic_rename(left, right, ops);
                let signature_changed = left.fact.signature_id != right.fact.signature_id;
                let content_changed = left.fact.content_id != right.fact.content_id;
                if rename.is_some() {
                    changes.push(change_with_op(
                        ChangeKind::Renamed,
                        left,
                        Some(right),
                        rename,
                    ));
                }
                // Comparing two standalone paths is often how editors present
                // an unsaved/formatted buffer. A path change is a semantic move
                // only when the declaration also changed; identical checked
                // definitions do not manufacture move churn.
                if left.fact.module_path != right.fact.module_path
                    && (rename.is_some() || signature_changed || content_changed)
                {
                    changes.push(change(ChangeKind::Moved, left, Some(right)));
                }
                if signature_changed { changes.push(change(ChangeKind::Signature, left, Some(right))); }
                else if content_changed { changes.push(change(ChangeKind::Body, left, Some(right))); }
            }
        }
    }
    for (index, unit) in after.iter().enumerate() { if !used.contains(&index) { changes.push(change(ChangeKind::Added, unit, Some(unit))); } }
    changes.sort_by(|a, b| a.stable_id.cmp(&b.stable_id).then(a.kind.cmp(&b.kind)));
    changes
}

fn change(kind: ChangeKind, before: &Unit, after: Option<&Unit>) -> Change {
    change_with_op(kind, before, after, None)
}

fn change_with_op(
    kind: ChangeKind,
    before: &Unit,
    after: Option<&Unit>,
    semantic_op: Option<&SemanticOp>,
) -> Change {
    let stable_id = semantic_op
        .and_then(|op| op.targets.first().map(|target| target.stable_id.clone()))
        .or_else(|| after.map(|unit| unit.fact.stable_id.clone()))
        .unwrap_or_else(|| before.fact.stable_id.clone());
    Change {
        kind,
        stable_id,
        before: Some(before.fact.human_identity.clone()),
        after: after.map(|u| u.fact.human_identity.clone()),
        semantic_op: semantic_op.cloned(),
    }
}

fn match_units(base: &[Unit], side: &[Unit], ops: &[SemanticOp]) -> UnitMatches {
    let mut result = BTreeMap::new();
    let mut ambiguous = BTreeMap::new();
    let mut used = BTreeSet::new();

    // Reserve exact checked identities first. A later fuzzy match must never
    // steal an unchanged declaration from a same-shape sibling.
    for (index, unit) in base.iter().enumerate() {
        let base_count = base.iter().filter(|candidate| exact_key(candidate) == exact_key(unit)).count();
        let candidates: Vec<usize> = side.iter().enumerate()
            .filter(|(_, candidate)| exact_key(candidate) == exact_key(unit))
            .map(|(i, _)| i)
            .collect();
        if base_count == 1 && candidates.len() == 1 {
            result.insert(index, Some(candidates[0]));
            used.insert(candidates[0]);
        }
    }

    for (index, unit) in base.iter().enumerate() {
        if result.contains_key(&index) { continue; }
        let semantic = candidates(side, &used, |candidate| {
            semantic_rename(unit, candidate, ops).is_some()
        });
        let signature = candidates(side, &used, |candidate| candidate.fact.signature_id == unit.fact.signature_id);
        let ancestry = candidates(side, &used, |candidate| candidate.fact.stable_id == unit.fact.stable_id);
        let selected = [semantic, signature, ancestry]
            .into_iter()
            .find(|set| !set.is_empty())
            .unwrap_or_default();
        if selected.len() == 1 {
            used.insert(selected[0]);
            result.insert(index, Some(selected[0]));
        } else {
            if selected.len() > 1 { ambiguous.insert(index, selected); }
            result.insert(index, None);
        }
    }
    UnitMatches { matched: result, ambiguous }
}

fn exact_key(unit: &Unit) -> (&str, &str) { (&unit.fact.kind, &unit.fact.name) }

fn candidates<F>(side: &[Unit], used: &BTreeSet<usize>, predicate: F) -> Vec<usize>
where F: Fn(&Unit) -> bool {
    side.iter().enumerate().filter(|(i, candidate)| !used.contains(i) && predicate(candidate)).map(|(i, _)| i).collect()
}

fn semantic_rename<'a>(
    before: &Unit,
    after: &Unit,
    ops: &'a [SemanticOp],
) -> Option<&'a SemanticOp> {
    ops.iter().find(|op| {
        op.kind == "rename"
            && op.from.as_deref() == Some(before.fact.name.as_str())
            && op.to.as_deref() == Some(after.fact.name.as_str())
            && op.targets.iter().any(|target| {
                target.stable_id == before.fact.stable_id
                    || target.before == before.fact.human_identity
                    || (target.kind == before.fact.kind
                        && target.module_path == before.fact.module_path)
            })
    })
}

fn merge_units(base: &Document, ours: &Document, theirs: &Document) -> (String, Vec<Conflict>) {
    let ours_ops = transition_ops(base, ours);
    let theirs_ops = transition_ops(base, theirs);
    let ours_matches = match_units(&base.units, &ours.units, &ours_ops);
    let theirs_matches = match_units(&base.units, &theirs.units, &theirs_ops);
    let mut ours_used = BTreeSet::new();
    let mut theirs_used = BTreeSet::new();
    let mut merged = String::new();
    let mut conflicts = Vec::new();
    for (index, original) in base.units.iter().enumerate() {
        if let Some(indices) = ours_matches.ambiguous.get(&index) {
            ours_used.extend(indices.iter().copied());
            conflicts.push(ambiguous_conflict(original, "ours", indices, &ours.units));
            continue;
        }
        if let Some(indices) = theirs_matches.ambiguous.get(&index) {
            theirs_used.extend(indices.iter().copied());
            conflicts.push(ambiguous_conflict(original, "theirs", indices, &theirs.units));
            continue;
        }
        let oi = ours_matches.matched.get(&index).copied().flatten();
        let ti = theirs_matches.matched.get(&index).copied().flatten();
        if let Some(i) = oi { ours_used.insert(i); }
        if let Some(i) = ti { theirs_used.insert(i); }
        let leading = match (oi.map(|i| &ours.units[i]), ti.map(|i| &theirs.units[i])) {
            (Some(o), Some(t)) => merge_text("inter_item_trivia", original, &original.leading, &o.leading, &t.leading, &mut conflicts),
            (Some(o), None) => o.leading.clone(),
            (None, Some(t)) => t.leading.clone(),
            (None, None) => String::new(),
        };
        match (oi.map(|i| &ours.units[i]), ti.map(|i| &theirs.units[i])) {
            (None, None) => {}
            (Some(o), None) if o.fact.content_id == original.fact.content_id => {}
            (None, Some(t)) if t.fact.content_id == original.fact.content_id => {}
            (Some(o), Some(t)) if o.fact.content_id == t.fact.content_id => push_unit(&mut merged, &leading, &o.source),
            (Some(o), Some(t)) if o.fact.content_id == original.fact.content_id => push_unit(&mut merged, &leading, &t.source),
            (Some(o), Some(t)) if t.fact.content_id == original.fact.content_id => push_unit(&mut merged, &leading, &o.source),
            (Some(o), None) | (None, Some(o)) => conflicts.push(conflict("delete_edit", original, o, o)),
            (Some(o), Some(t)) => {
                if let Some(source) = merge_recorded_rename(original, o, t, &ours_ops, &theirs_ops) {
                    push_unit(&mut merged, &leading, &source);
                } else {
                    conflicts.push(conflict("overlapping_edit", original, o, t));
                }
            }
        }
    }
    let ours_added: Vec<&Unit> = ours.units.iter().enumerate().filter(|(i, _)| !ours_used.contains(i)).map(|(_, u)| u).collect();
    let theirs_added: Vec<&Unit> = theirs.units.iter().enumerate().filter(|(i, _)| !theirs_used.contains(i)).map(|(_, u)| u).collect();
    let mut theirs_paired = BTreeSet::new();
    for ours_unit in ours_added {
        let candidates: Vec<usize> = theirs_added.iter().enumerate()
            .filter(|(index, theirs_unit)| !theirs_paired.contains(index) && exact_key(theirs_unit) == exact_key(ours_unit))
            .map(|(index, _)| index)
            .collect();
        match candidates.as_slice() {
            [] => push_unit(&mut merged, &ours_unit.leading, &ours_unit.source),
            [index] => {
                theirs_paired.insert(*index);
                let theirs_unit = theirs_added[*index];
                if ours_unit.fact.content_id != theirs_unit.fact.content_id {
                    conflicts.push(conflict("competing_add", ours_unit, ours_unit, theirs_unit));
                } else {
                    let leading = merge_text(
                        "inter_item_trivia",
                        ours_unit,
                        "",
                        &ours_unit.leading,
                        &theirs_unit.leading,
                        &mut conflicts,
                    );
                    push_unit(&mut merged, &leading, &ours_unit.source);
                }
            }
            _ => conflicts.push(Conflict {
                kind: "ambiguous_identity",
                stable_id: ours_unit.fact.stable_id.clone(),
                human_identity: ours_unit.fact.human_identity.clone(),
                ours: ours_unit.fact.content_id.clone(),
                theirs: "ambiguous additions".to_string(),
            }),
        }
    }
    for (index, theirs_unit) in theirs_added.into_iter().enumerate() {
        if !theirs_paired.contains(&index) {
            push_unit(&mut merged, &theirs_unit.leading, &theirs_unit.source);
        }
    }
    let suffix = merge_shell("suffix", &base.suffix, &ours.suffix, &theirs.suffix, &mut conflicts);
    merged.push_str(&suffix);
    (merged, conflicts)
}

fn transition_ops(before: &Document, after: &Document) -> Vec<SemanticOp> {
    let mut out = Vec::new();
    for op in before
        .semantic_ops
        .iter()
        .chain(after.semantic_ops.iter())
        .filter(|op| op.matches_transition(&before.source_hash, &after.source_hash))
    {
        if !out.contains(op) {
            out.push(op.clone());
        }
    }
    out
}

fn merge_recorded_rename(
    base: &Unit,
    ours: &Unit,
    theirs: &Unit,
    ours_ops: &[SemanticOp],
    theirs_ops: &[SemanticOp],
) -> Option<String> {
    let ours_rename = semantic_rename(base, ours, ours_ops);
    let theirs_rename = semantic_rename(base, theirs, theirs_ops);
    let (from, to) = match (ours_rename.as_ref(), theirs_rename.as_ref()) {
        (Some(ours), Some(theirs)) if ours.to == theirs.to => {
            (base.fact.name.as_str(), ours.to.as_deref()?)
        }
        (Some(ours), None) => (base.fact.name.as_str(), ours.to.as_deref()?),
        (None, Some(theirs)) => (base.fact.name.as_str(), theirs.to.as_deref()?),
        _ => return None,
    };
    let renamed_base = rename_source(&base.source, from, to)?;
    let other = if ours_rename.is_some() && theirs_rename.is_none() {
        rename_source(&theirs.source, from, to)?
    } else if theirs_rename.is_some() && ours_rename.is_none() {
        rename_source(&ours.source, from, to)?
    } else {
        theirs.source.clone()
    };
    if ours.source == theirs.source {
        Some(ours.source.clone())
    } else if ours.source == renamed_base {
        Some(other)
    } else if theirs.source == renamed_base {
        Some(ours.source.clone())
    } else {
        None
    }
}

fn rename_source(source: &str, from: &str, to: &str) -> Option<String> {
    let (start, _) = source.match_indices(from).find(|(start, _)| {
        let before = source[..*start].chars().next_back();
        let end = *start + from.len();
        let after = source[end..].chars().next();
        before.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_')
            && after.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_')
    })?;
    let mut renamed = source.to_string();
    renamed.replace_range(start..start + from.len(), to);
    Some(renamed)
}

fn push_unit(output: &mut String, leading: &str, source: &str) {
    output.push_str(leading);
    output.push_str(source);
}

fn merge_text(
    kind: &'static str,
    unit: &Unit,
    base: &str,
    ours: &str,
    theirs: &str,
    conflicts: &mut Vec<Conflict>,
) -> String {
    if ours == theirs { return ours.to_string(); }
    if ours == base { return theirs.to_string(); }
    if theirs == base { return ours.to_string(); }
    conflicts.push(Conflict {
        kind,
        stable_id: unit.fact.stable_id.clone(),
        human_identity: unit.fact.human_identity.clone(),
        ours: format!("sha256:{}", jet::SHA256::sha256_hex(ours.as_bytes())),
        theirs: format!("sha256:{}", jet::SHA256::sha256_hex(theirs.as_bytes())),
    });
    base.to_string()
}

fn ambiguous_conflict(base: &Unit, side: &str, indices: &[usize], units: &[Unit]) -> Conflict {
    let ids = indices.iter().map(|index| units[*index].fact.human_identity.as_str()).collect::<Vec<_>>().join(",");
    Conflict {
        kind: "ambiguous_identity",
        stable_id: base.fact.stable_id.clone(),
        human_identity: base.fact.human_identity.clone(),
        ours: if side == "ours" { format!("ambiguous:{ids}") } else { base.fact.content_id.clone() },
        theirs: if side == "theirs" { format!("ambiguous:{ids}") } else { base.fact.content_id.clone() },
    }
}

fn merge_shell(
    label: &'static str,
    base: &str,
    ours: &str,
    theirs: &str,
    conflicts: &mut Vec<Conflict>,
) -> String {
    if ours == theirs { return ours.to_string(); }
    if ours == base { return theirs.to_string(); }
    if theirs == base { return ours.to_string(); }
    conflicts.push(Conflict {
        kind: "file_scope_edit",
        stable_id: format!("file:{label}"),
        human_identity: format!("file {label}"),
        ours: format!("sha256:{}", jet::SHA256::sha256_hex(ours.as_bytes())),
        theirs: format!("sha256:{}", jet::SHA256::sha256_hex(theirs.as_bytes())),
    });
    base.to_string()
}

fn conflict(kind: &'static str, base: &Unit, ours: &Unit, theirs: &Unit) -> Conflict {
    Conflict { kind, stable_id: base.fact.stable_id.clone(), human_identity: base.fact.human_identity.clone(), ours: ours.fact.content_id.clone(), theirs: theirs.fact.content_id.clone() }
}

fn render_conflicts(conflicts: &[Conflict], mode: &str) {
    if mode == "text" {
        for conflict in conflicts { eprintln!("conflict: {} ({})\n stable id: {}\n ours: {}\n theirs: {}", conflict.human_identity, conflict.kind, conflict.stable_id, conflict.ours, conflict.theirs); }
        eprintln!("merge stopped: resolve conflicts manually; no output was written");
    } else {
        let payload = format!(
            "{{\"conflicts\":[{}]}}",
            conflicts.iter().map(conflict_json).collect::<Vec<_>>().join(",")
        );
        eprintln!(
            "{}",
            render_status_json("conflict", false, "merge.structural", &format!(",\"structural_merge\":{payload}"))
        );
    }
}

fn install_driver(args: &[String]) {
    let repo = flag_value(args, "--repo").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let config = git_config_path(&repo);
    git_config_set(&config, "merge.jetstruct.name", "Jet structural merge");
    git_config_set(&config, "merge.jetstruct.driver", "jet merge --structural %O %A %B --out %A");
    for (key, expected) in [
        ("merge.jetstruct.name", "Jet structural merge"),
        ("merge.jetstruct.driver", "jet merge --structural %O %A %B --out %A"),
    ] {
        let actual = git_config_get(&config, key);
        if actual.trim() != expected { fail(&format!("Git did not retain `{key}` exactly"), "check repository config permissions and includes"); }
    }
    let attributes = repo.join(".gitattributes");
    let mut attrs = fs::read_to_string(&attributes).unwrap_or_default();
    let line = "*.jet merge=jetstruct";
    if !attrs.lines().any(|existing| existing.trim() == line) { if !attrs.is_empty() && !attrs.ends_with('\n') { attrs.push('\n'); } attrs.push_str(line); attrs.push('\n'); write_file(&attributes, &attrs); }
    println!("installed structural merge driver in {}", repo.display());
}

fn git_config_path(repo: &Path) -> PathBuf {
    let output = Command::new("git").arg("-C").arg(repo).args(["rev-parse", "--git-path", "config"]).output()
        .unwrap_or_else(|err| fail(&format!("could not run Git: {err}"), "install Git before enabling its merge driver"));
    if !output.status.success() { fail(&format!("`{}` is not a Git worktree", repo.display()), "run inside a Git repository or pass --repo <path>"); }
    let raw = String::from_utf8(output.stdout).unwrap_or_else(|_| fail("Git returned a non-UTF-8 config path", "repair the Git worktree metadata"));
    let reported = Path::new(raw.trim());
    let config = if reported.is_absolute() { normalize_path(reported) } else { normalize_path(&repo.join(reported)) };
    if !config.is_file() { fail(&format!("`{}` has no Git config", repo.display()), "repair the Git worktree metadata"); }
    config
}

fn git_config_set(config: &Path, key: &str, value: &str) {
    let output = Command::new("git").args(["config", "--file"]).arg(config).args(["--replace-all", key, value]).output()
        .unwrap_or_else(|err| fail(&format!("could not run Git: {err}"), "install Git before enabling its merge driver"));
    if !output.status.success() { fail(&format!("could not update Git config `{key}`"), "fix repository config permissions"); }
}

fn git_config_get(config: &Path, key: &str) -> String {
    let output = Command::new("git").args(["config", "--file"]).arg(config).args(["--get", key]).output()
        .unwrap_or_else(|err| fail(&format!("could not run Git: {err}"), "install Git before enabling its merge driver"));
    if !output.status.success() { fail(&format!("could not read back Git config `{key}`"), "repair the repository config"); }
    String::from_utf8(output.stdout).unwrap_or_else(|_| fail("Git config contained non-UTF-8 data", "repair the repository config"))
}

fn write_file(path: &Path, content: &str) { if let Err(err) = fs::write(path, content) { fail(&format!("could not write `{}`: {err}", path.display()), "fix repository permissions"); } }
fn positional(args: &[String], command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_value = false;
    for arg in args.iter().skip_while(|arg| arg.as_str() != command).skip(1) {
        if skip_value { skip_value = false; continue; }
        if matches!(arg.as_str(), "--out" | "--report" | "--repo") { skip_value = true; continue; }
        if !arg.starts_with('-') && !matches!(arg.as_str(), "install-driver" | "text" | "json" | "editor") { out.push(arg.clone()); }
    }
    out
}
fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> { args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].as_str()) }
fn report_mode(args: &[String]) -> &str { flag_value(args, "--report").unwrap_or("text") }
fn wants_help(args: &[String]) -> bool { args.iter().any(|arg| jet::CLI::is_help_flag(arg)) || args.get(1).is_some_and(|arg| arg == "help") }
fn diff_help() -> &'static str { "usage: jet diff --structural <before.jet> <after.jet> [--report text|json|editor]\n\nCompares checked Jet definitions by semantic identity.\n" }
fn merge_help() -> &'static str { "usage:\n  jet merge --structural <base.jet> <ours.jet> <theirs.jet> [--out <file.jet>] [--report text|json|editor]\n  jet merge install-driver [--repo <path>]\n\nPerforms a checked three-way structural merge or installs the opt-in Git driver.\n" }
fn same_module(path: &Path, module: &str) -> bool { absolute_normalized(path) == absolute_normalized(Path::new(module)) }
fn absolute_normalized(path: &Path) -> PathBuf {
    if path.is_absolute() { normalize_path(path) }
    else { normalize_path(&std::env::current_dir().unwrap_or_else(|err| fail(&format!("could not read current directory: {err}"), "run from a readable directory")).join(path)) }
}
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => { out.pop(); }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
fn change_json(change: &Change) -> String {
    let semantic_op = change
        .semantic_op
        .as_ref()
        .map(semantic_op_json)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"kind\":{},\"stable_id\":{},\"before\":{},\"after\":{},\"semantic_op\":{}}}",
        json_string(change.kind.name()),
        json_string(&change.stable_id),
        change.before.as_ref().map(|v| json_string(v)).unwrap_or_else(|| "null".into()),
        change.after.as_ref().map(|v| json_string(v)).unwrap_or_else(|| "null".into()),
        semantic_op,
    )
}
fn semantic_op_json(op: &SemanticOp) -> String {
    let optional = |value: &Option<String>| {
        value
            .as_deref()
            .map(|value| json_string(value))
            .unwrap_or_else(|| "null".to_string())
    };
    format!(
        "{{\"kind\":{},\"rule_id\":{},\"from\":{},\"to\":{},\"node\":{},\"match\":{},\"replace\":{}}}",
        json_string(&op.kind),
        optional(&op.rule_id),
        optional(&op.from),
        optional(&op.to),
        optional(&op.node),
        optional(&op.match_template),
        optional(&op.replace_template),
    )
}
fn conflict_json(conflict: &Conflict) -> String { format!("{{\"kind\":{},\"stable_id\":{},\"human_identity\":{},\"ours\":{},\"theirs\":{}}}", json_string(conflict.kind), json_string(&conflict.stable_id), json_string(&conflict.human_identity), json_string(&conflict.ours), json_string(&conflict.theirs)) }
fn render_index_error(context: &str, error: SemIndexError) -> ! { crate::cli_error!(@fix "E2105", context, "correct source errors; structural tools never merge unchecked code"); let SemIndexError::Load(diags) = error; for diagnostic in diags { eprintln!("  {}: {}", diagnostic.code, diagnostic.what); } exit(ExitCodes::USER_ERROR) }
fn fail(message: &str, fix: &str) -> ! { crate::cli_error!(@fix "E2104", message, fix); exit(ExitCodes::USAGE) }
