// D-GAME-WORLD-INSPECTOR1: one bounded, typed game-world projection for the
// devtools surface. This kernel owns scene-tree identity, provenance, and the
// read-only paused-frame boundary. Hosts marshal these values through the
// existing jet.devtools.v1 envelope; this file does not define another wire
// protocol or an application-side state store.

pub const JET_GAME_WORLD_MAX_ENTITIES: usize = 4_096;
pub const JET_GAME_WORLD_MAX_COMPONENTS_PER_ENTITY: usize = 256;
pub const JET_GAME_WORLD_MAX_PROPERTIES_PER_COMPONENT: usize = 256;
pub const JET_GAME_WORLD_MAX_FRAME_FACTS: usize = 256;
pub const JET_GAME_WORLD_MAX_TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct JetGameWorldSourceSpan {
    pub start: usize,
    pub end: usize,
}

impl JetGameWorldSourceSpan {
    pub fn new(start: usize, end: usize) -> Result<Self, JetGameWorldInspectorError> {
        if start > end {
            return Err(JetGameWorldInspectorError::InvalidSourceSpan);
        }
        Ok(Self { start, end })
    }

    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }

    pub fn render_json(self) -> String {
        format!("{{\"start\":{},\"end\":{}}}", self.start, self.end)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetGameWorldInspectorError {
    EmptyText { field: &'static str },
    TextTooLong { field: &'static str },
    TooManyEntities,
    TooManyComponents { entity_id: String },
    TooManyProperties { component_id: String },
    TooManyFrames,
    DuplicateId { id: String },
    DuplicateProperty { component_id: String, field: String },
    MissingParent { child_id: String, parent_id: String },
    Cycle { id: String },
    InvalidSourceSpan,
    NoWorld,
    MismatchedWorld { expected: String, actual: String },
    UnknownEntity { id: String },
    UnknownComponent { id: String },
    UnknownProperty { id: String },
    InvalidSelectionHierarchy,
    NoSelection,
    NotEditable { field: String },
    InvalidEdit { reason: &'static str },
    DuplicateEditTarget,
    StaleRevision { expected: String, actual: String },
    StaleSession { expected: String, actual: String },
    StaleTier { expected: String, actual: String },
    InvalidEvaluation { reason: &'static str },
}

impl std::fmt::Display for JetGameWorldInspectorError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyText { field } => write!(output, "game world {field} must not be empty"),
            Self::TextTooLong { field } => {
                write!(output, "game world {field} exceeds the Prelude limit")
            }
            Self::TooManyEntities => write!(output, "game world entity limit exceeded"),
            Self::TooManyComponents { entity_id } => {
                write!(output, "component limit exceeded for entity `{entity_id}`")
            }
            Self::TooManyProperties { component_id } => {
                write!(output, "property limit exceeded for component `{component_id}`")
            }
            Self::TooManyFrames => write!(output, "game world frame limit exceeded"),
            Self::DuplicateId { id } => write!(output, "duplicate game world id `{id}`"),
            Self::DuplicateProperty {
                component_id,
                field,
            } => write!(output, "duplicate property `{field}` on component `{component_id}`"),
            Self::MissingParent {
                child_id,
                parent_id,
            } => write!(
                output,
                "game world parent `{parent_id}` for `{child_id}` is missing"
            ),
            Self::Cycle { id } => write!(output, "game world cycle at `{id}`"),
            Self::InvalidSourceSpan => write!(output, "invalid game world source span"),
            Self::NoWorld => write!(output, "game world inspector has no world"),
            Self::MismatchedWorld { expected, actual } => write!(
                output,
                "game world selection names `{actual}`, current world is `{expected}`"
            ),
            Self::UnknownEntity { id } => write!(output, "unknown game world entity `{id}`"),
            Self::UnknownComponent { id } => write!(output, "unknown game world component `{id}`"),
            Self::UnknownProperty { id } => write!(output, "unknown game world property `{id}`"),
            Self::InvalidSelectionHierarchy => {
                write!(output, "game world selection does not name a valid hierarchy")
            }
            Self::NoSelection => write!(output, "game world inspector has no selection"),
            Self::NotEditable { field } => {
                write!(output, "game world property `{field}` is not authored-editable")
            }
            Self::InvalidEdit { reason } => write!(output, "invalid game world edit: {reason}"),
            Self::DuplicateEditTarget => write!(output, "duplicate game world edit target"),
            Self::StaleRevision { expected, actual } => write!(
                output,
                "stale game world revision `{actual}`, current revision is `{expected}`"
            ),
            Self::StaleSession { expected, actual } => write!(
                output,
                "stale game world session `{actual}`, current session is `{expected}`"
            ),
            Self::StaleTier { expected, actual } => write!(
                output,
                "stale game world tier `{actual}`, current tier is `{expected}`"
            ),
            Self::InvalidEvaluation { reason } => {
                write!(output, "invalid paused game evaluation: {reason}")
            }
        }
    }
}

impl std::error::Error for JetGameWorldInspectorError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldPropertyFact {
    pub field: String,
    pub type_name: String,
    pub published_value: Option<String>,
    pub transient: bool,
    pub source_span: JetGameWorldSourceSpan,
}

impl JetGameWorldPropertyFact {
    pub fn new(
        field: impl Into<String>,
        type_name: impl Into<String>,
        published_value: Option<String>,
        transient: bool,
        source_span: JetGameWorldSourceSpan,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let fact = Self {
            field: field.into(),
            type_name: type_name.into(),
            published_value,
            transient,
            source_span,
        };
        fact.validate()?;
        Ok(fact)
    }

    pub fn is_authored(&self) -> bool {
        !self.transient
    }

    pub fn state(&self) -> &'static str {
        if self.transient {
            "transient"
        } else {
            "authored"
        }
    }

    fn validate(&self) -> Result<(), JetGameWorldInspectorError> {
        jet_game_world_validate_text(&self.field, "property field")?;
        jet_game_world_validate_text(&self.type_name, "property type")?;
        if let Some(value) = &self.published_value {
            jet_game_world_validate_text(value, "published value")?;
        }
        jet_game_world_validate_fact_span(self.source_span)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldComponentFact {
    pub component_id: String,
    pub type_name: String,
    pub authored_instance_id: String,
    pub properties: Vec<JetGameWorldPropertyFact>,
}

impl JetGameWorldComponentFact {
    pub fn new(
        component_id: impl Into<String>,
        type_name: impl Into<String>,
        authored_instance_id: impl Into<String>,
        properties: Vec<JetGameWorldPropertyFact>,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let fact = Self {
            component_id: component_id.into(),
            type_name: type_name.into(),
            authored_instance_id: authored_instance_id.into(),
            properties,
        };
        fact.validate()?;
        Ok(fact)
    }

    fn validate(&self) -> Result<(), JetGameWorldInspectorError> {
        jet_game_world_validate_text(&self.component_id, "component id")?;
        jet_game_world_validate_text(&self.type_name, "component type")?;
        jet_game_world_validate_text(&self.authored_instance_id, "authored instance id")?;
        if self.properties.len() > JET_GAME_WORLD_MAX_PROPERTIES_PER_COMPONENT {
            return Err(JetGameWorldInspectorError::TooManyProperties {
                component_id: self.component_id.clone(),
            });
        }
        let mut fields = std::collections::BTreeSet::new();
        for property in &self.properties {
            property.validate()?;
            if !fields.insert(property.field.clone()) {
                return Err(JetGameWorldInspectorError::DuplicateProperty {
                    component_id: self.component_id.clone(),
                    field: property.field.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldEntityFact {
    pub entity_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub components: Vec<JetGameWorldComponentFact>,
}

impl JetGameWorldEntityFact {
    pub fn new(
        entity_id: impl Into<String>,
        parent_id: Option<String>,
        name: impl Into<String>,
        components: Vec<JetGameWorldComponentFact>,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let fact = Self {
            entity_id: entity_id.into(),
            parent_id,
            name: name.into(),
            components,
        };
        fact.validate()?;
        Ok(fact)
    }

    fn validate(&self) -> Result<(), JetGameWorldInspectorError> {
        jet_game_world_validate_text(&self.entity_id, "entity id")?;
        jet_game_world_validate_text(&self.name, "entity name")?;
        if let Some(parent_id) = &self.parent_id {
            jet_game_world_validate_text(parent_id, "parent id")?;
        }
        if self.components.len() > JET_GAME_WORLD_MAX_COMPONENTS_PER_ENTITY {
            return Err(JetGameWorldInspectorError::TooManyComponents {
                entity_id: self.entity_id.clone(),
            });
        }
        let mut component_ids = std::collections::BTreeSet::new();
        for component in &self.components {
            component.validate()?;
            if !component_ids.insert(component.component_id.clone()) {
                return Err(JetGameWorldInspectorError::DuplicateId {
                    id: component.component_id.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldFact {
    pub world_id: String,
    pub frame_id: u64,
    pub source_id: String,
    pub revision: String,
    pub entities: Vec<JetGameWorldEntityFact>,
}

impl JetGameWorldFact {
    pub fn new(
        world_id: impl Into<String>,
        frame_id: u64,
        source_id: impl Into<String>,
        revision: impl Into<String>,
        entities: Vec<JetGameWorldEntityFact>,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let fact = Self {
            world_id: world_id.into(),
            frame_id,
            source_id: source_id.into(),
            revision: revision.into(),
            entities,
        };
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), JetGameWorldInspectorError> {
        jet_game_world_validate_text(&self.world_id, "world id")?;
        jet_game_world_validate_text(&self.source_id, "source id")?;
        jet_game_world_validate_text(&self.revision, "revision")?;
        if self.entities.len() > JET_GAME_WORLD_MAX_ENTITIES {
            return Err(JetGameWorldInspectorError::TooManyEntities);
        }

        let mut ids = std::collections::BTreeSet::new();
        let mut parents = std::collections::BTreeMap::new();
        for entity in &self.entities {
            entity.validate()?;
            if !ids.insert(entity.entity_id.clone()) {
                return Err(JetGameWorldInspectorError::DuplicateId {
                    id: entity.entity_id.clone(),
                });
            }
            parents.insert(entity.entity_id.clone(), entity.parent_id.clone());
            for component in &entity.components {
                if !ids.insert(component.component_id.clone()) {
                    return Err(JetGameWorldInspectorError::DuplicateId {
                        id: component.component_id.clone(),
                    });
                }
            }
        }

        for entity in &self.entities {
            let mut visited = std::collections::BTreeSet::new();
            let mut cursor = Some(entity.entity_id.as_str());
            while let Some(id) = cursor {
                if !visited.insert(id.to_string()) {
                    return Err(JetGameWorldInspectorError::Cycle { id: id.to_string() });
                }
                let Some(parent_id) = parents.get(id) else {
                    return Err(JetGameWorldInspectorError::MissingParent {
                        child_id: entity.entity_id.clone(),
                        parent_id: id.to_string(),
                    });
                };
                cursor = parent_id.as_deref();
            }
        }
        Ok(())
    }

    /// Canonical JSON is a payload representation only. Hosts still put it in
    /// the existing typed `jet.devtools.v1` event payload.
    pub fn render_json(&self) -> String {
        let mut entities = self.entities.iter().collect::<Vec<_>>();
        entities.sort_by(|left, right| left.entity_id.cmp(&right.entity_id));
        let entities_json = entities
            .iter()
            .map(|entity| jet_game_world_entity_json(entity))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"world_id\":{},\"frame_id\":{},\"source_id\":{},\"revision\":{},\"entities\":[{}]}}",
            jet_game_world_json_string(&self.world_id),
            self.frame_id,
            jet_game_world_json_string(&self.source_id),
            jet_game_world_json_string(&self.revision),
            entities_json,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldSelection {
    pub world_id: String,
    pub entity_id: Option<String>,
    pub component_id: Option<String>,
    pub property_id: Option<String>,
}

impl JetGameWorldSelection {
    pub fn new(
        world_id: impl Into<String>,
        entity_id: Option<String>,
        component_id: Option<String>,
        property_id: Option<String>,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let selection = Self {
            world_id: world_id.into(),
            entity_id,
            component_id,
            property_id,
        };
        jet_game_world_validate_text(&selection.world_id, "selection world id")?;
        jet_game_world_validate_optional_text(selection.entity_id.as_deref(), "selection entity id")?;
        jet_game_world_validate_optional_text(
            selection.component_id.as_deref(),
            "selection component id",
        )?;
        jet_game_world_validate_optional_text(
            selection.property_id.as_deref(),
            "selection property id",
        )?;
        if selection.property_id.is_some() && selection.component_id.is_none()
            || selection.component_id.is_some() && selection.entity_id.is_none()
        {
            return Err(JetGameWorldInspectorError::InvalidSelectionHierarchy);
        }
        Ok(selection)
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"world_id\":{},\"entity_id\":{},\"component_id\":{},\"property_id\":{}}}",
            jet_game_world_json_string(&self.world_id),
            jet_game_world_json_option(&self.entity_id),
            jet_game_world_json_option(&self.component_id),
            jet_game_world_json_option(&self.property_id),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldSelectionProjection {
    pub selection: JetGameWorldSelection,
    pub world_id: String,
    pub frame_id: u64,
    pub source_id: String,
    pub revision: String,
    pub entity: Option<JetGameWorldEntityFact>,
    pub component: Option<JetGameWorldComponentFact>,
    pub property: Option<JetGameWorldPropertyFact>,
}

impl JetGameWorldSelectionProjection {
    pub fn render_json(&self) -> String {
        format!(
            "{{\"selection\":{},\"world_id\":{},\"frame_id\":{},\"source_id\":{},\"revision\":{},\"entity\":{},\"component\":{},\"property\":{}}}",
            self.selection.render_json(),
            jet_game_world_json_string(&self.world_id),
            self.frame_id,
            jet_game_world_json_string(&self.source_id),
            jet_game_world_json_string(&self.revision),
            self.entity
                .as_ref()
                .map(jet_game_world_entity_json)
                .unwrap_or_else(|| "null".to_string()),
            self.component
                .as_ref()
                .map(jet_game_world_component_json)
                .unwrap_or_else(|| "null".to_string()),
            self.property
                .as_ref()
                .map(jet_game_world_property_json)
                .unwrap_or_else(|| "null".to_string()),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldEditTarget {
    pub source_id: String,
    pub revision: String,
    pub authored_instance_id: String,
    pub source_span: JetGameWorldSourceSpan,
    /// Optional session/tier provenance is filled by a live inspector. It is
    /// optional on the constructor so offline projections remain reusable.
    pub session_id: Option<String>,
    pub tier: Option<String>,
}

impl JetGameWorldEditTarget {
    pub fn new(
        source_id: impl Into<String>,
        revision: impl Into<String>,
        authored_instance_id: impl Into<String>,
        source_span: JetGameWorldSourceSpan,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let target = Self {
            source_id: source_id.into(),
            revision: revision.into(),
            authored_instance_id: authored_instance_id.into(),
            source_span,
            session_id: None,
            tier: None,
        };
        jet_game_world_validate_edit_target(&target)?;
        Ok(target)
    }

    pub fn with_session_identity(
        mut self,
        session_id: impl Into<String>,
        tier: impl Into<String>,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let session_id = session_id.into();
        let tier = tier.into();
        jet_game_world_validate_text(&session_id, "edit session id")?;
        jet_game_world_validate_text(&tier, "edit tier")?;
        self.session_id = Some(session_id);
        self.tier = Some(tier);
        Ok(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldFieldPatch {
    pub field: String,
    pub value: String,
}

impl JetGameWorldFieldPatch {
    pub fn new(
        field: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let patch = Self {
            field: field.into(),
            value: value.into(),
        };
        jet_game_world_validate_text(&patch.field, "edit field")?;
        jet_game_world_validate_text(&patch.value, "edit value")?;
        Ok(patch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldEditRequest {
    pub targets: Vec<JetGameWorldEditTarget>,
    pub field_patch: JetGameWorldFieldPatch,
}

impl JetGameWorldEditRequest {
    pub fn new(
        mut targets: Vec<JetGameWorldEditTarget>,
        field: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, JetGameWorldInspectorError> {
        targets.sort_by(|left, right| jet_game_world_edit_target_cmp(left, right));
        let request = Self {
            targets,
            field_patch: JetGameWorldFieldPatch::new(field, value)?,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), JetGameWorldInspectorError> {
        if self.targets.is_empty() {
            return Err(JetGameWorldInspectorError::InvalidEdit {
                reason: "targets must not be empty",
            });
        }
        jet_game_world_validate_text(&self.field_patch.field, "edit field")?;
        jet_game_world_validate_text(&self.field_patch.value, "edit value")?;
        for (index, target) in self.targets.iter().enumerate() {
            jet_game_world_validate_edit_target(target)?;
            if self.targets[index + 1..]
                .iter()
                .any(|other| other == target)
            {
                return Err(JetGameWorldInspectorError::DuplicateEditTarget);
            }
        }
        Ok(())
    }

    pub fn render_json(&self) -> String {
        let targets_json = self
            .targets
            .iter()
            .map(jet_game_world_edit_target_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"targets\":[{}],\"field_patch\":{}}}",
            targets_json,
            jet_game_world_field_patch_json(&self.field_patch),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGamePausedEvalRequest {
    pub request_id: String,
    pub session_id: String,
    pub tier: String,
    pub world_id: String,
    pub frame_id: u64,
    pub entity_id: String,
    pub component_id: String,
    pub expression: String,
    pub required_authority: String,
    pub budget: u64,
}

impl JetGamePausedEvalRequest {
    pub fn new(
        request_id: impl Into<String>,
        session_id: impl Into<String>,
        world_id: impl Into<String>,
        frame_id: u64,
        entity_id: impl Into<String>,
        component_id: impl Into<String>,
        expression: impl Into<String>,
        required_authority: impl Into<String>,
        budget: u64,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let request = Self {
            request_id: request_id.into(),
            session_id: session_id.into(),
            tier: "game".to_string(),
            world_id: world_id.into(),
            frame_id,
            entity_id: entity_id.into(),
            component_id: component_id.into(),
            expression: expression.into(),
            required_authority: required_authority.into(),
            budget,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn with_tier(
        mut self,
        tier: impl Into<String>,
    ) -> Result<Self, JetGameWorldInspectorError> {
        self.tier = tier.into();
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), JetGameWorldInspectorError> {
        jet_game_world_validate_text(&self.request_id, "evaluation request id")?;
        jet_game_world_validate_text(&self.session_id, "evaluation session id")?;
        jet_game_world_validate_text(&self.tier, "evaluation tier")?;
        jet_game_world_validate_text(&self.world_id, "evaluation world id")?;
        jet_game_world_validate_text(&self.entity_id, "evaluation entity id")?;
        jet_game_world_validate_text(&self.component_id, "evaluation component id")?;
        jet_game_world_validate_text(&self.expression, "evaluation expression")?;
        jet_game_world_validate_text(&self.required_authority, "evaluation authority")?;
        if self.budget == 0 {
            return Err(JetGameWorldInspectorError::InvalidEvaluation {
                reason: "budget must be positive",
            });
        }
        Ok(())
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"request_id\":{},\"session_id\":{},\"tier\":{},\"world_id\":{},\"frame_id\":{},\"entity_id\":{},\"component_id\":{},\"expression\":{},\"required_authority\":{},\"budget\":{}}}",
            jet_game_world_json_string(&self.request_id),
            jet_game_world_json_string(&self.session_id),
            jet_game_world_json_string(&self.tier),
            jet_game_world_json_string(&self.world_id),
            self.frame_id,
            jet_game_world_json_string(&self.entity_id),
            jet_game_world_json_string(&self.component_id),
            jet_game_world_json_string(&self.expression),
            jet_game_world_json_string(&self.required_authority),
            self.budget,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGamePausedEvalResult {
    pub request_id: String,
    pub status: String,
    pub type_name: String,
    pub published_value: Option<String>,
    pub source_span: Option<JetGameWorldSourceSpan>,
}

impl JetGamePausedEvalResult {
    pub fn new(
        request_id: impl Into<String>,
        status: impl Into<String>,
        type_name: impl Into<String>,
        published_value: Option<String>,
        source_span: Option<JetGameWorldSourceSpan>,
    ) -> Result<Self, JetGameWorldInspectorError> {
        let result = Self {
            request_id: request_id.into(),
            status: status.into(),
            type_name: type_name.into(),
            published_value,
            source_span,
        };
        jet_game_world_validate_text(&result.request_id, "evaluation result id")?;
        jet_game_world_validate_text(&result.status, "evaluation result status")?;
        jet_game_world_validate_optional_text(
            if result.type_name.is_empty() {
                None
            } else {
                Some(result.type_name.as_str())
            },
            "evaluation result type",
        )?;
        if let Some(value) = &result.published_value {
            jet_game_world_validate_text(value, "evaluation result value")?;
        }
        if let Some(span) = result.source_span {
            jet_game_world_validate_fact_span(span)?;
        }
        Ok(result)
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"request_id\":{},\"status\":{},\"type_name\":{},\"published_value\":{},\"source_span\":{}}}",
            jet_game_world_json_string(&self.request_id),
            jet_game_world_json_string(&self.status),
            jet_game_world_json_string(&self.type_name),
            jet_game_world_json_option(&self.published_value),
            self.source_span
                .map(|span| span.render_json())
                .unwrap_or_else(|| "null".to_string()),
        )
    }
}

pub const JET_GAME_PAUSED_EVAL_OK: &str = "ok";
pub const JET_GAME_PAUSED_EVAL_STALE: &str = "stale";
pub const JET_GAME_PAUSED_EVAL_RUNNING: &str = "running_frame_rejected";
pub const JET_GAME_PAUSED_EVAL_UNAUTHORIZED: &str = "authority_rejected";
pub const JET_GAME_PAUSED_EVAL_INVALID: &str = "invalid_request";
pub const JET_GAME_PAUSED_EVAL_UNSUPPORTED: &str = "unsupported_expression";
pub const JET_GAME_PAUSED_EVAL_NO_WORLD: &str = "no_world";
pub const JET_GAME_PAUSED_EVAL_BUDGET: &str = "budget_exceeded";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameWorldInspector {
    world: Option<JetGameWorldFact>,
    frames: Vec<JetGameWorldFact>,
    selection: Option<JetGameWorldSelection>,
    session_id: Option<String>,
    tier: Option<String>,
}

impl JetGameWorldInspector {
    pub fn new() -> Result<Self, JetGameWorldInspectorError> {
        Ok(Self::default())
    }

    /// Bind requests to one live devtools session and runtime tier. The
    /// identity is optional for offline snapshots, but once configured every
    /// edit/evaluation request must carry the same pair.
    pub fn set_session_identity(
        &mut self,
        session_id: impl Into<String>,
        tier: impl Into<String>,
    ) {
        self.session_id = Some(session_id.into());
        self.tier = Some(tier.into());
    }

    pub fn session_identity(&self) -> Option<(&str, &str)> {
        match (self.session_id.as_deref(), self.tier.as_deref()) {
            (Some(session_id), Some(tier)) => Some((session_id, tier)),
            _ => None,
        }
    }

    /// Replace the current paused snapshot atomically. A new snapshot clears
    /// the old selection so a source revision can never silently reuse it.
    pub fn ingest(&mut self, world: JetGameWorldFact) -> Result<(), JetGameWorldInspectorError> {
        world.validate()?;
        if self.frames.len() >= JET_GAME_WORLD_MAX_FRAME_FACTS {
            self.frames.remove(0);
        }
        self.frames.push(world.clone());
        self.world = Some(world);
        self.selection = None;
        Ok(())
    }

    pub fn select(
        &mut self,
        selection: JetGameWorldSelection,
    ) -> Result<(), JetGameWorldInspectorError> {
        let world = self
            .world
            .as_ref()
            .ok_or(JetGameWorldInspectorError::NoWorld)?;
        if selection.world_id != world.world_id {
            return Err(JetGameWorldInspectorError::MismatchedWorld {
                expected: world.world_id.clone(),
                actual: selection.world_id,
            });
        }
        if selection.property_id.is_some() && selection.component_id.is_none()
            || selection.component_id.is_some() && selection.entity_id.is_none()
        {
            return Err(JetGameWorldInspectorError::InvalidSelectionHierarchy);
        }
        if let Some(entity_id) = &selection.entity_id {
            let entity = world
                .entities
                .iter()
                .find(|entity| entity.entity_id == *entity_id)
                .ok_or_else(|| JetGameWorldInspectorError::UnknownEntity {
                    id: entity_id.clone(),
                })?;
            if let Some(component_id) = &selection.component_id {
                let component = entity
                    .components
                    .iter()
                    .find(|component| component.component_id == *component_id)
                    .ok_or_else(|| JetGameWorldInspectorError::UnknownComponent {
                        id: component_id.clone(),
                    })?;
                if let Some(property_id) = &selection.property_id {
                    if !component
                        .properties
                        .iter()
                        .any(|property| property.field == *property_id)
                    {
                        return Err(JetGameWorldInspectorError::UnknownProperty {
                            id: property_id.clone(),
                        });
                    }
                }
            }
        }
        self.selection = Some(selection);
        Ok(())
    }

    pub fn project(&self) -> Option<JetGameWorldSelectionProjection> {
        let world = self.world.as_ref()?;
        let selection = self.selection.as_ref()?.clone();
        if selection.world_id != world.world_id {
            return None;
        }
        let entity = selection.entity_id.as_ref().and_then(|entity_id| {
            world
                .entities
                .iter()
                .find(|entity| entity.entity_id == *entity_id)
        });
        if selection.entity_id.is_some() && entity.is_none() {
            return None;
        }
        let component = match (entity, selection.component_id.as_ref()) {
            (Some(entity), Some(component_id)) => entity
                .components
                .iter()
                .find(|component| component.component_id == *component_id),
            _ => None,
        };
        if selection.component_id.is_some() && component.is_none() {
            return None;
        }
        let property = match (component, selection.property_id.as_ref()) {
            (Some(component), Some(property_id)) => component
                .properties
                .iter()
                .find(|property| property.field == *property_id),
            _ => None,
        };
        if selection.property_id.is_some() && property.is_none() {
            return None;
        }
        Some(JetGameWorldSelectionProjection {
            selection,
            world_id: world.world_id.clone(),
            frame_id: world.frame_id,
            source_id: world.source_id.clone(),
            revision: world.revision.clone(),
            entity: entity.cloned(),
            component: component.cloned(),
            property: property.cloned(),
        })
    }

    /// Create one checked source transaction request from the selected,
    /// authored property. This only copies provenance; it never changes the
    /// snapshot or keeps a sidecar edit value.
    pub fn edit_request(
        &self,
        field: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<JetGameWorldEditRequest, JetGameWorldInspectorError> {
        let field = field.into();
        let value = value.into();
        let world = self
            .world
            .as_ref()
            .ok_or(JetGameWorldInspectorError::NoWorld)?;
        let selection = self
            .selection
            .as_ref()
            .ok_or(JetGameWorldInspectorError::NoSelection)?;
        let entity_id = selection
            .entity_id
            .as_ref()
            .ok_or(JetGameWorldInspectorError::InvalidSelectionHierarchy)?;
        let component_id = selection
            .component_id
            .as_ref()
            .ok_or(JetGameWorldInspectorError::InvalidSelectionHierarchy)?;
        let property_id = selection
            .property_id
            .as_ref()
            .ok_or(JetGameWorldInspectorError::InvalidSelectionHierarchy)?;
        let entity = world
            .entities
            .iter()
            .find(|entity| entity.entity_id == *entity_id)
            .ok_or_else(|| JetGameWorldInspectorError::UnknownEntity {
                id: entity_id.clone(),
            })?;
        let component = entity
            .components
            .iter()
            .find(|component| component.component_id == *component_id)
            .ok_or_else(|| JetGameWorldInspectorError::UnknownComponent {
                id: component_id.clone(),
            })?;
        let property = component
            .properties
            .iter()
            .find(|property| property.field == *property_id)
            .ok_or_else(|| JetGameWorldInspectorError::UnknownProperty {
                id: property_id.clone(),
            })?;
        if property.field != field || property.transient {
            return Err(JetGameWorldInspectorError::NotEditable { field });
        }
        if property.source_span.is_empty() {
            return Err(JetGameWorldInspectorError::InvalidSourceSpan);
        }
        let mut target = JetGameWorldEditTarget::new(
            world.source_id.clone(),
            world.revision.clone(),
            component.authored_instance_id.clone(),
            property.source_span,
        )?;
        if let Some((session_id, tier)) = self.session_identity() {
            target = target.with_session_identity(session_id, tier)?;
        }
        JetGameWorldEditRequest::new(vec![target], field, value)
    }

    /// Validate an edit request against the current source snapshot without
    /// applying it. Stale revisions fail closed before a Canvas transaction.
    pub fn validate_edit_request(
        &self,
        request: &JetGameWorldEditRequest,
    ) -> Result<(), JetGameWorldInspectorError> {
        request.validate()?;
        let world = self
            .world
            .as_ref()
            .ok_or(JetGameWorldInspectorError::NoWorld)?;
        for target in &request.targets {
            if target.source_id != world.source_id {
                return Err(JetGameWorldInspectorError::InvalidEdit {
                    reason: "target source is not the current world source",
                });
            }
            if target.revision != world.revision {
                return Err(JetGameWorldInspectorError::StaleRevision {
                    expected: world.revision.clone(),
                    actual: target.revision.clone(),
                });
            }
            if let Some((expected_session, expected_tier)) = self.session_identity() {
                if target.session_id.as_deref() != Some(expected_session) {
                    return Err(JetGameWorldInspectorError::StaleSession {
                        expected: expected_session.to_string(),
                        actual: target.session_id.clone().unwrap_or_default(),
                    });
                }
                if target.tier.as_deref() != Some(expected_tier) {
                    return Err(JetGameWorldInspectorError::StaleTier {
                        expected: expected_tier.to_string(),
                        actual: target.tier.clone().unwrap_or_default(),
                    });
                }
            }
            let provenance_matches = world.entities.iter().any(|entity| {
                entity.components.iter().any(|component| {
                    component.authored_instance_id == target.authored_instance_id
                        && component.properties.iter().any(|property| {
                            property.source_span == target.source_span
                                && property.field == request.field_patch.field
                                && !property.transient
                        })
                })
            });
            if !provenance_matches {
                return Err(JetGameWorldInspectorError::InvalidEdit {
                    reason: "target provenance does not name an authored property",
                });
            }
        }
        Ok(())
    }

    /// Evaluate only a published property from the exact paused frame fact.
    /// Running/live authorities are rejected before lookup; no expression can
    /// execute arbitrary code or mutate state here.
    pub fn evaluate_request(
        &self,
        request: JetGamePausedEvalRequest,
    ) -> JetGamePausedEvalResult {
        let request_id = request.request_id.clone();
        let invalid = |status: &str| {
            jet_game_world_eval_result(request_id.clone(), status, "", None, None)
        };
        if request.validate().is_err() {
            return invalid(JET_GAME_PAUSED_EVAL_INVALID);
        }
        if jet_game_world_authority_is_running(&request.required_authority) {
            return invalid(JET_GAME_PAUSED_EVAL_RUNNING);
        }
        if !jet_game_world_authority_is_paused(&request.required_authority) {
            return invalid(JET_GAME_PAUSED_EVAL_UNAUTHORIZED);
        }
        if let Some((expected_session, expected_tier)) = self.session_identity() {
            if request.session_id != expected_session {
                return invalid(JET_GAME_PAUSED_EVAL_STALE);
            }
            if request.tier != expected_tier {
                return invalid(JET_GAME_PAUSED_EVAL_STALE);
            }
        }
        let Some(current) = self.world.as_ref() else {
            return invalid(JET_GAME_PAUSED_EVAL_NO_WORLD);
        };
        let Some(frame) = self
            .frames
            .iter()
            .rev()
            .find(|frame| frame.world_id == request.world_id && frame.frame_id == request.frame_id)
        else {
            return invalid(JET_GAME_PAUSED_EVAL_STALE);
        };
        // The revision is resolved from the exact frame fact. A historical
        // frame is not evaluable after the current source revision changes.
        if frame.source_id != current.source_id || frame.revision != current.revision {
            return invalid(JET_GAME_PAUSED_EVAL_STALE);
        }
        let Some(entity) = frame
            .entities
            .iter()
            .find(|entity| entity.entity_id == request.entity_id)
        else {
            return invalid(JET_GAME_PAUSED_EVAL_UNSUPPORTED);
        };
        let Some(component) = entity
            .components
            .iter()
            .find(|component| component.component_id == request.component_id)
        else {
            return invalid(JET_GAME_PAUSED_EVAL_UNSUPPORTED);
        };
        if request.expression.len() as u64 > request.budget {
            return invalid(JET_GAME_PAUSED_EVAL_BUDGET);
        }
        let Some(property) = component
            .properties
            .iter()
            .find(|property| property.field == request.expression)
        else {
            return invalid(JET_GAME_PAUSED_EVAL_UNSUPPORTED);
        };
        jet_game_world_eval_result(
            request.request_id,
            JET_GAME_PAUSED_EVAL_OK,
            &property.type_name,
            property.published_value.clone(),
            Some(property.source_span),
        )
    }
}

impl Default for JetGameWorldInspector {
    fn default() -> Self {
        Self {
            world: None,
            frames: Vec::new(),
            selection: None,
            session_id: None,
            tier: None,
        }
    }
}

fn jet_game_world_eval_result(
    request_id: String,
    status: &str,
    type_name: &str,
    published_value: Option<String>,
    source_span: Option<JetGameWorldSourceSpan>,
) -> JetGamePausedEvalResult {
    JetGamePausedEvalResult {
        request_id,
        status: status.to_string(),
        type_name: type_name.to_string(),
        published_value,
        source_span,
    }
}

fn jet_game_world_validate_text(
    value: &str,
    field: &'static str,
) -> Result<(), JetGameWorldInspectorError> {
    if value.trim().is_empty() {
        return Err(JetGameWorldInspectorError::EmptyText { field });
    }
    if value.len() > JET_GAME_WORLD_MAX_TEXT_BYTES {
        return Err(JetGameWorldInspectorError::TextTooLong { field });
    }
    Ok(())
}

fn jet_game_world_validate_optional_text(
    value: Option<&str>,
    field: &'static str,
) -> Result<(), JetGameWorldInspectorError> {
    if let Some(value) = value {
        jet_game_world_validate_text(value, field)?;
    }
    Ok(())
}

fn jet_game_world_validate_fact_span(
    span: JetGameWorldSourceSpan,
) -> Result<(), JetGameWorldInspectorError> {
    if span.start > span.end {
        return Err(JetGameWorldInspectorError::InvalidSourceSpan);
    }
    Ok(())
}

fn jet_game_world_validate_edit_target(
    target: &JetGameWorldEditTarget,
) -> Result<(), JetGameWorldInspectorError> {
    jet_game_world_validate_text(&target.source_id, "edit source id")?;
    jet_game_world_validate_text(&target.revision, "edit revision")?;
    jet_game_world_validate_text(&target.authored_instance_id, "authored instance id")?;
    if let Some(session_id) = &target.session_id {
        jet_game_world_validate_text(session_id, "edit session id")?;
    }
    if let Some(tier) = &target.tier {
        jet_game_world_validate_text(tier, "edit tier")?;
    }
    if target.session_id.is_some() != target.tier.is_some() {
        return Err(JetGameWorldInspectorError::InvalidEdit {
            reason: "edit session and tier must be supplied together",
        });
    }
    if target.source_span.is_empty() {
        return Err(JetGameWorldInspectorError::InvalidSourceSpan);
    }
    Ok(())
}

fn jet_game_world_edit_target_cmp(
    left: &JetGameWorldEditTarget,
    right: &JetGameWorldEditTarget,
) -> std::cmp::Ordering {
    left.source_id
        .cmp(&right.source_id)
        .then_with(|| left.revision.cmp(&right.revision))
        .then_with(|| left.authored_instance_id.cmp(&right.authored_instance_id))
        .then_with(|| left.source_span.cmp(&right.source_span))
        .then_with(|| left.session_id.cmp(&right.session_id))
        .then_with(|| left.tier.cmp(&right.tier))
}


fn jet_game_world_authority_is_running(authority: &str) -> bool {
    authority.eq_ignore_ascii_case("running")
        || authority.eq_ignore_ascii_case("running-frame")
        || authority.eq_ignore_ascii_case("running_frame")
        || authority.eq_ignore_ascii_case("live")
        || authority.eq_ignore_ascii_case("live-frame")
        || authority.eq_ignore_ascii_case("live_frame")
}

fn jet_game_world_authority_is_paused(authority: &str) -> bool {
    authority.eq_ignore_ascii_case("paused")
        || authority.eq_ignore_ascii_case("paused-frame")
        || authority.eq_ignore_ascii_case("paused_frame")
        || authority.eq_ignore_ascii_case("debug.paused")
        || authority.eq_ignore_ascii_case("game.debug.paused")
}

fn jet_game_world_entity_json(entity: &JetGameWorldEntityFact) -> String {
    let mut components = entity.components.iter().collect::<Vec<_>>();
    components.sort_by(|left, right| left.component_id.cmp(&right.component_id));
    let components_json = components
        .iter()
        .map(|component| jet_game_world_component_json(component))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"entity_id\":{},\"parent_id\":{},\"name\":{},\"components\":[{}]}}",
        jet_game_world_json_string(&entity.entity_id),
        jet_game_world_json_option(&entity.parent_id),
        jet_game_world_json_string(&entity.name),
        components_json,
    )
}

fn jet_game_world_component_json(component: &JetGameWorldComponentFact) -> String {
    let mut properties = component.properties.iter().collect::<Vec<_>>();
    properties.sort_by(|left, right| left.field.cmp(&right.field));
    let properties_json = properties
        .iter()
        .map(|property| jet_game_world_property_json(property))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"component_id\":{},\"type_name\":{},\"authored_instance_id\":{},\"properties\":[{}]}}",
        jet_game_world_json_string(&component.component_id),
        jet_game_world_json_string(&component.type_name),
        jet_game_world_json_string(&component.authored_instance_id),
        properties_json,
    )
}

fn jet_game_world_property_json(property: &JetGameWorldPropertyFact) -> String {
    format!(
        "{{\"field\":{},\"type_name\":{},\"published_value\":{},\"transient\":{},\"source_span\":{}}}",
        jet_game_world_json_string(&property.field),
        jet_game_world_json_string(&property.type_name),
        jet_game_world_json_option(&property.published_value),
        property.transient,
        property.source_span.render_json(),
    )
}

fn jet_game_world_edit_target_json(target: &JetGameWorldEditTarget) -> String {
    format!(
        "{{\"source_id\":{},\"revision\":{},\"authored_instance_id\":{},\"source_span\":{},\"session_id\":{},\"tier\":{}}}",
        jet_game_world_json_string(&target.source_id),
        jet_game_world_json_string(&target.revision),
        jet_game_world_json_string(&target.authored_instance_id),
        target.source_span.render_json(),
        jet_game_world_json_option(&target.session_id),
        jet_game_world_json_option(&target.tier),
    )
}

fn jet_game_world_field_patch_json(patch: &JetGameWorldFieldPatch) -> String {
    format!(
        "{{\"field\":{},\"value\":{}}}",
        jet_game_world_json_string(&patch.field),
        jet_game_world_json_string(&patch.value),
    )
}

fn jet_game_world_json_option(value: &Option<String>) -> String {
    value
        .as_deref()
        .map(jet_game_world_json_string)
        .unwrap_or_else(|| "null".to_string())
}

fn jet_game_world_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len().saturating_add(2));
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character < '\u{20}' => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}
