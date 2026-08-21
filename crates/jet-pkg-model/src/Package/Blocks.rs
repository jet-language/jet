//! Structural sub-parsers for the `deps:`, `packages:`, `build:`,
//! `authority:`, and `policy:` blocks of the ratified `package.jet`
//! vocabulary
//! (D-CONF-PLANE1, D-CONF-NAME1). These blocks carry semantics ratified by
//! their own, separate decisions (D-JPK23, D-TGT1/D-TGT2/D-TGT3, D-CFFI2,
//! D-BUILDPROFILE1, D-CTEFFECT1, D-EFFBUDGET1, D-AUTHORITY-MANIFEST1,
//! D-BOUND-PROV1, D-JPK-GRANTSCHEMA1, D-JPK-PROVIDERAUTH1, D-LINTPOLICY1,
//! D-PACKAGE-POLICY-SCOPE1,
//! D-ONCE-AUTODERIVE1) — this module is the single reader for all of them, so
//! `package.jet` never again has two parsers for one fact.

use super::PackageParseError;
use crate::RefSpec::{self, Source};
use crate::Syntax;
use std::collections::{BTreeMap, BTreeSet, HashSet};

// ── small structural helpers (std-only, comment-stripped input) ────────────

/// Split a `{ … }` body into `key: value` entries at top-level commas.
pub(super) fn key_value_entries(body: &str) -> Result<Vec<(String, String)>, PackageParseError> {
    let mut entries = Vec::new();
    for entry in top_level_commas(body) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some((k, v)) = entry.split_once(':') {
            entries.push((k.trim().to_string(), v.trim().to_string()));
        } else {
            return Err(err(format!("malformed metadata field `{entry}`")));
        }
    }
    Ok(entries)
}

/// Split on commas that are not nested inside `()`/`[]`/`{}`.
pub(super) fn top_level_commas(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    let mut quoted = false;
    let mut escaped = false;
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
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Strip surrounding double quotes if present; otherwise return as-is, trimmed.
pub(super) fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn err(detail: impl Into<String>) -> PackageParseError {
    PackageParseError::Composition(detail.into())
}

fn bad_mem(detail: impl Into<String>) -> PackageParseError {
    PackageParseError::BadMemoryPolicy { detail: detail.into() }
}

fn bad_guarantee(detail: impl Into<String>) -> PackageParseError {
    PackageParseError::BadGuaranteePolicy { detail: detail.into() }
}

fn bad_allocator(detail: impl Into<String>) -> PackageParseError {
    PackageParseError::BadAllocatorPolicy { detail: detail.into() }
}

/// D-ALLOC-PROGRAM1=A: parse the hosted program allocator as one typed fact.
/// v1 ships the hidden system heap and its counting/capping wrapper; unknown
/// values fail closed instead of becoming strings that engines reinterpret.
pub(super) fn parse_program_allocator(
    value: &str,
) -> Result<crate::TargetMachine::AllocatorPolicy, PackageParseError> {
    use crate::TargetMachine::{AllocatorPolicy, ByteSize};

    let compact = value.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
    if compact == "mem.Heap" {
        return Ok(AllocatorPolicy::HostedDefault);
    }
    let Some(args) = compact
        .strip_prefix("mem.Counting.over(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(bad_allocator(
            "`allocator:` must be `mem.Heap` or `mem.Counting.over(mem.Heap, cap: <bytes>)`",
        ));
    };
    let args = top_level_commas(args);
    if args.is_empty() || args[0] != "mem.Heap" || args.len() > 2 {
        return Err(bad_allocator(
            "`mem.Counting.over` wraps `mem.Heap` and accepts only the optional `cap:` argument",
        ));
    }
    let cap = match args.get(1) {
        None => None,
        Some(value) => {
            let Some(value) = value.strip_prefix("cap:") else {
                return Err(bad_allocator(
                    "the counting allocator's optional argument is spelled `cap:`",
                ));
            };
            Some(ByteSize::bytes(parse_allocator_bytes(value)?))
        }
    };
    Ok(AllocatorPolicy::Counting { cap })
}

fn parse_allocator_bytes(value: &str) -> Result<u64, PackageParseError> {
    let (digits, multiplier) = match value.rsplit_once('.') {
        Some((digits, "bytes")) => (digits, 1u64),
        Some((digits, "kb" | "kib")) => (digits, 1024u64),
        Some((digits, "mb" | "mib")) => (digits, 1024u64.pow(2)),
        Some((digits, "gb" | "gib")) => (digits, 1024u64.pow(3)),
        Some(_) => {
            return Err(bad_allocator(
                "`cap:` uses exact bytes or a `.kb`, `.mb`, or `.gb` byte quantity",
            ))
        }
        None => (value, 1u64),
    };
    let digits = digits.replace('_', "");
    let count = digits.parse::<u64>().map_err(|_| {
        bad_allocator("`cap:` must be a positive whole-byte quantity such as `2.gb`")
    })?;
    let bytes = count.checked_mul(multiplier).filter(|bytes| *bytes > 0).ok_or_else(|| {
        bad_allocator("`cap:` must be a positive byte quantity that fits in 64 bits")
    })?;
    Ok(bytes)
}

// ── deps: … (D-JPK23, D-JPK-REF1, S59/D-CFFI2) ──────────────────────────────

/// Where a dependency resolves from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    /// The selector from a `name#version` registry ref.
    Version(String),
    /// A bare path or `target@provider` source ref.
    Provider { provider: Source, target: String },
    /// An inline git dependency (D-JPK23): any remote, with an explicit
    /// selector — `{ git: "<url>", tag/branch/rev: "<value>" }`.
    Git {
        url: String,
        selector: crate::Manifest::GitSelector,
    },
    /// A native C-library link dependency (S59/D-CFFI2): `lib: c@system`
    /// (pkg-config, bare `-l <lib>` fallback) or `lib: c@"vendor/path"`
    /// (local dir). A CLib dep is a link dep, not a Jet package.
    CLib { target: String },
}

/// Render one `DepSource` back to its display/audit string form.
pub fn dep_display(source: &DepSource) -> String {
    match source {
        DepSource::Version(value) => value.clone(),
        DepSource::Provider { provider, target } => {
            if matches!(provider, Source::Path) {
                target.clone()
            } else {
                format!("{target}@{}", provider.label())
            }
        }
        DepSource::Git { url, selector } => {
            let (field, value) = match selector {
                crate::Manifest::GitSelector::Tag(value) => ("tag", value),
                crate::Manifest::GitSelector::Branch(value) => ("branch", value),
                crate::Manifest::GitSelector::Rev(value) => ("rev", value),
            };
            format!("{{ git: {url:?}, {field}: {value:?} }}")
        }
        DepSource::CLib { target } => format!("lib: {target}"),
    }
}

fn bad_dep_value(name: &str, value: &str) -> PackageParseError {
    err(format!(
        "dependency `{name}` has value `{value}`, which is not a `name#version`, bare path, `target@provider` ref, or inline git struct"
    ))
}

fn ref_error(name: &str, error: &RefSpec::RefError) -> PackageParseError {
    match error {
        RefSpec::RefError::ProviderFirst { raw, replacement } => err(format!(
            "dependency `{name}` uses retired provider-first ref `{raw}`; write `{replacement}`"
        )),
        RefSpec::RefError::PathProviderRetired { raw, path } => err(format!(
            "dependency `{name}` uses retired path-provider ref `{raw}`; write the bare path `{path}`"
        )),
        other => err(format!("dependency `{name}`'s ref is invalid: {other:?}")),
    }
}

pub(super) fn parse_deps(body: &str) -> Result<BTreeMap<String, DepSource>, PackageParseError> {
    let mut deps = BTreeMap::new();
    for (name, value) in key_value_entries(body)? {
        let trimmed = value.trim();
        let source = if let Some(inner) = trimmed.strip_prefix('{') {
            let inner = inner.strip_suffix('}').unwrap_or(inner);
            parse_git_dep(&name, inner)?
        } else if let Some(target) = parse_c_lib_ref(trimmed) {
            DepSource::CLib { target }
        } else {
            // Both the bare legacy tokens (`helpers: ../helpers`,
            // `textkit: textkit#1.2.0`) and the ratified quoted spelling
            // (`httpkit: "^2"`, D-CONF-NAME1) are accepted — unquote before
            // classifying so both read the same.
            let unquoted = unquote(trimmed);
            if RefSpec::is_bare_path(&unquoted) || unquoted.contains(Syntax::REF_PROVIDER_AT) {
                match RefSpec::classify_provider_ref(&unquoted) {
                    Ok(r) => DepSource::Provider {
                        provider: r.provider,
                        target: r.target,
                    },
                    Err(error) => return Err(ref_error(&name, &error)),
                }
            } else if let Some((package, selector)) = unquoted.split_once('#') {
                if package == name && !selector.is_empty() {
                    DepSource::Version(selector.to_string())
                } else {
                    return Err(bad_dep_value(&name, trimmed));
                }
            } else if trimmed.starts_with('"') {
                // A bare quoted string with no `#`/`@`/path shape is a plain
                // version selector (D-CONF-NAME1: `deps: .{ httpkit: "^2" }`).
                DepSource::Version(unquoted)
            } else {
                return Err(bad_dep_value(&name, trimmed));
            }
        };
        if deps.insert(name.clone(), source).is_some() {
            return Err(err(format!("dependency `{name}` is declared more than once")));
        }
    }
    Ok(deps)
}

/// Detect a native C-library link ref (S59/D-CFFI2): `c@system` or
/// `c@"vendor/path"`.
fn parse_c_lib_ref(value: &str) -> Option<String> {
    let (provider, target) = value.split_once(Syntax::REF_PROVIDER_AT)?;
    if provider.trim() != Syntax::DEP_PROVIDER_C {
        return None;
    }
    Some(unquote(target))
}

/// Parse an inline git dependency's body: `git: "<url>", tag/branch/rev:
/// "<value>"` — exactly one selector (D-JPK23).
fn parse_git_dep(name: &str, body: &str) -> Result<DepSource, PackageParseError> {
    let mut url = None;
    let mut tag = None;
    let mut branch = None;
    let mut rev = None;
    let mut seen = HashSet::new();
    for (key, value) in key_value_entries(body)? {
        if !seen.insert(key.clone()) {
            return Err(err(format!("git dependency `{name}.{key}` is declared more than once")));
        }
        let v = unquote(&value);
        match key.as_str() {
            "git" => url = Some(v),
            "tag" => tag = Some(v),
            "branch" => branch = Some(v),
            "rev" => rev = Some(v),
            _ => {}
        }
    }
    let Some(url) = url else {
        return Err(err(format!("git dependency `{name}` is missing `git:`")));
    };
    let selector = match (tag, branch, rev) {
        (Some(t), None, None) => crate::Manifest::GitSelector::Tag(t),
        (None, Some(b), None) => crate::Manifest::GitSelector::Branch(b),
        (None, None, Some(r)) => crate::Manifest::GitSelector::Rev(r),
        _ => {
            return Err(err(format!(
                "git dependency `{name}` must have exactly one of `tag`, `branch`, `rev`"
            )))
        }
    };
    Ok(DepSource::Git { url, selector })
}

// ── packages: … (U10, D-TGT1/D-TGT2/D-TGT3) ────────────────────────────────

/// The realize axis for a package (U10): `library` is imported for code;
/// `executable` installs a binary on PATH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Library,
    Executable,
}

/// One build target of a package (D-TGT1/D-TGT2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Library,
    Executable,
    Test,
    Example,
    Plugin { export: Option<String> },
}

/// One entry in the `packages: { … }` block (U10 + D-TGT1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    pub name: String,
    pub targets: Vec<Target>,
}

pub(super) fn parse_packages(body: &str) -> Result<Vec<PackageEntry>, PackageParseError> {
    let mut packages = Vec::new();
    for entry in top_level_commas(body) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (name, targets) = match entry.split_once(':') {
            Some((k, v)) => {
                let name = k.trim().to_string();
                let value = v.trim();
                if let Some(inner) = value.strip_prefix('{') {
                    let inner = inner.trim_end().strip_suffix('}').unwrap_or(inner.trim_end());
                    let targets = parse_package_entry_block(&name, inner)?;
                    (name, targets)
                } else {
                    let target = parse_target(&name, value)?;
                    (name, vec![target])
                }
            }
            None => (entry.to_string(), Vec::new()),
        };
        packages.push(PackageEntry { name, targets });
    }
    Ok(packages)
}

fn parse_target(name: &str, value: &str) -> Result<Target, PackageParseError> {
    let (keyword, block) = match value.split_once('{') {
        Some((kw, rest)) => {
            let body = rest.trim_end().strip_suffix('}').unwrap_or(rest.trim_end());
            (kw.trim(), Some(body))
        }
        None => (value.trim(), None),
    };
    if keyword == Syntax::TARGET_SANDBOX || keyword == Syntax::RETIRED_TARGET_PLUGIN {
        let export = match block {
            Some(body) => validate_plugin_block(name, body)?,
            None => None,
        };
        return Ok(Target::Plugin { export });
    }
    let kind = match keyword {
        k if k == Syntax::TARGET_LIBRARY => Target::Library,
        k if k == Syntax::TARGET_EXECUTABLE => Target::Executable,
        k if k == Syntax::TARGET_TEST => Target::Test,
        k if k == Syntax::TARGET_EXAMPLE => Target::Example,
        k if Syntax::TARGET_RESERVED.contains(&k) => {
            return Err(PackageParseError::BadTarget {
                name: name.to_string(),
                value: k.to_string(),
                reserved: true,
            });
        }
        other => {
            return Err(PackageParseError::BadTarget {
                name: name.to_string(),
                value: other.to_string(),
                reserved: false,
            });
        }
    };
    if let Some(body) = block {
        validate_target_block(name, body)?;
    }
    Ok(kind)
}

fn validate_target_block(name: &str, body: &str) -> Result<(), PackageParseError> {
    let mut seen = HashSet::new();
    for (key, _value) in key_value_entries(body)? {
        if !seen.insert(key.clone()) {
            return Err(PackageParseError::BadTargetField {
                name: name.to_string(),
                detail: format!("target field `{key}` is declared more than once"),
            });
        }
        if key != Syntax::TARGET_FIELD_ENTRY && key != Syntax::TARGET_FIELD_NAME {
            return Err(PackageParseError::BadTargetField {
                name: name.to_string(),
                detail: format!(
                    "unknown target field `{key}` (allowed: `{}`, `{}`)",
                    Syntax::TARGET_FIELD_ENTRY,
                    Syntax::TARGET_FIELD_NAME,
                ),
            });
        }
    }
    Ok(())
}

fn validate_plugin_block(name: &str, body: &str) -> Result<Option<String>, PackageParseError> {
    let mut export = None;
    let mut seen = HashSet::new();
    for (key, value) in key_value_entries(body)? {
        if !seen.insert(key.clone()) {
            return Err(PackageParseError::BadTargetField {
                name: name.to_string(),
                detail: format!("sandbox field `{key}` is declared more than once"),
            });
        }
        if key == Syntax::TARGET_FIELD_ENTRY || key == Syntax::TARGET_FIELD_NAME {
            // free-form, consumed by the build pipeline.
        } else if key == Syntax::TARGET_FIELD_EXPORT {
            export = Some(unquote(&value));
        } else {
            return Err(PackageParseError::BadTargetField {
                name: name.to_string(),
                detail: format!(
                    "unknown field `{key}` on `sandbox` (allowed: `{}`, `{}`, `{}`)",
                    Syntax::TARGET_FIELD_ENTRY,
                    Syntax::TARGET_FIELD_NAME,
                    Syntax::TARGET_FIELD_EXPORT,
                ),
            });
        }
    }
    Ok(export)
}

fn parse_package_entry_block(name: &str, body: &str) -> Result<Vec<Target>, PackageParseError> {
    let mut seen = HashSet::new();
    for (key, value) in key_value_entries(body)? {
        if !seen.insert(key.clone()) {
            return Err(PackageParseError::BadTargetField {
                name: name.to_string(),
                detail: format!("package field `{key}` is declared more than once"),
            });
        }
        if key == Syntax::PACKAGE_FIELD_KIND_REMOVED {
            return Err(PackageParseError::KindFieldRemoved { name: name.to_string() });
        }
        if key == Syntax::PACKAGE_FIELD_TARGETS {
            return parse_targets_list(name, value.trim());
        }
        return Err(PackageParseError::BadTargetField {
            name: name.to_string(),
            detail: format!("unknown package field `{key}`"),
        });
    }
    Ok(Vec::new())
}

fn parse_targets_list(name: &str, value: &str) -> Result<Vec<Target>, PackageParseError> {
    let inner = value.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(value);
    let mut targets = Vec::new();
    for entry in top_level_commas(inner) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        targets.push(parse_target(name, entry)?);
    }
    Ok(targets)
}

// ── build: … (D-BUILDPROFILE1, D-CTEFFECT1) ─────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOptimize {
    None,
    Basic,
    Full,
}

impl BuildOptimize {
    pub fn as_str(self) -> &'static str {
        match self {
            BuildOptimize::None => Syntax::BUILD_OPTIMIZE_NONE,
            BuildOptimize::Basic => Syntax::BUILD_OPTIMIZE_BASIC,
            BuildOptimize::Full => Syntax::BUILD_OPTIMIZE_FULL,
        }
    }

    pub fn cache_tag(self) -> &'static str {
        match self {
            BuildOptimize::None => "opt:none",
            BuildOptimize::Basic => "opt:basic",
            BuildOptimize::Full => "opt:full",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildPanic {
    #[default]
    Unwind,
    Abort,
}

/// One named build profile declared in `build: .{ … }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProfileDef {
    pub name: String,
    pub optimize: BuildOptimize,
    pub debug_info: bool,
    pub small: bool,
    pub panic: Option<BuildPanic>,
    /// D-CONF-MODULE1=A: profile contributions to declared typed settings.
    pub settings: BTreeMap<String, String>,
}

pub(super) fn parse_build(body: &str) -> Result<Vec<BuildProfileDef>, PackageParseError> {
    let mut profiles = Vec::new();
    let mut seen = HashSet::new();
    for (name, value) in key_value_entries(body)? {
        if !seen.insert(name.clone()) {
            return Err(err(format!("build field `{name}` is declared more than once")));
        }
        if name == Syntax::BUILD_FIELD_ALLOW {
            continue;
        }
        let value = value.trim();
        let inner_block = if let Some(rest) = value.strip_prefix(Syntax::BUILD_CTOR) {
            let rest = rest.trim_start_matches('.').trim();
            rest.strip_prefix('{').and_then(|s| s.strip_suffix('}')).map(|s| s.trim())
        } else if let Some(s) = value.strip_prefix('{') {
            s.strip_suffix('}').map(|s| s.trim())
        } else {
            return Err(err(format!(
                "build profile `{name}` needs a `Build.{{ optimize: none|basic|full, … }}` value"
            )));
        };
        let inner = inner_block.unwrap_or("");
        let mut optimize_val: Option<String> = None;
        let mut debug_info = false;
        let mut small = false;
        let mut panic = None;
        let mut settings = BTreeMap::new();
        let mut seen_fields = HashSet::new();
        for (key, val) in key_value_entries(inner)? {
            if !seen_fields.insert(key.clone()) {
                return Err(err(format!("build profile `{name}.{key}` is declared more than once")));
            }
            if key == Syntax::BUILD_FIELD_OPTIMIZE {
                optimize_val = Some(unquote(&val));
            } else if key == Syntax::BUILD_FIELD_DEBUG_INFO {
                debug_info = parse_bool(&name, &val)?;
            } else if key == Syntax::BUILD_FIELD_SMALL {
                small = parse_bool(&name, &val)?;
            } else if key == Syntax::BUILD_FIELD_PANIC {
                let val = unquote(&val);
                panic = Some(match val.as_str() {
                    s if s == Syntax::BUILD_PANIC_ABORT => BuildPanic::Abort,
                    s if s == Syntax::BUILD_PANIC_UNWIND => BuildPanic::Unwind,
                    _ => return Err(err(format!("build profile `{name}` has an unknown `panic:` value — use `abort` or `unwind`"))),
                });
            } else if key == Syntax::BUILD_FIELD_SETTINGS {
                let inner = val
                    .trim_start_matches('.')
                    .trim()
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                    .map(|s| s.trim())
                    .ok_or_else(|| err(format!("build profile `{name}` needs `settings: .{{ key: value, … }}`")))?;
                for (k, v) in key_value_entries(inner)? {
                    if settings.insert(k.clone(), unquote(&v)).is_some() {
                        return Err(err(format!("build profile `{name}.settings.{k}` is declared more than once")));
                    }
                }
            } else if matches!(key.as_str(), Syntax::RETIRED_BUILD_FIELD_FEATURES | Syntax::RETIRED_BUILD_FIELD_ENV) {
                return Err(err(format!(
                    "build profile `{name}` uses retired `{key}:`; declare a typed `settings: .{{ key: Type = default }}` entry and override it with `--set key=value`"
                )));
            } else {
                return Err(err(format!(
                    "build profile `{name}` has an unknown field `{key}` (allowed: optimize, debug_info, small, panic, settings)"
                )));
            }
        }
        let optimize = match optimize_val.as_deref() {
            Some(s) if s == Syntax::BUILD_OPTIMIZE_NONE => BuildOptimize::None,
            Some(s) if s == Syntax::BUILD_OPTIMIZE_BASIC => BuildOptimize::Basic,
            Some(s) if s == Syntax::BUILD_OPTIMIZE_FULL => BuildOptimize::Full,
            Some(_) | None => {
                return Err(err(format!(
                    "build profile `{name}` is missing `optimize: none|basic|full`"
                )))
            }
        };
        profiles.push(BuildProfileDef { name, optimize, debug_info, small, panic, settings });
    }
    Ok(profiles)
}

/// D-CTEFFECT1: `build: .{ allow: #(FS, Exec) }`.
pub(super) fn parse_build_allow(body: &str) -> Result<Vec<String>, PackageParseError> {
    let mut allow = Vec::new();
    let mut seen_allow = false;
    for (name, value) in key_value_entries(body)? {
        if name != Syntax::BUILD_FIELD_ALLOW {
            continue;
        }
        if seen_allow {
            return Err(PackageParseError::BadEffectsBlock(
                "`build.allow:` is declared more than once".to_string(),
            ));
        }
        seen_allow = true;
        let value = value.trim();
        let inner = value
            .strip_prefix("#(")
            .and_then(|value| value.strip_suffix(')'))
            .ok_or_else(|| {
                PackageParseError::BadEffectsBlock(
                    "`build.allow:` must be an effect tuple like `#(FS, Exec)`".to_string(),
                )
            })?;
        for effect in top_level_commas(inner) {
            let effect = unquote(effect.trim());
            if crate::Sema::Effect::parse(crate::Sema::effect_root(&effect)).is_none() {
                return Err(PackageParseError::BadEffectsBlock(format!(
                    "`{effect}` isn't a known build effect"
                )));
            }
            allow.push(effect);
        }
    }
    Ok(allow)
}

fn parse_bool(profile: &str, value: &str) -> Result<bool, PackageParseError> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(err(format!("build profile `{profile}` expected `true` or `false`"))),
    }
}

fn parse_string_list(value: &str) -> Result<Vec<String>, ()> {
    let inner = value.trim().strip_prefix('[').and_then(|s| s.strip_suffix(']')).ok_or(())?;
    let mut out = Vec::new();
    for entry in top_level_commas(inner) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        out.push(unquote(entry));
    }
    Ok(out)
}

pub(super) fn parse_grants(body: &str) -> Result<Vec<(String, Vec<String>)>, PackageParseError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (key, value) in key_value_entries(body)? {
        let dep = unquote(&key);
        if !seen.insert(dep.clone()) {
            return Err(err(format!("grant `{dep}` is declared more than once")));
        }
        let effects = parse_effect_list(&dep, value.trim())?;
        out.push((dep, effects));
    }
    Ok(out)
}

fn parse_effect_list(field: &str, value: &str) -> Result<Vec<String>, PackageParseError> {
    let names = parse_string_list(value)
        .map_err(|_| PackageParseError::BadEffectsBlock(format!("`{field}:` must be a list like `[DB, Net]`")))?;
    for name in &names {
        if crate::Sema::Effect::parse(crate::Sema::effect_root(name)).is_none() {
            return Err(PackageParseError::BadEffectsBlock(format!(
                "`{name}` isn't a known effect (see the closed vocabulary in Prelude/Effects.jet)"
            )));
        }
        if name.contains('(') {
            if !field.ends_with("deny") {
                return Err(PackageParseError::BadEffectsBlock(format!(
                    "`{name}` is a parameterized memory denial and belongs only in an `authority.holds.deny` list — use `Mem.Alloc(above: 65536)` there"
                )));
            }
            if crate::Sema::memory_allocation_bound(name).is_none() {
                return Err(PackageParseError::BadEffectsBlock(format!(
                "`{name}` has an invalid parameterized rights entry — use `Mem.Alloc(above: 65536)`"
                )));
            }
        }
    }
    Ok(names)
}

// ── authority: … / policy: … (D-AUTHORITY-MANIFEST1, D-BOUND-PROV1,
//    D-JPK-GRANTSCHEMA1, D-JPK-PROVIDERAUTH1, D-LINTPOLICY1,
//    D-PACKAGE-POLICY-SCOPE1, D-ONCE-AUTODERIVE1, D-MEM-GUARANTEE1) ──────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDecision {
    Allow,
    Prompt,
    Deny,
}

/// D-AUTHORITY-MANIFEST1=A: the package's own authority bound.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthorityHolds {
    pub allow: Option<Vec<String>>,
    pub deny: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustPolicy {
    pub default: Option<TrustDecision>,
    pub ci_prompt: Option<TrustDecision>,
    pub services: Vec<(String, TrustDecision)>,
    /// D-BOUND-PROV1: optional provenance floor for dependency resolution.
    pub require: Option<ProvenanceRequirement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceRequirement {
    None,
    Logged,
    Attested,
}

impl ProvenanceRequirement {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => Syntax::AUTHORITY_TRUST_REQUIRE_NONE,
            Self::Logged => Syntax::AUTHORITY_TRUST_REQUIRE_LOGGED,
            Self::Attested => Syntax::AUTHORITY_TRUST_REQUIRE_ATTESTED,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAuthority {
    pub provider: String,
    pub registry: String,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

/// D-AUTHORITY-MANIFEST1=A: one parsed authority block. The package model
/// copies this into its public authority fact and projects holds/grants into
/// the effect-budget summary inputs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthorityBlock {
    pub holds: AuthorityHolds,
    pub grants: Vec<(String, Vec<String>)>,
    pub trust: Option<TrustPolicy>,
    pub providers: Vec<ProviderAuthority>,
}

fn authority_bad(detail: impl Into<String>) -> PackageParseError {
    PackageParseError::BadEffectsBlock(format!("malformed `authority:` block: {}", detail.into()))
}

fn authority_object_body<'a>(value: &'a str, field: &str) -> Result<&'a str, PackageParseError> {
    let value = value.trim();
    let Some(open) = value.find('{') else {
        return Err(authority_bad(format!("`authority.{field}` must be a record")));
    };
    let Some(close) = value.rfind('}') else {
        return Err(authority_bad(format!("`authority.{field}` is missing `}}`")));
    };
    if close <= open || !value[close + 1..].trim().is_empty() {
        return Err(authority_bad(format!(
            "`authority.{field}` must contain one complete record"
        )));
    }
    Ok(&value[open + 1..close])
}

fn parse_authority_holds(body: &str) -> Result<AuthorityHolds, PackageParseError> {
    let mut holds = AuthorityHolds::default();
    let mut seen = HashSet::new();
    for (key, value) in key_value_entries(body).map_err(authority_error)? {
        if !seen.insert(key.clone()) {
            return Err(authority_bad(format!("authority.holds.{key} is declared more than once")));
        }
        match key.as_str() {
            Syntax::AUTHORITY_HOLDS_FIELD_ALLOW => {
                holds.allow = Some(
                    parse_effect_list("authority.holds.allow", value.trim())
                        .map_err(authority_error)?,
                );
            }
            Syntax::AUTHORITY_HOLDS_FIELD_DENY => {
                holds.deny = Some(
                    parse_effect_list("authority.holds.deny", value.trim())
                        .map_err(authority_error)?,
                );
            }
            _ => {
                return Err(authority_bad(format!(
                    "unknown `authority.holds` field `{key}` — allowed: `{}`, `{}`",
                    Syntax::AUTHORITY_HOLDS_FIELD_ALLOW,
                    Syntax::AUTHORITY_HOLDS_FIELD_DENY,
                )))
            }
        }
    }
    Ok(holds)
}

pub(super) fn parse_authority(body: &str) -> Result<AuthorityBlock, PackageParseError> {
    let mut authority = AuthorityBlock::default();
    let mut seen = HashSet::new();
    for (key, value) in key_value_entries(body).map_err(authority_error)? {
        if !seen.insert(key.clone()) {
            return Err(authority_bad(format!("authority.{key} is declared more than once")));
        }
        match key.as_str() {
            Syntax::AUTHORITY_FIELD_HOLDS => {
                authority.holds = parse_authority_holds(authority_object_body(&value, "holds")?)?;
            }
            Syntax::AUTHORITY_FIELD_GRANTS => {
                authority.grants = parse_grants(authority_object_body(&value, "grants")?)
                    .map_err(authority_error)?;
            }
            Syntax::AUTHORITY_FIELD_TRUST => {
                authority.trust = Some(parse_authority_trust_body(
                    authority_object_body(&value, "trust")?,
                )?);
            }
            Syntax::AUTHORITY_FIELD_PROVIDERS => {
                authority.providers = parse_provider_authority_body(
                    authority_object_body(&value, "providers")?,
                )?;
            }
            _ => {
                return Err(authority_bad(format!(
                    "unknown `authority` field `{key}` — allowed: `{}`, `{}`, `{}`, `{}`",
                    Syntax::AUTHORITY_FIELD_HOLDS,
                    Syntax::AUTHORITY_FIELD_GRANTS,
                    Syntax::AUTHORITY_FIELD_TRUST,
                    Syntax::AUTHORITY_FIELD_PROVIDERS,
                )))
            }
        }
    }
    Ok(authority)
}

/// Parse the complete `authority:` value so a missing or non-record outer
/// value uses the same malformed-authority error as malformed nested fields.
pub(crate) fn parse_authority_value(value: &str) -> Result<AuthorityBlock, PackageParseError> {
    let body = authority_object_body(value, "authority")?;
    parse_authority(body)
}

fn authority_error(error: PackageParseError) -> PackageParseError {
    match error {
        PackageParseError::Composition(detail) => authority_bad(detail),
        other => other,
    }
}

fn parse_authority_trust_body(body: &str) -> Result<TrustPolicy, PackageParseError> {
    let mut policy = TrustPolicy::default();
    let mut seen = HashSet::new();
    for (key, value) in key_value_entries(body).map_err(authority_error)? {
        if !seen.insert(key.clone()) {
            return Err(authority_bad(format!("authority.trust.{key} is declared more than once")));
        }
        if key == Syntax::AUTHORITY_TRUST_FIELD_DEFAULT {
            policy.default = Some(parse_trust_decision(&value).map_err(authority_error)?);
        } else if key == Syntax::AUTHORITY_TRUST_FIELD_CI {
            policy.ci_prompt = Some(parse_ci_trust_prompt(&value).map_err(authority_error)?);
        } else if key == Syntax::AUTHORITY_TRUST_FIELD_SERVICES {
            policy.services = parse_service_trust(&value).map_err(authority_error)?;
        } else if key == Syntax::AUTHORITY_TRUST_FIELD_REQUIRE {
            policy.require = Some(parse_provenance_requirement(&value).map_err(authority_error)?);
        } else {
            return Err(authority_bad(format!(
                "unknown `authority.trust` field `{key}` — allowed: `{}`, `{}`, `{}`, `{}`",
                Syntax::AUTHORITY_TRUST_FIELD_DEFAULT,
                Syntax::AUTHORITY_TRUST_FIELD_CI,
                Syntax::AUTHORITY_TRUST_FIELD_SERVICES,
                Syntax::AUTHORITY_TRUST_FIELD_REQUIRE,
            )));
        }
    }
    Ok(policy)
}

fn parse_provider_authority_body(providers: &str) -> Result<Vec<ProviderAuthority>, PackageParseError> {
    let mut out = Vec::new();
    let mut seen_providers = HashSet::new();
    for (provider, value) in key_value_entries(providers).map_err(authority_error)? {
        if !seen_providers.insert(provider.clone()) {
            return Err(authority_bad(format!("authority.providers.{provider} is declared more than once")));
        }
        let authority = authority_object_body(&value, &format!("providers.{provider}"))?;
        let mut registry = None;
        let mut allow = Vec::new();
        let mut deny = Vec::new();
        let mut seen_fields = HashSet::new();
        for (field, value) in key_value_entries(authority).map_err(authority_error)? {
            if !seen_fields.insert(field.clone()) {
                return Err(authority_bad(format!(
                    "authority.providers.{provider}.{field} is declared more than once"
                )));
            }
            if field == Syntax::PROVIDER_FIELD_REGISTRY {
                let value = value.trim();
                if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
                    return Err(authority_bad(format!(
                        "authority.providers.{provider}.registry must be a string"
                    )));
                }
                registry = Some(unquote(value));
            } else if field == Syntax::PROVIDER_FIELD_ALLOW {
                allow = parse_string_list(&value)
                    .map_err(|_| authority_bad(format!(
                        "authority.providers.{provider}.allow must be a list"
                    )))?;
            } else if field == Syntax::PROVIDER_FIELD_DENY {
                deny = parse_string_list(&value)
                    .map_err(|_| authority_bad(format!(
                        "authority.providers.{provider}.deny must be a list"
                    )))?;
            } else {
                return Err(authority_bad(format!(
                    "unknown authority.providers.{provider} field `{field}`"
                )));
            }
        }
        let registry = registry.ok_or_else(|| {
            authority_bad(format!("authority.providers.{provider} needs registry"))
        })?;
        out.push(ProviderAuthority { provider, registry, allow, deny });
    }
    Ok(out)
}

fn parse_provenance_requirement(value: &str) -> Result<ProvenanceRequirement, PackageParseError> {
    match unquote(value).as_str() {
        v if v == Syntax::AUTHORITY_TRUST_REQUIRE_NONE => Ok(ProvenanceRequirement::None),
        v if v == Syntax::AUTHORITY_TRUST_REQUIRE_LOGGED => Ok(ProvenanceRequirement::Logged),
        v if v == Syntax::AUTHORITY_TRUST_REQUIRE_ATTESTED => Ok(ProvenanceRequirement::Attested),
        other => Err(err(format!(
            "`authority.trust.require` must be `{}`, `{}`, or `{}`, not `{other}`",
            Syntax::AUTHORITY_TRUST_REQUIRE_NONE,
            Syntax::AUTHORITY_TRUST_REQUIRE_LOGGED,
            Syntax::AUTHORITY_TRUST_REQUIRE_ATTESTED,
        ))),
    }
}

fn parse_ci_trust_prompt(value: &str) -> Result<TrustDecision, PackageParseError> {
    let value = value.trim();
    let value = value.strip_prefix('.').unwrap_or(value).trim_start();
    let body = value
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| err("`authority.trust.ci` must be `{ prompt: allow|prompt|deny }`"))?;
    let mut prompt = None;
    let mut seen = HashSet::new();
    for (key, value) in key_value_entries(body)? {
        if !seen.insert(key.clone()) {
            return Err(err(format!("authority.trust.ci.{key} is declared more than once")));
        }
        if key != Syntax::AUTHORITY_TRUST_FIELD_PROMPT {
            return Err(err(format!("unknown `authority.trust.ci` field `{key}` — allowed: `prompt`")));
        }
        prompt = Some(parse_trust_decision(&value)?);
    }
    prompt.ok_or_else(|| err("`authority.trust.ci` needs `prompt:`"))
}

fn parse_service_trust(value: &str) -> Result<Vec<(String, TrustDecision)>, PackageParseError> {
    let value = value.trim();
    let value = value.strip_prefix('.').unwrap_or(value).trim_start();
    let body = value
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| err("`authority.trust.services` must be `{ name: allow|prompt|deny }`"))?;
    let mut services = Vec::new();
    let mut seen = HashSet::new();
    for (name, value) in key_value_entries(body)? {
        if !seen.insert(name.clone()) {
            return Err(err(format!("authority.trust.services.{name} is declared more than once")));
        }
        services.push((name, parse_trust_decision(&value)?));
    }
    Ok(services)
}

fn parse_trust_decision(value: &str) -> Result<TrustDecision, PackageParseError> {
    match unquote(value).as_str() {
        v if v == Syntax::AUTHORITY_TRUST_DECISION_ALLOW => Ok(TrustDecision::Allow),
        v if v == Syntax::AUTHORITY_TRUST_DECISION_PROMPT => Ok(TrustDecision::Prompt),
        v if v == Syntax::AUTHORITY_TRUST_DECISION_DENY => Ok(TrustDecision::Deny),
        other => Err(err(format!("`{other}` is not a trust decision — use `allow`, `prompt`, or `deny`"))),
    }
}

pub(super) fn parse_lints_policy(body: &str) -> Result<Option<Vec<String>>, PackageParseError> {
    jet_foundation::LintPolicy::parse_policy_lints(body).map_err(|error| {
        let jet_foundation::LintPolicy::LintPolicyError { detail, code, name } = error;
        match (code, name) {
            (Some(code), Some(name)) => PackageParseError::LintPolicyCode { code, name },
            _ => err(detail),
        }
    })
}

pub(super) fn parse_policy(
    body: &str,
    package_only_fields: bool,
) -> Result<Vec<crate::Policy::PolicyDeclaration>, PackageParseError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (name, raw) in key_value_entries(body)? {
        if matches!(name.as_str(), "trust" | "providers") {
            return Err(PackageParseError::RetiredAuthorityField {
                field: format!("policy.{name}"),
                replacement: format!("authority.{name}"),
            });
        }
        let Some(key) = crate::Policy::PolicyKey::parse(&name) else {
            let replacement = match name.as_str() {
                "no_alloc" => Some("`authority: .{ holds: { deny: [Mem.Alloc] } }`".to_string()),
                "zero_rc" => Some("`authority: .{ holds: { deny: [Mem.Rc] } }`".to_string()),
                "arena_bounded" => raw
                    .parse::<u64>()
                    .ok()
                    .filter(|bytes| *bytes > 0)
                    .map(|bytes| format!("`authority: .{{ holds: {{ deny: [Mem.Alloc(above: {bytes})] }} }}`")),
                _ => None,
            };
            if let Some(replacement) = replacement {
                return Err(bad_mem(format!(
                    "`{name}` is a retired memory floor; write the denial {replacement}"
                )));
            }
            if name == "lints"
                || (package_only_fields
                    && (name == Syntax::POLICY_FIELD_CONTAIN
                        || name == Syntax::POLICY_FIELD_HARDEN))
            {
                continue;
            }
            if name == jet_foundation::LintPolicy::auto_derive_lint()
                .lint_name
                .expect("auto_derive lint must have a name")
            {
                return Err(PackageParseError::RetiredPolicyField {
                    field: format!("policy.{name}"),
                    replacement: format!(
                        "policy: .{{ lints: .{{ deny: [{}] }} }}",
                        jet_foundation::LintPolicy::auto_derive_lint()
                            .lint_name
                            .expect("auto_derive lint must have a name")
                    ),
                });
            }
            return Err(bad_mem(format!("`{name}` is not a registered package policy; memory floors belong in `authority.holds.deny`")));
        };
        if !seen.insert(key) {
            return Err(bad_mem(format!("package policy `{name}` is declared more than once")));
        }
        let value = crate::Policy::parse_value(key, &raw)
            .map_err(|detail| bad_mem(detail))?;
        out.push(crate::Policy::PolicyDeclaration {
            key,
            value,
            scope: crate::Policy::PolicyScope::Package,
            span: crate::Diagnostics::Span::new(0, 0),
            target: None,
            source: "package.jet".to_string(),
        });
    }
    for key in crate::Policy::POLICY_RULES.iter().map(|rule| rule.key) {
        crate::Policy::resolve(key, out.clone())
            .map_err(|error| bad_mem(format!("conflicting `{}` declarations: {error:?}", key.name())))?;
    }
    Ok(out)
}

/// Parse the package-only dependency guarantee dial. The source policy
/// registry deliberately does not know these names: they govern the package
/// graph and release profile, not lexical source scopes.
pub(super) fn parse_guarantee_policy(
    body: &str,
) -> Result<(BTreeSet<String>, bool), PackageParseError> {
    let mut contain = BTreeSet::new();
    let mut contain_seen = false;
    let mut harden_seen = false;
    let mut harden = false;
    for (name, raw) in key_value_entries(body)? {
        if name == Syntax::POLICY_FIELD_CONTAIN {
            if contain_seen {
                return Err(bad_guarantee("`policy.contain` may be declared once"));
            }
            contain_seen = true;
            let values = parse_string_list(raw.trim()).map_err(|_| {
                bad_guarantee("`policy.contain` must be a list like `[\"libxml\"]`")
            })?;
            for dependency in values {
                if dependency.trim().is_empty() {
                    return Err(bad_guarantee(
                        "`policy.contain` cannot name an empty dependency",
                    ));
                }
                if !contain.insert(dependency) {
                    return Err(bad_guarantee(
                        "`policy.contain` cannot name one dependency more than once",
                    ));
                }
            }
        } else if name == Syntax::POLICY_FIELD_HARDEN {
            if harden_seen {
                return Err(bad_guarantee("`policy.harden` may be declared once"));
            }
            harden_seen = true;
            match raw.trim() {
                "true" => harden = true,
                "false" => {
                    return Err(bad_guarantee(
                        "`policy.harden` only tightens; omit it instead of writing `false`",
                    ))
                }
                _ => {
                    return Err(bad_guarantee(
                        "`policy.harden` must be `true`; the package guarantee dial only tightens",
                    ))
                }
            }
        }
    }
    Ok((contain, harden))
}

/// Parse a standalone organization policy file whose entire content is one
/// `policy: .{ … }` block (`JET_ORG_UNSAFE_POLICY`) — no other manifest
/// fields are legal there.
pub fn parse_policy_document(text: &str) -> Result<Vec<crate::Policy::PolicyDeclaration>, PackageParseError> {
    let text = super::strip_comments(text);
    let mut rest = text
        .trim()
        .strip_prefix(Syntax::MANIFEST_BLOCK_POLICY)
        .ok_or_else(|| bad_mem("expected only `policy: .{ … }`"))?
        .trim_start();
    rest = rest.strip_prefix(':').ok_or_else(|| bad_mem("expected `:` after `policy`"))?.trim_start();
    rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
    rest = rest.strip_prefix('{').ok_or_else(|| bad_mem("expected `.{` after `policy:`"))?;
    let close = rest.find('}').ok_or_else(|| bad_mem("missing `}` after organization policy"))?;
    if !rest[close + 1..].trim().is_empty() {
        return Err(bad_mem("organization policy file may contain only the `policy` block"));
    }
    parse_policy(&rest[..close], false)
}

// ── `fn build` co-location (D-BUILDSCOPE1) ──────────────────────────────────

/// Return the Jet declarations that live beside the manifest blocks in
/// `package.jet`, preserving their original byte offsets. D-BUILDSCOPE1 makes
/// `package.jet` a valid home for the package's single `fn build`; the
/// manifest reader must therefore expose that source without teaching the
/// manifest grammar how to parse ordinary Jet items.
///
/// Known manifest blocks are blanked with spaces (newlines are retained), so
/// diagnostics from the later compiler parse still point at `package.jet`.
/// The result is `None` when the remaining top-level source has no `fn build`.
pub fn build_entry_source(text: &str) -> Option<String> {
    let mut masked = text.as_bytes().to_vec();
    mask_manifest_blocks(text, &mut masked);
    let source = String::from_utf8(masked).ok()?;
    has_top_level_build_function(&source).then_some(source)
}

const MANIFEST_BLOCKS: &[&str] = &[
    "deps",
    "packages",
    "services",
    "outputs",
    "environments",
    "defaults",
    "settings",
    Syntax::MANIFEST_BLOCK_BUILD,
    Syntax::MANIFEST_BLOCK_EFFECTS,
    Syntax::MANIFEST_BLOCK_GRANTS,
    Syntax::MANIFEST_BLOCK_AUTHORITY,
    Syntax::MANIFEST_BLOCK_POLICY,
    "dev_deps",
    "patch",
    "workspace",
];

/// D-CONF-NAME1: the bare top-level identity/metadata fields — unlike
/// `MANIFEST_BLOCKS`, these have no `{ … }` body to balance, so masking them
/// means blanking to the next top-level separator instead.
const MANIFEST_SCALAR_FIELDS: &[&str] = &[
    Syntax::MANIFEST_FIELD_NAME,
    Syntax::MANIFEST_FIELD_VERSION,
    "jet",
    "source",
    "edition",
    "description",
    "license",
    "repository",
    "target",
    "runtime",
    "allocator",
    "members",
    "configs",
];

fn mask_manifest_blocks(text: &str, masked: &mut [u8]) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        if in_string {
            if escaped {
                escaped = false;
            } else if bytes[i] == b'\\' {
                escaped = true;
            } else if bytes[i] == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if brace_depth == 0 && bracket_depth == 0 && paren_depth == 0 {
            if let Some((start, open)) = manifest_block_start(bytes, i) {
                if let Some(end) = balanced_block_end(text, open) {
                    blank_range(masked, start, end);
                    i = end;
                    continue;
                }
            }
            if let Some((start, end)) = manifest_scalar_start(bytes, i) {
                blank_range(masked, start, end);
                i = end;
                continue;
            }
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment(bytes, i);
            continue;
        }
        match bytes[i] {
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
}

fn manifest_block_start(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    for key in MANIFEST_BLOCKS {
        let end = at.checked_add(key.len())?;
        if bytes.get(at..end) != Some(key.as_bytes()) {
            continue;
        }
        let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
        let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
        if !before_ok || !after_ok {
            continue;
        }
        let mut cursor = end;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b':') {
            continue;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'.') {
            cursor += 1;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
        }
        if bytes.get(cursor) == Some(&b'{') {
            return Some((at, cursor));
        }
    }
    None
}

/// Like `manifest_block_start`, but for a bare `field: value` entry with no
/// `{ … }` body — the end is the next top-level separator (`,`/`\n`) or EOF.
fn manifest_scalar_start(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    for key in MANIFEST_SCALAR_FIELDS {
        let Some(end) = at.checked_add(key.len()) else { continue };
        if bytes.get(at..end) != Some(key.as_bytes()) {
            continue;
        }
        let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
        let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
        if !before_ok || !after_ok {
            continue;
        }
        let mut cursor = end;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b':') {
            continue;
        }
        // A colon that starts a *block* (`{`/`.{`) belongs to
        // `manifest_block_start` instead — never true for these field names
        // today, but stay out of its way if that ever changes.
        let mut value_start = cursor + 1;
        while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
            value_start += 1;
        }
        let peek = bytes.get(value_start).copied();
        let after_dot = (peek == Some(b'.')).then(|| {
            let mut c = value_start + 1;
            while bytes.get(c).is_some_and(u8::is_ascii_whitespace) {
                c += 1;
            }
            bytes.get(c).copied()
        });
        if peek == Some(b'{') || after_dot == Some(Some(b'{')) {
            continue;
        }
        let mut i = value_start;
        let mut depth = 0i32;
        let mut quoted = false;
        let mut escaped = false;
        while i < bytes.len() {
            let byte = bytes[i];
            if quoted {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    quoted = false;
                }
                i += 1;
                continue;
            }
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' | b'(' => depth += 1,
                b'}' | b']' | b')' => depth -= 1,
                b',' | b'\n' if depth == 0 => return Some((at, i)),
                _ => {}
            }
            i += 1;
        }
        return Some((at, bytes.len()));
    }
    None
}

fn balanced_block_end(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        if in_string {
            if escaped {
                escaped = false;
            } else if bytes[i] == b'\\' {
                escaped = true;
            } else if bytes[i] == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment(bytes, i);
            continue;
        } else if bytes[i] == b'{' {
            depth += 1;
        } else if bytes[i] == b'}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

fn blank_range(masked: &mut [u8], start: usize, end: usize) {
    for byte in masked.get_mut(start..end).into_iter().flatten() {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn has_top_level_build_function(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        if in_string {
            if escaped {
                escaped = false;
            } else if bytes[i] == b'\\' {
                escaped = true;
            } else if bytes[i] == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i = skip_block_comment(bytes, i);
            continue;
        }
        if depth == 0 && word_at(bytes, i, b"fn") {
            let mut cursor = i + 2;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            if word_at(bytes, cursor, b"build") {
                return true;
            }
        }
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    false
}

fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut depth = 1usize;
    let mut i = start.saturating_add(2);
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            depth = depth.saturating_add(1);
            i += 2;
        } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
            depth = depth.saturating_sub(1);
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn word_at(bytes: &[u8], at: usize, word: &[u8]) -> bool {
    let end = at.saturating_add(word.len());
    end <= bytes.len()
        && &bytes[at..end] == word
        && (at == 0 || !is_ident_byte(bytes[at - 1]))
        && (end == bytes.len() || !is_ident_byte(bytes[end]))
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
