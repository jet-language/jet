//! D-MERGE-* (Tower #143): semantic-index-backed structural diff and merge.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet_semindex::{open, open_with_overlays, DefinitionFact, SemIndexError};

#[derive(Clone)]
struct Unit {
    fact: DefinitionFact,
    source: String,
}

struct Document {
    units: Vec<Unit>,
    prefix: String,
    suffix: String,
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
    if !args.iter().any(|arg| arg == "--structural") {
        fail("`jet diff` currently requires `--structural`", "jet diff --structural before.jet after.jet");
    }
    let paths = positional(args, "diff");
    if paths.len() != 2 { fail("`jet diff --structural` needs two checked Jet files", "jet diff --structural before.jet after.jet"); }
    let before = load(Path::new(&paths[0]));
    let after = load(Path::new(&paths[1]));
    let changes = structural_diff(&before.units, &after.units);
    let report = report_mode(args);
    if report == "text" {
        if changes.is_empty() { println!("no structural changes"); }
        for change in changes { println!("{}: {} [{}]", change.kind.name(), change.after.as_deref().or(change.before.as_deref()).unwrap_or("definition"), change.stable_id); }
    } else {
        println!("{{\"schema_version\":1,\"kind\":\"structural_diff\",\"changes\":[{}]}}", changes.iter().map(change_json).collect::<Vec<_>>().join(","));
    }
}

pub(crate) fn run_merge(args: &[String]) {
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
        exit(1);
    }
    let formatted = jet::format_source(&candidate).unwrap_or_else(|_| fail("structural merge produced source that does not parse", "resolve edits manually; no output was written"));
    let output_path = flag_value(args, "--out").map(PathBuf::from).unwrap_or_else(|| ours_path.to_path_buf());
    let overlays = [(output_path.as_path(), formatted.as_str())];
    if let Err(err) = open_with_overlays(&output_path, &overlays) { render_index_error("merged output did not pass parser and sema", err); }
    if let Err(err) = fs::write(&output_path, formatted.as_bytes()) { fail(&format!("could not write `{}`: {err}", output_path.display()), "choose a writable --out path"); }
    if report_mode(args) == "text" { println!("merged: {}", output_path.display()); }
    else { println!("{{\"schema_version\":1,\"kind\":\"structural_merge\",\"status\":\"merged\",\"output\":{}}}", json_string(&output_path.display().to_string())); }
}

fn load(path: &Path) -> Document {
    let source = fs::read_to_string(path).unwrap_or_else(|err| fail(&format!("could not read `{}`: {err}", path.display()), "pass a readable Jet source file"));
    let index = open(path).unwrap_or_else(|err| render_index_error(&format!("`{}` did not pass parser and sema", path.display()), err));
    let mut units = Vec::new();
    for fact in index.definition_facts() {
        let Some(slice) = source.get(fact.span.start..fact.span.end) else { fail("semantic index returned an invalid source span", "run `jet check` and report this compiler bug"); };
        units.push(Unit { fact: fact.clone(), source: slice.trim().to_string() });
    }
    units.sort_by_key(|unit| unit.fact.span.start);
    let first = units.first().map_or(source.len(), |unit| unit.fact.span.start);
    let last = units.last().map_or(0, |unit| unit.fact.span.end);
    Document {
        units,
        prefix: source.get(..first).unwrap_or("").to_string(),
        suffix: source.get(last..).unwrap_or("").to_string(),
    }
}

fn structural_diff(before: &[Unit], after: &[Unit]) -> Vec<Change> {
    let matches = match_units(before, after);
    let mut used = BTreeSet::new();
    let mut changes = Vec::new();
    for (left, right) in before.iter().enumerate().map(|(i, unit)| (unit, matches.get(&i).copied().flatten())) {
        match right {
            None => changes.push(change(ChangeKind::Removed, left, None)),
            Some(index) => {
                used.insert(index);
                let right = &after[index];
                if left.fact.name != right.fact.name { changes.push(change(ChangeKind::Renamed, left, Some(right))); }
                if left.fact.module_path != right.fact.module_path && Path::new(&left.fact.module_path).file_name() == Path::new(&right.fact.module_path).file_name() { changes.push(change(ChangeKind::Moved, left, Some(right))); }
                if left.fact.stable_id != right.fact.stable_id { changes.push(change(ChangeKind::Signature, left, Some(right))); }
                else if left.fact.content_id != right.fact.content_id { changes.push(change(ChangeKind::Body, left, Some(right))); }
            }
        }
    }
    for (index, unit) in after.iter().enumerate() { if !used.contains(&index) { changes.push(change(ChangeKind::Added, unit, Some(unit))); } }
    changes.sort_by(|a, b| a.stable_id.cmp(&b.stable_id).then(a.kind.cmp(&b.kind)));
    changes
}

fn change(kind: ChangeKind, before: &Unit, after: Option<&Unit>) -> Change {
    Change { kind, stable_id: after.map(|u| u.fact.stable_id.clone()).unwrap_or_else(|| before.fact.stable_id.clone()), before: Some(before.fact.human_identity.clone()), after: after.map(|u| u.fact.human_identity.clone()) }
}

fn match_units(base: &[Unit], side: &[Unit]) -> BTreeMap<usize, Option<usize>> {
    let mut result = BTreeMap::new();
    let mut used = BTreeSet::new();
    for (index, unit) in base.iter().enumerate() {
        let stable: Vec<usize> = side.iter().enumerate().filter(|(i, candidate)| !used.contains(i) && candidate.fact.stable_id == unit.fact.stable_id).map(|(i, _)| i).collect();
        let named: Vec<usize> = side.iter().enumerate().filter(|(i, candidate)| !used.contains(i) && candidate.fact.name == unit.fact.name && candidate.fact.kind == unit.fact.kind).map(|(i, _)| i).collect();
        let found = if stable.len() == 1 { Some(stable[0]) } else if named.len() == 1 { Some(named[0]) } else { None };
        if let Some(found) = found { used.insert(found); }
        result.insert(index, found);
    }
    result
}

fn merge_units(base: &Document, ours: &Document, theirs: &Document) -> (String, Vec<Conflict>) {
    let ours_matches = match_units(&base.units, &ours.units);
    let theirs_matches = match_units(&base.units, &theirs.units);
    let mut ours_used = BTreeSet::new();
    let mut theirs_used = BTreeSet::new();
    let mut merged = Vec::new();
    let mut conflicts = Vec::new();
    for (index, original) in base.units.iter().enumerate() {
        let oi = ours_matches.get(&index).copied().flatten();
        let ti = theirs_matches.get(&index).copied().flatten();
        if let Some(i) = oi { ours_used.insert(i); }
        if let Some(i) = ti { theirs_used.insert(i); }
        match (oi.map(|i| &ours.units[i]), ti.map(|i| &theirs.units[i])) {
            (None, None) => {}
            (Some(o), None) if o.fact.content_id == original.fact.content_id => {}
            (None, Some(t)) if t.fact.content_id == original.fact.content_id => {}
            (Some(o), Some(t)) if o.fact.content_id == t.fact.content_id => merged.push(o.source.clone()),
            (Some(o), Some(t)) if o.fact.content_id == original.fact.content_id => merged.push(t.source.clone()),
            (Some(o), Some(t)) if t.fact.content_id == original.fact.content_id => merged.push(o.source.clone()),
            (Some(o), None) | (None, Some(o)) => conflicts.push(conflict("delete_edit", original, o, o)),
            (Some(o), Some(t)) => conflicts.push(conflict("overlapping_edit", original, o, t)),
        }
    }
    let ours_added: Vec<&Unit> = ours.units.iter().enumerate().filter(|(i, _)| !ours_used.contains(i)).map(|(_, u)| u).collect();
    let theirs_added: Vec<&Unit> = theirs.units.iter().enumerate().filter(|(i, _)| !theirs_used.contains(i)).map(|(_, u)| u).collect();
    let mut added_names = BTreeMap::new();
    for unit in ours_added.iter().chain(theirs_added.iter()) {
        let key = (&unit.fact.kind, &unit.fact.name);
        if let Some(previous) = added_names.get(&key) {
            let previous: &&Unit = previous;
            if previous.fact.content_id != unit.fact.content_id { conflicts.push(conflict("competing_add", unit, previous, unit)); }
        } else { added_names.insert(key, unit); merged.push(unit.source.clone()); }
    }
    let prefix = merge_shell("prefix", &base.prefix, &ours.prefix, &theirs.prefix, &mut conflicts);
    let suffix = merge_shell("suffix", &base.suffix, &ours.suffix, &theirs.suffix, &mut conflicts);
    (format!("{}{}\n{}", prefix, merged.join("\n\n"), suffix), conflicts)
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
        eprintln!("{{\"schema_version\":1,\"kind\":\"structural_merge\",\"status\":\"conflict\",\"conflicts\":[{}]}}", conflicts.iter().map(conflict_json).collect::<Vec<_>>().join(","));
    }
}

fn install_driver(args: &[String]) {
    let repo = flag_value(args, "--repo").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let git = repo.join(".git");
    let config = git.join("config");
    if !git.is_dir() || !config.is_file() { fail(&format!("`{}` is not a Git worktree", repo.display()), "run inside a Git repository or pass --repo <path>"); }
    let mut config_text = fs::read_to_string(&config).unwrap_or_else(|err| fail(&format!("could not read `{}`: {err}", config.display()), "fix repository permissions"));
    let stanza = "[merge \"jetstruct\"]\n\tname = Jet structural merge\n\tdriver = jet merge --structural %O %A %B --out %A\n";
    if !config_text.contains("[merge \"jetstruct\"]") { if !config_text.ends_with('\n') { config_text.push('\n'); } config_text.push_str(stanza); write_file(&config, &config_text); }
    let attributes = repo.join(".gitattributes");
    let mut attrs = fs::read_to_string(&attributes).unwrap_or_default();
    let line = "*.jet merge=jetstruct";
    if !attrs.lines().any(|existing| existing.trim() == line) { if !attrs.is_empty() && !attrs.ends_with('\n') { attrs.push('\n'); } attrs.push_str(line); attrs.push('\n'); write_file(&attributes, &attrs); }
    println!("installed structural merge driver in {}", repo.display());
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
fn json_string(value: &str) -> String { format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")) }
fn change_json(change: &Change) -> String { format!("{{\"kind\":{},\"stable_id\":{},\"before\":{},\"after\":{}}}", json_string(change.kind.name()), json_string(&change.stable_id), change.before.as_ref().map(|v| json_string(v)).unwrap_or_else(|| "null".into()), change.after.as_ref().map(|v| json_string(v)).unwrap_or_else(|| "null".into())) }
fn conflict_json(conflict: &Conflict) -> String { format!("{{\"kind\":{},\"stable_id\":{},\"human_identity\":{},\"ours\":{},\"theirs\":{}}}", json_string(conflict.kind), json_string(&conflict.stable_id), json_string(&conflict.human_identity), json_string(&conflict.ours), json_string(&conflict.theirs)) }
fn render_index_error(context: &str, error: SemIndexError) -> ! { eprintln!("error: {context}"); let SemIndexError::Load(diags) = error; for diagnostic in diags { eprintln!("  {}: {}", diagnostic.code, diagnostic.what); } eprintln!(" fix: correct source errors; structural tools never merge unchecked code"); exit(1) }
fn fail(message: &str, fix: &str) -> ! { eprintln!("error: {message}\n fix: {fix}"); exit(2) }
