// D-DX-GAMEHOTSWAP1 / card #2489: backend-neutral game hot-swap facts.
//
// This module owns only checked identities, a #Persist-compatible world
// snapshot, and the synchronized state transition.  Script and asset facts are
// committed as one source/build revision.  It does not own listeners, native
// links, renderers, scene objects, or a transport envelope; those adapters must
// consume the receipts emitted here.
//
// A plan is deliberately fail-closed.  It validates every candidate fact before
// a live snapshot is touched, then a transaction requires an explicit pause,
// apply, and commit.  Rollback restores the exact pre-apply snapshot.

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct JetGameHotSwapIdentity {
    pub source_revision: String,
    pub build_revision: String,
}

impl JetGameHotSwapIdentity {
    pub fn new(
        source_revision: impl Into<String>,
        build_revision: impl Into<String>,
    ) -> Result<Self, JetGameHotSwapError> {
        let source_revision = source_revision.into();
        let build_revision = build_revision.into();
        jet_game_hot_swap_validate_revision(&source_revision, "source_revision")?;
        jet_game_hot_swap_validate_revision(&build_revision, "build_revision")?;
        Ok(Self {
            source_revision,
            build_revision,
        })
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"source_revision\":{},\"build_revision\":{}}}",
            jet_game_hot_swap_json_string(&self.source_revision),
            jet_game_hot_swap_json_string(&self.build_revision),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum JetGameLinkMode {
    /// Do not replace resident code or resources through a link transaction.
    Restart,
    /// Rebind script bodies, but do not synchronize scene/asset facts.
    Script,
    /// Synchronize scene/asset facts, but do not rebind scripts.
    Scene,
    /// Synchronize both script and scene/asset facts.
    ScriptAndScene,
}

impl JetGameLinkMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Restart => "restart",
            Self::Script => "script",
            Self::Scene => "scene",
            Self::ScriptAndScene => "script_and_scene",
        }
    }

    pub const fn scripts_enabled(self) -> bool {
        matches!(self, Self::Script | Self::ScriptAndScene)
    }

    pub const fn scene_enabled(self) -> bool {
        matches!(self, Self::Scene | Self::ScriptAndScene)
    }

    pub const fn is_restart(self) -> bool {
        matches!(self, Self::Restart)
    }
}

/// The incompatibilities that stop a game transaction before live state is
/// changed.  Revision mismatches are separate facts so hosts can explain the
/// exact rejected boundary without parsing a human message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetGameHotSwapIncompatibility {
    SourceRevisionMismatch { expected: String, actual: String },
    BuildRevisionMismatch { expected: String, actual: String },
    WorldSchemaChanged { expected: String, actual: String },
    PersistBindingRemoved { key: String },
    PersistSchemaChanged {
        key: String,
        expected: String,
        actual: String,
    },
    PersistValueTypeChanged {
        key: String,
        expected: &'static str,
        actual: &'static str,
    },
    ScriptTypeChanged {
        name: String,
        expected: String,
        actual: String,
    },
    ScriptRemoved { name: String },
    ScriptSyncDisabled { mode: JetGameLinkMode },
    AssetSyncDisabled { mode: JetGameLinkMode },
}

impl JetGameHotSwapIncompatibility {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SourceRevisionMismatch { .. } => "source_revision_mismatch",
            Self::BuildRevisionMismatch { .. } => "build_revision_mismatch",
            Self::WorldSchemaChanged { .. } => "world_schema_changed",
            Self::PersistBindingRemoved { .. } => "persist_binding_removed",
            Self::PersistSchemaChanged { .. } => "persist_schema_changed",
            Self::PersistValueTypeChanged { .. } => "persist_value_type_changed",
            Self::ScriptTypeChanged { .. } => "script_type_changed",
            Self::ScriptRemoved { .. } => "script_removed",
            Self::ScriptSyncDisabled { .. } => "script_sync_disabled",
            Self::AssetSyncDisabled { .. } => "asset_sync_disabled",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::SourceRevisionMismatch { expected, actual } => format!(
                "source revision mismatch: expected `{expected}`, got `{actual}`"
            ),
            Self::BuildRevisionMismatch { expected, actual } => format!(
                "build revision mismatch: expected `{expected}`, got `{actual}`"
            ),
            Self::WorldSchemaChanged { expected, actual } => format!(
                "persisted world schema changed from `{expected}` to `{actual}`"
            ),
            Self::PersistBindingRemoved { key } => {
                format!("persisted binding `{key}` was removed or renamed")
            }
            Self::PersistSchemaChanged {
                key,
                expected,
                actual,
            } => format!(
                "persisted binding `{key}` schema changed from `{expected}` to `{actual}`"
            ),
            Self::PersistValueTypeChanged {
                key,
                expected,
                actual,
            } => format!(
                "persisted binding `{key}` value type changed from `{expected}` to `{actual}`"
            ),
            Self::ScriptTypeChanged {
                name,
                expected,
                actual,
            } => format!("script `{name}` type changed from `{expected}` to `{actual}`"),
            Self::ScriptRemoved { name } => {
                format!("resident script `{name}` was removed or renamed")
            }
            Self::ScriptSyncDisabled { mode } => format!(
                "script changes are not enabled by explicit link mode `{}`",
                mode.as_str()
            ),
            Self::AssetSyncDisabled { mode } => format!(
                "asset changes are not enabled by explicit link mode `{}`",
                mode.as_str()
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetGameHotSwapError {
    InvalidRevision { field: String },
    InvalidIdentity { field: String },
    InvalidFact { kind: String, key: String },
    DuplicateFact { kind: String, key: String },
    InvalidPersistValue { key: String, reason: String },
    Incompatible {
        reason: JetGameHotSwapIncompatibility,
    },
    StaleLive {
        expected: JetGameHotSwapIdentity,
        actual: JetGameHotSwapIdentity,
    },
    TransactionIdMismatch { expected: u64, actual: u64 },
    InvalidPhase {
        expected: JetGameHotSwapReceiptPhase,
        actual: JetGameHotSwapReceiptPhase,
    },
    AlreadyFinalized,
}

impl JetGameHotSwapError {
    pub fn incompatibility(&self) -> Option<&JetGameHotSwapIncompatibility> {
        match self {
            Self::Incompatible { reason } => Some(reason),
            _ => None,
        }
    }

    pub const fn is_incompatible(&self) -> bool {
        matches!(self, Self::Incompatible { .. })
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidRevision { field } => {
                format!("game hot-swap {field} revision must be non-empty and printable")
            }
            Self::InvalidIdentity { field } => {
                format!("game hot-swap {field} identity must be non-empty and printable")
            }
            Self::InvalidFact { kind, key } => {
                format!("game hot-swap {kind} fact `{key}` is invalid")
            }
            Self::DuplicateFact { kind, key } => {
                format!("game hot-swap contains duplicate {kind} fact `{key}`")
            }
            Self::InvalidPersistValue { key, reason } => {
                format!("persisted value `{key}` is invalid: {reason}")
            }
            Self::Incompatible { reason } => reason.message(),
            Self::StaleLive { expected, actual } => format!(
                "game hot-swap live world is stale: expected source/build `{}/{}`, got `{}/{}`",
                expected.source_revision,
                expected.build_revision,
                actual.source_revision,
                actual.build_revision,
            ),
            Self::TransactionIdMismatch { expected, actual } => format!(
                "game hot-swap transaction id mismatch: expected `{expected}`, got `{actual}`"
            ),
            Self::InvalidPhase { expected, actual } => format!(
                "game hot-swap transaction requires phase `{}`, got `{}`",
                expected.as_str(),
                actual.as_str(),
            ),
            Self::AlreadyFinalized => "game hot-swap transaction is already finalized".to_string(),
        }
    }
}

/// A world type/layout identity.  The identity is repeated on each fact so a
/// host cannot accidentally combine facts from different source/build pairs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldRevisionFact {
    pub identity: JetGameHotSwapIdentity,
    pub schema_revision: String,
}

impl JetGameWorldRevisionFact {
    pub fn new(
        identity: JetGameHotSwapIdentity,
        schema_revision: impl Into<String>,
    ) -> Result<Self, JetGameHotSwapError> {
        let schema_revision = schema_revision.into();
        jet_game_hot_swap_validate_revision(&schema_revision, "world.schema_revision")?;
        Ok(Self {
            identity,
            schema_revision,
        })
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"identity\":{},\"schema_revision\":{}}}",
            self.identity.render_json(),
            jet_game_hot_swap_json_string(&self.schema_revision),
        )
    }
}

/// A checked script revision.  A body revision may change while the type
/// revision remains stable; that is the eligible script-link case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameScriptRevisionFact {
    pub name: String,
    pub identity: JetGameHotSwapIdentity,
    pub type_revision: String,
    pub body_revision: String,
}

impl JetGameScriptRevisionFact {
    pub fn new(
        name: impl Into<String>,
        identity: JetGameHotSwapIdentity,
        type_revision: impl Into<String>,
        body_revision: impl Into<String>,
    ) -> Result<Self, JetGameHotSwapError> {
        let name = name.into();
        let type_revision = type_revision.into();
        let body_revision = body_revision.into();
        jet_game_hot_swap_validate_identity(&name, "script.name")?;
        jet_game_hot_swap_validate_revision(&type_revision, "script.type_revision")?;
        jet_game_hot_swap_validate_revision(&body_revision, "script.body_revision")?;
        Ok(Self {
            name,
            identity,
            type_revision,
            body_revision,
        })
    }

    pub fn changed_body_from(&self, prior: &Self) -> bool {
        self.body_revision != prior.body_revision
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"name\":{},\"identity\":{},\"type_revision\":{},\"body_revision\":{}}}",
            jet_game_hot_swap_json_string(&self.name),
            self.identity.render_json(),
            jet_game_hot_swap_json_string(&self.type_revision),
            jet_game_hot_swap_json_string(&self.body_revision),
        )
    }
}

/// A checked asset output revision.  The source/build identity is shared with
/// the transaction; output_revision identifies the imported resource itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameAssetRevisionFact {
    pub logical_path: String,
    pub identity: JetGameHotSwapIdentity,
    pub output_revision: String,
}

impl JetGameAssetRevisionFact {
    pub fn new(
        logical_path: impl Into<String>,
        identity: JetGameHotSwapIdentity,
        output_revision: impl Into<String>,
    ) -> Result<Self, JetGameHotSwapError> {
        let logical_path = logical_path.into();
        let output_revision = output_revision.into();
        jet_game_hot_swap_validate_identity(&logical_path, "asset.logical_path")?;
        jet_game_hot_swap_validate_revision(&output_revision, "asset.output_revision")?;
        Ok(Self {
            logical_path,
            identity,
            output_revision,
        })
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"logical_path\":{},\"identity\":{},\"output_revision\":{}}}",
            jet_game_hot_swap_json_string(&self.logical_path),
            self.identity.render_json(),
            jet_game_hot_swap_json_string(&self.output_revision),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetGameHotSwapValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    List(Vec<Self>),
    Record(Vec<(String, Self)>),
}

impl JetGameHotSwapValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    pub fn record(mut fields: Vec<(String, Self)>) -> Result<Self, JetGameHotSwapError> {
        fields.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        for window in fields.windows(2) {
            if window[0].0 == window[1].0 {
                return Err(JetGameHotSwapError::DuplicateFact {
                    kind: "persist record field".to_string(),
                    key: window[0].0.clone(),
                });
            }
        }
        let value = Self::Record(fields);
        value.validate("record")?;
        Ok(value)
    }

    pub const fn type_tag(&self) -> &'static str {
        match self {
            Self::Bool(_) => "Bool",
            Self::Int(_) => "Int",
            Self::Float(_) => "Float",
            Self::Text(_) => "String",
            Self::Bytes(_) => "Bytes",
            Self::List(_) => "List",
            Self::Record(_) => "Record",
        }
    }

    fn validate(&self, key: &str) -> Result<(), JetGameHotSwapError> {
        match self {
            Self::Float(value) if !value.is_finite() => {
                Err(JetGameHotSwapError::InvalidPersistValue {
                    key: key.to_string(),
                    reason: "Float must be finite for deterministic state".to_string(),
                })
            }
            Self::Text(_) | Self::Bool(_) | Self::Int(_) | Self::Bytes(_) => Ok(()),
            Self::List(values) => {
                for (index, value) in values.iter().enumerate() {
                    value.validate(&format!("{key}[{index}]"))?;
                }
                Ok(())
            }
            Self::Record(fields) => {
                for window in fields.windows(2) {
                    if window[0].0 >= window[1].0 {
                        return Err(JetGameHotSwapError::InvalidPersistValue {
                            key: key.to_string(),
                            reason: "Record fields must be sorted and unique".to_string(),
                        });
                    }
                }
                for (field, value) in fields {
                    jet_game_hot_swap_validate_identity(field, "persist.record.field")?;
                    value.validate(&format!("{key}.{field}"))?;
                }
                Ok(())
            }
            Self::Float(_) => Ok(()),
        }
    }

    pub fn render_json(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => format!("{value:?}"),
            Self::Text(value) => jet_game_hot_swap_json_string(value),
            Self::Bytes(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::List(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Self::render_json)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Record(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{}:{}",
                            jet_game_hot_swap_json_string(key),
                            value.render_json()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

/// One module-level #Persist binding.  `schema_revision` is the checked shape
/// fingerprint; the value is retained only when both it and the value tag stay
/// compatible.
#[derive(Clone, Debug, PartialEq)]
pub struct JetGameHotSwapPersistState {
    pub key: String,
    pub schema_revision: String,
    pub value: JetGameHotSwapValue,
}

impl JetGameHotSwapPersistState {
    pub fn new(
        key: impl Into<String>,
        schema_revision: impl Into<String>,
        value: JetGameHotSwapValue,
    ) -> Result<Self, JetGameHotSwapError> {
        let key = key.into();
        let schema_revision = schema_revision.into();
        jet_game_hot_swap_validate_identity(&key, "persist.key")?;
        jet_game_hot_swap_validate_revision(&schema_revision, "persist.schema_revision")?;
        value.validate(&key)?;
        Ok(Self {
            key,
            schema_revision,
            value,
        })
    }

    pub fn value_type(&self) -> &'static str {
        self.value.type_tag()
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"key\":{},\"schema_revision\":{},\"value_type\":{},\"value\":{}}}",
            jet_game_hot_swap_json_string(&self.key),
            jet_game_hot_swap_json_string(&self.schema_revision),
            jet_game_hot_swap_json_string(self.value.type_tag()),
            self.value.render_json(),
        )
    }
}

/// The complete checked game state at one source/build revision.
#[derive(Clone, Debug, PartialEq)]
pub struct JetGameHotSwapSnapshot {
    pub identity: JetGameHotSwapIdentity,
    pub world: JetGameWorldRevisionFact,
    pub persist: Vec<JetGameHotSwapPersistState>,
    pub scripts: Vec<JetGameScriptRevisionFact>,
    pub assets: Vec<JetGameAssetRevisionFact>,
}

impl JetGameHotSwapSnapshot {
    pub fn new(
        identity: JetGameHotSwapIdentity,
        world: JetGameWorldRevisionFact,
        mut persist: Vec<JetGameHotSwapPersistState>,
        mut scripts: Vec<JetGameScriptRevisionFact>,
        mut assets: Vec<JetGameAssetRevisionFact>,
    ) -> Result<Self, JetGameHotSwapError> {
        let snapshot = Self {
            identity,
            world,
            persist: {
                persist.sort_unstable_by(|left, right| left.key.cmp(&right.key));
                persist
            },
            scripts: {
                scripts.sort_unstable_by(|left, right| left.name.cmp(&right.name));
                scripts
            },
            assets: {
                assets.sort_unstable_by(|left, right| left.logical_path.cmp(&right.logical_path));
                assets
            },
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), JetGameHotSwapError> {
        if self.world.identity != self.identity {
            jet_game_hot_swap_identity_error(&self.identity, &self.world.identity)?;
        }
        let mut prior = None;
        for state in &self.persist {
            if prior.is_some_and(|key: &String| key >= &state.key) {
                return Err(JetGameHotSwapError::DuplicateFact {
                    kind: "persist".to_string(),
                    key: state.key.clone(),
                });
            }
            state.value.validate(&state.key)?;
            prior = Some(&state.key);
        }
        let mut prior = None;
        for script in &self.scripts {
            if prior.is_some_and(|name: &String| name >= &script.name) {
                return Err(JetGameHotSwapError::DuplicateFact {
                    kind: "script".to_string(),
                    key: script.name.clone(),
                });
            }
            jet_game_hot_swap_identity_error(&self.identity, &script.identity)?;
            prior = Some(&script.name);
        }
        let mut prior = None;
        for asset in &self.assets {
            if prior.is_some_and(|path: &String| path >= &asset.logical_path) {
                return Err(JetGameHotSwapError::DuplicateFact {
                    kind: "asset".to_string(),
                    key: asset.logical_path.clone(),
                });
            }
            jet_game_hot_swap_identity_error(&self.identity, &asset.identity)?;
            prior = Some(&asset.logical_path);
        }
        Ok(())
    }

    pub fn persist_value(&self, key: &str) -> Option<&JetGameHotSwapPersistState> {
        self.persist.iter().find(|state| state.key == key)
    }

    pub fn script(&self, name: &str) -> Option<&JetGameScriptRevisionFact> {
        self.scripts.iter().find(|script| script.name == name)
    }

    pub fn asset(&self, logical_path: &str) -> Option<&JetGameAssetRevisionFact> {
        self.assets
            .iter()
            .find(|asset| asset.logical_path == logical_path)
    }

    pub fn render_json(&self) -> String {
        let persist = self
            .persist
            .iter()
            .map(JetGameHotSwapPersistState::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let scripts = self
            .scripts
            .iter()
            .map(JetGameScriptRevisionFact::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let assets = self
            .assets
            .iter()
            .map(JetGameAssetRevisionFact::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"identity\":{},\"world\":{},\"persist\":[{}],\"scripts\":[{}],\"assets\":[{}]}}",
            self.identity.render_json(),
            self.world.render_json(),
            persist,
            scripts,
            assets,
        )
    }
}


#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum JetGameHotSwapReceiptPhase {
    Prepared,
    Paused,
    Applied,
    Committed,
    RolledBack,
}

impl JetGameHotSwapReceiptPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Paused => "paused",
            Self::Applied => "applied",
            Self::Committed => "committed",
            Self::RolledBack => "rolled_back",
        }
    }
}

/// One auditable transaction receipt.  The revision pair is present on every
/// phase, so a host can publish a receipt without consulting mutable runtime
/// state or guessing which source/build produced it.
#[derive(Clone, Debug, PartialEq)]
pub struct JetGameHotSwapReceipt {
    pub transaction_id: u64,
    pub phase: JetGameHotSwapReceiptPhase,
    pub from_identity: JetGameHotSwapIdentity,
    pub to_identity: JetGameHotSwapIdentity,
    pub link_mode: JetGameLinkMode,
    pub kept_values: Vec<String>,
    pub reset_values: Vec<String>,
    pub fresh_values: Vec<String>,
    pub changed_scripts: Vec<String>,
    pub changed_assets: Vec<String>,
    pub incompatibility: Option<JetGameHotSwapIncompatibility>,
}

impl JetGameHotSwapReceipt {
    pub const fn is_committed(&self) -> bool {
        matches!(self.phase, JetGameHotSwapReceiptPhase::Committed)
    }

    pub const fn is_rolled_back(&self) -> bool {
        matches!(self.phase, JetGameHotSwapReceiptPhase::RolledBack)
    }

    pub fn render_json(&self) -> String {
        let list = |values: &[String]| {
            values
                .iter()
                .map(|value| jet_game_hot_swap_json_string(value))
                .collect::<Vec<_>>()
                .join(",")
        };
        let incompatibility = self
            .incompatibility
            .as_ref()
            .map(|reason| {
                format!(
                    "{{\"code\":{},\"message\":{}}}",
                    jet_game_hot_swap_json_string(reason.code()),
                    jet_game_hot_swap_json_string(&reason.message()),
                )
            })
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"transaction_id\":{},\"phase\":{},\"from\":{},\"to\":{},\"link_mode\":{},\"kept_values\":[{}],\"reset_values\":[{}],\"fresh_values\":[{}],\"changed_scripts\":[{}],\"changed_assets\":[{}],\"incompatibility\":{}}}",
            self.transaction_id,
            jet_game_hot_swap_json_string(self.phase.as_str()),
            self.from_identity.render_json(),
            self.to_identity.render_json(),
            jet_game_hot_swap_json_string(self.link_mode.as_str()),
            list(&self.kept_values),
            list(&self.reset_values),
            list(&self.fresh_values),
            list(&self.changed_scripts),
            list(&self.changed_assets),
            incompatibility,
        )
    }
}

/// A preflighted, explicit link selection.  It owns the checked old/new
/// snapshots so transaction start can reject a stale live world before any
/// mutation.  The vectors are sorted facts, not adapter instructions.
#[derive(Clone, Debug, PartialEq)]
pub struct JetGameHotSwapPlan {
    pub current_identity: JetGameHotSwapIdentity,
    pub candidate_identity: JetGameHotSwapIdentity,
    pub link_mode: JetGameLinkMode,
    pub kept_values: Vec<String>,
    pub reset_values: Vec<String>,
    pub fresh_values: Vec<String>,
    pub changed_scripts: Vec<String>,
    pub changed_assets: Vec<String>,
    transaction_id: u64,
    current: JetGameHotSwapSnapshot,
    candidate: JetGameHotSwapSnapshot,
}

impl JetGameHotSwapPlan {
    pub fn new(
        current: &JetGameHotSwapSnapshot,
        candidate: &JetGameHotSwapSnapshot,
        link_mode: JetGameLinkMode,
    ) -> Result<Self, JetGameHotSwapError> {
        current.validate()?;
        candidate.validate()?;

        let mut old_persist = std::collections::BTreeMap::new();
        for state in &current.persist {
            old_persist.insert(state.key.as_str(), state);
        }
        let mut new_persist = std::collections::BTreeMap::new();
        for state in &candidate.persist {
            new_persist.insert(state.key.as_str(), state);
        }

        let mut kept_values = Vec::new();
        let mut reset_values = Vec::new();
        let mut fresh_values = Vec::new();
        for state in &current.persist {
            let Some(next) = new_persist.get(state.key.as_str()) else {
                return Err(JetGameHotSwapError::Incompatible {
                    reason: JetGameHotSwapIncompatibility::PersistBindingRemoved {
                        key: state.key.clone(),
                    },
                });
            };
            if state.schema_revision != next.schema_revision {
                return Err(JetGameHotSwapError::Incompatible {
                    reason: JetGameHotSwapIncompatibility::PersistSchemaChanged {
                        key: state.key.clone(),
                        expected: state.schema_revision.clone(),
                        actual: next.schema_revision.clone(),
                    },
                });
            }
            if state.value_type() != next.value_type() {
                return Err(JetGameHotSwapError::Incompatible {
                    reason: JetGameHotSwapIncompatibility::PersistValueTypeChanged {
                        key: state.key.clone(),
                        expected: state.value_type(),
                        actual: next.value_type(),
                    },
                });
            }
            if link_mode.is_restart() {
                reset_values.push(state.key.clone());
            } else {
                kept_values.push(state.key.clone());
            }
        }
        for state in &candidate.persist {
            if !old_persist.contains_key(state.key.as_str()) {
                fresh_values.push(state.key.clone());
            }
        }

        if current.world.schema_revision != candidate.world.schema_revision {
            return Err(JetGameHotSwapError::Incompatible {
                reason: JetGameHotSwapIncompatibility::WorldSchemaChanged {
                    expected: current.world.schema_revision.clone(),
                    actual: candidate.world.schema_revision.clone(),
                },
            });
        }

        let mut old_scripts = std::collections::BTreeMap::new();
        for script in &current.scripts {
            old_scripts.insert(script.name.as_str(), script);
        }
        let mut new_scripts = std::collections::BTreeMap::new();
        for script in &candidate.scripts {
            new_scripts.insert(script.name.as_str(), script);
        }
        let mut changed_scripts = Vec::new();
        for script in &current.scripts {
            let Some(next) = new_scripts.get(script.name.as_str()) else {
                return Err(JetGameHotSwapError::Incompatible {
                    reason: JetGameHotSwapIncompatibility::ScriptRemoved {
                        name: script.name.clone(),
                    },
                });
            };
            if script.type_revision != next.type_revision {
                return Err(JetGameHotSwapError::Incompatible {
                    reason: JetGameHotSwapIncompatibility::ScriptTypeChanged {
                        name: script.name.clone(),
                        expected: script.type_revision.clone(),
                        actual: next.type_revision.clone(),
                    },
                });
            }
            if script.body_revision != next.body_revision {
                changed_scripts.push(script.name.clone());
            }
        }
        for script in &candidate.scripts {
            if !old_scripts.contains_key(script.name.as_str()) {
                changed_scripts.push(script.name.clone());
            }
        }
        changed_scripts.sort_unstable();
        changed_scripts.dedup();

        let mut old_assets = std::collections::BTreeMap::new();
        for asset in &current.assets {
            old_assets.insert(asset.logical_path.as_str(), asset);
        }
        let mut new_assets = std::collections::BTreeMap::new();
        for asset in &candidate.assets {
            new_assets.insert(asset.logical_path.as_str(), asset);
        }
        let mut changed_assets = Vec::new();
        for asset in &current.assets {
            match new_assets.get(asset.logical_path.as_str()) {
                Some(next) if next.output_revision == asset.output_revision => {}
                _ => changed_assets.push(asset.logical_path.clone()),
            }
        }
        for asset in &candidate.assets {
            if !old_assets.contains_key(asset.logical_path.as_str()) {
                changed_assets.push(asset.logical_path.clone());
            }
        }
        changed_assets.sort_unstable();
        changed_assets.dedup();

        if !changed_scripts.is_empty() && !link_mode.scripts_enabled() && !link_mode.is_restart() {
            return Err(JetGameHotSwapError::Incompatible {
                reason: JetGameHotSwapIncompatibility::ScriptSyncDisabled { mode: link_mode },
            });
        }
        if !changed_assets.is_empty() && !link_mode.scene_enabled() && !link_mode.is_restart() {
            return Err(JetGameHotSwapError::Incompatible {
                reason: JetGameHotSwapIncompatibility::AssetSyncDisabled { mode: link_mode },
            });
        }

        let transaction_id = jet_game_hot_swap_transaction_id(
            current,
            candidate,
            link_mode,
            &changed_scripts,
            &changed_assets,
        );
        Ok(Self {
            current_identity: current.identity.clone(),
            candidate_identity: candidate.identity.clone(),
            link_mode,
            kept_values,
            reset_values,
            fresh_values,
            changed_scripts,
            changed_assets,
            transaction_id,
            current: current.clone(),
            candidate: candidate.clone(),
        })
    }

    pub fn transaction_id(&self) -> u64 {
        self.transaction_id
    }

    /// Recheck the exact source/build snapshot immediately before pausing.
    /// This is intentionally public: adapters must not skip the revision and
    /// content check when a plan has spent time in a queue.
    pub fn preflight(
        &self,
        live: &JetGameHotSwapSnapshot,
    ) -> Result<(), JetGameHotSwapError> {
        self.verify_live(live)
    }

    /// Run the full pause/apply/commit sequence without mutating `live` until
    /// the final committed snapshot is returned.  Any failed post-apply check
    /// attempts rollback before returning the error.
    pub fn execute(
        &self,
        live: &mut JetGameHotSwapSnapshot,
    ) -> Result<JetGameHotSwapReceipt, JetGameHotSwapError> {
        let mut transaction = JetGameHotSwapTransaction::begin(live, self)?;
        let transaction_id = transaction.transaction_id();
        transaction.pause(transaction_id)?;
        let (staged, _) =
            transaction.apply_scripts_and_assets(live.clone(), transaction_id)?;
        let rollback_snapshot = staged.clone();
        match transaction.commit(staged, transaction_id) {
            Ok((committed, receipt)) => {
                *live = committed;
                Ok(receipt)
            }
            Err(error) => {
                let _ = transaction.rollback(rollback_snapshot, transaction_id);
                Err(error)
            }
        }
    }

    fn verify_live(
        &self,
        live: &JetGameHotSwapSnapshot,
    ) -> Result<(), JetGameHotSwapError> {
        live.validate()?;
        if live.identity != self.current_identity || live != &self.current {
            return Err(JetGameHotSwapError::StaleLive {
                expected: self.current_identity.clone(),
                actual: live.identity.clone(),
            });
        }
        Ok(())
    }

    fn staged_snapshot(&self, prior: &JetGameHotSwapSnapshot) -> JetGameHotSwapSnapshot {
        if self.link_mode.is_restart() {
            return self.candidate.clone();
        }
        let mut staged = self.candidate.clone();
        for state in &mut staged.persist {
            if let Some(old) = prior.persist_value(&state.key) {
                state.value = old.value.clone();
            }
        }
        staged
    }

    fn receipt(&self, phase: JetGameHotSwapReceiptPhase) -> JetGameHotSwapReceipt {
        JetGameHotSwapReceipt {
            transaction_id: self.transaction_id,
            phase,
            from_identity: self.current_identity.clone(),
            to_identity: self.candidate_identity.clone(),
            link_mode: self.link_mode,
            kept_values: self.kept_values.clone(),
            reset_values: self.reset_values.clone(),
            fresh_values: self.fresh_values.clone(),
            changed_scripts: self.changed_scripts.clone(),
            changed_assets: self.changed_assets.clone(),
            incompatibility: None,
        }
    }
}
/// Structured answer for `jet explain --reload`. Hosts can display the
/// incompatibility code without scraping a human diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameHotSwapExplanation {
    pub current_identity: JetGameHotSwapIdentity,
    pub candidate_identity: JetGameHotSwapIdentity,
    pub link_mode: JetGameLinkMode,
    pub compatible: bool,
    pub transaction_id: Option<u64>,
    pub code: String,
    pub message: String,
}

impl JetGameHotSwapExplanation {
    pub fn explain(
        current: &JetGameHotSwapSnapshot,
        candidate: &JetGameHotSwapSnapshot,
        link_mode: JetGameLinkMode,
    ) -> Self {
        match JetGameHotSwapPlan::new(current, candidate, link_mode) {
            Ok(plan) => Self {
                current_identity: current.identity.clone(),
                candidate_identity: candidate.identity.clone(),
                link_mode,
                compatible: true,
                transaction_id: Some(plan.transaction_id()),
                code: "ok".to_string(),
                message: "reload can commit with the selected link mode".to_string(),
            },
            Err(error) => Self {
                current_identity: current.identity.clone(),
                candidate_identity: candidate.identity.clone(),
                link_mode,
                compatible: false,
                transaction_id: None,
                code: error
                    .incompatibility()
                    .map(|reason| reason.code().to_string())
                    .unwrap_or_else(|| "invalid_reload".to_string()),
                message: error.message(),
            },
        }
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"current\":{},\"candidate\":{},\"link_mode\":{},\"compatible\":{},\
\"transaction_id\":{},\"code\":{},\"message\":{}}}",
            self.current_identity.render_json(),
            self.candidate_identity.render_json(),
            jet_game_hot_swap_json_string(self.link_mode.as_str()),
            self.compatible,
            self.transaction_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            jet_game_hot_swap_json_string(&self.code),
            jet_game_hot_swap_json_string(&self.message),
        )
    }
}


/// The stateful phase guard for one synchronized game replacement.
#[derive(Clone, Debug, PartialEq)]
pub struct JetGameHotSwapTransaction {
    plan: JetGameHotSwapPlan,
    prior: JetGameHotSwapSnapshot,
    staged: Option<JetGameHotSwapSnapshot>,
    phase: JetGameHotSwapReceiptPhase,
}

impl JetGameHotSwapTransaction {
    /// Begin from an owned revision plan only after checking the exact live
    /// snapshot.  The transaction id is derived from all revision facts and is
    /// required by every transition below.
    pub fn begin(
        snapshot: &JetGameHotSwapSnapshot,
        plan: &JetGameHotSwapPlan,
    ) -> Result<Self, JetGameHotSwapError> {
        plan.preflight(snapshot)?;
        Ok(Self {
            plan: plan.clone(),
            prior: snapshot.clone(),
            staged: None,
            phase: JetGameHotSwapReceiptPhase::Prepared,
        })
    }

    pub fn transaction_id(&self) -> u64 {
        self.plan.transaction_id
    }

    pub fn phase(&self) -> JetGameHotSwapReceiptPhase {
        self.phase
    }

    fn check_transaction_id(&self, transaction_id: u64) -> Result<(), JetGameHotSwapError> {
        if transaction_id != self.plan.transaction_id {
            return Err(JetGameHotSwapError::TransactionIdMismatch {
                expected: self.plan.transaction_id,
                actual: transaction_id,
            });
        }
        Ok(())
    }

    pub fn pause(
        &mut self,
        transaction_id: u64,
    ) -> Result<JetGameHotSwapReceipt, JetGameHotSwapError> {
        self.check_transaction_id(transaction_id)?;
        if self.phase != JetGameHotSwapReceiptPhase::Prepared {
            return Err(JetGameHotSwapError::InvalidPhase {
                expected: JetGameHotSwapReceiptPhase::Prepared,
                actual: self.phase,
            });
        }
        self.phase = JetGameHotSwapReceiptPhase::Paused;
        Ok(self.plan.receipt(self.phase))
    }

    /// Validate and stage scripts and assets together.  This method returns a
    /// new snapshot rather than touching caller-owned state; a host can publish
    /// or discard it before commit.  No partial script/asset application is
    /// possible.
    pub fn apply_scripts_and_assets(
        &mut self,
        live: JetGameHotSwapSnapshot,
        transaction_id: u64,
    ) -> Result<(JetGameHotSwapSnapshot, JetGameHotSwapReceipt), JetGameHotSwapError> {
        self.check_transaction_id(transaction_id)?;
        if self.phase != JetGameHotSwapReceiptPhase::Paused {
            return Err(JetGameHotSwapError::InvalidPhase {
                expected: JetGameHotSwapReceiptPhase::Paused,
                actual: self.phase,
            });
        }
        self.plan.preflight(&live)?;
        let staged = self.plan.staged_snapshot(&self.prior);
        staged.validate()?;
        self.staged = Some(staged.clone());
        self.phase = JetGameHotSwapReceiptPhase::Applied;
        Ok((staged, self.plan.receipt(self.phase)))
    }

    /// Commit the exact staged snapshot.  The returned pair is the only
    /// committed state transition; the source/build and transaction ids are
    /// checked before the phase changes.
    pub fn commit(
        &mut self,
        live: JetGameHotSwapSnapshot,
        transaction_id: u64,
    ) -> Result<(JetGameHotSwapSnapshot, JetGameHotSwapReceipt), JetGameHotSwapError> {
        self.check_transaction_id(transaction_id)?;
        if self.phase != JetGameHotSwapReceiptPhase::Applied {
            return Err(JetGameHotSwapError::InvalidPhase {
                expected: JetGameHotSwapReceiptPhase::Applied,
                actual: self.phase,
            });
        }
        if self.staged.as_ref() != Some(&live) {
            return Err(JetGameHotSwapError::StaleLive {
                expected: self.plan.candidate_identity.clone(),
                actual: live.identity.clone(),
            });
        }
        self.phase = JetGameHotSwapReceiptPhase::Committed;
        Ok((live, self.plan.receipt(self.phase)))
    }

    /// Return the exact pre-apply snapshot.  Applied transactions verify the
    /// caller still holds this transaction's staged revision before restoring.
    pub fn rollback(
        &mut self,
        live: JetGameHotSwapSnapshot,
        transaction_id: u64,
    ) -> Result<(JetGameHotSwapSnapshot, JetGameHotSwapReceipt), JetGameHotSwapError> {
        self.check_transaction_id(transaction_id)?;
        match self.phase {
            JetGameHotSwapReceiptPhase::Prepared | JetGameHotSwapReceiptPhase::Paused => {
                if live != self.prior {
                    return Err(JetGameHotSwapError::StaleLive {
                        expected: self.plan.current_identity.clone(),
                        actual: live.identity.clone(),
                    });
                }
                self.phase = JetGameHotSwapReceiptPhase::RolledBack;
                Ok((live, self.plan.receipt(self.phase)))
            }
            JetGameHotSwapReceiptPhase::Applied => {
                if self.staged.as_ref() != Some(&live) {
                    return Err(JetGameHotSwapError::StaleLive {
                        expected: self.plan.candidate_identity.clone(),
                        actual: live.identity.clone(),
                    });
                }
                self.phase = JetGameHotSwapReceiptPhase::RolledBack;
                Ok((self.prior.clone(), self.plan.receipt(self.phase)))
            }
            JetGameHotSwapReceiptPhase::Committed | JetGameHotSwapReceiptPhase::RolledBack => {
                Err(JetGameHotSwapError::AlreadyFinalized)
            }
        }
    }
}

fn jet_game_hot_swap_validate_revision(
    value: &str,
    field: &str,
) -> Result<(), JetGameHotSwapError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(JetGameHotSwapError::InvalidRevision {
            field: field.to_string(),
        });
    }
    Ok(())
}

fn jet_game_hot_swap_validate_identity(
    value: &str,
    field: &str,
) -> Result<(), JetGameHotSwapError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(JetGameHotSwapError::InvalidIdentity {
            field: field.to_string(),
        });
    }
    Ok(())
}

fn jet_game_hot_swap_identity_error(
    expected: &JetGameHotSwapIdentity,
    actual: &JetGameHotSwapIdentity,
) -> Result<(), JetGameHotSwapError> {
    if expected.source_revision != actual.source_revision {
        return Err(JetGameHotSwapError::Incompatible {
            reason: JetGameHotSwapIncompatibility::SourceRevisionMismatch {
                expected: expected.source_revision.clone(),
                actual: actual.source_revision.clone(),
            },
        });
    }
    if expected.build_revision != actual.build_revision {
        return Err(JetGameHotSwapError::Incompatible {
            reason: JetGameHotSwapIncompatibility::BuildRevisionMismatch {
                expected: expected.build_revision.clone(),
                actual: actual.build_revision.clone(),
            },
        });
    }
    Ok(())
}

fn jet_game_hot_swap_transaction_id(
    current: &JetGameHotSwapSnapshot,
    candidate: &JetGameHotSwapSnapshot,
    link_mode: JetGameLinkMode,
    changed_scripts: &[String],
    changed_assets: &[String],
) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for value in [
        current.identity.source_revision.as_str(),
        current.identity.build_revision.as_str(),
        candidate.identity.source_revision.as_str(),
        candidate.identity.build_revision.as_str(),
        link_mode.as_str(),
    ] {
        jet_game_hot_swap_hash_field(&mut hash, value);
    }
    for value in changed_scripts.iter().chain(changed_assets.iter()) {
        jet_game_hot_swap_hash_field(&mut hash, value);
    }
    hash
}

fn jet_game_hot_swap_hash_field(hash: &mut u64, value: &str) {
    for byte in (value.len() as u64).to_le_bytes() {
        *hash ^= u64::from(byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    for byte in value.as_bytes() {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn jet_game_hot_swap_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod game_hot_swap_tests {
    use super::*;

    fn identity(source: &str, build: &str) -> JetGameHotSwapIdentity {
        JetGameHotSwapIdentity::new(source, build).expect("valid identity")
    }

    fn snapshot(
        source: &str,
        build: &str,
        schema: &str,
        value: i64,
        body: &str,
        asset: &str,
    ) -> JetGameHotSwapSnapshot {
        let identity = identity(source, build);
        let world = JetGameWorldRevisionFact::new(identity.clone(), schema).expect("world");
        let persist = JetGameHotSwapPersistState::new(
            "world",
            "World{score:Int}",
            JetGameHotSwapValue::Int(value),
        )
        .expect("persist");
        let script = JetGameScriptRevisionFact::new(
            "tick",
            identity.clone(),
            "tick(Int)->Unit",
            body,
        )
        .expect("script");
        let asset = JetGameAssetRevisionFact::new("scene/main", identity.clone(), asset)
            .expect("asset");
        JetGameHotSwapSnapshot::new(identity, world, vec![persist], vec![script], vec![asset])
            .expect("snapshot")
    }

    #[test]
    fn body_edit_keeps_persisted_value_and_commits_script_and_asset_revision() {
        let current = snapshot("source-1", "build-1", "World.v1", 41, "body-1", "asset-1");
        let candidate = snapshot("source-2", "build-2", "World.v1", 0, "body-2", "asset-2");
        let plan = JetGameHotSwapPlan::new(
            &current,
            &candidate,
            JetGameLinkMode::ScriptAndScene,
        )
        .expect("eligible plan");
        let mut live = current.clone();
        let receipt = plan.execute(&mut live).expect("commit");
        assert!(receipt.is_committed());
        assert_eq!(live.persist_value("world").unwrap().value, JetGameHotSwapValue::Int(41));
        assert_eq!(live.identity, candidate.identity);
        assert_eq!(receipt.changed_scripts, vec!["tick"]);
        assert_eq!(receipt.changed_assets, vec!["scene/main"]);
    }

    #[test]
    fn changed_schema_is_rejected_before_live_mutation() {
        let current = snapshot("source-1", "build-1", "World.v1", 41, "body-1", "asset-1");
        let candidate = snapshot("source-2", "build-2", "World.v2", 0, "body-2", "asset-2");
        let live = current.clone();
        let error = JetGameHotSwapPlan::new(
            &current,
            &candidate,
            JetGameLinkMode::ScriptAndScene,
        )
        .expect_err("layout change must fail closed");
        assert!(error.is_incompatible());
        assert_eq!(live, current);
    }

    #[test]
    fn rollback_restores_prior_world_after_apply() {
        let current = snapshot("source-1", "build-1", "World.v1", 41, "body-1", "asset-1");
        let candidate = snapshot("source-2", "build-2", "World.v1", 0, "body-2", "asset-2");
        let plan = JetGameHotSwapPlan::new(
            &current,
            &candidate,
            JetGameLinkMode::ScriptAndScene,
        )
        .expect("eligible plan");
        let mut transaction = JetGameHotSwapTransaction::begin(&current, &plan).expect("begin");
        let transaction_id = transaction.transaction_id();
        transaction.pause(transaction_id).expect("pause");
        let (staged, _) = transaction
            .apply_scripts_and_assets(current.clone(), transaction_id)
            .expect("apply");
        assert_eq!(staged.identity, candidate.identity);
        let (live, receipt) = transaction
            .rollback(staged, transaction_id)
            .expect("rollback");
        assert!(receipt.is_rolled_back());
        assert_eq!(live, current);
    }

    #[test]
    fn wrong_transaction_id_is_rejected_without_advancing_phase() {
        let current = snapshot("source-1", "build-1", "World.v1", 41, "body-1", "asset-1");
        let candidate = snapshot("source-2", "build-2", "World.v1", 0, "body-2", "asset-2");
        let plan = JetGameHotSwapPlan::new(
            &current,
            &candidate,
            JetGameLinkMode::ScriptAndScene,
        )
        .expect("eligible plan");
        let mut transaction = JetGameHotSwapTransaction::begin(&current, &plan).expect("begin");
        let error = transaction.pause(transaction.transaction_id() ^ 1).expect_err("wrong id");
        assert!(matches!(
            error,
            JetGameHotSwapError::TransactionIdMismatch { .. }
        ));
        assert_eq!(
            transaction.phase(),
            JetGameHotSwapReceiptPhase::Prepared
        );
    }

    #[test]
    fn disabled_link_mode_is_explicit_and_auditable() {
        let current = snapshot("source-1", "build-1", "World.v1", 41, "body-1", "asset-1");
        let candidate = snapshot("source-2", "build-2", "World.v1", 0, "body-2", "asset-2");
        let error = JetGameHotSwapPlan::new(&current, &candidate, JetGameLinkMode::Script)
            .expect_err("scene changes require scene link mode");
        match error {
            JetGameHotSwapError::Incompatible { reason } => {
                assert_eq!(reason.code(), "asset_sync_disabled");
                assert!(reason.message().contains("script"));
            }
            other => panic!("wrong error: {other:?}"),
        }
    }
}
