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
        let mut selector = ProviderSelector {
            raw: raw.to_string(),
            ..Default::default()
        };
        for part in raw
            .strip_prefix('#')
            .unwrap_or(raw)
            .split(['&', ','])
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            let (key, value) = part.split_once('=').unwrap_or(("revision", part));
            match key.trim() {
                "version" => selector.version = value.trim().to_string(),
                "revision" | "rev" => selector.revision = value.trim().to_string(),
                "baseline" => selector.baseline = value.trim().to_string(),
                "feature" | "features" => {
                    selector.features = value
                        .split('+')
                        .chain(value.split(','))
                        .map(str::trim)
                        .filter(|feature| !feature.is_empty())
                        .map(str::to_string)
                        .collect();
                    selector.features.sort();
                    selector.features.dedup();
                }
                "digest" | "hash" | "sha256" => selector.digest = value.trim().to_string(),
                "platform" | "target" => selector.platform = value.trim().to_string(),
                _ => {}
            }
        }
        selector
    }

    pub fn is_exact(&self) -> bool {
        !self.version.is_empty() || !self.revision.is_empty() || !self.digest.is_empty()
    }
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
        let provider = if provider.trim().is_empty() {
            inferred_provider
        } else {
            provider
        };
        let selector_raw = selector_part
            .map(|selector| format!("#{selector}"))
            .unwrap_or_default();
        let selector = ProviderSelector::parse(&selector_raw);
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
            return Err("provider facts need a provider, target, and fully qualified reference".to_string());
        }
        Ok(())
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
        let root = JSON::parse(text)?.as_object()?.clone();
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
            lines.push(format!("loss {}: {} ({})", loss.key, loss.reason, loss.source));
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
                key: object.get("key").ok_or("loss lacks key")?.as_str()?.to_string(),
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
                key: object.get("key").ok_or("conflict lacks key")?.as_str()?.to_string(),
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
