//! Canonical published-schema migration facts and runtime step application.
//!
//! Sema/codegen own the plan. Execution tiers only provide callbacks for the
//! checked synthetic default/converter functions; field movement and receipts
//! stay here so the interpreter and resident JIT cannot drift.

use crate::MIR::MirRuntimeValue;
use std::collections::BTreeSet;
use std::fmt;

/// A checked migration chain for one concrete `#PublishedSchema` record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaMigrationPlan {
    pub type_name: String,
    /// Historical wire-key sets, oldest first. The current shape is the
    /// implicit shape after the final step.
    pub historical_shapes: Vec<Vec<String>>,
    pub steps: Vec<SchemaMigrationStep>,
}

impl SchemaMigrationPlan {
    pub fn new(
        type_name: impl Into<String>,
        historical_shapes: Vec<Vec<String>>,
        steps: Vec<SchemaMigrationStep>,
    ) -> Self {
        Self {
            type_name: type_name.into(),
            historical_shapes,
            steps,
        }
    }

    /// Number of migration steps, with the current shape after the final step.
    pub fn current_shape(&self) -> usize {
        self.steps.len()
    }

    /// Apply the chain to a runtime record. The callbacks execute sema-created
    /// zero-argument defaults and one-argument converters; all key movement,
    /// shape selection, and receipt construction is shared here.
    pub fn apply_wire_step<F, G>(
        &self,
        value: &MirRuntimeValue,
        mut default: F,
        mut convert: G,
    ) -> Result<SchemaMigrationResult, SchemaMigrationError>
    where
        F: FnMut(&str, &str) -> Result<MirRuntimeValue, String>,
        G: FnMut(&str, &str, &str, &MirRuntimeValue) -> Result<MirRuntimeValue, String>,
    {
        let MirRuntimeValue::Struct { type_name, fields } = value else {
            return Err(SchemaMigrationError::NotRecord);
        };
        if type_name != &self.type_name {
            return Err(SchemaMigrationError::TypeMismatch {
                expected: self.type_name.clone(),
                actual: type_name.clone(),
            });
        }
        let wire_keys = fields
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<BTreeSet<_>>();
        let current = self.current_shape_keys();
        let current_shape = self.current_shape_name();
        if wire_keys == current {
            return Ok(SchemaMigrationResult {
                value: value.clone(),
                receipt: SchemaMigrationReceipt::kept(&self.type_name, &current_shape),
            });
        }
        let Some(start) = self
            .historical_shapes
            .iter()
            .position(|shape| shape.iter().cloned().collect::<BTreeSet<_>>() == wire_keys)
        else {
            return Err(SchemaMigrationError::UnknownShape {
                type_name: self.type_name.clone(),
                keys: wire_keys.into_iter().collect(),
            });
        };
        let mut fields = fields
            .iter()
            .cloned()
            .collect::<std::collections::BTreeMap<_, _>>();
        let from_shape = self
            .steps
            .get(start)
            .map(|step| step.from_shape.clone())
            .or_else(|| self.historical_shape_name(start))
            .unwrap_or_else(|| current_shape.clone());
        let mut receipt =
            SchemaMigrationReceipt::migrated(&self.type_name, &from_shape, &current_shape);
        for (index, step) in self.steps.iter().enumerate().skip(start) {
            for op in &step.ops {
                match op {
                    SchemaMigrationOp::Rename { from_key, to_key } => {
                        let Some(old) = fields.remove(from_key) else {
                            return Err(SchemaMigrationError::MissingField {
                                step: step.name(),
                                field: from_key.clone(),
                            });
                        };
                        if fields.contains_key(to_key) {
                            return Err(SchemaMigrationError::DuplicateField {
                                step: step.name(),
                                field: to_key.clone(),
                            });
                        }
                        fields.insert(to_key.clone(), old);
                        receipt.swapped.push(format!("{from_key}->{to_key}"));
                        receipt.migrated.push(to_key.clone());
                    }
                    SchemaMigrationOp::Remove { key } => {
                        if fields.remove(key).is_none() {
                            return Err(SchemaMigrationError::MissingField {
                                step: step.name(),
                                field: key.clone(),
                            });
                        }
                        receipt.migrated.push(key.clone());
                    }
                    SchemaMigrationOp::Add {
                        key,
                        type_name,
                        default_fn,
                    } => {
                        if fields.contains_key(key) {
                            return Err(SchemaMigrationError::DuplicateField {
                                step: step.name(),
                                field: key.clone(),
                            });
                        }
                        let value = default(default_fn, type_name).map_err(|detail| {
                            SchemaMigrationError::Callback {
                                step: step.name(),
                                field: key.clone(),
                                detail,
                            }
                        })?;
                        fields.insert(key.clone(), value);
                        receipt.migrated.push(key.clone());
                    }
                    SchemaMigrationOp::Change {
                        key,
                        from_type,
                        to_type,
                        converter_fn,
                    } => {
                        let old = fields.get(key).ok_or_else(|| SchemaMigrationError::MissingField {
                            step: step.name(),
                            field: key.clone(),
                        })?;
                        let value = convert(converter_fn, from_type, to_type, old).map_err(|detail| {
                            SchemaMigrationError::Callback {
                                step: step.name(),
                                field: key.clone(),
                                detail,
                            }
                        })?;
                        fields.insert(key.clone(), value);
                        receipt.migrated.push(key.clone());
                    }
                }
            }
            if index + 1 == self.steps.len() {
                break;
            }
        }
        receipt.kept = fields
            .keys()
            .filter(|key| !receipt.migrated.iter().any(|migrated| migrated == *key))
            .cloned()
            .collect();
        Ok(SchemaMigrationResult {
            value: MirRuntimeValue::Struct {
                type_name: type_name.clone(),
                fields: fields.into_iter().collect(),
            },
            receipt,
        })
    }

    pub fn current_shape_keys(&self) -> BTreeSet<String> {
        let mut keys = self
            .historical_shapes
            .first()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<BTreeSet<_>>();
        for step in &self.steps {
            for op in &step.ops {
                match op {
                    SchemaMigrationOp::Rename { from_key, to_key } => {
                        keys.remove(from_key);
                        keys.insert(to_key.clone());
                    }
                    SchemaMigrationOp::Remove { key } => {
                        keys.remove(key);
                    }
                    SchemaMigrationOp::Add { key, .. } => {
                        keys.insert(key.clone());
                    }
                    SchemaMigrationOp::Change { .. } => {}
                }
            }
        }
        keys
    }

    fn historical_shape_name(&self, index: usize) -> Option<String> {
        self.steps
            .get(index)
            .map(|step| step.from_shape.clone())
            .or_else(|| Some(format!("v{}", index + 1)))
    }

    pub fn current_shape_name(&self) -> String {
        self.steps
            .last()
            .map(|step| step.to_shape.clone())
            .unwrap_or_else(|| "current".to_string())
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaMigrationStep {
    pub from_shape: String,
    pub to_shape: String,
    pub ops: Vec<SchemaMigrationOp>,
}

impl SchemaMigrationStep {
    pub fn new(
        from_shape: impl Into<String>,
        to_shape: impl Into<String>,
        ops: Vec<SchemaMigrationOp>,
    ) -> Self {
        Self {
            from_shape: from_shape.into(),
            to_shape: to_shape.into(),
            ops,
        }
    }

    fn name(&self) -> String {
        format!("{}->{}", self.from_shape, self.to_shape)
    }
}

/// A wire-key operation. Function names are checked synthetic functions, not
/// source text; the tier callback resolves them through its canonical program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaMigrationOp {
    Rename {
        from_key: String,
        to_key: String,
    },
    Remove {
        key: String,
    },
    Add {
        key: String,
        type_name: String,
        default_fn: String,
    },
    Change {
        key: String,
        from_type: String,
        to_type: String,
        converter_fn: String,
    },
}

/// Shared result of applying a migration chain.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaMigrationResult {
    pub value: MirRuntimeValue,
    pub receipt: SchemaMigrationReceipt,
}

/// Human-readable, machine-stable migration accounting. `swapped` is only
/// rename work; `kept` is untouched data; `migrated` is actual step work;
/// `restarting` is populated by an adapter when the chain cannot be applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaMigrationReceipt {
    pub type_name: String,
    pub from_shape: String,
    pub to_shape: String,
    pub swapped: Vec<String>,
    pub kept: Vec<String>,
    pub migrated: Vec<String>,
    pub restarting: Option<String>,
}

impl SchemaMigrationReceipt {
    pub fn kept(type_name: &str, shape: &str) -> Self {
        Self {
            type_name: type_name.to_string(),
            from_shape: shape.to_string(),
            to_shape: shape.to_string(),
            swapped: Vec::new(),
            kept: Vec::new(),
            migrated: Vec::new(),
            restarting: None,
        }
    }

    pub fn migrated(type_name: &str, from: &str, to: &str) -> Self {
        Self {
            type_name: type_name.to_string(),
            from_shape: from.to_string(),
            to_shape: to.to_string(),
            swapped: Vec::new(),
            kept: Vec::new(),
            migrated: Vec::new(),
            restarting: None,
        }
    }

    pub fn restarting(type_name: &str, reason: impl Into<String>) -> Self {
        Self {
            type_name: type_name.to_string(),
            from_shape: String::new(),
            to_shape: String::new(),
            swapped: Vec::new(),
            kept: Vec::new(),
            migrated: Vec::new(),
            restarting: Some(reason.into()),
        }
    }
}

/// Why one typed migration could not be applied. An adapter must convert this
/// into a restart receipt rather than silently dropping the resident value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaMigrationError {
    NotRecord,
    TypeMismatch {
        expected: String,
        actual: String,
    },
    UnknownShape {
        type_name: String,
        keys: Vec<String>,
    },
    MissingField {
        step: String,
        field: String,
    },
    DuplicateField {
        step: String,
        field: String,
    },
    Callback {
        step: String,
        field: String,
        detail: String,
    },
}

impl fmt::Display for SchemaMigrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRecord => formatter.write_str("published-schema value is not a record"),
            Self::TypeMismatch { expected, actual } => {
                write!(formatter, "published-schema type `{actual}` does not match `{expected}`")
            }
            Self::UnknownShape { type_name, keys } => {
                write!(formatter, "published-schema `{type_name}` has unknown wire keys {keys:?}")
            }
            Self::MissingField { step, field } => {
                write!(formatter, "migration {step} needs wire field `{field}`")
            }
            Self::DuplicateField { step, field } => {
                write!(formatter, "migration {step} would duplicate wire field `{field}`")
            }
            Self::Callback { step, field, detail } => {
                write!(formatter, "migration {step} failed for `{field}`: {detail}")
            }
        }
    }
}

impl std::error::Error for SchemaMigrationError {}
