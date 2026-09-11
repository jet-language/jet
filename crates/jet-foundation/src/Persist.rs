//! D-PERSIST1 — `#Persist` value store at the shared runtime-heap boundary.
//!
//! Tier-0 (interpreter) and tier-1 (JIT) both consult this process-local store
//! when seeding module bindings. State is keyed by module path + binding name
//! plus a checked shape fingerprint. Compatible reloads keep the prior payload;
//! incompatible ones reinitialize and report the exact reset reason.
//!
//! Not stored inside a Cranelift resident module — survives hot-swap across
//! both tiers. Cleared on explicit restart.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{LazyLock, Mutex};

use crate::AST::{CtFloat, CtValue, Expr, Item, ProgramBundle, StrPart, Type};
use crate::MIR::MirRuntimeValue;
use crate::JSON::json_escape;


static SHARED: LazyLock<Mutex<PersistStore>> = LazyLock::new(|| Mutex::new(PersistStore::new()));

fn shared_store() -> &'static Mutex<PersistStore> {
    &SHARED
}

/// Stable identity for one `#Persist` binding.
///
/// The module path and binding name are the identity.  The type fingerprint is
/// deliberately kept on `PersistEntry`: changing a shape never silently
/// changes which resident value a reload addresses.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PersistIdentity {
    pub module: String,
    pub name: String,
}

impl PersistIdentity {
    pub fn new(module: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            name: name.into(),
        }
    }

    pub fn key(&self) -> String {
        format!("{}::{}", self.module, self.name)
    }

    pub fn is_valid(&self) -> bool {
        !self.module.is_empty()
            && !self.name.is_empty()
            && !self
                .module
                .chars()
                .chain(self.name.chars())
                .any(char::is_control)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistEntry {
    pub module: String,
    pub name: String,
    pub shape: String,
    pub payload: String,
}

impl PersistEntry {
    pub fn identity(&self) -> PersistIdentity {
        PersistIdentity::new(self.module.clone(), self.name.clone())
    }

    pub fn stable_key(&self) -> String {
        self.identity().key()
    }

    pub fn type_identity(&self) -> &str {
        &self.shape
    }

    pub fn is_valid(&self) -> bool {
        self.identity().is_valid() && !self.shape.is_empty() && !self.shape.chars().any(char::is_control)
    }
}

/// A typed reason a resident value cannot participate in a live replacement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistRejectReason {
    InvalidIdentity,
    ShapeChanged { from: String, to: String },
    MigrationUnavailable { from: String, to: String },
    ExplicitRestart,
    Transaction { detail: String },
}

impl PersistRejectReason {
    pub fn message(&self) -> String {
        match self {
            Self::InvalidIdentity => "persist identity is empty or contains control text".to_string(),
            Self::ShapeChanged { from, to } => {
                format!("persist shape changed: `{from}` → `{to}`")
            }
            Self::MigrationUnavailable { from, to } => {
                format!("no migration is available for persist shape `{from}` → `{to}`")
            }
            Self::ExplicitRestart => "replacement is disabled by the explicit restart policy".to_string(),
            Self::Transaction { detail } => detail.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistOutcome {
    Kept(PersistEntry),
    Migrated(PersistEntry),
    Reset { reason: String, entry: PersistEntry },
    Rejected {
        reason: PersistRejectReason,
        prior: PersistIdentity,
    },
}

impl PersistOutcome {
    pub fn disposition(&self) -> &'static str {
        match self {
            Self::Kept(_) => "kept",
            Self::Migrated(_) => "migrated",
            Self::Reset { .. } => "reset",
            Self::Rejected { .. } => "rejected",
        }
    }

    pub fn entry(&self) -> Option<&PersistEntry> {
        match self {
            Self::Kept(entry) | Self::Migrated(entry) => Some(entry),
            Self::Reset { entry, .. } => Some(entry),
            Self::Rejected { .. } => None,
        }
    }

    pub fn identity(&self) -> PersistIdentity {
        match self {
            Self::Kept(entry) | Self::Migrated(entry) => entry.identity(),
            Self::Reset { entry, .. } => entry.identity(),
            Self::Rejected { prior, .. } => prior.clone(),
        }
    }

    pub fn reason(&self) -> Option<String> {
        match self {
            Self::Kept(_) => None,
            Self::Migrated(_) => Some("published schema migration applied".to_string()),
            Self::Reset { reason, .. } => Some(reason.clone()),
            Self::Rejected { reason, .. } => Some(reason.message()),
        }
    }

    pub fn is_accepted(&self) -> bool {
        !matches!(self, Self::Rejected { .. })
    }
}

#[derive(Clone, Debug)]
struct PersistRuntimeValue {
    shape: String,
    value: MirRuntimeValue,
}

impl PartialEq for PersistRuntimeValue {
    fn eq(&self, other: &Self) -> bool {
        self.shape == other.shape && self.value == other.value
    }
}

impl Eq for PersistRuntimeValue {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PersistStore {
    entries: BTreeMap<String, PersistEntry>,
    /// Runtime values are owned by the shared typed heap boundary, never by a
    /// JIT arena.  The shape is repeated here so a stale value can never be
    /// read after a binding is replaced.
    runtime_values: BTreeMap<String, PersistRuntimeValue>,
}

impl PersistStore {
    fn key(module: &str, name: &str) -> String {
        PersistIdentity::new(module, name).key()
    }
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, entry: PersistEntry) {
        let key = entry.stable_key();
        let same_shape = self
            .entries
            .get(&key)
            .is_some_and(|prior| prior.shape == entry.shape);
        if !same_shape {
            self.runtime_values.remove(&key);
        }
        self.entries.insert(key, entry);
    }

    pub fn get(&self, module: &str, name: &str) -> Option<&PersistEntry> {
        self.entries.get(&Self::key(module, name))
    }

    /// Values are keyed by their stable module-path plus binding identity.
    /// `BTreeMap` keeps inspection and receipts deterministic without a second
    /// ordering pass.
    pub fn entries(&self) -> impl Iterator<Item = &PersistEntry> {
        self.entries.values()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn reset(
        &mut self,
        module: &str,
        name: &str,
        shape: &str,
        payload: &str,
        reason: impl Into<String>,
    ) -> PersistOutcome {
        let identity = PersistIdentity::new(module, name);
        if !identity.is_valid() || shape.is_empty() || shape.chars().any(char::is_control) {
            return PersistOutcome::Rejected {
                reason: PersistRejectReason::InvalidIdentity,
                prior: identity,
            };
        }
        let entry = PersistEntry {
            module: module.to_string(),
            name: name.to_string(),
            shape: shape.to_string(),
            payload: payload.to_string(),
        };
        self.runtime_values.remove(&entry.stable_key());
        self.put(entry.clone());
        PersistOutcome::Reset {
            reason: reason.into(),
            entry,
        }
    }

    /// Compute a migration outcome without changing this store.  The live
    /// transaction uses this method during preflight, so an incompatible
    /// resident value is rejected instead of being silently replaced.
    pub fn plan_migrate(
        &self,
        module: &str,
        name: &str,
        new_shape: &str,
        fresh_payload: &str,
    ) -> PersistOutcome {
        let identity = PersistIdentity::new(module, name);
        if !identity.is_valid()
            || new_shape.is_empty()
            || new_shape.chars().any(char::is_control)
        {
            return PersistOutcome::Rejected {
                reason: PersistRejectReason::InvalidIdentity,
                prior: identity,
            };
        }
        let key = identity.key();
        let fresh = || PersistEntry {
            module: module.to_string(),
            name: name.to_string(),
            shape: new_shape.to_string(),
            payload: fresh_payload.to_string(),
        };
        match self.entries.get(&key).cloned() {
            None => PersistOutcome::Kept(fresh()),
            Some(old)
                if old.shape == new_shape
                    && (payload_matches_shape(new_shape, &old.payload)
                        || self
                            .runtime_values
                            .get(&key)
                            .is_some_and(|runtime| {
                                runtime.shape == old.shape
                                    && runtime_value_matches_shape(new_shape, &runtime.value)
                            })) =>
            {
                PersistOutcome::Kept(old)
            }
            Some(old) if old.shape == new_shape => PersistOutcome::Rejected {
                reason: PersistRejectReason::Transaction {
                    detail: format!(
                        "persist payload does not match declared shape `{new_shape}`"
                    ),
                },
                prior: old.identity(),
            },
            Some(old)
                if payload_compatible(&old.payload, new_shape)
                    || self
                        .runtime_values
                        .get(&key)
                        .is_some_and(|runtime| {
                            runtime.shape == old.shape
                                && runtime_shape_compatible(&old.shape, new_shape)
                        }) =>
            {
                PersistOutcome::Migrated(PersistEntry {
                    module: module.to_string(),
                    name: name.to_string(),
                    shape: new_shape.to_string(),
                    payload: old.payload,
                })
            }
            Some(old) => PersistOutcome::Rejected {
                reason: PersistRejectReason::MigrationUnavailable {
                    from: old.shape.clone(),
                    to: new_shape.to_string(),
                },
                prior: old.identity(),
            },
        }
    }

    /// Apply one already-preflighted outcome to a candidate store.
    pub fn apply_outcome(&mut self, outcome: &PersistOutcome) -> Result<(), PersistRejectReason> {
        let Some(entry) = outcome.entry() else {
            if let PersistOutcome::Rejected { prior, .. } = outcome {
                self.runtime_values.remove(&prior.key());
            }
            return match outcome {
                PersistOutcome::Rejected { reason, .. } => Err(reason.clone()),
                _ => Ok(()),
            };
        };
        self.put(entry.clone());
        Ok(())
    }

    /// Sync one binding.  Unlike the old implementation, an incompatible
    /// shape is reported as a typed rejection and leaves the prior value in
    /// place.  Call `reset` only when the caller has an explicit reset reason.
    pub fn migrate(
        &mut self,
        module: &str,
        name: &str,
        new_shape: &str,
        fresh_payload: &str,
    ) -> PersistOutcome {
        let outcome = self.plan_migrate(module, name, new_shape, fresh_payload);
        let _ = self.apply_outcome(&outcome);
        outcome
    }
}
/// A migration marker is an explicit shape fact, not a guess from opaque
/// payload text.  Until sema supplies a richer published-schema fact, only
/// the existing `old-shape+` marker is accepted; unions and untagged payloads
/// stay rejected.
fn payload_compatible(payload: &str, new_shape: &str) -> bool {
    let Some(base_shape) = new_shape.strip_suffix('+') else {
        return false;
    };
    json_string_field(payload, "shape").is_some_and(|shape| shape == base_shape)
}

/// Check the wire payload before it crosses the typed persist boundary.
/// Integer-width declarations share the scalar `Int` carrier, while float
/// width is retained explicitly in the payload.
fn payload_matches_shape(expected: &str, payload: &str) -> bool {
    let Some(actual) = json_string_field(payload, "shape") else {
        return false;
    };
    let expected_carrier = expected.split_once('|').map_or(expected, |(carrier, _)| carrier);
    match (expected_carrier, actual.as_str()) {
        (expected, "Int" | "BigInt") if integer_shape(expected) => true,
        ("F32", "F32") => true,
        ("F32", "Float") => json_bool_field(payload, "f32") == Some(true),
        ("Float", "Float") => json_bool_field(payload, "f32") != Some(true),
        (expected, actual) => expected == actual,
    }
}

/// Runtime values are the canonical typed representation at the heap
/// boundary.  Reject engine-only carriers before they can be retained.
fn runtime_value_supported(value: &MirRuntimeValue) -> bool {
    match value {
        MirRuntimeValue::Moved | MirRuntimeValue::Closure(_) => false,
        MirRuntimeValue::Int(_)
        | MirRuntimeValue::BigInt(_)
        | MirRuntimeValue::Float { .. }
        | MirRuntimeValue::Bool(_)
        | MirRuntimeValue::Char(_)
        | MirRuntimeValue::String(_)
        | MirRuntimeValue::Bytes(_)
        | MirRuntimeValue::Unit => true,
        MirRuntimeValue::List(values) => values.iter().all(runtime_value_supported),
        MirRuntimeValue::Map(values) => values
            .iter()
            .all(|(key, value)| runtime_key_supported(key) && runtime_value_supported(value)),
        MirRuntimeValue::Struct { type_name, fields } => {
            !type_name.is_empty()
                && !type_name.chars().any(char::is_control)
                && fields.iter().all(|(name, value)| {
                    !name.is_empty()
                        && !name.chars().any(char::is_control)
                        && runtime_value_supported(value)
                })
        }
        MirRuntimeValue::Enum {
            type_name,
            variant,
            args,
        } => {
            !type_name.is_empty()
                && !type_name.chars().any(char::is_control)
                && !variant.is_empty()
                && !variant.chars().any(char::is_control)
                && args.iter().all(|(name, value)| {
                    name.as_ref()
                        .is_none_or(|name| !name.is_empty() && !name.chars().any(char::is_control))
                        && runtime_value_supported(value)
                })
        }
        MirRuntimeValue::Present(value) | MirRuntimeValue::FailedTold(value) => {
            runtime_value_supported(value)
        }
        MirRuntimeValue::Absent { element } => runtime_type_supported(element),
    }
}

fn runtime_key_supported(key: &crate::MIR::MirConstKey) -> bool {
    match key {
        crate::MIR::MirConstKey::Int(_)
        | crate::MIR::MirConstKey::String(_)
        | crate::MIR::MirConstKey::Bool(_)
        | crate::MIR::MirConstKey::Char(_)
        | crate::MIR::MirConstKey::Enum { .. } => true,
        crate::MIR::MirConstKey::Tuple(fields)
        | crate::MIR::MirConstKey::Struct { fields, .. } => fields
            .iter()
            .all(|(_, key)| runtime_key_supported(key)),
    }
}

fn runtime_type_supported(ty: &crate::MIR::MirType) -> bool {
    use crate::MIR::MirTypeKind;
    match &ty.kind {
        MirTypeKind::Fn(_) | MirTypeKind::SendFn { .. } | MirTypeKind::TraitObject(_) => false,
        MirTypeKind::List(inner)
        | MirTypeKind::Shared(inner)
        | MirTypeKind::Option(inner)
        | MirTypeKind::FixedList { elem: inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => runtime_type_supported(inner),
        MirTypeKind::Map { key, value } => {
            runtime_type_supported(key) && runtime_type_supported(value)
        }
        MirTypeKind::Result { ok, err } => {
            runtime_type_supported(ok) && runtime_type_supported(err)
        }
        MirTypeKind::Apply { args, .. } => args.iter().all(runtime_type_supported),
        MirTypeKind::Tuple(fields) => fields
            .iter()
            .all(|(_, ty)| runtime_type_supported(ty)),
        MirTypeKind::Union(members) => members.iter().all(runtime_type_supported),
        MirTypeKind::Int
        | MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::String
        | MirTypeKind::Char
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float32
        | MirTypeKind::Measure(_) => true,
    }
}

/// Match only the stable top-level carrier here.  The JIT descriptor checks
/// nested fields while crossing the heap boundary; this store check prevents
/// a value of the wrong broad kind from being installed.
fn runtime_value_matches_shape(shape: &str, value: &MirRuntimeValue) -> bool {
    if !runtime_value_supported(value) {
        return false;
    }
    let carrier = shape.split_once('|').map_or(shape, |(carrier, _)| carrier);
    if carrier.starts_with("id:") {
        return true;
    }
    match value {
        MirRuntimeValue::Int(_) | MirRuntimeValue::BigInt(_) => {
            carrier == "Int" || integer_shape(carrier)
        }
        MirRuntimeValue::Float { f32, .. } => {
            (carrier == "F32" && *f32)
                || (carrier == "Float" && !*f32)
                || (carrier == "Float32" && *f32)
        }
        MirRuntimeValue::Bool(_) => carrier == "Bool",
        MirRuntimeValue::Char(_) => carrier == "Char",
        MirRuntimeValue::String(_) => carrier == "String",
        MirRuntimeValue::Bytes(_) => carrier == "Bytes" || carrier == "[U8]",
        MirRuntimeValue::List(_) => {
            (carrier.starts_with('[') && !carrier.contains(':'))
                || carrier.starts_with("List<")
                || carrier.starts_with("FixedList<")
        }
        MirRuntimeValue::Map(_) => {
            (carrier.starts_with('[') && carrier.contains(':'))
                || carrier.starts_with("Map<")
        }
        MirRuntimeValue::Struct { type_name, .. } => {
            carrier == type_name
                || carrier.starts_with("Tuple(")
                || carrier.starts_with("Struct")
        }
        MirRuntimeValue::Enum { type_name, .. } => {
            carrier == type_name
                || carrier.starts_with("Enum")
                || carrier.starts_with("Union")
        }
        MirRuntimeValue::Present(_) | MirRuntimeValue::FailedTold(_) | MirRuntimeValue::Absent { .. } => {
            carrier.starts_with('?')
                || carrier.starts_with("Option<")
                || carrier.starts_with("Result<")
                || carrier.contains(" ! ")
        }
        MirRuntimeValue::Unit => carrier == "Unit" || carrier == "()",
        MirRuntimeValue::Moved | MirRuntimeValue::Closure(_) => false,
    }
}

fn runtime_shape_compatible(old_shape: &str, new_shape: &str) -> bool {
    new_shape
        .strip_suffix('+')
        .is_some_and(|base_shape| base_shape == old_shape)
}

fn ct_to_runtime_key(key: &crate::AST::CtKey) -> Option<crate::MIR::MirConstKey> {
    match key {
        crate::AST::CtKey::Int(value) => Some(crate::MIR::MirConstKey::Int(*value)),
        crate::AST::CtKey::Str(value) => {
            Some(crate::MIR::MirConstKey::String(value.clone()))
        }
        crate::AST::CtKey::Bool(value) => Some(crate::MIR::MirConstKey::Bool(*value)),
        crate::AST::CtKey::Char(value) => Some(crate::MIR::MirConstKey::Char(*value)),
        crate::AST::CtKey::Tuple(fields) => fields
            .iter()
            .map(|(name, key)| Some((name.clone(), ct_to_runtime_key(key)?)))
            .collect::<Option<Vec<_>>>()
            .map(crate::MIR::MirConstKey::Tuple),
        crate::AST::CtKey::Struct { type_name, fields } => fields
            .iter()
            .map(|(name, key)| Some((name.clone(), ct_to_runtime_key(key)?)))
            .collect::<Option<Vec<_>>>()
            .map(|fields| crate::MIR::MirConstKey::Struct {
                type_name: type_name.clone(),
                fields,
            }),
        crate::AST::CtKey::Enum { type_name, variant } => {
            Some(crate::MIR::MirConstKey::Enum {
                type_name: type_name.clone(),
                variant: variant.clone(),
            })
        }
    }
}

fn ct_to_runtime_value(value: &CtValue) -> Option<MirRuntimeValue> {
    match value {
        CtValue::Int(value) => Some(MirRuntimeValue::Int(*value)),
        CtValue::Float(CtFloat::F32(value)) => Some(MirRuntimeValue::Float {
            value: f64::from(*value),
            f32: true,
        }),
        CtValue::Float(CtFloat::F64(value)) => Some(MirRuntimeValue::Float {
            value: *value,
            f32: false,
        }),
        CtValue::Bool(value) => Some(MirRuntimeValue::Bool(*value)),
        CtValue::Char(value) => Some(MirRuntimeValue::Char(*value)),
        CtValue::Str(value) => Some(MirRuntimeValue::String(value.clone())),
        CtValue::BigInt(value) => Some(MirRuntimeValue::BigInt(value.to_string_rep())),
        CtValue::Bytes(values) => Some(MirRuntimeValue::Bytes(values.clone())),
        CtValue::List(values) => values
            .iter()
            .map(ct_to_runtime_value)
            .collect::<Option<Vec<_>>>()
            .map(MirRuntimeValue::List),
        CtValue::Map(values) => values
            .iter()
            .map(|(key, value)| {
                Some((ct_to_runtime_key(key)?, ct_to_runtime_value(value)?))
            })
            .collect::<Option<Vec<_>>>()
            .map(MirRuntimeValue::Map),
        CtValue::Struct { type_name, fields } => Some(MirRuntimeValue::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| Some((name.clone(), ct_to_runtime_value(value)?)))
                .collect::<Option<Vec<_>>>()?,
        }),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } => Some(MirRuntimeValue::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args
                .iter()
                .map(|(name, value)| Some((name.clone(), ct_to_runtime_value(value)?)))
                .collect::<Option<Vec<_>>>()?,
        }),
        CtValue::Present(value) => Some(MirRuntimeValue::Present(Box::new(
            ct_to_runtime_value(value)?,
        ))),
        CtValue::Failed(crate::AST::CtReport::Clean(ty)) => Some(MirRuntimeValue::Absent {
            element: crate::MIR::MirType::from_kind(crate::MIR::MirTypeKind::Apply {
                name: crate::MIR::MirNominalRef::from_name(ty.name()),
                args: Vec::new(),
            }),
        }),
        CtValue::Failed(crate::AST::CtReport::Told(value)) => Some(
            MirRuntimeValue::FailedTold(Box::new(ct_to_runtime_value(value)?)),
        ),
        CtValue::Unit => Some(MirRuntimeValue::Unit),
        CtValue::Closure(_) => None,
    }
}

fn integer_shape(shape: &str) -> bool {
    if shape == "Int" || shape.len() < 2 {
        return shape == "Int";
    }
    let (prefix, bits) = shape.split_at(1);
    matches!(prefix, "I" | "U") && bits.parse::<u8>().is_ok()
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
    /// A rejected row aborts the complete preparation transaction.  The
    /// shared store remains unchanged and callers must keep the last-good
    /// compiled program active.
    pub error: Option<String>,
}

/// Clone of the process-local shared store (session snapshots, tests).
pub fn shared_clone() -> PersistStore {
    shared_store()
        .lock()
        .expect("persist store lock poisoned")
        .clone()
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
        .and_then(|entry| {
            payload_matches_shape(&entry.shape, &entry.payload)
                .then(|| decode_payload(&entry.payload))
                .flatten()
        })
}

/// Update a live value by the store's canonical `module::binding` identity.
/// The next `prepare_bundle` observes this payload and carries it across a
/// compatible hot reload.
pub fn shared_write_key(key: &str, value: &CtValue) -> bool {
    let mut store = shared_store().lock().expect("persist store lock poisoned");
    let Some(entry) = store.entries.get(key) else {
        return false;
    };
    let shape = entry.shape.clone();
    let payload = encode_payload(value);
    if !payload_matches_shape(&shape, &payload) {
        return false;
    }
    let runtime_value = ct_to_runtime_value(value);
    if let Some(runtime_value) = runtime_value.as_ref() {
        if !runtime_value_matches_shape(&shape, runtime_value) {
            return false;
        }
    }
    if let Some(entry) = store.entries.get_mut(key) {
        entry.payload = payload;
    }
    match runtime_value {
        Some(value) => {
            store.runtime_values.insert(
                key.to_string(),
                PersistRuntimeValue {
                    shape,
                    value,
                },
            );
        }
        None => {
            store.runtime_values.remove(key);
        }
    }
    true
}

/// One checked resident value produced by a canonical published-schema
/// migration.  The whole batch is committed under the shared store lock;
/// callers must not update individual entries with `shared_write_runtime_key`
/// while a migration transaction is in flight.
#[derive(Clone, Debug, PartialEq)]
pub struct PersistRuntimeMigration {
    pub key: String,
    pub shape: String,
    pub value: MirRuntimeValue,
}

/// Atomically install a batch of already-evaluated resident migrations.
///
/// Validation is completed for every row before the first entry is changed.
/// A missing binding, duplicate key, invalid shape, or unsupported runtime
/// value therefore leaves the complete prior store untouched.
pub fn shared_commit_runtime_migrations(
    migrations: &[PersistRuntimeMigration],
) -> Result<(), String> {
    if migrations.is_empty() {
        return Ok(());
    }
    let mut store = shared_store().lock().expect("persist store lock poisoned");
    let mut seen = BTreeSet::new();
    let mut prepared = Vec::with_capacity(migrations.len());
    for migration in migrations {
        if !seen.insert(migration.key.clone()) {
            return Err(format!(
                "duplicate persist migration key `{}`",
                migration.key
            ));
        }
        if migration.shape.is_empty() || migration.shape.chars().any(char::is_control) {
            return Err(format!(
                "persist migration `{}` has an invalid target shape",
                migration.key
            ));
        }
        let Some(_entry) = store.entries.get(&migration.key) else {
            return Err(format!(
                "persist migration `{}` has no resident entry",
                migration.key
            ));
        };
        if !runtime_value_matches_shape(&migration.shape, &migration.value) {
            return Err(format!(
                "persist migration `{}` produced a value incompatible with `{}`",
                migration.key, migration.shape
            ));
        }
        prepared.push((
            migration.key.clone(),
            migration.shape.clone(),
            encode_runtime_payload(&migration.value),
            migration.value.clone(),
        ));
    }
    // No store mutation occurs before every row above has passed validation.
    for (key, shape, payload, value) in prepared {
        {
            let Some(entry) = store.entries.get_mut(&key) else {
                return Err(format!("persist migration `{key}` lost its entry"));
            };
            entry.shape = shape.clone();
            entry.payload = payload;
        }
        store
            .runtime_values
            .insert(key, PersistRuntimeValue { shape, value });
    }
    Ok(())
}

/// Encode a runtime value for the durable payload slot.  Runtime values remain
/// authoritative for resident execution; the payload is retained for scalar
/// inspection and for the existing typed-store boundary.
fn encode_runtime_payload(value: &MirRuntimeValue) -> String {
    match value {
        MirRuntimeValue::Int(n) => format!(r#"{{"shape":"Int","value":{n}}}"#),
        MirRuntimeValue::BigInt(n) => format!(
            r#"{{"shape":"BigInt","value":"{}"}}"#,
            json_escape(n)
        ),
        MirRuntimeValue::Float { value, f32 } => format!(
            r#"{{"shape":"{}","value":{},"f32":{f32}}}"#,
            if *f32 { "F32" } else { "Float" },
            value
        ),
        MirRuntimeValue::Bool(value) => format!(r#"{{"shape":"Bool","value":{value}}}"#),
        MirRuntimeValue::Char(value) => format!(
            r#"{{"shape":"Char","value":"{}"}}"#,
            json_escape(&value.to_string())
        ),
        MirRuntimeValue::String(value) => format!(
            r#"{{"shape":"String","value":"{}"}}"#,
            json_escape(value)
        ),
        _ => format!(
            r#"{{"shape":"Opaque","debug":"{}"}}"#,
            json_escape(&format!("{value:?}"))
        ),
    }
}

/// Read a persisted value for a MIR execution tier.  Values are cloned out of
/// the shared typed store, so no execution tier can retain a heap-owned handle.
pub fn shared_read_runtime_key(key: &str) -> Option<MirRuntimeValue> {
    let store = shared_store().lock().expect("persist store lock poisoned");
    let entry = store.entries.get(key)?;
    let runtime = store.runtime_values.get(key)?;
    (runtime.shape == entry.shape
        && runtime_value_matches_shape(&entry.shape, &runtime.value))
        .then(|| runtime.value.clone())
}

/// Update a persisted value from a MIR execution tier.
pub fn shared_write_runtime_key(key: &str, value: &MirRuntimeValue) -> bool {
    let mut store = shared_store().lock().expect("persist store lock poisoned");
    let Some(entry) = store.entries.get(key) else {
        return false;
    };
    if !runtime_value_matches_shape(&entry.shape, value) {
        return false;
    }
    let shape = entry.shape.clone();
    store.runtime_values.insert(
        key.to_string(),
        PersistRuntimeValue {
            shape,
            value: value.clone(),
        },
    );
    true
}

/// Remove one persisted binding by its exact canonical `module::binding` key.
///
/// Adapters use this only when sema says the key is `Fresh`; no prefix or
/// binding-name fallback is allowed.
pub fn shared_remove_key(key: &str) -> bool {
    let mut store = shared_store().lock().expect("persist store lock poisoned");
    let removed_entry = store.entries.remove(key).is_some();
    let removed_runtime = store.runtime_values.remove(key).is_some();
    removed_entry || removed_runtime
}

/// Sync every `#Persist` binding in `bundle` into the shared store and return
/// the values that should seed this generation's globals / JIT constants.
pub fn prepare_bundle(bundle: &ProgramBundle) -> PersistPrep {
    let mut store = shared_store().lock().expect("persist store lock poisoned");
    let mut candidate = store.clone();
    let mut prep = prepare_bundle_into(bundle, &mut candidate);
    if let Some(error) = prep.error.clone() {
        prep.messages
            .push(format!("[persist] transaction rejected: {error}"));
    } else {
        *store = candidate;
    }
    prep
}

fn prepare_bundle_into(bundle: &ProgramBundle, store: &mut PersistStore) -> PersistPrep {
    let mut prep = PersistPrep::default();
    let mut rejection = None;
    for module in &bundle.modules {
        for item in &module.items {
            let Item::Const(c) = item else {
                continue;
            };
            if !c.is_persist {
                continue;
            };
            let Some(fresh) = const_runtime_value(c) else {
                continue;
            };
            let shape = shape_fingerprint(c.ty.as_ref(), &fresh);
            let fresh_payload = encode_payload(&fresh);
            let key = format!("{}::{}", module.alias, c.name);
            let outcome = store.plan_migrate(&module.alias, &c.name, &shape, &fresh_payload);
            let rejected = matches!(&outcome, PersistOutcome::Rejected { .. });
            if rejected {
                let reason = outcome
                    .reason()
                    .unwrap_or_else(|| "replacement refused".to_string());
                rejection.get_or_insert_with(|| format!("{}: {reason}", outcome.identity().key()));
            } else {
                let _ = store.apply_outcome(&outcome);
            }
            let entry = outcome
                .entry()
                .cloned()
                .or_else(|| store.entries.get(&key).cloned())
                .unwrap_or_else(|| PersistEntry {
                    module: module.alias.clone(),
                    name: c.name.clone(),
                    shape: shape.clone(),
                    payload: fresh_payload.clone(),
                });
            let note = match outcome.disposition() {
                "migrated" => Some(format!(
                    "[persist] migrated `{}::{}` to new shape",
                    module.alias, c.name
                )),
                "rejected" => Some(format!(
                    "[persist] rejected `{}`: {}",
                    outcome.identity().key(),
                    outcome.reason().unwrap_or_else(|| "replacement refused".to_string())
                )),
                _ => outcome.reason().map(|reason| format!("[persist] {reason}")),
            };
            if let Some(msg) = note {
                prep.messages.push(msg);
            }
            let same_shape_kept = matches!(
                &outcome,
                PersistOutcome::Kept(entry) if entry.shape == shape
            );
            let value = if same_shape_kept || rejected {
                decode_payload(&entry.payload).unwrap_or_else(|| fresh.clone())
            } else {
                fresh.clone()
            };
            if !rejected && (!same_shape_kept || !store.runtime_values.contains_key(&key)) {
                if let Some(runtime_value) = ct_to_runtime_value(&value) {
                    if runtime_value_matches_shape(&entry.shape, &runtime_value) {
                        store.runtime_values.insert(
                            key.clone(),
                            PersistRuntimeValue {
                                shape: entry.shape.clone(),
                                value: runtime_value,
                            },
                        );
                    }
                }
            }
            prep.by_name.insert(c.name.clone(), value.clone());
            prep.by_key.insert(key, value);
        }
    }
    prep.error = rejection;
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
    ty.map(Type::identity_key)
        .unwrap_or_else(|| value.jet_type().identity_key())
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
        CtValue::Float(f) => {
            let (shape, is_f32) = match f {
                CtFloat::F32(_) => ("F32", true),
                CtFloat::F64(_) => ("Float", false),
            };
            format!(
                r#"{{"shape":"{shape}","value":{},"f32":{is_f32}}}"#,
                f.render()
            )
        }
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
        "F32" | "Float" => {
            let raw = json_raw_field(payload, "value")?;
            let value = raw.parse::<f64>().ok()?;
            let f32 = json_bool_field(payload, "f32").unwrap_or(shape == "F32");
            Some(CtValue::Float(if f32 {
                CtFloat::f32(value as f32)
            } else {
                CtFloat::f64(value)
            }))
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
    fn migrate_keeps_compatible_and_rejects_incompatible() {
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
            PersistOutcome::Rejected {
                reason: PersistRejectReason::MigrationUnavailable { from, to },
                prior,
            } if from == "Int"
                && to == "String"
                && prior.key() == "app::counter"
        ));
        assert_eq!(
            store.get("app", "counter").map(PersistEntry::type_identity),
            Some("Int")
        );
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
