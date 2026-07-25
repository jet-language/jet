//! Rich-output trust (D-NOTEBOOK-TRUST1=D).

use jet_foundation::SHA256;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Bound into every grant key so policy edits revoke automatically.
pub const POLICY_VERSION: &str = "notebook-trust-1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MimeBundle {
    pub text_plain: String,
    pub mime: Vec<(String, String)>,
    /// True when this payload arrived via ipynb import (always quarantined).
    pub quarantined: bool,
    pub widget_id: Option<String>,
    pub requested_origins: Vec<String>,
    pub requested_messages: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveRequest {
    pub notebook_source_hash: String,
    pub payload_hash: String,
    pub renderer_hash: String,
    pub environment_hash: String,
    pub policy_version: String,
    pub widget_id: String,
    pub origins: Vec<String>,
    pub messages: Vec<String>,
}

impl ActiveRequest {
    pub fn key(&self) -> String {
        grant_key(
            &self.notebook_source_hash,
            &self.payload_hash,
            &self.renderer_hash,
            &self.environment_hash,
            &self.policy_version,
        )
    }

    pub fn needs_capabilities(&self) -> bool {
        !self.origins.is_empty() || !self.messages.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustGrant {
    pub key: String,
    pub widget_id: String,
    pub origins: Vec<String>,
    pub messages: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct TrustStore {
    grants: BTreeSet<String>,
    /// Full grant records keyed by grant key.
    records: Vec<TrustGrant>,
}

impl TrustStore {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let mut store = Self::default();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("grant:user:notebook:") {
                // subject encodes key|widget|origins|messages
                if let Some(grant) = parse_notebook_grant_subject(rest) {
                    store.grants.insert(grant.key.clone());
                    store.records.push(grant);
                }
            }
        }
        store
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut existing = String::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                if !line.starts_with("grant:user:notebook:") {
                    existing.push_str(line);
                    existing.push('\n');
                }
            }
        }
        for grant in &self.records {
            existing.push_str("grant:user:notebook:");
            existing.push_str(&encode_notebook_grant_subject(grant));
            existing.push('\n');
        }
        std::fs::write(path, existing).map_err(|e| e.to_string())
    }

    pub fn insert(&mut self, grant: TrustGrant) {
        self.grants.insert(grant.key.clone());
        self.records.retain(|g| g.key != grant.key);
        self.records.push(grant);
    }

    pub fn revoke_key(&mut self, key: &str) {
        self.grants.remove(key);
        self.records.retain(|g| g.key != key);
    }
}

pub fn trust_store_path() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".jet").join("trust")
}

pub fn grant_key(
    notebook_source_hash: &str,
    payload_hash: &str,
    renderer_hash: &str,
    environment_hash: &str,
    policy_version: &str,
) -> String {
    SHA256::sha256_hex(
        format!(
            "{notebook_source_hash}\0{payload_hash}\0{renderer_hash}\0{environment_hash}\0{policy_version}"
        )
        .as_bytes(),
    )
}

pub fn is_granted(store: &TrustStore, key: &str) -> bool {
    store.grants.contains(key)
}

pub fn grant_active(store: &mut TrustStore, request: &ActiveRequest) -> TrustGrant {
    let grant = TrustGrant {
        key: request.key(),
        widget_id: request.widget_id.clone(),
        origins: request.origins.clone(),
        messages: request.messages.clone(),
    };
    store.insert(grant.clone());
    grant
}

/// Source/payload/renderer/environment/policy edits revoke local grants.
/// Session stores are notebook-scoped, so a source-hash change clears them.
pub fn revoke_matching(store: &mut TrustStore, _notebook_source_hash: &str) {
    store.grants.clear();
    store.records.clear();
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderDecision {
    /// Sanitized passive MIME or zero-capability sandboxed widget — no prompt.
    AllowPassive { text_plain: String, mime: Vec<(String, String)> },
    /// Capability widget with a matching local grant.
    AllowActive { text_plain: String, mime: Vec<(String, String)> },
    /// Quarantined / ungated active content — safe text fallback only.
    FallbackPlain { text_plain: String, reason: String },
}

pub fn quarantine_outputs(bundle: &mut MimeBundle) {
    bundle.quarantined = true;
}

/// Decide how a MIME bundle may render under D-NOTEBOOK-TRUST1=D.
pub fn decide_render(
    store: &TrustStore,
    notebook_source_hash: &str,
    environment_hash: &str,
    renderer_hash: &str,
    bundle: &MimeBundle,
) -> RenderDecision {
    let text = if bundle.text_plain.is_empty() {
        "(no text/plain)".to_string()
    } else {
        bundle.text_plain.clone()
    };

    if bundle.quarantined {
        return RenderDecision::FallbackPlain {
            text_plain: text,
            reason: "imported output is quarantined until re-run under local trust".into(),
        };
    }

    let payload_hash = SHA256::sha256_hex(
        format!("{:?}\0{:?}", bundle.mime, bundle.widget_id).as_bytes(),
    );

    let active = ActiveRequest {
        notebook_source_hash: notebook_source_hash.to_string(),
        payload_hash,
        renderer_hash: renderer_hash.to_string(),
        environment_hash: environment_hash.to_string(),
        policy_version: POLICY_VERSION.to_string(),
        widget_id: bundle.widget_id.clone().unwrap_or_default(),
        origins: bundle.requested_origins.clone(),
        messages: bundle.requested_messages.clone(),
    };

    let is_active = bundle.widget_id.is_some()
        || bundle.mime.iter().any(|(m, _)| {
            m == "application/javascript"
                || m == "text/html"
                || m.starts_with("application/vnd.jet.widget")
        });

    if !is_active {
        // Passive MIME after sanitization (caller supplies already-safe bytes).
        return RenderDecision::AllowPassive {
            text_plain: text,
            mime: sanitize_passive(&bundle.mime),
        };
    }

    if !active.needs_capabilities() {
        // Zero-capability opaque-origin sandbox — passive-equivalent, no prompt.
        return RenderDecision::AllowPassive {
            text_plain: text,
            mime: sanitize_passive(&bundle.mime),
        };
    }

    if is_granted(store, &active.key()) {
        return RenderDecision::AllowActive {
            text_plain: text,
            mime: bundle.mime.clone(),
        };
    }

    RenderDecision::FallbackPlain {
        text_plain: text,
        reason: format!(
            "active output blocked: widget `{}` needs an explicit jet trust notebook grant",
            active.widget_id
        ),
    }
}

fn sanitize_passive(mime: &[(String, String)]) -> Vec<(String, String)> {
    mime.iter()
        .filter(|(m, _)| {
            matches!(
                m.as_str(),
                "text/plain"
                    | "text/markdown"
                    | "image/png"
                    | "image/jpeg"
                    | "image/svg+xml"
                    | "text/html"
                    | "application/json"
            )
        })
        .cloned()
        .collect()
}

fn encode_notebook_grant_subject(grant: &TrustGrant) -> String {
    format!(
        "{}|{}|{}|{}",
        grant.key,
        escape(grant.widget_id.as_str()),
        escape(&grant.origins.join(",")),
        escape(&grant.messages.join(",")),
    )
}

fn parse_notebook_grant_subject(subject: &str) -> Option<TrustGrant> {
    let mut parts = subject.splitn(4, '|');
    let key = parts.next()?.to_string();
    let widget_id = unescape(parts.next()?);
    let origins = split_list(&unescape(parts.next()?));
    let messages = split_list(&unescape(parts.next()?));
    Some(TrustGrant {
        key,
        widget_id,
        origins,
        messages,
    })
}

fn split_list(s: &str) -> Vec<String> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split(',').map(|x| x.to_string()).collect()
    }
}

fn escape(s: &str) -> String {
    s.replace('%', "%25").replace('|', "%7C").replace('\n', "")
}

fn unescape(s: &str) -> String {
    s.replace("%7C", "|").replace("%25", "%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_cap_widget_renders_without_grant() {
        let store = TrustStore::default();
        let bundle = MimeBundle {
            text_plain: "chart".into(),
            mime: vec![("application/vnd.jet.widget".into(), "{}".into())],
            quarantined: false,
            widget_id: Some("sales.chart".into()),
            requested_origins: vec![],
            requested_messages: vec![],
        };
        match decide_render(&store, "src", "env", "rend", &bundle) {
            RenderDecision::AllowPassive { .. } => {}
            other => panic!("expected passive allow, got {other:?}"),
        }
    }

    #[test]
    fn capability_widget_needs_grant() {
        let mut store = TrustStore::default();
        let bundle = MimeBundle {
            text_plain: "chart".into(),
            mime: vec![("application/javascript".into(), "1".into())],
            quarantined: false,
            widget_id: Some("sales.chart".into()),
            requested_origins: vec!["https://data.example".into()],
            requested_messages: vec!["SelectionChanged".into()],
        };
        assert!(matches!(
            decide_render(&store, "src", "env", "rend", &bundle),
            RenderDecision::FallbackPlain { .. }
        ));
        let payload_hash = SHA256::sha256_hex(
            format!("{:?}\0{:?}", bundle.mime, bundle.widget_id).as_bytes(),
        );
        let req = ActiveRequest {
            notebook_source_hash: "src".into(),
            payload_hash,
            renderer_hash: "rend".into(),
            environment_hash: "env".into(),
            policy_version: POLICY_VERSION.into(),
            widget_id: "sales.chart".into(),
            origins: bundle.requested_origins.clone(),
            messages: bundle.requested_messages.clone(),
        };
        grant_active(&mut store, &req);
        assert!(matches!(
            decide_render(&store, "src", "env", "rend", &bundle),
            RenderDecision::AllowActive { .. }
        ));
    }

    #[test]
    fn imported_output_quarantined() {
        let store = TrustStore::default();
        let mut bundle = MimeBundle {
            text_plain: "x".into(),
            mime: vec![("text/html".into(), "<b>x</b>".into())],
            quarantined: false,
            widget_id: None,
            requested_origins: vec![],
            requested_messages: vec![],
        };
        quarantine_outputs(&mut bundle);
        assert!(matches!(
            decide_render(&store, "s", "e", "r", &bundle),
            RenderDecision::FallbackPlain { .. }
        ));
    }
}
