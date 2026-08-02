//! Pure overlay policy data and parse/strip (D-JPK-OVERLAY1=A).
//!
//! This is the read-only data model layer for workspace overlay policies: the
//! typed data structures (`OverlayPolicy`, `OverlaySet`, `PackageOverride`,
//! `ProviderOverride`) and the source-level parse/strip helpers
//! (`parse_workspace_policy`, `strip_overlay_policy`) used by `WorkspaceFile`
//! evaluation. No network, shell, or engine code lives here.
//!
//! Engine functions (`apply_overlay_patches`, `semantic_records`,
//! `policy_fingerprints`, `invalidations_against`, `draft_overlay_source`)
//! live in `jetpack::Overlay`, which imports these types from here and adds
//! its own `SemanticLock`/patch-application machinery on top.

use crate::Syntax;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverlayPolicy {
    pub overlays: Vec<OverlaySet>,
    pub allow_unfree: Vec<String>,
    /// D-CTEFFECT1 workspace ceiling for programmable builds.
    pub build_deny: Vec<String>,
    /// D-BUILDSCOPE1 per-package/dependency grants inside the workspace
    /// ceiling. The package name is the authority subject, never a path or
    /// an arbitrary value supplied by build code.
    pub build_grants: Vec<(String, Vec<String>)>,
}

impl OverlayPolicy {
    pub fn is_empty(&self) -> bool {
        self.overlays.is_empty()
            && self.allow_unfree.is_empty()
            && self.build_deny.is_empty()
            && self.build_grants.is_empty()
    }

    pub fn overlay(&self, name: &str) -> Option<&OverlaySet> {
        self.overlays.iter().find(|o| o.name == name)
    }

    pub fn package_override(&self, overlay: &str, package: &str) -> Option<&PackageOverride> {
        self.overlay(overlay)?
            .packages
            .iter()
            .find(|p| p.package == package)
    }

    pub fn allows_unfree(&self, package: &str) -> bool {
        self.allow_unfree.iter().any(|p| p == package)
            || self
                .overlays
                .iter()
                .flat_map(|o| &o.packages)
                .any(|p| p.package == package && p.allow_unfree)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlaySet {
    pub name: String,
    pub provider: Option<ProviderOverride>,
    pub packages: Vec<PackageOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderOverride {
    pub provider: String,
    pub channel: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOverride {
    pub package: String,
    pub source: Option<String>,
    pub version: Option<String>,
    pub flags: Vec<String>,
    /// D-JOS-PRIORITY1 tier for scalar/environment facts in this record.
    /// Ordinary is `0`; `Default`, `Force`, and `Priority(n)` are explicit.
    pub priority: i32,
    /// Environment overrides are stored as typed key/value facts.  The
    /// keyed-record parser and the legacy dotted spelling both lower here so
    /// provider consumers never need a second representation.
    pub env: Vec<(String, String)>,
    pub patches: Vec<String>,
    pub allow_unfree: bool,
}

impl PackageOverride {
    pub fn new(package: String) -> PackageOverride {
        PackageOverride {
            package,
            source: None,
            version: None,
            flags: Vec::new(),
            priority: 0,
            env: Vec::new(),
            patches: Vec::new(),
            allow_unfree: false,
        }
    }

    pub fn policy_fingerprint(&self, overlay: &OverlaySet) -> String {
        let provider = overlay
            .provider
            .as_ref()
            .map(|p| {
                p.channel
                    .as_ref()
                    .map(|c| format!("{}#{c}", p.provider))
                    .unwrap_or_else(|| p.provider.clone())
            })
            .unwrap_or_else(|| "provider:unchanged".to_string());
        let mut env = self.env.clone();
        env.sort();
        format!(
            "workspace.overlay.{}:{}:{}:{}:{}:{}:{}:{}",
            overlay.name,
            self.package,
            provider,
            format!(
                "{}@{}",
                self.version.as_deref().unwrap_or("version:unchanged"),
                self.priority
            ),
            self.source.as_deref().unwrap_or("source:unchanged"),
            self.flags.join("+"),
            env
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("+"),
            self.patches.join("+")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchApplication {
    pub path: String,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayError {
    Malformed(String),
    IO(String),
    Patch(String),
}

impl OverlayError {
    pub fn message(&self) -> &str {
        match self {
            OverlayError::Malformed(s) | OverlayError::IO(s) | OverlayError::Patch(s) => s,
        }
    }
}

pub fn parse_workspace_policy(src: &str) -> Result<OverlayPolicy, OverlayError> {
    let Some(body) = workspace_body(src) else {
        return Ok(OverlayPolicy::default());
    };
    let mut policy = OverlayPolicy::default();
    policy.allow_unfree = parse_allow_unfree(&body)?;
    policy.build_deny = parse_build_deny(&body)?;
    policy.build_grants = parse_build_grants(&body)?;
    let mut pos = 0;
    while let Some(rel) = body[pos..].find(Syntax::WORKSPACE_OVERLAY) {
        let at = pos + rel;
        if !word_boundary_before(&body, at) {
            pos = at + Syntax::WORKSPACE_OVERLAY.len();
            continue;
        }
        let rest = body[at + Syntax::WORKSPACE_OVERLAY.len()..].trim_start();
        let (name, after_name) = read_ident(rest).ok_or_else(|| {
            OverlayError::Malformed("`overlay` needs a name in `workspace.jet`".to_string())
        })?;
        let after_name = after_name.trim_start();
        let block_src = after_name.strip_prefix('{').ok_or_else(|| {
            OverlayError::Malformed(format!("`overlay {name}` needs a `{{ … }}` body"))
        })?;
        let (overlay_body, consumed) = balanced_with_len(block_src, '{', '}');
        let parsed = parse_overlay_set(name, &overlay_body)?;
        if policy.overlays.iter().any(|overlay| overlay.name == parsed.name) {
            return Err(OverlayError::Malformed(format!(
                "overlay `{}` is declared more than once",
                parsed.name
            )));
        }
        policy.overlays.push(parsed);
        pos = at + Syntax::WORKSPACE_OVERLAY.len() + rest.len() - after_name.len() + 1 + consumed;
    }
    Ok(policy)
}

fn parse_overlay_set(name: String, body: &str) -> Result<OverlaySet, OverlayError> {
    let mut overlay = OverlaySet {
        name,
        provider: None,
        packages: Vec::new(),
    };
    for line in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with("//") {
            continue;
        }
        if let Some(value) = line.strip_prefix("provider:") {
            overlay.provider = Some(parse_provider_override(value.trim())?);
            continue;
        }
        if let Some(pkg) = parse_package_override_line(line)? {
            merge_package_override(&mut overlay.packages, pkg);
        }
    }
    if let Some(overrides) = named_record(body, "overrides")? {
        for package in parse_keyed_overrides(&overrides)? {
            merge_package_override(&mut overlay.packages, package);
        }
    }
    Ok(overlay)
}

/// Parse the ratified keyed override record:
///
/// ```text
/// overrides: { mpv: .{ version: "0.38.0", flags: .{ vapoursynth: true } } }
/// ```
///
/// This is intentionally a source-level parser. `workspace.jet` policy is
/// stripped before Jet's normal evaluator runs, so accepting the record here
/// keeps policy facts out of the compiler's ordinary module evaluator while
/// still giving every consumer one typed `PackageOverride` value.
fn parse_keyed_overrides(body: &str) -> Result<Vec<PackageOverride>, OverlayError> {
    let mut packages = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for entry in top_level_commas(body) {
        let entry = entry.trim();
        let Some((raw_package, raw_record)) = split_top_level_colon(entry) else {
            return Err(OverlayError::Malformed(
                "each `overrides` entry needs `<package>: .{ … }`".to_string(),
            ));
        };
        let package = unquote(raw_package.trim());
        if package.is_empty() {
            return Err(OverlayError::Malformed(
                "`overrides` package names cannot be empty".to_string(),
            ));
        }
        if !names.insert(package.clone()) {
            return Err(OverlayError::Malformed(format!(
                "override `{package}` is declared more than once"
            )));
        }
        let record = raw_record.trim();
        let Some(record) = record.strip_prefix(".{") else {
            return Err(OverlayError::Malformed(format!(
                "override `{package}` must use the standard `.{{ … }}` record"
            )));
        };
        let Some(record) = record.strip_suffix('}') else {
            return Err(OverlayError::Malformed(format!(
                "override `{package}` record is not closed"
            )));
        };
        packages.push(parse_keyed_override_record(package, record)?);
    }
    Ok(packages)
}

fn parse_keyed_override_record(
    package: String,
    body: &str,
) -> Result<PackageOverride, OverlayError> {
    let mut result = PackageOverride::new(package);
    let mut seen = std::collections::BTreeMap::<String, String>::new();
    for entry in top_level_commas(body) {
        let entry = entry.trim();
        let Some((field, value)) = split_top_level_colon(entry) else {
            return Err(OverlayError::Malformed(
                "each package override field needs `name: value`".to_string(),
            ));
        };
        let field = field.trim();
        let value = value.trim();
        if let Some(previous) = seen.get(field) {
            if previous != value {
                return Err(OverlayError::Malformed(format!(
                    "override `{}` has conflicting `{field}` fields",
                    result.package
                )));
            }
            continue;
        }
        seen.insert(field.to_string(), value.to_string());
        match field {
            "source" => {
                let (source, priority) = parse_priority_value(value)?;
                result.source = Some(source);
                result.priority = result.priority.max(priority);
            }
            "version" => {
                let (version, priority) = parse_priority_value(value)?;
                result.version = Some(version);
                result.priority = result.priority.max(priority);
            }
            "flags" => result.flags = parse_flag_record(value)?,
            "env" => result.env = parse_env_record(value)?,
            "patches" => result.patches = parse_patch_list(value)?,
            "allowUnfree" => result.allow_unfree = parse_bool(value)?,
            other => {
                return Err(OverlayError::Malformed(format!(
                    "unknown package override field `{other}`"
                )))
            }
        }
    }
    Ok(result)
}

fn parse_priority_value(raw: &str) -> Result<(String, i32), OverlayError> {
    let raw = raw.trim().trim_end_matches(',');
    for (wrapper, priority) in [("Force(", 100), ("Default(", -100)] {
        if let Some(inner) = raw.strip_prefix(wrapper).and_then(|v| v.strip_suffix(')')) {
            return Ok((unquote(inner), priority));
        }
    }
    if let Some(inner) = raw
        .strip_prefix("Priority(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let (weight, value) = if let Some((weight, value)) = split_top_level_colon(inner) {
            (weight.trim().to_string(), value.trim().to_string())
        } else {
            let parts = top_level_commas(inner);
            if parts.len() < 2 {
                return Err(OverlayError::Malformed(
                    "`Priority(n)` override needs `n: value`".to_string(),
                ));
            }
            (parts[0].trim().to_string(), parts[1..].join(",").trim().to_string())
        };
        let priority = weight.parse::<i32>().map_err(|_| {
            OverlayError::Malformed("`Priority(n)` needs an integer weight".to_string())
        })?;
        return Ok((unquote(&value), priority));
    }
    Ok((unquote(raw), 0))
}

fn parse_flag_record(raw: &str) -> Result<Vec<String>, OverlayError> {
    let raw = raw.trim();
    let Some(inner) = raw.strip_prefix(".{").and_then(|v| v.strip_suffix('}')) else {
        return Err(OverlayError::Malformed(
            "`flags` must be a typed record like `.{ feature: true }`".to_string(),
        ));
    };
    let mut flags = Vec::new();
    for entry in top_level_commas(inner) {
        let Some((name, value)) = split_top_level_colon(entry.trim()) else {
            return Err(OverlayError::Malformed(
                "each `flags` entry needs `name: true|false`".to_string(),
            ));
        };
        match parse_bool(value)? {
            true => flags.push(unquote(name.trim())),
            false => {}
        }
    }
    Ok(flags)
}

fn parse_env_record(raw: &str) -> Result<Vec<(String, String)>, OverlayError> {
    let raw = raw.trim();
    let Some(inner) = raw.strip_prefix(".{").and_then(|v| v.strip_suffix('}')) else {
        return Err(OverlayError::Malformed(
            "`env` must be a typed record like `.{ CC: \"clang\" }`".to_string(),
        ));
    };
    let mut values = Vec::new();
    for entry in top_level_commas(inner) {
        let Some((key, value)) = split_top_level_colon(entry.trim()) else {
            return Err(OverlayError::Malformed(
                "each `env` entry needs `NAME: \"value\"`".to_string(),
            ));
        };
        let key = unquote(key.trim());
        if !valid_env_name(&key) {
            return Err(OverlayError::Malformed(
                format!("`env` variable name `{key}` is invalid"),
            ));
        }
        let value = unquote(value);
        if let Some((_, previous)) = values.iter().find(|(existing, _)| existing == &key) {
            if previous != &value {
                return Err(OverlayError::Malformed(format!(
                    "`env.{key}` is declared with conflicting values"
                )));
            }
            continue;
        }
        values.push((key, value));
    }
    Ok(values)
}

fn named_record(body: &str, name: &str) -> Result<Option<String>, OverlayError> {
    let Some(rel) = body.find(name) else {
        return Ok(None);
    };
    let at = rel;
    let after = at + name.len();
    if !word_boundary_before(body, at)
        || body[after..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Ok(None);
    }
    let rest = body[after..].trim_start();
    let Some(rest) = rest.strip_prefix(':').map(str::trim_start) else {
        return Err(OverlayError::Malformed(format!("`{name}` needs `:`")));
    };
    let Some(rest) = rest.strip_prefix('{') else {
        return Err(OverlayError::Malformed(format!(
            "`{name}` must be a record"
        )));
    };
    Ok(Some(balanced_policy_body(rest)?))
}

fn parse_provider_override(raw: &str) -> Result<ProviderOverride, OverlayError> {
    let raw = raw.trim().trim_end_matches(',');
    let Some(rest) = raw.strip_prefix("Provider.") else {
        return Err(OverlayError::Malformed(
            "`provider:` must use `Provider.<name>(channel: \"...\")`".to_string(),
        ));
    };
    let (provider, args) = match rest.split_once('(') {
        Some((name, args)) => (name.trim(), Some(args.trim_end_matches(')').trim())),
        None => (rest.trim(), None),
    };
    if provider.is_empty() {
        return Err(OverlayError::Malformed(
            "`provider:` needs a provider name".to_string(),
        ));
    }
    let channel = args.and_then(|args| {
        args.split_once("channel:")
            .map(|(_, v)| unquote(v.trim().trim_end_matches(',')))
    });
    Ok(ProviderOverride {
        provider: provider.to_string(),
        channel,
    })
}

fn parse_package_override_line(line: &str) -> Result<Option<PackageOverride>, OverlayError> {
    let Some(rest) = line.strip_prefix("package(") else {
        return Ok(None);
    };
    let (raw_pkg, rest) = rest.split_once(')').ok_or_else(|| {
        OverlayError::Malformed("`package(...)` override needs a closing `)`".to_string())
    })?;
    let package = unquote(raw_pkg);
    let mut pkg = PackageOverride::new(package);
    let rest = rest.trim().trim_end_matches(',');
    let Some(field_rest) = rest.strip_prefix('.') else {
        return Err(OverlayError::Malformed(
            "`package(...)` override needs a field like `.patches`".to_string(),
        ));
    };
    let (field, op_value) = if let Some((field, value)) = field_rest.split_once("+=") {
        (field.trim(), ("+=", value.trim()))
    } else if let Some((field, value)) = field_rest.split_once(':') {
        (field.trim(), (":", value.trim()))
    } else if let Some((field, value)) = field_rest.split_once('=') {
        (field.trim(), ("=", value.trim()))
    } else {
        return Err(OverlayError::Malformed(
            "`package(...)` override needs `:`, `=`, or `+=`".to_string(),
        ));
    };
    match field {
        "patches" => pkg.patches = parse_patch_list(op_value.1)?,
        "flags" => pkg.flags = parse_string_list(op_value.1)?,
        "env" => pkg.env = parse_env_record(op_value.1)?,
        "version" => {
            let (version, priority) = parse_priority_value(op_value.1)?;
            pkg.version = Some(version);
            pkg.priority = priority;
        }
        "source" => {
            let (source, priority) = parse_priority_value(op_value.1)?;
            pkg.source = Some(source);
            pkg.priority = priority;
        }
        "allowUnfree" => pkg.allow_unfree = parse_bool(op_value.1)?,
        other => {
            return Err(OverlayError::Malformed(format!(
                "unknown package override field `{other}`"
            )));
        }
    }
    Ok(Some(pkg))
}

fn parse_patch_list(raw: &str) -> Result<Vec<String>, OverlayError> {
    parse_list(raw)?
        .into_iter()
        .map(|entry| {
            let entry = entry.trim();
            if let Some(inner) = entry
                .strip_prefix("patch(")
                .and_then(|s| s.strip_suffix(')'))
            {
                Ok(unquote(inner))
            } else {
                Ok(unquote(entry))
            }
        })
        .collect()
}

fn parse_string_list(raw: &str) -> Result<Vec<String>, OverlayError> {
    parse_list(raw).map(|items| {
        items
            .into_iter()
            .map(|s| unquote(s.trim()))
            .collect::<Vec<_>>()
    })
}

fn parse_list(raw: &str) -> Result<Vec<String>, OverlayError> {
    let raw = raw.trim().trim_end_matches(',');
    let inner = raw
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| OverlayError::Malformed("expected a list literal".to_string()))?;
    Ok(top_level_commas(inner))
}

fn parse_bool(raw: &str) -> Result<bool, OverlayError> {
    match raw.trim().trim_end_matches(',') {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(OverlayError::Malformed(format!(
            "`{other}` is not a boolean"
        ))),
    }
}

fn parse_allow_unfree(body: &str) -> Result<Vec<String>, OverlayError> {
    let Some((_, rest)) = body.split_once("policy.allowUnfree") else {
        return Ok(Vec::new());
    };
    let Some((_, value)) = rest.split_once(':') else {
        return Err(OverlayError::Malformed(
            "`policy.allowUnfree` needs `:`".to_string(),
        ));
    };
    parse_string_list(value.lines().next().unwrap_or(value))
}

fn parse_build_deny(body: &str) -> Result<Vec<String>, OverlayError> {
    let Some(policy_body) = named_block(body, Syntax::MANIFEST_BLOCK_POLICY)? else {
        return Ok(Vec::new());
    };
    let Some(raw) = exact_field_value(&policy_body, Syntax::EFFECTS_FIELD_DENY)? else {
        return Ok(Vec::new());
    };
    let inner = raw
        .trim()
        .strip_prefix("#(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| OverlayError::Malformed(
            "`policy.deny:` must be an effect tuple like `#(Net, Exec)`".to_string(),
        ))?;
    Ok(top_level_commas(inner)
        .into_iter()
        .map(|value| unquote(value.trim()))
        .collect())
}

fn parse_build_grants(body: &str) -> Result<Vec<(String, Vec<String>)>, OverlayError> {
    let Some(policy_body) = named_block(body, Syntax::MANIFEST_BLOCK_POLICY)? else {
        return Ok(Vec::new());
    };
    let Some(raw) = exact_field_value(&policy_body, Syntax::MANIFEST_BLOCK_GRANTS)? else {
        return Ok(Vec::new());
    };
    let raw = raw.trim();
    let inner = raw
        .strip_prefix(".{")
        .and_then(|value| value.strip_suffix('}'))
        .or_else(|| raw.strip_prefix('{').and_then(|value| value.strip_suffix('}')))
        .ok_or_else(|| {
            OverlayError::Malformed(
                "`policy.grants:` must be a package map like `.{ \"pkg\": #(Net) }`"
                    .to_string(),
            )
        })?;
    let mut grants = Vec::new();
    for entry in top_level_commas(inner) {
        let entry = entry.trim();
        let Some((subject, effects)) = split_top_level_colon(entry) else {
            return Err(OverlayError::Malformed(
                "each `policy.grants` entry needs `\"package\": #(…)`".to_string(),
            ));
        };
        let subject = unquote(subject.trim());
        if subject.trim().is_empty() {
            return Err(OverlayError::Malformed(
                "`policy.grants` package names cannot be empty".to_string(),
            ));
        }
        let effects = parse_effect_tuple(effects.trim(), "policy.grants")?;
        grants.push((subject, effects));
    }
    Ok(grants)
}

fn parse_effect_tuple(raw: &str, field: &str) -> Result<Vec<String>, OverlayError> {
    let inner = raw
        .strip_prefix("#(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| {
            OverlayError::Malformed(format!(
                "`{field}:` must contain an effect tuple like `#(Net, Exec)`"
            ))
        })?;
    let effects = top_level_commas(inner)
        .into_iter()
        .map(|value| unquote(value.trim()))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if effects.is_empty() {
        return Err(OverlayError::Malformed(format!(
            "`{field}:` must grant at least one effect"
        )));
    }
    Ok(effects)
}

fn named_block(body: &str, name: &str) -> Result<Option<String>, OverlayError> {
    let mut pos = 0;
    while let Some(rel) = body[pos..].find(name) {
        let at = pos + rel;
        let after = at + name.len();
        let before_ok = word_boundary_before(body, at);
        let after_ok = !body[after..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        if before_ok && after_ok && code_depth_at(body, at) == Some(0) {
            let rest = body[after..].trim_start();
            if let Some(rest) = rest.strip_prefix(':') {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('.') {
                    if let Some(inner) = rest.trim_start().strip_prefix('{') {
                        return balanced_policy_body(inner).map(Some);
                    }
                }
            }
        }
        pos = after;
    }
    Ok(None)
}

fn code_depth_at(source: &str, stop: usize) -> Option<i32> {
    let mut depth = 0;
    let mut quoted = false;
    let mut escaped = false;
    let mut comment = false;
    let mut chars = source.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if index >= stop { break; }
        if comment {
            if ch == '\n' { comment = false; }
            continue;
        }
        if quoted {
            if escaped { escaped = false; }
            else if ch == '\\' { escaped = true; }
            else if ch == '"' { quoted = false; }
            continue;
        }
        if ch == '/' && chars.peek().is_some_and(|(_, next)| *next == '/') {
            comment = true;
        } else if ch == '"' { quoted = true; }
        else if matches!(ch, '(' | '[' | '{') { depth += 1; }
        else if matches!(ch, ')' | ']' | '}') { depth -= 1; }
    }
    (!quoted && !comment).then_some(depth)
}

fn balanced_policy_body(source: &str) -> Result<String, OverlayError> {
    let mut depth = 1i32;
    let mut quoted = false;
    let mut escaped = false;
    let mut comment = false;
    let mut chars = source.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if comment {
            if ch == '\n' { comment = false; }
            continue;
        }
        if quoted {
            if escaped { escaped = false; }
            else if ch == '\\' { escaped = true; }
            else if ch == '"' { quoted = false; }
            continue;
        }
        if ch == '/' && chars.peek().is_some_and(|(_, next)| *next == '/') { comment = true; }
        else if ch == '"' { quoted = true; }
        else if ch == '{' { depth += 1; }
        else if ch == '}' {
            depth -= 1;
            if depth == 0 { return Ok(source[..index].to_string()); }
        }
    }
    Err(OverlayError::Malformed("unclosed `policy: .{ … }` block".to_string()))
}

fn exact_field_value(body: &str, field: &str) -> Result<Option<String>, OverlayError> {
    for entry in top_level_commas(body) {
        let entry = entry.trim();
        if let Some((name, value)) = split_top_level_colon(entry) {
            if name.trim() == field {
                return Ok(Some(value.trim().to_string()));
            }
        } else if entry.split_whitespace().next() == Some(field) {
            return Err(OverlayError::Malformed(format!("`policy.{field}` needs `:`")));
        }
    }
    Ok(None)
}

fn split_top_level_colon(value: &str) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => return Some((&value[..index], &value[index + 1..])),
            _ => {}
        }
    }
    None
}

fn merge_package_override(packages: &mut Vec<PackageOverride>, incoming: PackageOverride) {
    if let Some(existing) = packages.iter_mut().find(|p| p.package == incoming.package) {
        if incoming.priority >= existing.priority && incoming.source.is_some() {
            existing.source = incoming.source;
        }
        if incoming.priority >= existing.priority && incoming.version.is_some() {
            existing.version = incoming.version;
        }
        existing.flags.extend(incoming.flags);
        for (key, value) in incoming.env {
            if let Some(existing_value) = existing.env.iter_mut().find(|(k, _)| k == &key) {
                if incoming.priority >= existing.priority {
                    existing_value.1 = value;
                }
            } else {
                existing.env.push((key, value));
            }
        }
        existing.patches.extend(incoming.patches);
        existing.allow_unfree |= incoming.allow_unfree;
        existing.priority = existing.priority.max(incoming.priority);
        existing.flags.sort();
        existing.flags.dedup();
        existing.env.sort();
        existing.patches.sort();
        existing.patches.dedup();
    } else {
        let mut incoming = incoming;
        incoming.flags.sort();
        incoming.flags.dedup();
        incoming.env.sort();
        incoming.patches.sort();
        incoming.patches.dedup();
        packages.push(incoming);
    }
}

fn valid_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

pub fn strip_overlay_policy(src: &str) -> String {
    let Some(body_start) = find_workspace_body_start(src) else {
        return src.to_string();
    };
    let (body, consumed) = balanced_with_len(&src[body_start + 1..], '{', '}');
    let stripped_body = strip_overlay_blocks(&strip_policy_allow_unfree_lines(&body));
    let mut out = String::new();
    out.push_str(&src[..body_start + 1]);
    out.push_str(&stripped_body);
    out.push('}');
    out.push_str(&src[body_start + 1 + consumed..]);
    out
}

fn strip_overlay_blocks(body: &str) -> String {
    let mut out = String::new();
    let mut pos = 0;
    while let Some(rel) = body[pos..].find(Syntax::WORKSPACE_OVERLAY) {
        let at = pos + rel;
        if !word_boundary_before(body, at) {
            out.push_str(&body[pos..at + Syntax::WORKSPACE_OVERLAY.len()]);
            pos = at + Syntax::WORKSPACE_OVERLAY.len();
            continue;
        }
        let before = &body[pos..at];
        out.push_str(before);
        let rest = &body[at + Syntax::WORKSPACE_OVERLAY.len()..];
        let Some(open_rel) = rest.find('{') else {
            pos = at + Syntax::WORKSPACE_OVERLAY.len();
            continue;
        };
        let (_, consumed) = balanced_with_len(&rest[open_rel + 1..], '{', '}');
        pos = at + Syntax::WORKSPACE_OVERLAY.len() + open_rel + 1 + consumed;
    }
    out.push_str(&body[pos..]);
    out
}

fn strip_policy_allow_unfree_lines(body: &str) -> String {
    body.lines()
        .filter(|line| !line.trim_start().starts_with("policy.allowUnfree"))
        .map(|line| {
            let mut s = line.to_string();
            s.push('\n');
            s
        })
        .collect()
}

/// Locate the `{` that opens the `module workspace { … }` body.
/// Pub for use by `jetpack::Overlay`'s `draft_overlay_source`.
pub fn find_workspace_body_start(src: &str) -> Option<usize> {
    let marker = format!("{} {}", Syntax::KW_MODULE, Syntax::NS_WORKSPACE);
    let at = src.find(&marker)?;
    src[at + marker.len()..]
        .find('{')
        .map(|rel| at + marker.len() + rel)
}

/// Returns `(body_content, chars_consumed_through_closing_brace)`.
/// Pub for use by `jetpack::Overlay`'s `draft_overlay_source`.
pub fn balanced_with_len(s: &str, open: char, close: char) -> (String, usize) {
    let mut depth = 1i32;
    let mut out = String::new();
    let mut consumed = 0usize;
    for (i, c) in s.char_indices() {
        consumed = i + c.len_utf8();
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        out.push(c);
    }
    (out, consumed)
}

fn read_ident(s: &str) -> Option<(String, &str)> {
    let mut end = 0usize;
    for (i, c) in s.char_indices() {
        if i == 0 {
            if !(c.is_ascii_alphabetic() || c == '_') {
                return None;
            }
        } else if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            break;
        }
        end = i + c.len_utf8();
    }
    (end > 0).then(|| (s[..end].to_string(), &s[end..]))
}

fn word_boundary_before(s: &str, at: usize) -> bool {
    at == 0
        || !s[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn top_level_commas(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    let mut cur = String::new();
    for c in body.chars() {
        if quoted {
            cur.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                cur.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                if !cur.trim().is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

pub fn unquote(s: &str) -> String {
    let s = s.trim().trim_end_matches(',');
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn workspace_body(src: &str) -> Option<String> {
    let start = find_workspace_body_start(src)?;
    Some(balanced_with_len(&src[start + 1..], '{', '}').0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_deny_uses_exact_balanced_policy_fields() {
        let policy = parse_workspace_policy(r#"
module workspace {
    policy_note: .{ deny: #(Exec) }
    policy: .{ trust: .{ note: "deny: #(FS)" }, Deny: #(Net), deny: #(Exec, FS) }
}
"#).unwrap();
        assert_eq!(policy.build_deny, vec!["Exec", "FS"]);
    }

    #[test]
    fn build_grants_are_subject_scoped_and_quote_aware() {
        let policy = parse_workspace_policy(r#"
module workspace {
    policy: .{
        grants: .{ "native:tools": #(Net, Exec), "app": #(FS) },
        deny: #(Time)
    }
}
"#)
        .unwrap();
        assert_eq!(policy.build_deny, vec!["Time"]);
        assert_eq!(
            policy.build_grants,
            vec![
                ("native:tools".to_string(), vec!["Net".to_string(), "Exec".to_string()]),
                ("app".to_string(), vec!["FS".to_string()]),
            ]
        );
    }

    #[test]
    fn malformed_build_grants_fail_closed() {
        let error = parse_workspace_policy(
            "module workspace { policy: .{ grants: .{ \"app\": [Net] } } }",
        )
        .unwrap_err();
        assert!(error.message().contains("policy.grants"));
    }

    #[test]
    fn parses_workspace_overlay_policy() {
        let src = r#"
module workspace {
    members: ["./packages/app"]
    policy.allowUnfree: ["discord"]
    overlay plasma_beta {
        provider: Provider.nixpkgs(channel: "plasma-beta")
        package("kdePackages.plasma-desktop").patches += [patch("patches/focus.patch")]
        package("kdePackages.plasma-desktop").flags += ["wayland"]
        package("discord").allowUnfree: true
    }
}
"#;
        let policy = parse_workspace_policy(src).unwrap();
        assert_eq!(policy.allow_unfree, vec!["discord"]);
        let overlay = policy.overlay("plasma_beta").unwrap();
        assert_eq!(overlay.provider.as_ref().unwrap().provider, "nixpkgs");
        assert_eq!(
            overlay.provider.as_ref().unwrap().channel.as_deref(),
            Some("plasma-beta")
        );
        let pkg = policy
            .package_override("plasma_beta", "kdePackages.plasma-desktop")
            .unwrap();
        assert_eq!(pkg.patches, vec!["patches/focus.patch"]);
        assert_eq!(pkg.flags, vec!["wayland"]);
        assert!(policy.allows_unfree("discord"));
    }

    #[test]
    fn strips_overlay_policy_before_workspace_eval() {
        let src = r#"
module workspace {
    members: ["./packages/app"]
    overlay test {
        package("foo").patches += [patch("patches/foo.patch")]
    }
    policy.allowUnfree: ["foo"]
}
"#;
        let stripped = strip_overlay_policy(src);
        assert!(stripped.contains("members:"));
        assert!(!stripped.contains("overlay test"));
        assert!(!stripped.contains("policy.allowUnfree"));
    }

    #[test]
    fn keyed_overrides_merge_typed_environment_by_priority() {
        let policy = parse_workspace_policy(
            r#"
module workspace {
    overlay dev {
        overrides: {
            "app": .{ version: Priority(10: "2"), env: .{ CC: "clang", CFLAGS: "-O2" }, flags: .{ lto: true } }
        }
        package("app").version = "1"
        package("app").env += .{ CC: "gcc", RUST_LOG: "debug" }
    }
}
"#,
        )
        .unwrap();
        let app = policy.package_override("dev", "app").unwrap();
        assert_eq!(app.version.as_deref(), Some("2"));
        assert_eq!(
            app.env,
            vec![
                ("CC".into(), "clang".into()),
                ("CFLAGS".into(), "-O2".into()),
                ("RUST_LOG".into(), "debug".into()),
            ]
        );
        assert_eq!(app.flags, vec!["lto"]);
        assert_eq!(app.priority, 10);
    }

    #[test]
    fn malformed_or_duplicate_environment_facts_fail_closed() {
        let invalid = parse_workspace_policy(
            "module workspace { overlay dev { overrides: { app: .{ env: .{ 1BAD: \"x\" } } } } }",
        )
        .unwrap_err();
        assert!(invalid.message().contains("variable name"));

        let duplicate = parse_workspace_policy(
            "module workspace { overlay dev { overrides: { app: .{ version: \"1\" }, app: .{ version: \"2\" } } } }",
        )
        .unwrap_err();
        assert!(duplicate.message().contains("declared more than once"));
    }
}
