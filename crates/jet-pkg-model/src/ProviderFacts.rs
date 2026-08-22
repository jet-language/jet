//! One lossless provider-fact carrier shared by plans, locks, and explain.
//!
//! A provider adapter may project typed facts, but it may not discard the
//! native document or hide a field it cannot prove.  This record is the one
//! identity/provenance seam consumed by package profiles and semantic locks.

use crate::JSON::{self, JSONValue};
use crate::SHA256;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFactValue {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    List(Vec<ProviderFactValue>),
    Map(BTreeMap<String, ProviderFactValue>),
}

impl ProviderFactValue {
    fn to_json(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => value.clone(),
            Self::Text(value) => JSON::quote(value),
            Self::List(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Self::to_json)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Map(values) => format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{}:{}", JSON::quote(key), value.to_json()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }

    fn from_json(value: &JSONValue) -> Result<Self, String> {
        match value {
            JSONValue::Null => Ok(Self::Null),
            JSONValue::Bool(value) => Ok(Self::Bool(*value)),
            JSONValue::Number(value) => Ok(Self::Number(value.to_string())),
            JSONValue::Flt(value) => Ok(Self::Number(value.to_string())),
            JSONValue::String(value) => Ok(Self::Text(value.clone())),
            JSONValue::Array(values) => values
                .iter()
                .map(Self::from_json)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List),
            JSONValue::Object(values) => values
                .iter()
                .map(|(key, value)| Ok((key.clone(), Self::from_json(value)?)))
                .collect::<Result<BTreeMap<_, _>, String>>()
                .map(Self::Map),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderSelector {
    pub raw: String,
    pub version: String,
    pub revision: String,
    pub baseline: String,
    pub features: Vec<String>,
    pub digest: String,
    pub platform: String,
}

impl ProviderSelector {
    pub fn parse(raw: &str) -> ProviderSelector {
        parse_selector_details(raw).selector
    }

    pub fn is_exact(&self) -> bool {
        let has_exact_identity = [
            exact_selector_value(&self.version, SelectorValue::Version),
            exact_selector_value(&self.revision, SelectorValue::Revision),
            exact_selector_value(&self.digest, SelectorValue::Digest),
        ]
        .into_iter()
        .any(|exact| exact);
        let has_invalid_identity = [
            (&self.version, SelectorValue::Version),
            (&self.revision, SelectorValue::Revision),
            (&self.digest, SelectorValue::Digest),
        ]
        .into_iter()
        .any(|(value, kind)| !value.is_empty() && !exact_selector_value(value, kind));
        has_exact_identity && !has_invalid_identity
    }
}

#[derive(Clone, Copy)]
enum SelectorValue {
    Version,
    Revision,
    Digest,
}

fn exact_selector_value(value: &str, kind: SelectorValue) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    match kind {
        SelectorValue::Version => {
            !value.starts_with(['^', '~', '>', '<', '=', '*'])
                && !value.contains("||")
                && !value.contains(',')
                && !matches!(value, "latest" | "next" | "main" | "master" | "head")
        }
        SelectorValue::Revision => {
            !matches!(
                value,
                "latest" | "next" | "main" | "master" | "head" | "develop"
            ) && !value.starts_with("refs/heads/")
        }
        SelectorValue::Digest => {
            value.starts_with("sha256-") || value.starts_with("sha256:") || value.len() >= 32
        }
    }
}

#[derive(Default)]
struct SelectorDetails {
    selector: ProviderSelector,
    unknown: Vec<String>,
    conflicts: Vec<(String, String, String)>,
}

fn parse_selector_details(raw: &str) -> SelectorDetails {
    let mut details = SelectorDetails {
        selector: ProviderSelector {
            raw: raw.to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let body = raw.strip_prefix('#').unwrap_or(raw);
    for group in body.split('&') {
        let mut start = 0;
        for (offset, character) in group.char_indices() {
            if character == ',' && is_selector_assignment(group[offset + 1..].trim_start()) {
                parse_selector_part(&mut details, group[start..offset].trim());
                start = offset + 1;
            }
        }
        parse_selector_part(&mut details, group[start..].trim());
    }
    details
}

fn parse_selector_part(details: &mut SelectorDetails, part: &str) {
    if part.is_empty() {
        return;
    }
    let (key, value) = part.split_once('=').unwrap_or(("version", part));
    let key = key.trim();
    let value = value.trim();
    match key {
        "version" => set_selector_value(
            &mut details.selector.version,
            "version",
            value,
            &mut details.conflicts,
        ),
        "revision" | "rev" => set_selector_value(
            &mut details.selector.revision,
            "revision",
            value,
            &mut details.conflicts,
        ),
        "baseline" => set_selector_value(
            &mut details.selector.baseline,
            "baseline",
            value,
            &mut details.conflicts,
        ),
        "feature" | "features" => {
            details.selector.features.extend(
                value
                    .split(['+', ','])
                    .map(str::trim)
                    .filter(|feature| !feature.is_empty())
                    .map(str::to_string),
            );
            details.selector.features.sort();
            details.selector.features.dedup();
        }
        "digest" | "hash" | "sha256" => set_selector_value(
            &mut details.selector.digest,
            "digest",
            value,
            &mut details.conflicts,
        ),
        "platform" | "target" => set_selector_value(
            &mut details.selector.platform,
            "platform",
            value,
            &mut details.conflicts,
        ),
        _ => details.unknown.push(part.to_string()),
    }
}

fn set_selector_value(
    slot: &mut String,
    key: &str,
    value: &str,
    conflicts: &mut Vec<(String, String, String)>,
) {
    if !slot.is_empty() && slot != value {
        conflicts.push((key.to_string(), slot.clone(), value.to_string()));
    } else if slot.is_empty() {
        *slot = value.to_string();
    }
}

fn is_selector_assignment(part: &str) -> bool {
    let Some((key, _)) = part.split_once('=') else {
        return false;
    };
    matches!(
        key.trim(),
        "version"
            | "revision"
            | "rev"
            | "baseline"
            | "feature"
            | "features"
            | "digest"
            | "hash"
            | "sha256"
            | "platform"
            | "target"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderLoss {
    pub key: String,
    pub reason: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConflict {
    pub key: String,
    pub left: String,
    pub right: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFacts {
    pub provider: String,
    pub reference: String,
    pub target: String,
    pub selector: ProviderSelector,
    pub resolved_source: String,
    pub profile: String,
    pub profile_provenance: String,
    pub facts: BTreeMap<String, ProviderFactValue>,
    pub provenance: BTreeMap<String, String>,
    pub native_format: String,
    pub native_document: String,
    pub losses: Vec<ProviderLoss>,
    pub conflicts: Vec<ProviderConflict>,
}

impl Default for ProviderFacts {
    fn default() -> Self {
        Self::for_reference("core", "profile@core")
    }
}

impl ProviderFacts {
    pub fn for_reference(provider: &str, reference: &str) -> ProviderFacts {
        let (reference_without_selector, selector_part) = split_reference_selector(reference);
        let (target, inferred_provider) = reference_without_selector
            .rsplit_once('@')
            .map(|(target, provider)| (target, provider))
            .unwrap_or((reference_without_selector.as_str(), ""));
        let explicit_provider = provider.trim();
        let provider = if explicit_provider.is_empty() {
            inferred_provider
        } else {
            provider
        };
        let selector_raw = selector_part
            .map(|selector| format!("#{selector}"))
            .unwrap_or_default();
        let selector_details = parse_selector_details(&selector_raw);
        let selector = selector_details.selector;
        let mut facts = ProviderFacts {
            provider: provider.to_string(),
            reference: reference.to_string(),
            target: target.to_string(),
            selector,
            resolved_source: String::new(),
            profile: String::new(),
            profile_provenance: String::new(),
            facts: BTreeMap::new(),
            provenance: BTreeMap::new(),
            native_format: String::new(),
            native_document: String::new(),
            losses: Vec::new(),
            conflicts: Vec::new(),
        };
        facts.add_fact(
            "provider.name",
            ProviderFactValue::Text(facts.provider.clone()),
            "ref",
        );
        facts.add_fact(
            "provider.target",
            ProviderFactValue::Text(facts.target.clone()),
            "ref",
        );
        if !facts.selector.raw.is_empty() {
            facts.add_fact(
                "provider.selector",
                ProviderFactValue::Text(facts.selector.raw.clone()),
                "ref",
            );
        }
        if !explicit_provider.is_empty()
            && !inferred_provider.trim().is_empty()
            && explicit_provider != inferred_provider
        {
            // Named sources such as `@default` are source authorities, not
            // provider names. Keep that spelling without treating it as a
            // conflict; two recognized external providers are contradictory.
            facts.add_fact(
                "provider.authority",
                ProviderFactValue::Text(inferred_provider.to_string()),
                "ref",
            );
            let provider_name = facts.provider.clone();
            if is_external_provider(&provider_name) && is_external_provider(inferred_provider) {
                facts.add_conflict(
                    "provider",
                    &provider_name,
                    inferred_provider,
                    "reference.provider",
                );
            }
        }
        if provider == "infer" {
            facts.add_loss(
                "provider",
                "ambiguous provider fact: unresolved inference",
                "profile-source",
            );
        } else if is_external_provider(provider) && !facts.selector.is_exact() {
            facts.add_loss(
                "provider.selector",
                "external provider references must retain an exact version, revision, or digest",
                "reference.selector",
            );
        }
        for unknown in selector_details.unknown {
            facts.add_loss(
                "provider.selector",
                &format!("unsupported selector fact `{unknown}` cannot be normalized losslessly"),
                "reference.selector",
            );
        }
        for (key, left, right) in selector_details.conflicts {
            facts.add_conflict(
                &format!("provider.selector.{key}"),
                &left,
                &right,
                "reference.selector",
            );
        }
        for (key, value, kind) in [
            (
                "version",
                facts.selector.version.clone(),
                SelectorValue::Version,
            ),
            (
                "revision",
                facts.selector.revision.clone(),
                SelectorValue::Revision,
            ),
            (
                "digest",
                facts.selector.digest.clone(),
                SelectorValue::Digest,
            ),
        ] {
            if !value.is_empty() && !exact_selector_value(&value, kind) {
                facts.add_loss(
                    &format!("provider.selector.{key}"),
                    &format!("mutable {key} selector `{value}` is not an exact provider identity"),
                    "reference.selector",
                );
            }
        }
        facts
    }

    pub fn add_fact(&mut self, key: &str, value: ProviderFactValue, source: &str) {
        if let Some(existing) = self.facts.get(key) {
            if existing != &value {
                let left = existing.to_json();
                let right = value.to_json();
                self.conflicts.push(ProviderConflict {
                    key: key.to_string(),
                    left,
                    right,
                    source: source.to_string(),
                });
            } else if let Some(existing_source) = self.provenance.get_mut(key) {
                if existing_source != source
                    && !existing_source.split(" | ").any(|item| item == source)
                {
                    existing_source.push_str(" | ");
                    existing_source.push_str(source);
                }
            }
            return;
        }
        self.facts.insert(key.to_string(), value);
        self.provenance.insert(key.to_string(), source.to_string());
    }

    pub fn add_loss(&mut self, key: &str, reason: &str, source: &str) {
        self.losses.push(ProviderLoss {
            key: key.to_string(),
            reason: reason.to_string(),
            source: source.to_string(),
        });
    }

    pub fn add_conflict(&mut self, key: &str, left: &str, right: &str, source: &str) {
        self.conflicts.push(ProviderConflict {
            key: key.to_string(),
            left: left.to_string(),
            right: right.to_string(),
            source: source.to_string(),
        });
    }

    pub fn set_profile(&mut self, profile: &str, provenance: &str) {
        self.profile = profile.to_string();
        self.profile_provenance = provenance.to_string();
        self.add_fact(
            "profile.name",
            ProviderFactValue::Text(profile.to_string()),
            "profile.source",
        );
        self.add_fact(
            "profile.provenance",
            ProviderFactValue::Text(provenance.to_string()),
            "profile.source",
        );
    }

    pub fn set_resolved_source(&mut self, source: &str) {
        self.resolved_source = source.to_string();
        self.add_fact(
            "provider.resolved_source",
            ProviderFactValue::Text(source.to_string()),
            "resolver",
        );
    }

    pub fn set_native_document(&mut self, format: &str, document: &str) {
        self.native_format = format.to_string();
        self.native_document = document.to_string();
    }

    pub fn is_lossless(&self) -> bool {
        self.losses.is_empty() && self.conflicts.is_empty()
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(conflict) = self.conflicts.first() {
            return Err(format!(
                "provider fact `{}` conflicts: {} vs {} ({})",
                conflict.key, conflict.left, conflict.right, conflict.source
            ));
        }
        if let Some(loss) = self.losses.first() {
            return Err(format!(
                "provider fact `{}` is lossy: {} ({})",
                loss.key, loss.reason, loss.source
            ));
        }
        if self.provider.trim().is_empty()
            || self.reference.trim().is_empty()
            || self.target.trim().is_empty()
        {
            return Err(
                "provider facts need a provider, target, and fully qualified reference".to_string(),
            );
        }
        let (reference_root, reference_selector) = split_reference_selector(&self.reference);
        let (reference_target, _) = reference_root
            .rsplit_once('@')
            .unwrap_or((reference_root.as_str(), ""));
        if reference_target != self.target {
            return Err(format!(
                "provider fact target `{}` disagrees with reference target `{reference_target}`",
                self.target
            ));
        }
        if let Some(raw_selector) = reference_selector {
            let parsed = parse_selector_details(&format!("#{raw_selector}")).selector;
            if parsed.version != self.selector.version
                || parsed.revision != self.selector.revision
                || parsed.baseline != self.selector.baseline
                || parsed.features != self.selector.features
                || parsed.digest != self.selector.digest
                || parsed.platform != self.selector.platform
            {
                return Err(
                    "provider fact selector disagrees with its fully qualified reference"
                        .to_string(),
                );
            }
        } else if !self.selector.raw.is_empty() {
            return Err(
                "provider fact selector is present but the reference has no selector".to_string(),
            );
        }
        if is_external_provider(&self.provider) && !self.effective_selector().is_exact() {
            return Err(format!(
                "external provider `{}` requires an exact version, revision, or digest",
                self.provider
            ));
        }
        for key in self.facts.keys() {
            if !self.provenance.contains_key(key) {
                return Err(format!("provider fact `{key}` lacks provenance"));
            }
        }
        for key in self.provenance.keys() {
            if !self.facts.contains_key(key) {
                return Err(format!("provider provenance `{key}` has no fact"));
            }
        }
        for (key, expected) in [
            ("provider.name", self.provider.as_str()),
            ("provider.target", self.target.as_str()),
            ("provider.selector", self.selector.raw.as_str()),
        ] {
            if !expected.is_empty()
                && !matches!(self.facts.get(key), Some(ProviderFactValue::Text(value)) if value == expected)
            {
                return Err(format!(
                    "provider identity field `{key}` disagrees with its typed fact"
                ));
            }
        }
        for (key, expected) in [
            ("provider.resolved_source", self.resolved_source.as_str()),
            ("profile.name", self.profile.as_str()),
            ("profile.provenance", self.profile_provenance.as_str()),
        ] {
            if !expected.is_empty()
                && !matches!(self.facts.get(key), Some(ProviderFactValue::Text(value)) if value == expected)
            {
                return Err(format!(
                    "provider field `{key}` disagrees with its typed fact"
                ));
            }
        }
        if self.native_document.is_empty() != self.native_format.is_empty() {
            return Err(
                "provider native format and native document must be retained together".to_string(),
            );
        }
        Ok(())
    }

    /// Return the canonical direct-root spelling used by locks and generated
    /// output. The original source spelling remains in `reference`.
    pub fn qualified_reference(&self) -> String {
        let (root, _) = split_reference_selector(&self.reference);
        let selector = canonical_selector(&self.effective_selector());
        if selector.is_empty() {
            return root;
        }
        if let Some((target, provider)) = root.rsplit_once('@') {
            format!("{target}#{selector}@{provider}")
        } else {
            format!("{root}#{selector}")
        }
    }

    /// Use a selector resolved by an explicitly declared source authority when
    /// the package spelling itself is intentionally unpinned. The raw selector
    /// and the resolved selector remain separate facts.
    pub fn effective_selector(&self) -> ProviderSelector {
        if self.selector.is_exact() {
            return self.selector.clone();
        }
        let Some(ProviderFactValue::Text(raw)) = self.facts.get("provider.resolved_selector")
        else {
            return self.selector.clone();
        };
        ProviderSelector::parse(raw)
    }

    pub fn digest(&self) -> String {
        format!("sha256-{}", SHA256::sha256_hex(self.to_json().as_bytes()))
    }

    pub fn to_json(&self) -> String {
        let facts = self
            .facts
            .iter()
            .map(|(key, value)| format!("{}:{}", JSON::quote(key), value.to_json()))
            .collect::<Vec<_>>()
            .join(",");
        let provenance = self
            .provenance
            .iter()
            .map(|(key, value)| format!("{}:{}", JSON::quote(key), JSON::quote(value)))
            .collect::<Vec<_>>()
            .join(",");
        let losses = self
            .losses
            .iter()
            .map(|loss| {
                format!(
                    "{{\"key\":{},\"reason\":{},\"source\":{}}}",
                    JSON::quote(&loss.key),
                    JSON::quote(&loss.reason),
                    JSON::quote(&loss.source)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let conflicts = self
            .conflicts
            .iter()
            .map(|conflict| {
                format!(
                    "{{\"key\":{},\"left\":{},\"right\":{},\"source\":{}}}",
                    JSON::quote(&conflict.key),
                    JSON::quote(&conflict.left),
                    JSON::quote(&conflict.right),
                    JSON::quote(&conflict.source)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":\"jet-provider-facts-v1\",\"provider\":{},\"reference\":{},\"target\":{},\"selector\":{},\"resolved_source\":{},\"profile\":{},\"profile_provenance\":{},\"facts\":{{{}}},\"provenance\":{{{}}},\"native_format\":{},\"native_document\":{},\"losses\":[{}],\"conflicts\":[{}]}}",
            JSON::quote(&self.provider),
            JSON::quote(&self.reference),
            JSON::quote(&self.target),
            selector_json(&self.selector),
            JSON::quote(&self.resolved_source),
            JSON::quote(&self.profile),
            JSON::quote(&self.profile_provenance),
            facts,
            provenance,
            JSON::quote(&self.native_format),
            JSON::quote(&self.native_document),
            losses,
            conflicts,
        )
    }

    pub fn from_json(text: &str) -> Result<ProviderFacts, String> {
        let value = JSON::parse(text)?;
        Self::from_json_value(&value)
    }

    pub fn from_json_value(value: &JSONValue) -> Result<ProviderFacts, String> {
        let root = value.as_object()?;
        if root
            .get("schema")
            .ok_or_else(|| "provider facts lack `schema`".to_string())?
            .as_str()?
            != "jet-provider-facts-v1"
        {
            return Err("provider facts have an unsupported schema".to_string());
        }
        let string = |key: &str| {
            root.get(key)
                .ok_or_else(|| format!("provider facts lack `{key}`"))?
                .as_str()
                .map(str::to_string)
        };
        let selector = parse_selector_value(root.get("selector"))?;
        let facts = match root.get("facts") {
            Some(JSONValue::Object(values)) => values
                .iter()
                .map(|(key, value)| Ok((key.clone(), ProviderFactValue::from_json(value)?)))
                .collect::<Result<BTreeMap<_, _>, String>>()?,
            _ => return Err("provider facts `facts` is not an object".to_string()),
        };
        let provenance = match root.get("provenance") {
            Some(JSONValue::Object(values)) => values
                .iter()
                .map(|(key, value)| Ok((key.clone(), value.as_str()?.to_string())))
                .collect::<Result<BTreeMap<_, _>, String>>()?,
            _ => return Err("provider facts `provenance` is not an object".to_string()),
        };
        let losses = parse_losses(root.get("losses"))?;
        let conflicts = parse_conflicts(root.get("conflicts"))?;
        Ok(ProviderFacts {
            provider: string("provider")?,
            reference: string("reference")?,
            target: string("target")?,
            selector,
            resolved_source: string("resolved_source")?,
            profile: string("profile")?,
            profile_provenance: string("profile_provenance")?,
            facts,
            provenance,
            native_format: string("native_format")?,
            native_document: string("native_document")?,
            losses,
            conflicts,
        })
    }

    pub fn explain_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("provider: {}", self.provider),
            format!("reference: {}", self.reference),
            format!("target: {}", self.target),
        ];
        if !self.resolved_source.is_empty() {
            lines.push(format!("resolved-source: {}", self.resolved_source));
        }
        if !self.profile.is_empty() {
            lines.push(format!("profile: {}", self.profile));
            lines.push(format!("profile-provenance: {}", self.profile_provenance));
        }
        for (key, value) in &self.facts {
            lines.push(format!("fact {key}: {}", value.to_json()));
            if let Some(source) = self.provenance.get(key) {
                lines.push(format!("  provenance {key}: {source}"));
            }
        }
        for loss in &self.losses {
            lines.push(format!(
                "loss {}: {} ({})",
                loss.key, loss.reason, loss.source
            ));
        }
        for conflict in &self.conflicts {
            lines.push(format!(
                "conflict {}: {} vs {} ({})",
                conflict.key, conflict.left, conflict.right, conflict.source
            ));
        }
        if !self.native_document.is_empty() {
            lines.push(format!("native {}: retained", self.native_format));
        }
        lines
    }
}

/// Accept both Jet refs (`target#selector@provider`) and provider records that
/// already use the normalized form (`target@provider#selector`).
fn split_reference_selector(reference: &str) -> (String, Option<String>) {
    let Some((prefix, suffix)) = reference.split_once('#') else {
        return (reference.to_string(), None);
    };
    if let Some((selector, provider)) = suffix.rsplit_once('@') {
        return (format!("{prefix}@{provider}"), Some(selector.to_string()));
    }
    (prefix.to_string(), Some(suffix.to_string()))
}

fn canonical_selector(selector: &ProviderSelector) -> String {
    let mut parts = Vec::new();
    if !selector.version.is_empty() {
        parts.push(format!("version={}", selector.version));
    }
    if !selector.revision.is_empty() {
        parts.push(format!("revision={}", selector.revision));
    }
    if !selector.baseline.is_empty() {
        parts.push(format!("baseline={}", selector.baseline));
    }
    if !selector.features.is_empty() {
        parts.push(format!("features={}", selector.features.join("+")));
    }
    if !selector.digest.is_empty() {
        parts.push(format!("digest={}", selector.digest));
    }
    if !selector.platform.is_empty() {
        parts.push(format!("platform={}", selector.platform));
    }
    parts.join("&")
}

fn is_external_provider(provider: &str) -> bool {
    matches!(
        provider,
        "jet-registry"
            | "npm"
            | "pypi"
            | "cargo"
            | "swiftpm"
            | "maven"
            | "nuget"
            | "conan"
            | "vcpkg"
            | "github"
            | "homebrew"
            | "binary"
            | "cran"
            | "luarocks"
            | "ruby"
            | "perl"
            | "php"
    )
}

fn selector_json(selector: &ProviderSelector) -> String {
    format!(
        "{{\"raw\":{},\"version\":{},\"revision\":{},\"baseline\":{},\"features\":[{}],\"digest\":{},\"platform\":{}}}",
        JSON::quote(&selector.raw),
        JSON::quote(&selector.version),
        JSON::quote(&selector.revision),
        JSON::quote(&selector.baseline),
        selector
            .features
            .iter()
            .map(|feature| JSON::quote(feature))
            .collect::<Vec<_>>()
            .join(","),
        JSON::quote(&selector.digest),
        JSON::quote(&selector.platform),
    )
}

fn parse_selector_value(value: Option<&JSONValue>) -> Result<ProviderSelector, String> {
    let Some(JSONValue::Object(object)) = value else {
        return Err("provider facts `selector` is not an object".to_string());
    };
    let string = |key: &str| {
        object
            .get(key)
            .ok_or_else(|| format!("provider selector lacks `{key}`"))?
            .as_str()
            .map(str::to_string)
    };
    let features = object
        .get("features")
        .ok_or_else(|| "provider selector lacks `features`".to_string())?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_string))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ProviderSelector {
        raw: string("raw")?,
        version: string("version")?,
        revision: string("revision")?,
        baseline: string("baseline")?,
        features,
        digest: string("digest")?,
        platform: string("platform")?,
    })
}

fn parse_losses(value: Option<&JSONValue>) -> Result<Vec<ProviderLoss>, String> {
    let Some(JSONValue::Array(values)) = value else {
        return Err("provider facts `losses` is not an array".to_string());
    };
    values
        .iter()
        .map(|value| {
            let object = value.as_object()?;
            Ok(ProviderLoss {
                key: object
                    .get("key")
                    .ok_or("loss lacks key")?
                    .as_str()?
                    .to_string(),
                reason: object
                    .get("reason")
                    .ok_or("loss lacks reason")?
                    .as_str()?
                    .to_string(),
                source: object
                    .get("source")
                    .ok_or("loss lacks source")?
                    .as_str()?
                    .to_string(),
            })
        })
        .collect()
}

fn parse_conflicts(value: Option<&JSONValue>) -> Result<Vec<ProviderConflict>, String> {
    let Some(JSONValue::Array(values)) = value else {
        return Err("provider facts `conflicts` is not an array".to_string());
    };
    values
        .iter()
        .map(|value| {
            let object = value.as_object()?;
            Ok(ProviderConflict {
                key: object
                    .get("key")
                    .ok_or("conflict lacks key")?
                    .as_str()?
                    .to_string(),
                left: object
                    .get("left")
                    .ok_or("conflict lacks left")?
                    .as_str()?
                    .to_string(),
                right: object
                    .get("right")
                    .ok_or("conflict lacks right")?
                    .as_str()?
                    .to_string(),
                source: object
                    .get("source")
                    .ok_or("conflict lacks source")?
                    .as_str()?
                    .to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ProviderFacts, ProviderSelector};

    #[test]
    fn bare_selector_is_version_and_features_are_lossless() {
        let selector = ProviderSelector::parse("#2.0.17&features=lua,ssl");
        assert_eq!(selector.version, "2.0.17");
        assert_eq!(selector.revision, "");
        assert_eq!(
            selector.features,
            vec!["lua".to_string(), "ssl".to_string()]
        );
        assert!(selector.is_exact());
    }

    #[test]
    fn provider_facts_round_trip_identity_provenance_and_native_bytes() {
        let mut facts = ProviderFacts::for_reference("npm", "left-pad#2.0.17@npm");
        facts.set_resolved_source("npm:left-pad@2.0.17");
        facts.set_native_document("package.json", "{\"name\":\"left-pad\"}\n");
        facts.add_fact(
            "package.version",
            super::ProviderFactValue::Text("2.0.17".to_string()),
            "package.json.version",
        );
        facts.validate().expect("lossless provider facts");
        assert_eq!(facts.qualified_reference(), "left-pad#version=2.0.17@npm");
        let round_trip = ProviderFacts::from_json(&facts.to_json()).expect("provider facts JSON");
        assert_eq!(round_trip, facts);
        assert!(round_trip
            .explain_lines()
            .iter()
            .any(|line| line == "native package.json: retained"));
    }

    #[test]
    fn mutable_selector_is_an_explicit_loss() {
        let facts = ProviderFacts::for_reference("npm", "vite#^5@npm");
        assert!(!facts.is_lossless());
        let error = facts
            .validate()
            .expect_err("mutable selector must fail closed");
        assert!(
            error.contains("lossy") || error.contains("exact"),
            "{error}"
        );
    }

    #[test]
    fn source_alias_is_retained_and_external_provider_mismatch_conflicts() {
        let alias = ProviderFacts::for_reference("nix", "ripgrep@default");
        assert_eq!(
            alias.facts.get("provider.authority"),
            Some(&super::ProviderFactValue::Text("default".to_string()))
        );
        alias.validate().expect("named source authority is valid");

        let conflict = ProviderFacts::for_reference("npm", "left-pad#1.0.0@cargo");
        assert!(conflict
            .conflicts
            .iter()
            .any(|item| item.key == "provider"));
        assert!(conflict.validate().is_err());
    }
}
