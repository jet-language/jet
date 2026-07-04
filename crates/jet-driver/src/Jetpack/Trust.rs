//! U19 env/dev trust gate (D-JPK-DEVCOMPOSE1=D, card c9jetpackgates).
//!
//! `jetpack enter` (`jet env`) and `jetpack dev` (project-level `jet dev`)
//! both realize a project's declared env — code the project author wrote,
//! not the user. First entry to a repo whose env definition is trust-sensitive
//! (today: it declares any package ref/source — U12 services and U13 secrets
//! don't exist yet, but [`is_trust_sensitive`] is the one place they register
//! their own trigger later, see its doc comment) shows a summary and asks.
//! Accepting persists a grant keyed by [`env_definition_hash`], so the same
//! (unchanged) env never re-prompts; `--trust` is a one-shot bypass that
//! persists nothing. `jetpack config trust add/list/remove` manages durable
//! glob/prefix patterns that pre-authorize matching projects with no hash
//! grant at all.
//!
//! Store: `~/.jet/trust` (`Syntax::TRUST_FILE` under `Syntax::CONFIG_DEFAULT_DIR`,
//! HOME-resolved the same way `JetOS::resolve_config_path` resolves
//! `~/.jet/config.jet`). Plain newline-separated lines, `hash:<sha256>` or
//! `pattern:<glob/prefix>` — the same plain-text style `Recipe::trust_first_build`
//! already uses for its own (project-local, adapter-recipe) trust marker.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use super::Output::Theme;
use super::RefSpec::{RefSpec, SourceTable};
use crate::Syntax;

const HASH_PREFIX: &str = "hash:";
const PATTERN_PREFIX: &str = "pattern:";

/// `~/.jet/trust`. `HOME` is test-overridable (existing convention, see
/// `JetOS::resolve_config_path` and `tests/jetpack.rs`'s `os_build_default_
/// config_path_uses_home_dot_jet`).
pub fn store_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(Syntax::CONFIG_DEFAULT_DIR).join(Syntax::TRUST_FILE)
}

fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn append_line(path: &Path, line: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut existing = std::fs::read_to_string(path).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(line);
    existing.push('\n');
    let _ = std::fs::write(path, existing);
}

/// A stable hash over the env definition's trust-sensitive content: every
/// realized ref (sorted) plus every declared named source (sorted, via
/// `SourceTable::trust_lines`). U12 (services) / U13 (secrets) fold their own
/// rendered form in here the day they ship, so a hash change re-prompts.
pub fn env_definition_hash(refs: &[RefSpec], table: &SourceTable) -> String {
    let mut ref_lines: Vec<String> = refs.iter().map(|r| r.raw.clone()).collect();
    ref_lines.sort();
    let mut source_lines = table.trust_lines();
    source_lines.sort();
    let mut content = String::new();
    for line in &ref_lines {
        content.push_str(line);
        content.push('\n');
    }
    content.push_str("--sources--\n");
    for line in &source_lines {
        content.push_str(line);
        content.push('\n');
    }
    crate::SHA256::sha256_hex(content.as_bytes())
}

/// Whether this env definition is trust-sensitive at all — i.e. whether
/// entering it should ever prompt. Today: it declares at least one package
/// ref (any external code/binary a project pulls in is a supply-chain
/// decision). This is the extension point U12 (any `services:`) and U13 (any
/// `secrets:`) add their own `||` arm to once those env fields exist — no
/// call site above this function needs to change when they do.
pub fn is_trust_sensitive(refs: &[RefSpec]) -> bool {
    !refs.is_empty()
}

/// Already trusted: an exact hash grant, or a pattern matching `project_dir`.
pub fn is_trusted(store: &Path, project_dir: &Path, hash: &str) -> bool {
    let project_str = project_dir.to_string_lossy();
    let target_hash_line = format!("{HASH_PREFIX}{hash}");
    for line in read_lines(store) {
        if line == target_hash_line {
            return true;
        }
        if let Some(pattern) = line.strip_prefix(PATTERN_PREFIX) {
            if matches_pattern(pattern, &project_str) {
                return true;
            }
        }
    }
    false
}

/// Glob/prefix match: a trailing `*` is a prefix wildcard, else an exact or
/// prefix match. One wildcard shape is all U19 asks for (no glob crate, I6).
fn matches_pattern(pattern: &str, subject: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => subject.starts_with(prefix),
        None => subject == pattern || subject.starts_with(pattern),
    }
}

/// Persist a hash grant (the interactive prompt's "yes"). Idempotent.
pub fn grant_hash(store: &Path, hash: &str) {
    let line = format!("{HASH_PREFIX}{hash}");
    if read_lines(store).iter().any(|l| *l == line) {
        return;
    }
    append_line(store, &line);
}

/// `jetpack config trust add <pattern>`. Returns `false` if already present.
pub fn add_pattern(store: &Path, pattern: &str) -> bool {
    let line = format!("{PATTERN_PREFIX}{pattern}");
    if read_lines(store).iter().any(|l| *l == line) {
        return false;
    }
    append_line(store, &line);
    true
}

/// `jetpack config trust list` — every raw stored line (hash + pattern).
pub fn list_entries(store: &Path) -> Vec<String> {
    read_lines(store)
}

/// `jetpack config trust remove <pattern>`. Returns `false` if not present.
pub fn remove_pattern(store: &Path, pattern: &str) -> bool {
    let line = format!("{PATTERN_PREFIX}{pattern}");
    let lines = read_lines(store);
    if !lines.iter().any(|l| *l == line) {
        return false;
    }
    let mut content: String = lines
        .into_iter()
        .filter(|l| *l != line)
        .map(|l| l + "\n")
        .collect();
    if content.is_empty() {
        content = String::new();
    }
    let _ = std::fs::write(store, content);
    true
}

/// The trust gate: shared by `jetpack enter` and `jetpack dev`. Not
/// trust-sensitive, already trusted, or `--trust`-bypassed → proceed
/// silently. Otherwise: a non-TTY stdin gets a clean E1255 error (never a
/// hung prompt); a TTY gets a summary + y/N prompt, and "yes" persists the
/// hash grant. Returns the exit code to return on refusal/error.
pub fn gate(
    theme: &Theme,
    store: &Path,
    project_dir: &Path,
    refs: &[RefSpec],
    table: &SourceTable,
    bypass: bool,
) -> Result<(), i32> {
    if !is_trust_sensitive(refs) {
        return Ok(());
    }
    let hash = env_definition_hash(refs, table);
    if bypass || is_trusted(store, project_dir, &hash) {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        theme.error_coded(
            "E1255",
            "this project's environment isn't trusted yet",
            &format!(
                "entering this project realizes {} package(s) it declares; a first entry needs a \
                 trust decision, and stdin isn't a terminal to ask interactively",
                refs.len()
            ),
            "pass `--trust` for this one run, or pre-authorize with `jetpack config trust add <pattern>`.",
        );
        return Err(2);
    }
    theme.note(&format!(
        "first entry to this project — it declares {} package(s):",
        refs.len()
    ));
    for r in refs {
        theme.detail(&r.raw);
    }
    eprint!("  trust this environment? [y/N] ");
    {
        use std::io::Write;
        let _ = std::io::stderr().flush();
    }
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return Err(2);
    }
    if matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        grant_hash(store, &hash);
        Ok(())
    } else {
        theme.status("not trusted — exiting.");
        Err(2)
    }
}

/// A stable hash over a foreign flake/devenv file's content (U16) — the same
/// role `env_definition_hash` plays for a declared env, but for untrusted
/// input jetpack didn't write: an arbitrary `flake.nix`/`devenv.nix` runs
/// Nix-evaluator code the moment jetpack shells out to it, so a first
/// encounter needs the same trust decision (D-JPK-DEVCOMPOSE1's rationale
/// extended to U16's two new untrusted-input surfaces, `-p` ad-hoc packages
/// and a foreign flake).
pub fn flake_definition_hash(content: &str) -> String {
    format!("flake:{}", crate::SHA256::sha256_hex(content.as_bytes()))
}

/// The trust gate for a foreign flake/devenv file (U16) — `jet env`'s
/// foreign-flake fallback and `jet bridge flake` both reach this before
/// shelling out to `nix`. Same store, same hash-grant/pattern machinery, same
/// non-interactive-stdin refusal as [`gate`]; keyed on the file's content
/// instead of a ref list, since there is no `RefSpec` for "arbitrary flake.nix
/// text". Ad-hoc `-p` packages do NOT go through this function — they become
/// ordinary `RefSpec`s and are folded into the normal `gate` call alongside
/// the project's declared refs, so one trust decision covers both.
pub fn gate_flake(
    theme: &Theme,
    store: &Path,
    project_dir: &Path,
    flake_path: &Path,
    bypass: bool,
) -> Result<(), i32> {
    let content = std::fs::read_to_string(flake_path).unwrap_or_default();
    let hash = flake_definition_hash(&content);
    if bypass || is_trusted(store, project_dir, &hash) {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        theme.error_coded(
            "E1255",
            "this project's environment isn't trusted yet",
            &format!(
                "entering `{}` shells out to `nix` against a foreign flake this project didn't \
                 declare through `env.*`; a first entry needs a trust decision, and stdin isn't a \
                 terminal to ask interactively",
                flake_path.display()
            ),
            "pass `--trust` for this one run, or pre-authorize with `jetpack config trust add <pattern>`.",
        );
        return Err(2);
    }
    theme.note(&format!(
        "first entry to this project's foreign flake: {}",
        flake_path.display()
    ));
    eprint!("  trust this flake? [y/N] ");
    {
        use std::io::Write;
        let _ = std::io::stderr().flush();
    }
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return Err(2);
    }
    if matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        grant_hash(store, &hash);
        Ok(())
    } else {
        theme.status("not trusted — exiting.");
        Err(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::RefSpec::Source;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jetpack_trust_unit_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn ref_spec(raw: &str) -> RefSpec {
        RefSpec {
            source: Source::Nixpkgs,
            package: raw.to_string(),
            raw: raw.to_string(),
        }
    }

    #[test]
    fn empty_refs_are_never_trust_sensitive() {
        assert!(!is_trust_sensitive(&[]));
    }

    #[test]
    fn nonempty_refs_are_trust_sensitive() {
        assert!(is_trust_sensitive(&[ref_spec("nixpkgs:fastfetch")]));
    }

    #[test]
    fn hash_is_stable_and_order_independent() {
        let table = SourceTable::empty();
        let a = env_definition_hash(&[ref_spec("nixpkgs:a"), ref_spec("nixpkgs:b")], &table);
        let b = env_definition_hash(&[ref_spec("nixpkgs:b"), ref_spec("nixpkgs:a")], &table);
        assert_eq!(a, b);
    }

    #[test]
    fn hash_changes_when_refs_change() {
        let table = SourceTable::empty();
        let a = env_definition_hash(&[ref_spec("nixpkgs:a")], &table);
        let b = env_definition_hash(&[ref_spec("nixpkgs:a"), ref_spec("nixpkgs:b")], &table);
        assert_ne!(a, b);
    }

    #[test]
    fn hash_grant_round_trips() {
        let dir = scratch("hashgrant");
        let store = dir.join("trust");
        let table = SourceTable::empty();
        let refs = [ref_spec("nixpkgs:fastfetch")];
        let hash = env_definition_hash(&refs, &table);
        assert!(!is_trusted(&store, &dir, &hash));
        grant_hash(&store, &hash);
        assert!(is_trusted(&store, &dir, &hash));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pattern_add_list_remove() {
        let dir = scratch("pattern");
        let store = dir.join("trust");
        assert!(add_pattern(&store, "/home/dev/*"));
        assert!(!add_pattern(&store, "/home/dev/*"), "idempotent");
        assert_eq!(list_entries(&store), vec!["pattern:/home/dev/*"]);
        let project = Path::new("/home/dev/myproj");
        assert!(is_trusted(&store, project, "irrelevant-hash"));
        assert!(remove_pattern(&store, "/home/dev/*"));
        assert!(!is_trusted(&store, project, "irrelevant-hash"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prefix_pattern_without_wildcard_matches_prefix() {
        let dir = scratch("prefix");
        let store = dir.join("trust");
        add_pattern(&store, "/home/dev/");
        assert!(is_trusted(&store, Path::new("/home/dev/anything"), "h"));
        assert!(!is_trusted(&store, Path::new("/home/other"), "h"));
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── U16 foreign-flake trust gate ──

    #[test]
    fn flake_hash_is_stable_and_content_sensitive() {
        let a = flake_definition_hash("{ devShells.default = {}; }");
        let b = flake_definition_hash("{ devShells.default = {}; }");
        let c = flake_definition_hash("{ devShells.default = { buildInputs = [1]; }; }");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn gate_flake_bypass_never_grants() {
        let dir = scratch("flake_bypass");
        let store = dir.join("trust");
        let flake = dir.join("flake.nix");
        std::fs::write(&flake, "{ }").unwrap();
        let theme = Theme::resolve(true);
        assert!(gate_flake(&theme, &store, &dir, &flake, true).is_ok());
        // A one-shot bypass persists nothing (mirrors `gate`'s `--trust`).
        assert!(!is_trusted(&store, &dir, &flake_definition_hash("{ }")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gate_flake_grant_round_trips() {
        let dir = scratch("flake_grant");
        let store = dir.join("trust");
        let content = "{ devShells.default = {}; }";
        let hash = flake_definition_hash(content);
        assert!(!is_trusted(&store, &dir, &hash));
        grant_hash(&store, &hash);
        assert!(is_trusted(&store, &dir, &hash));
        std::fs::remove_dir_all(&dir).ok();
    }
}
