//! U19 env/dev trust gate (D-JPK-DEVCOMPOSE1=D, card c9jetpackgates).
//!
//! `jetpack enter` (`jet env`) and `jetpack dev` (project-level `jet dev`)
//! both realize a project's declared env — code the project author wrote,
//! not the user. First entry to a repo whose env definition is trust-sensitive
//! (today: it declares any package ref/source, or U13 secrets) shows a summary
//! and asks.
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
const GRANT_PREFIX: &str = "grant:";

const AUTH_PACKAGE: &str = "package";
const AUTH_BUILD: &str = "build";
const AUTH_ENV: &str = "env";
const AUTH_SERVICE: &str = "service";
const AUTH_IMAGE: &str = "image";
const AUTH_FLEET: &str = "fleet";
const AUTH_JETOS: &str = "jetos";
const AUTH_VAULT_WRITE: &str = "vault.write";
const AUTH_INTEGRATION: &str = "integration";

const AUTHORITY_KINDS: &[&str] = &[
    AUTH_PACKAGE,
    AUTH_BUILD,
    AUTH_ENV,
    AUTH_SERVICE,
    AUTH_IMAGE,
    AUTH_FLEET,
    AUTH_JETOS,
    AUTH_VAULT_WRITE,
    AUTH_INTEGRATION,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustRecord {
    Hash { hash: String },
    Pattern { pattern: String },
    Grant(TrustGrant),
    Raw { line: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustGrant {
    pub authority: String,
    pub subject: String,
    pub scope: String,
}

impl TrustGrant {
    pub fn key(&self) -> String {
        format!("{}:{}", self.authority, self.subject)
    }

    fn line(&self) -> String {
        format!(
            "{GRANT_PREFIX}{}:{}:{}",
            escape_component(&self.scope),
            escape_component(&self.authority),
            escape_component(&self.subject),
        )
    }
}

pub fn parse_grant_selector(selector: &str, scope: &str) -> Result<TrustGrant, String> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err("grant selector is empty".to_string());
    }
    if let Some((head, tail)) = selector.split_once(':') {
        if AUTHORITY_KINDS.contains(&head) && tail.trim().is_empty() {
            return Err(format!("trust grant `{head}:` needs a subject"));
        }
    }
    let scope = match scope {
        s if s == Syntax::TRUST_SCOPE_USER || s == Syntax::TRUST_SCOPE_REPO => s,
        other => return Err(format!("unknown trust scope `{other}`")),
    };
    let (authority, subject) = split_explicit_authority(selector)
        .unwrap_or_else(|| (infer_authority(selector), selector.to_string()));
    Ok(TrustGrant {
        authority,
        subject,
        scope: scope.to_string(),
    })
}

fn split_explicit_authority(selector: &str) -> Option<(String, String)> {
    let (head, tail) = selector.split_once(':')?;
    if AUTHORITY_KINDS.contains(&head) && !tail.trim().is_empty() {
        return Some((head.to_string(), tail.to_string()));
    }
    None
}

fn infer_authority(selector: &str) -> String {
    if selector.ends_with(".service") || selector.contains(":") && selector.ends_with(".service") {
        AUTH_SERVICE
    } else if selector.starts_with("image.") {
        AUTH_IMAGE
    } else if selector.starts_with("fleet.") {
        AUTH_FLEET
    } else if selector.starts_with("jetos.") {
        AUTH_JETOS
    } else if selector.starts_with("env.") {
        AUTH_ENV
    } else if selector.starts_with("build-sha256:")
        || selector.contains("@ci")
        || selector.starts_with("cache@")
    {
        AUTH_BUILD
    } else {
        AUTH_PACKAGE
    }
    .to_string()
}

fn parse_record(line: &str) -> TrustRecord {
    if let Some(hash) = line.strip_prefix(HASH_PREFIX) {
        TrustRecord::Hash {
            hash: hash.to_string(),
        }
    } else if let Some(pattern) = line.strip_prefix(PATTERN_PREFIX) {
        TrustRecord::Pattern {
            pattern: pattern.to_string(),
        }
    } else if let Some(rest) = line.strip_prefix(GRANT_PREFIX) {
        let mut parts = rest.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(scope), Some(authority), Some(subject)) => TrustRecord::Grant(TrustGrant {
                scope: unescape_component(scope),
                authority: unescape_component(authority),
                subject: unescape_component(subject),
            }),
            _ => TrustRecord::Raw {
                line: line.to_string(),
            },
        }
    } else {
        TrustRecord::Raw {
            line: line.to_string(),
        }
    }
}

fn escape_component(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '%' => out.push_str("%25"),
            ':' => out.push_str("%3A"),
            '\n' | '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

fn unescape_component(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `~/.jet/trust`. `HOME` is test-overridable (existing convention, see
/// `JetOS::resolve_config_path` and `tests/jetpack_jetos.rs`'s `os_build_default_
/// config_path_uses_home_dot_jet`).
pub fn store_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(Syntax::CONFIG_DEFAULT_DIR)
        .join(Syntax::TRUST_FILE)
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
/// realized ref (sorted), every declared named source (sorted, via
/// `SourceTable::trust_lines`), and every U13 declared secret name (sorted).
/// A change in any of those re-prompts.
pub fn env_definition_hash(refs: &[RefSpec], table: &SourceTable, secrets: &[String]) -> String {
    let mut ref_lines: Vec<String> = refs.iter().map(|r| r.raw.clone()).collect();
    ref_lines.sort();
    let mut source_lines = table.trust_lines();
    source_lines.sort();
    let mut secret_lines = secrets.to_vec();
    secret_lines.sort();
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
    content.push_str("--secrets--\n");
    for line in &secret_lines {
        content.push_str(line);
        content.push('\n');
    }
    crate::SHA256::sha256_hex(content.as_bytes())
}

/// Extend the trust identity with typed lifecycle facts. Hooks, dotenv paths,
/// profile selection, and language-pack expansion are executable environment
/// policy just like package refs, so changing any of them invalidates the old
/// grant.
pub fn environment_definition_hash(
    refs: &[RefSpec],
    table: &SourceTable,
    secrets: &[String],
    facts: &jet_env_model::ModuleEval::EnvironmentFacts,
) -> String {
    let mut content = env_definition_hash(refs, table, secrets);
    content.push_str("\n--lifecycle--\n");
    content.push_str(&facts.lifecycle.fingerprint());
    content.push_str("--profiles--\n");
    for profile in &facts.profiles {
        content.push_str(&profile.name);
        content.push('\n');
        for parent in &profile.extends {
            content.push_str("extends=");
            content.push_str(parent);
            content.push('\n');
        }
        for package in &profile.packages {
            content.push_str("package=");
            content.push_str(package);
            content.push('\n');
        }
        for (key, value) in &profile.variables {
            content.push_str("var=");
            content.push_str(key);
            content.push('=');
            content.push_str(value);
            content.push('\n');
        }
        if let Some(hostname) = &profile.hostname {
            content.push_str("hostname=");
            content.push_str(hostname);
            content.push('\n');
        }
        if let Some(user) = &profile.user {
            content.push_str("user=");
            content.push_str(user);
            content.push('\n');
        }
    }
    content.push_str("--package-profiles--\n");
    for profile in &facts.package_profiles {
        content.push_str(&profile.name);
        content.push('\n');
        for parent in &profile.extends {
            content.push_str("extends=");
            content.push_str(parent);
            content.push('\n');
        }
        for package in &profile.packages {
            content.push_str("package=");
            content.push_str(package);
            content.push('\n');
        }
        for (path, provider) in &profile.collisions {
            content.push_str("collision=");
            content.push_str(path);
            content.push('=');
            content.push_str(provider);
            content.push('\n');
        }
        for source in &profile.sources {
            content.push_str("source=");
            content.push_str(source);
            content.push('\n');
        }
    }
    content.push_str("--environment-names--\n");
    for name in &facts.environment_names {
        content.push_str(name);
        content.push('\n');
    }
    if let Some(profile) = &facts.selected_profile {
        content.push_str("selected=");
        content.push_str(&profile.name);
        content.push('\n');
        for name in &profile.selected_profiles {
            content.push_str("selected-profile=");
            content.push_str(name);
            content.push('\n');
        }
        for applied in &profile.applied {
            content.push_str("selected-applied=");
            content.push_str(applied);
            content.push('\n');
        }
        for package in &profile.packages {
            content.push_str("selected-package=");
            content.push_str(package);
            content.push('\n');
        }
        for (key, value) in &profile.variables {
            content.push_str("selected-var=");
            content.push_str(key);
            content.push('=');
            content.push_str(value);
            content.push('\n');
        }
    }
    content.push_str("--languages--\n");
    for language in &facts.languages {
        content.push_str(&language.fingerprint());
        content.push('\n');
    }
    content.push_str("--language-expansion--\n");
    content.push_str(&facts.language_expansion.fingerprint());
    content.push_str("--language-projections--\n");
    for projection in &facts.language_projections {
        content.push_str(&projection.fingerprint());
    }
    for pack in &facts.language_packs {
        content.push_str("pack=");
        content.push_str(&pack.fingerprint());
    }
    content.push_str("--files--\n");
    let mut files = facts
        .files
        .iter()
        .map(|file| file.fingerprint())
        .collect::<Vec<_>>();
    files.sort();
    for file in files {
        content.push_str(&file);
        content.push('\n');
    }
    content.push_str("--services--\n");
    for service in &facts.dev_services {
        content.push_str(&format!("{service:?}"));
        content.push('\n');
    }
    content.push_str("--integrations--\n");
    let mut integrations = facts
        .integrations
        .iter()
        .map(|integration| integration.fingerprint())
        .collect::<Vec<_>>();
    integrations.sort();
    for integration in integrations {
        content.push_str(&integration);
    }
    content.push_str("--integration-facts--\n");
    content.push_str(&facts.integration_facts.fingerprint());
    crate::SHA256::sha256_hex(content.as_bytes())
}

pub fn environment_definition_hash_with_snapshot(
    refs: &[RefSpec],
    table: &SourceTable,
    secrets: &[String],
    facts: &jet_env_model::ModuleEval::EnvironmentFacts,
    source_snapshot: Option<&str>,
) -> String {
    let base = environment_definition_hash(refs, table, secrets, facts);
    let Some(source_snapshot) = source_snapshot else {
        return base;
    };
    let content = format!(
        "jet-env-definition-with-managed-source-v1\nbase={base}\nmanaged-source={source_snapshot}\n"
    );
    crate::SHA256::sha256_hex(content.as_bytes())
}

/// Whether this env definition is trust-sensitive at all — i.e. whether
/// entering it should ever prompt. It declares at least one package ref (any
/// external code/binary a project pulls in is a supply-chain decision), or
/// (U13) at least one declared `secrets:` name — reading a repo's encrypted
/// secrets is its own trust decision, independent of whether the env also
/// declares any packages.
pub fn is_trust_sensitive(refs: &[RefSpec]) -> bool {
    is_trust_sensitive_ext(refs, false)
}

/// U13: the general form — `secrets_declared` is whether any evaluated
/// `env.<name>` role-module in this project declares a non-empty `secrets:`
/// list. Kept as a separate function (rather than changing
/// [`is_trust_sensitive`]'s signature) so existing call sites that don't yet
/// thread a secrets flag through keep compiling unchanged.
pub fn is_trust_sensitive_ext(refs: &[RefSpec], secrets_declared: bool) -> bool {
    !refs.is_empty() || secrets_declared
}

/// Already trusted: an exact hash grant, or a pattern matching `project_dir`.
pub fn is_trusted(store: &Path, project_dir: &Path, hash: &str) -> bool {
    let project_str = project_dir.to_string_lossy();
    let canonical_project = project_dir
        .canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let target_hash_line = format!("{HASH_PREFIX}{hash}");
    for line in read_lines(store) {
        if line == target_hash_line {
            return true;
        }
        if let Some(pattern) = line.strip_prefix(PATTERN_PREFIX) {
            if matches_pattern(pattern, &project_str)
                || canonical_project
                    .as_deref()
                    .is_some_and(|subject| matches_pattern(pattern, subject))
                || matches_canonical_pattern(pattern, &project_str)
                || canonical_project
                    .as_deref()
                    .is_some_and(|subject| matches_canonical_pattern(pattern, subject))
            {
                return true;
            }
        }
    }
    false
}

/// Unified env trust check. Legacy hash/pattern grants still work; typed grants
/// feed the same gate so `jet trust grant` is not just a list/explain surface.
pub fn is_env_trusted(
    store: &Path,
    project_dir: &Path,
    hash: &str,
    refs: &[RefSpec],
    secrets: &[String],
) -> bool {
    if is_trusted(store, project_dir, hash) {
        return true;
    }
    let records = list_records(store);
    if records.iter().any(|record| match record {
        TrustRecord::Grant(grant) if grant.authority == AUTH_ENV => {
            grant_matches_project_or_hash(grant, project_dir, hash)
        }
        TrustRecord::Grant(grant) if grant.authority == AUTH_BUILD => {
            grant.subject == hash || grant.subject == format!("{HASH_PREFIX}{hash}")
        }
        _ => false,
    }) {
        return true;
    }
    if secrets.is_empty() && !refs.is_empty() {
        return refs.iter().all(|r| {
            records.iter().any(|record| match record {
                TrustRecord::Grant(grant) if grant.authority == AUTH_PACKAGE => {
                    grant.subject == r.package
                        || grant.subject == r.short_name()
                        || grant.subject == r.raw
                        || r.raw
                            .split_once(':')
                            .is_some_and(|(_, package)| grant.subject == package)
                }
                _ => false,
            })
        });
    }
    false
}

pub fn is_typed_environment(facts: &jet_env_model::ModuleEval::EnvironmentFacts) -> bool {
    !facts.environment_names.is_empty()
        || !facts.dev_services.is_empty()
        || !facts.lifecycle.dotenv.is_empty()
        || !facts.lifecycle.unset.is_empty()
        || !facts.lifecycle.on_enter.is_empty()
        || !facts.lifecycle.checks.is_empty()
        || facts.lifecycle.reload_explicit
        || !facts.profiles.is_empty()
        || !facts.package_profiles.is_empty()
        || !facts.languages.is_empty()
        || facts.selected_profile.is_some()
        || !facts.language_packs.is_empty()
        || !facts.files.is_empty()
        || !facts.integration_facts.tasks.is_empty()
        || !facts.integration_facts.task_facts.is_empty()
        || !facts.integration_facts.providers.is_empty()
        || !facts.integration_facts.host_checks.is_empty()
        || !facts.integration_facts.grants.is_empty()
        || !facts.integration_facts.losses.is_empty()
}

fn is_typed_environment_trusted(
    store: &Path,
    project_dir: &Path,
    hash: &str,
) -> bool {
    if is_trusted(store, project_dir, hash) {
        return true;
    }
    list_records(store).iter().any(|record| match record {
        TrustRecord::Grant(grant) if grant.authority == AUTH_ENV => {
            grant_matches_project_or_hash(grant, project_dir, hash)
        }
        TrustRecord::Grant(grant) if grant.authority == AUTH_BUILD => {
            grant.subject == hash || grant.subject == format!("{HASH_PREFIX}{hash}")
        }
        _ => false,
    })
}

pub fn is_environment_trusted(
    store: &Path,
    project_dir: &Path,
    hash: &str,
    refs: &[RefSpec],
    secrets: &[String],
    facts: &jet_env_model::ModuleEval::EnvironmentFacts,
) -> bool {
    if !integration_grants_trusted(store, facts) {
        return false;
    }
    if is_typed_environment(facts) {
        is_typed_environment_trusted(store, project_dir, hash)
    } else {
        is_env_trusted(store, project_dir, hash, refs, secrets)
    }
}

/// Integration authorities are separate from the broad environment hash.
/// `--trust` can approve a project definition for one run, but it cannot
/// manufacture permission to read a credential store, use MCP, or bind a
/// host provider. The user must review and persist each closed integration
/// grant explicitly.
pub fn integration_grants_trusted(
    store: &Path,
    facts: &jet_env_model::ModuleEval::EnvironmentFacts,
) -> bool {
    missing_integration_grant(store, facts).is_none()
}

fn missing_integration_grant(
    store: &Path,
    facts: &jet_env_model::ModuleEval::EnvironmentFacts,
) -> Option<String> {
    let records = list_records(store);
    facts.integration_facts.task_facts.iter().find_map(|task| {
        task.grants.iter().find_map(|grant| {
            let subject = format!("{}:{grant}", task.integration.as_str());
            (!records.iter().any(|record| {
                matches!(
                    record,
                    TrustRecord::Grant(stored)
                        if stored.authority == AUTH_INTEGRATION && stored.subject == subject
                )
            }))
            .then_some(subject)
        })
    })
}

fn grant_matches_project_or_hash(grant: &TrustGrant, project_dir: &Path, hash: &str) -> bool {
    if grant.subject == hash || grant.subject == format!("{HASH_PREFIX}{hash}") {
        return true;
    }
    let project_str = project_dir.to_string_lossy();
    if matches_pattern(&grant.subject, &project_str)
        || matches_canonical_pattern(&grant.subject, &project_str)
    {
        return true;
    }
    project_dir
        .canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .is_some_and(|canonical| {
            matches_pattern(&grant.subject, &canonical)
                || matches_canonical_pattern(&grant.subject, &canonical)
        })
}

/// Glob/prefix match: a trailing `*` is a prefix wildcard, else an exact or
/// prefix match. One wildcard shape is all U19 asks for (no glob crate, I6).
fn matches_pattern(pattern: &str, subject: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => subject.starts_with(prefix),
        None => subject == pattern || subject.starts_with(pattern),
    }
}

fn matches_canonical_pattern(pattern: &str, subject: &str) -> bool {
    let (prefix, wildcard) = match pattern.strip_suffix('*') {
        Some(prefix) => (prefix, true),
        None => (pattern, false),
    };
    let Ok(canonical_prefix) = Path::new(prefix).canonicalize() else {
        return false;
    };
    let canonical = canonical_prefix.to_string_lossy();
    if wildcard {
        subject.starts_with(canonical.as_ref())
    } else {
        subject == canonical.as_ref() || subject.starts_with(canonical.as_ref())
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

/// `jet trust grant <selector> [--scope user|repo]`. Returns `false` if the
/// exact grant already exists.
pub fn add_grant(store: &Path, grant: &TrustGrant) -> bool {
    let line = grant.line();
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

/// Typed view of the unified trust store.
pub fn list_records(store: &Path) -> Vec<TrustRecord> {
    read_lines(store)
        .into_iter()
        .map(|line| parse_record(&line))
        .collect()
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

/// Remove one trust grant, hash, pattern, or raw line selected by exact text or
/// grant key (`service:postgres.service`, etc.).
pub fn revoke(store: &Path, selector: &str) -> bool {
    let lines = read_lines(store);
    let mut removed = false;
    let mut kept = Vec::new();
    for line in lines {
        let record = parse_record(&line);
        let matches = match &record {
            TrustRecord::Hash { hash } => {
                selector == line || selector == hash || selector == format!("{HASH_PREFIX}{hash}")
            }
            TrustRecord::Pattern { pattern } => {
                selector == line
                    || selector == pattern
                    || selector == format!("{PATTERN_PREFIX}{pattern}")
            }
            TrustRecord::Grant(grant) => {
                selector == line
                    || selector == grant.subject
                    || selector == grant.key()
                    || selector == format!("{}:{}", grant.scope, grant.key())
            }
            TrustRecord::Raw { line: raw } => selector == raw,
        };
        if matches {
            removed = true;
        } else {
            kept.push(line);
        }
    }
    if removed {
        let content = kept.into_iter().map(|line| line + "\n").collect::<String>();
        let _ = std::fs::write(store, content);
    }
    removed
}

pub fn records_json(records: &[TrustRecord]) -> String {
    let mut out = String::from("{\"grants\":[");
    for (i, record) in records.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&record_json(record));
    }
    out.push_str("]}");
    out
}

pub fn record_json(record: &TrustRecord) -> String {
    match record {
        TrustRecord::Hash { hash } => format!(
            "{{\"type\":\"hash\",\"hash\":\"{}\",\"revocationKey\":\"{}{}\"}}",
            json_escape(hash),
            HASH_PREFIX,
            json_escape(hash)
        ),
        TrustRecord::Pattern { pattern } => format!(
            "{{\"type\":\"pattern\",\"pattern\":\"{}\",\"revocationKey\":\"{}{}\"}}",
            json_escape(pattern),
            PATTERN_PREFIX,
            json_escape(pattern)
        ),
        TrustRecord::Grant(grant) => format!(
            "{{\"type\":\"grant\",\"authority\":\"{}\",\"subject\":\"{}\",\"scope\":\"{}\",\"revocationKey\":\"{}\"}}",
            json_escape(&grant.authority),
            json_escape(&grant.subject),
            json_escape(&grant.scope),
            json_escape(&grant.key())
        ),
        TrustRecord::Raw { line } => format!(
            "{{\"type\":\"raw\",\"line\":\"{}\",\"revocationKey\":\"{}\"}}",
            json_escape(line),
            json_escape(line)
        ),
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
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
    secrets: &[String],
    bypass: bool,
) -> Result<(), i32> {
    let hash = env_definition_hash(refs, table, secrets);
    gate_with_hash(
        theme,
        store,
        project_dir,
        refs,
        secrets,
        bypass,
        hash,
        false,
        false,
    )
}

/// Trust gate variant for a plan whose typed lifecycle facts are part of its
/// executable identity.
pub fn gate_with_environment(
    theme: &Theme,
    store: &Path,
    project_dir: &Path,
    refs: &[RefSpec],
    table: &SourceTable,
    secrets: &[String],
    facts: &jet_env_model::ModuleEval::EnvironmentFacts,
    bypass: bool,
) -> Result<(), i32> {
    if let Some(subject) = missing_integration_grant(store, facts) {
        theme.error_coded(
            "E1335",
            "environment integration authority is not granted",
            &format!("the environment requests integration authority `{subject}`, but no persisted grant authorizes it"),
            &format!("review the integration, then run `jet trust grant integration:{subject} --scope user`"),
        );
        return Err(2);
    }
    let hash = environment_definition_hash(refs, table, secrets, facts);
    let typed = is_typed_environment(facts);
    let lifecycle_hooks = !facts.lifecycle.on_enter.is_empty() || !facts.lifecycle.checks.is_empty();
    gate_with_hash(
        theme,
        store,
        project_dir,
        refs,
        secrets,
        bypass,
        hash,
        typed,
        lifecycle_hooks,
    )
}

pub fn gate_with_environment_and_snapshot(
    theme: &Theme,
    store: &Path,
    project_dir: &Path,
    refs: &[RefSpec],
    table: &SourceTable,
    secrets: &[String],
    facts: &jet_env_model::ModuleEval::EnvironmentFacts,
    source_snapshot: Option<&str>,
    bypass: bool,
) -> Result<(), i32> {
    if let Some(subject) = missing_integration_grant(store, facts) {
        theme.error_coded(
            "E1335",
            "environment integration authority is not granted",
            &format!("the environment requests integration authority `{subject}`, but no persisted grant authorizes it"),
            &format!("review the integration, then run `jet trust grant integration:{subject} --scope user`"),
        );
        return Err(2);
    }
    let hash = environment_definition_hash_with_snapshot(
        refs,
        table,
        secrets,
        facts,
        source_snapshot,
    );
    let typed = is_typed_environment(facts);
    let lifecycle_hooks = !facts.lifecycle.on_enter.is_empty() || !facts.lifecycle.checks.is_empty();
    gate_with_hash(
        theme,
        store,
        project_dir,
        refs,
        secrets,
        bypass,
        hash,
        typed,
        lifecycle_hooks,
    )
}

/// Gate a finite build hook on its exact resolved identity. The subject comes
/// from the staged source digest, recipe digest, platform, and declared
/// capabilities; a package or environment grant cannot authorize a different
/// build graph.
pub fn gate_build_identity(
    theme: &Theme,
    store: &Path,
    identity: &str,
    bypass: bool,
) -> Result<(), i32> {
    let ci = std::env::var_os("CI").is_some();
    if ci && bypass {
        theme.error_coded(
            "E1255",
            "CI cannot bypass build-hook approval",
            "a build hook must use its exact package, provider/source, staged source, platform, recipe, and capability identity",
            &format!(
                "add the exact repository grant: `jet trust grant build:{identity} --scope repo`."
            ),
        );
        return Err(2);
    }
    let trusted = list_records(store).iter().any(|record| {
        matches!(
            record,
            TrustRecord::Grant(grant)
                if grant.authority == AUTH_BUILD
                    && (!ci || grant.scope == Syntax::TRUST_SCOPE_REPO)
                    && (grant.subject == identity
                        || grant.subject == format!("{HASH_PREFIX}{identity}"))
        )
    });
    if trusted {
        return Ok(());
    }
    if bypass {
        return Ok(());
    }
    if ci {
        theme.error_coded(
            "E1255",
            "this build hook is not approved for CI",
            "CI accepts only an exact repository-scoped grant for the complete package, provider/source, staged source, platform, recipe, and capability identity",
            &format!(
                "add the exact repository grant: `jet trust grant build:{identity} --scope repo`."
            ),
        );
        return Err(2);
    }
    if !std::io::stdin().is_terminal() {
        theme.error_coded(
            "E1255",
            "this build hook is not trusted yet",
            "the build action graph has a new source, recipe, platform, or capability identity and stdin is not a terminal to ask interactively",
            "pass `--trust` for this build, or pre-authorize the exact build identity with `jet trust grant`.",
        );
        return Err(2);
    }
    theme.note("first build for this exact action graph:");
    theme.detail(&format!("build identity: {identity}"));
    eprint!("  trust this build? [y/N] ");
    {
        use std::io::Write;
        let _ = std::io::stderr().flush();
    }
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return Err(2);
    }
    if matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        add_grant(
            store,
            &TrustGrant {
                authority: AUTH_BUILD.to_string(),
                subject: identity.to_string(),
                scope: Syntax::TRUST_SCOPE_USER.to_string(),
            },
        );
        Ok(())
    } else {
        theme.status("not trusted — exiting.");
        Err(2)
    }
}

fn gate_with_hash(
    theme: &Theme,
    store: &Path,
    project_dir: &Path,
    refs: &[RefSpec],
    secrets: &[String],
    bypass: bool,
    hash: String,
    typed: bool,
    lifecycle_hooks: bool,
) -> Result<(), i32> {
    if !is_trust_sensitive_ext(refs, !secrets.is_empty()) && !lifecycle_hooks {
        return Ok(());
    }
    let trusted = if typed {
        is_typed_environment_trusted(store, project_dir, &hash)
    } else {
        is_env_trusted(store, project_dir, &hash, refs, secrets)
    };
    if bypass || trusted {
        return Ok(());
    }
    if let Some(policy) = project_trust_policy(project_dir) {
        use super::PackageManifest::TrustDecision;
        let ci = std::env::var_os("CI").is_some();
        let decision = if ci {
            policy.ci_prompt.or(policy.default)
        } else {
            policy.default
        };
        match decision {
            Some(TrustDecision::Allow) => return Ok(()),
            Some(TrustDecision::Deny) => {
                theme.error_coded(
                    "E1255",
                    "this project's trust policy denies the environment grant",
                    "`policy.trust` is source-reviewed and told Jet not to prompt for this authority",
                    "change `policy.trust` to `prompt`/`allow`, pass `--trust` for this one run, or remove the trust-sensitive env facts.",
                );
                return Err(2);
            }
            Some(TrustDecision::Prompt) | None => {}
        }
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
        "first entry to this project — it declares {} package(s) and {} secret(s):",
        refs.len(),
        secrets.len()
    ));
    // One aligned row per ref: bold package, gray source. The supply-chain
    // decision is *which sources* run code here, so the source column is the
    // thing this prompt exists to surface.
    let name_w = refs
        .iter()
        .map(|r| r.package.len())
        .max()
        .unwrap_or(0)
        .max(8);
    for r in refs {
        let source = r
            .raw
            .strip_suffix(&r.package)
            .map(|s| s.trim_end_matches(':').to_string())
            .unwrap_or_else(|| r.raw.clone());
        theme.detail(&format!(
            "{}  {}",
            theme.bold(&format!("{:<name_w$}", r.package)),
            theme.gray(&source)
        ));
    }
    for s in secrets {
        theme.detail(&format!("secret:{s}"));
    }
    eprintln!(
        "\n  {}",
        theme.gray("a yes is remembered for this exact env; any change asks again.")
    );
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

fn project_trust_policy(project_dir: &Path) -> Option<super::PackageManifest::TrustPolicy> {
    super::PackageManifest::PackManifest::load(project_dir)
        .and_then(Result::ok)
        .and_then(|m| m.trust_policy)
}

/// A stable hash over a foreign flake/devenv file's content (U16) — the same
/// role `env_definition_hash` plays for a declared env, but for untrusted
/// input jetpack didn't write: an arbitrary `flake.nix`/`devenv.nix` is still
/// untrusted evaluator input, so a first encounter needs the same trust
/// decision (D-JPK-DEVCOMPOSE1's rationale
/// extended to U16's two new untrusted-input surfaces, `-p` ad-hoc packages
/// and a foreign flake).
pub fn flake_definition_hash(content: &str) -> String {
    format!("flake:{}", crate::SHA256::sha256_hex(content.as_bytes()))
}

/// The trust gate for a foreign flake/devenv file (U16) — `jet env`'s
/// foreign-flake projection and `jet bridge flake` both reach this before
/// native evaluation. Same store, same hash-grant/pattern machinery, same
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
                "entering `{}` evaluates a foreign flake this project didn't declare through \
                 `env.*`; a first entry needs a trust decision, and stdin isn't a \
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
    use super::super::RefSpec::Source;
    use super::*;

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
            package: raw
                .split_once('@')
                .map(|(package, _)| package)
                .unwrap_or(raw)
                .to_string(),
            raw: raw.to_string(),
        }
    }

    #[test]
    fn canonical_build_identity_selector_uses_build_authority() {
        let grant = parse_grant_selector(
            "build-sha256:0123456789abcdef",
            Syntax::TRUST_SCOPE_REPO,
        )
        .unwrap();
        assert_eq!(grant.authority, AUTH_BUILD);
        assert_eq!(grant.subject, "build-sha256:0123456789abcdef");
    }

    #[test]
    fn empty_refs_are_never_trust_sensitive() {
        assert!(!is_trust_sensitive(&[]));
        assert!(is_trust_sensitive_ext(&[], true));
    }

    #[test]
    fn nonempty_refs_are_trust_sensitive() {
        assert!(is_trust_sensitive(&[ref_spec("fastfetch@nixpkgs")]));
    }

    #[test]
    fn hash_is_stable_and_order_independent() {
        let table = SourceTable::empty();
        let a = env_definition_hash(
            &[ref_spec("a@nixpkgs"), ref_spec("b@nixpkgs")],
            &table,
            &["stripe".to_string(), "db".to_string()],
        );
        let b = env_definition_hash(
            &[ref_spec("b@nixpkgs"), ref_spec("a@nixpkgs")],
            &table,
            &["db".to_string(), "stripe".to_string()],
        );
        assert_eq!(a, b);
    }

    #[test]
    fn hash_changes_when_refs_change() {
        let table = SourceTable::empty();
        let a = env_definition_hash(&[ref_spec("a@nixpkgs")], &table, &[]);
        let b = env_definition_hash(&[ref_spec("a@nixpkgs"), ref_spec("b@nixpkgs")], &table, &[]);
        assert_ne!(a, b);
    }

    #[test]
    fn hash_changes_when_secrets_change() {
        let table = SourceTable::empty();
        let refs = [ref_spec("a@nixpkgs")];
        let a = env_definition_hash(&refs, &table, &["stripe".to_string()]);
        let b = env_definition_hash(&refs, &table, &["db".to_string()]);
        assert_ne!(a, b);
    }

    #[test]
    fn hash_grant_round_trips() {
        let dir = scratch("hashgrant");
        let store = dir.join("trust");
        let table = SourceTable::empty();
        let refs = [ref_spec("fastfetch@nixpkgs")];
        let hash = env_definition_hash(&refs, &table, &[]);
        assert!(!is_trusted(&store, &dir, &hash));
        grant_hash(&store, &hash);
        assert!(is_trusted(&store, &dir, &hash));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn typed_env_build_and_package_grants_authorize_env_gate() {
        let dir = scratch("typed_grants");
        let store = dir.join("trust");
        let table = SourceTable::empty();
        let refs = [ref_spec("fastfetch@nixpkgs")];
        let hash = env_definition_hash(&refs, &table, &[]);

        add_grant(
            &store,
            &TrustGrant {
                authority: AUTH_PACKAGE.to_string(),
                subject: "fastfetch".to_string(),
                scope: Syntax::TRUST_SCOPE_USER.to_string(),
            },
        );
        assert!(is_env_trusted(&store, &dir, &hash, &refs, &[]));
        assert!(!is_env_trusted(
            &store,
            &dir,
            &hash,
            &refs,
            &["db_password".to_string()]
        ));

        let store = dir.join("trust-build");
        add_grant(
            &store,
            &TrustGrant {
                authority: AUTH_BUILD.to_string(),
                subject: hash.clone(),
                scope: Syntax::TRUST_SCOPE_USER.to_string(),
            },
        );
        assert!(is_env_trusted(
            &store,
            &dir,
            &hash,
            &refs,
            &["db_password".to_string()]
        ));

        let store = dir.join("trust-env");
        add_grant(
            &store,
            &TrustGrant {
                authority: AUTH_ENV.to_string(),
                subject: format!("{}*", dir.display()),
                scope: Syntax::TRUST_SCOPE_REPO.to_string(),
            },
        );
        assert!(is_env_trusted(
            &store,
            &dir,
            &hash,
            &refs,
            &["db_password".to_string()]
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn vault_write_grant_keeps_its_exact_authority_and_repository_uuid() {
        let uuid = "00112233445566778899aabbccddeeff";
        let grant = parse_grant_selector(
            &format!("vault.write:{uuid}"),
            Syntax::TRUST_SCOPE_USER,
        )
        .unwrap();
        assert_eq!(grant.authority, AUTH_VAULT_WRITE);
        assert_eq!(grant.subject, uuid);
        assert_eq!(grant.line(), format!("grant:user:vault.write:{uuid}"));
    }

    #[test]
    fn policy_trust_default_allow_and_deny_feed_gate() {
        let allow_dir = scratch("policy_allow");
        std::fs::write(
            allow_dir.join(Syntax::PAYLOAD_FILE),
            "payload: { name: \"app\", version: \"0.1.0\" }\npolicy: { trust: { default: allow } }\n",
        )
        .unwrap();
        let deny_dir = scratch("policy_deny");
        std::fs::write(
            deny_dir.join(Syntax::PAYLOAD_FILE),
            "payload: { name: \"app\", version: \"0.1.0\" }\npolicy: { trust: { default: deny } }\n",
        )
        .unwrap();
        let refs = [ref_spec("fastfetch@nixpkgs")];
        let table = SourceTable::empty();
        let theme = Theme::resolve_choice(jet_foundation::Terminal::ColorChoice::Never);

        assert!(gate(
            &theme,
            &allow_dir.join("trust"),
            &allow_dir,
            &refs,
            &table,
            &[],
            false
        )
        .is_ok());
        assert!(gate(
            &theme,
            &deny_dir.join("trust"),
            &deny_dir,
            &refs,
            &table,
            &[],
            false
        )
        .is_err());
        std::fs::remove_dir_all(&allow_dir).ok();
        std::fs::remove_dir_all(&deny_dir).ok();
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

    #[test]
    fn pattern_matching_tolerates_canonical_path_aliases() {
        let dir = scratch("canonical_alias");
        let store = dir.join("trust");
        let lexical = dir.to_string_lossy().to_string();
        add_pattern(&store, &format!("{lexical}*"));
        let canonical = dir.canonicalize().unwrap();
        assert!(is_trusted(&store, &canonical, "h"));
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
        let theme = Theme::resolve_choice(jet_foundation::Terminal::ColorChoice::Never);
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
