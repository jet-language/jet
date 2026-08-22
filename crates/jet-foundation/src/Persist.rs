//! D-PERSIST1 — `#Persist` value store at the shared runtime-heap boundary.
//!
//! Tier-0 (interpreter) and tier-1 (JIT) both consult this process-local store
//! when seeding module bindings. State is keyed by module path + binding name
//! plus a checked shape fingerprint. Compatible reloads keep the prior payload;
//! incompatible ones reinitialize and report the exact reset reason.
//!
//! Not stored inside a Cranelift resident module — survives hot-swap across
//! both tiers. Cleared on explicit restart.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::AST::{CtFloat, CtValue, Expr, Item, ProgramBundle, StrPart, Type};
use crate::JSON::json_escape;

static SHARED: OnceLock<Mutex<PersistStore>> = OnceLock::new();

fn shared_store() -> &'static Mutex<PersistStore> {
    SHARED.get_or_init(|| Mutex::new(PersistStore::new()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistEntry {
    pub module: String,
    pub name: String,
    pub shape: String,
    pub payload: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistOutcome {
    Kept(PersistEntry),
    Migrated(PersistEntry),
    Reset { reason: String, entry: PersistEntry },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PersistStore {
    entries: HashMap<String, PersistEntry>,
}

impl PersistStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(module: &str, name: &str) -> String {
        format!("{module}::{name}")
    }

    pub fn put(&mut self, entry: PersistEntry) {
        self.entries
            .insert(Self::key(&entry.module, &entry.name), entry);
    }

    pub fn get(&self, module: &str, name: &str) -> Option<&PersistEntry> {
        self.entries.get(&Self::key(module, name))
    }

    /// Compatible shape keeps payload; same name + new shape tries migration
    /// via payload re-decode (string equality of shape fingerprint). Failure
    /// reinitializes with `fresh_payload` and records the reason.
    pub fn migrate(
        &mut self,
        module: &str,
        name: &str,
        new_shape: &str,
        fresh_payload: &str,
    ) -> PersistOutcome {
        let key = Self::key(module, name);
        match self.entries.get(&key).cloned() {
            None => {
                let entry = PersistEntry {
                    module: module.to_string(),
                    name: name.to_string(),
                    shape: new_shape.to_string(),
                    payload: fresh_payload.to_string(),
                };
                self.entries.insert(key, entry.clone());
                PersistOutcome::Kept(entry)
            }
            Some(old) if old.shape == new_shape => PersistOutcome::Kept(old),
            Some(old) => {
                if payload_compatible(&old.payload, new_shape) {
                    let entry = PersistEntry {
                        module: module.to_string(),
                        name: name.to_string(),
                        shape: new_shape.to_string(),
                        payload: old.payload,
                    };
                    self.entries.insert(key, entry.clone());
                    PersistOutcome::Migrated(entry)
                } else {
                    let entry = PersistEntry {
                        module: module.to_string(),
                        name: name.to_string(),
                        shape: new_shape.to_string(),
                        payload: fresh_payload.to_string(),
                    };
                    self.entries.insert(key, entry.clone());
                    PersistOutcome::Reset {
                        reason: format!(
                            "`{module}::{name}` shape `{}` → `{new_shape}`; reinitialized",
                            old.shape
                        ),
                        entry,
                    }
                }
            }
        }
    }
}

fn payload_compatible(payload: &str, new_shape: &str) -> bool {
    if payload.is_empty() {
        return false;
    }
    if let Some(base) = new_shape.strip_suffix('+') {
        return payload.contains(base) || !new_shape.contains('|');
    }
    if new_shape.contains('|') {
        return true;
    }
    let tag = format!("\"shape\":\"{new_shape}\"");
    payload.contains(&tag) || !payload.contains("\"shape\":")
}

/// Result of syncing a bundle's `#Persist` bindings into the shared store.
#[derive(Clone, Debug, Default)]
pub struct PersistPrep {
    /// Binding name → restored (or fresh) value for the entry module and
    /// cross-module bare names.
    pub by_name: HashMap<String, CtValue>,
    /// `module::name` → value.
    pub by_key: HashMap<String, CtValue>,
    /// Human-readable reset / migration notes (`[persist] …`).
    pub messages: Vec<String>,
}

/// Clone of the process-local shared store (session snapshots, tests).
pub fn shared_clone() -> PersistStore {
    shared_store().lock().expect("persist store lock poisoned").clone()
}

/// Replace the process-local shared store.
pub fn shared_replace(store: PersistStore) {
    *shared_store().lock().expect("persist store lock poisoned") = store;
}

/// Drop all persisted bindings (D-HOTSWAP1 restart / test isolation).
pub fn shared_clear() {
    *shared_store().lock().expect("persist store lock poisoned") = PersistStore::new();
}

/// Read a live value by the store's canonical `module::binding` identity.
/// Execution tiers use this as a marshalling seam; shape checking and reload
/// policy remain in `prepare_bundle`.
pub fn shared_read_key(key: &str) -> Option<CtValue> {
    shared_store()
        .lock()
        .expect("persist store lock poisoned")
        .entries
        .get(key)
        .and_then(|entry| decode_payload(&entry.payload))
}

/// Update a live value by the store's canonical `module::binding` identity.
/// The next `prepare_bundle` observes this payload and carries it across a
/// compatible hot reload.
pub fn shared_write_key(key: &str, value: &CtValue) -> bool {
    let mut store = shared_store().lock().expect("persist store lock poisoned");
    let Some(entry) = store.entries.get_mut(key) else {
        return false;
    };
    entry.payload = encode_payload(value);
    true
}

/// Sync every `#Persist` binding in `bundle` into the shared store and return
/// the values that should seed this generation's globals / JIT constants.
pub fn prepare_bundle(bundle: &ProgramBundle) -> PersistPrep {
    let mut prep = PersistPrep::default();
    let mut store = shared_store().lock().expect("persist store lock poisoned");
    for module in &bundle.modules {
        for item in &module.items {
            let Item::Const(c) = item else {
                continue;
            };
            if !c.is_persist {
                continue;
            }
            let Some(fresh) = const_runtime_value(c) else {
                continue;
            };
            let shape = shape_fingerprint(c.ty.as_ref(), &fresh);
            let fresh_payload = encode_payload(&fresh);
            let outcome = store.migrate(&module.alias, &c.name, &shape, &fresh_payload);
            let (entry, note) = match outcome {
                PersistOutcome::Kept(e) => (e, None),
                PersistOutcome::Migrated(e) => (
                    e,
                    Some(format!(
                        "[persist] migrated `{}::{}` to new shape",
                        module.alias, c.name
                    )),
                ),
                PersistOutcome::Reset { reason, entry } => {
                    (entry, Some(format!("[persist] {reason}")))
                }
            };
            if let Some(msg) = note {
                prep.messages.push(msg);
            }
            let value = decode_payload(&entry.payload).unwrap_or(fresh);
            prep.by_name.insert(c.name.clone(), value.clone());
            prep.by_key
                .insert(format!("{}::{}", module.alias, c.name), value);
        }
    }
    prep
}

fn const_runtime_value(c: &crate::AST::ConstDef) -> Option<CtValue> {
    if let Some(v) = &c.ct {
        return Some(v.clone());
    }
    match &c.value {
        Expr::Int(v, _, _, _) => Some(CtValue::Int(*v)),
        Expr::Bool(v, _) => Some(CtValue::Bool(*v)),
        Expr::Str(parts, _) => match parts.as_slice() {
            [StrPart::Lit(s)] => Some(CtValue::Str(s.clone())),
            _ => None,
        },
        Expr::Float(v, _, is_f32, _) => Some(CtValue::Float(if *is_f32 {
            CtFloat::f32(*v as f32)
        } else {
            CtFloat::f64(*v)
        })),
        Expr::Char(ch, _) => Some(CtValue::Char(*ch)),
        _ => None,
    }
}

fn shape_fingerprint(ty: Option<&Type>, value: &CtValue) -> String {
    if let Some(t) = ty {
        return match t {
            Type::Int => "Int".into(),
            Type::Bool => "Bool".into(),
            Type::String => "String".into(),
            Type::Float => "Float".into(),
            Type::Char => "Char".into(),
            other => format!("{other:?}"),
        };
    }
    match value.jet_type() {
        Type::Int => "Int".into(),
        Type::Bool => "Bool".into(),
        Type::String => "String".into(),
        Type::Float => "Float".into(),
        Type::Char => "Char".into(),
        other => format!("{other:?}"),
    }
}

fn encode_payload(value: &CtValue) -> String {
    match value {
        CtValue::Int(n) => format!(r#"{{"shape":"Int","value":{n}}}"#),
        CtValue::Bool(b) => format!(r#"{{"shape":"Bool","value":{b}}}"#),
        CtValue::Str(s) => {
            format!(r#"{{"shape":"String","value":"{}"}}"#, json_escape(s))
        }
        CtValue::Char(c) => {
            format!(
                r#"{{"shape":"Char","value":"{}"}}"#,
                json_escape(&c.to_string())
            )
        }
        CtValue::Float(f) => format!(r#"{{"shape":"Float","value":{}}}"#, f.render()),
        other => format!(
            r#"{{"shape":"Opaque","debug":"{}"}}"#,
            json_escape(&format!("{other:?}"))
        ),
    }
}

fn decode_payload(payload: &str) -> Option<CtValue> {
    let shape = json_string_field(payload, "shape")?;
    match shape.as_str() {
        "Int" => json_i64_field(payload, "value").map(CtValue::Int),
        "Bool" => json_bool_field(payload, "value").map(CtValue::Bool),
        "String" => json_string_field(payload, "value").map(CtValue::Str),
        "Char" => {
            let s = json_string_field(payload, "value")?;
            let mut chars = s.chars();
            let ch = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            Some(CtValue::Char(ch))
        }
        "Float" => {
            let raw = json_raw_field(payload, "value")?;
            raw.parse::<f64>()
                .ok()
                .map(|f| CtValue::Float(CtFloat::f64(f)))
        }
        _ => None,
    }
}

fn unescape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(value) = char::from_u32(code) {
                            out.push(value);
                        }
                    }
                }
                Some(other) => out.push(other),
                None => break,
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn json_string_field(payload: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\":\"");
    let start = payload.find(&marker)? + marker.len();
    let rest = &payload[start..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let next = chars.next()?;
            out.push('\\');
            out.push(next);
            continue;
        }
        if ch == '"' {
            return Some(unescape_json_string(&out));
        }
        out.push(ch);
    }
    None
}

fn json_raw_field<'a>(payload: &'a str, field: &str) -> Option<&'a str> {
    let marker = format!("\"{field}\":");
    let start = payload.find(&marker)? + marker.len();
    let rest = payload[start..].trim_start();
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    Some(rest[..end].trim())
}

fn json_i64_field(payload: &str, field: &str) -> Option<i64> {
    json_raw_field(payload, field)?.parse().ok()
}

fn json_bool_field(payload: &str, field: &str) -> Option<bool> {
    match json_raw_field(payload, field)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_keeps_compatible_and_resets_incompatible() {
        let mut store = PersistStore::new();
        store.put(PersistEntry {
            module: "app".into(),
            name: "counter".into(),
            shape: "Int".into(),
            payload: r#"{"shape":"Int","value":7}"#.into(),
        });
        assert!(matches!(
            store.migrate("app", "counter", "Int", r#"{"shape":"Int","value":0}"#),
            PersistOutcome::Kept(e) if e.payload.contains('7')
        ));
        assert!(matches!(
            store.migrate("app", "counter", "String", r#"{"shape":"String","value":""}"#),
            PersistOutcome::Reset { reason, entry }
                if reason.contains("reinitialized") && entry.payload.contains("String")
        ));
    }

    #[test]
    fn encode_decode_roundtrip_scalars() {
        for v in [
            CtValue::Int(42),
            CtValue::Bool(true),
            CtValue::Str("hi\"there".into()),
            CtValue::Char('J'),
        ] {
            let payload = encode_payload(&v);
            assert_eq!(decode_payload(&payload), Some(v));
        }
    }
}
