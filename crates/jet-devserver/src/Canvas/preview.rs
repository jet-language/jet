//! D-DX-PREVIEW1=A: typed, source-identified preview/playground state for Canvas.
//!
//! This module owns only the bounded host state.  It does not parse source,
//! evaluate a second UI tree, or invent a renderer.  A caller supplies the
//! checked callback and the normal Canvas renderer through `evaluate` (or
//! `publish` when the renderer is driven by the dev session).  The same
//! `CanvasPreviewSession<T>` can therefore carry the canonical `UiNode` type
//! without making this crate depend on the code generator.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant};

use jet_foundation::Devtools::{
    JetDevtoolsFreshnessState, JetDevtoolsSourceIdentityFact, JetDevtoolsSourceSpan,
};

pub const CANVAS_PREVIEW_SCHEMA_VERSION: u32 = 1;
pub const CANVAS_PREVIEW_MAX_PREVIEWS: usize = 64;
pub const CANVAS_PREVIEW_MAX_SELECTED: usize = 16;
pub const CANVAS_PREVIEW_MAX_CONTEXTS: usize = 16;
pub const CANVAS_PREVIEW_MAX_INPUTS: usize = 32;
pub const CANVAS_PREVIEW_MAX_INPUT_BYTES: usize = 16 * 1024;
pub const CANVAS_PREVIEW_MAX_TEXT_BYTES: usize = 16 * 1024;
pub const CANVAS_PREVIEW_MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;
pub const CANVAS_PREVIEW_MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub const CANVAS_PREVIEW_MAX_RESIDENT_BYTES: usize = 16 * 1024 * 1024;
pub const CANVAS_PREVIEW_MAX_CLIENTS: usize = 16;
pub const CANVAS_PREVIEW_CLIENT_IDLE_SECS: u64 = 60;
pub const CANVAS_PREVIEW_MAX_DIMENSION: u32 = 16_384;
pub const CANVAS_PREVIEW_MAX_SCALE: u16 = 8;

/// Foundation's source span is the one source-location vocabulary used by
/// Canvas, devtools, and the checked front end.
pub type CanvasPreviewSourceSpan = JetDevtoolsSourceSpan;

/// A preview identity is complete by construction: source, build, revision,
/// and the declaration span travel together.  A stale render can never be
/// presented as the current revision because the session compares this value
/// before accepting a frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasPreviewSourceIdentity {
    pub source_id: String,
    pub build_id: String,
    pub revision: String,
    pub span: CanvasPreviewSourceSpan,
}

impl CanvasPreviewSourceIdentity {
    pub fn new(
        source_id: impl Into<String>,
        build_id: impl Into<String>,
        revision: impl Into<String>,
        span: CanvasPreviewSourceSpan,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            build_id: build_id.into(),
            revision: revision.into(),
            span,
        }
    }

    pub fn validate(&self) -> Result<(), CanvasPreviewError> {
        preview_text(&self.source_id, "source id", CANVAS_PREVIEW_MAX_TEXT_BYTES)?;
        preview_text(&self.build_id, "build id", CANVAS_PREVIEW_MAX_TEXT_BYTES)?;
        preview_text(&self.revision, "revision", CANVAS_PREVIEW_MAX_TEXT_BYTES)?;
        self.span
            .validate()
            .map_err(|_| CanvasPreviewError::InvalidSource("source span is invalid"))?;
        if self.span.source_id != self.source_id {
            return Err(CanvasPreviewError::InvalidSource(
                "source span identity does not match preview source",
            ));
        }
        Ok(())
    }

    pub fn as_devtools_fact(&self) -> JetDevtoolsSourceIdentityFact {
        JetDevtoolsSourceIdentityFact::new(
            Some(self.source_id.clone()),
            Some(self.build_id.clone()),
            Some(self.revision.clone()),
            None,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanvasPreviewKind {
    Preview,
    Playground,
}

impl CanvasPreviewKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Playground => "playground",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CanvasPreviewDevice {
    Phone,
    Tablet,
    Desktop,
    Custom(String),
}

impl CanvasPreviewDevice {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Phone => "phone",
            Self::Tablet => "tablet",
            Self::Desktop => "desktop",
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasPreviewViewport {
    pub device: CanvasPreviewDevice,
    pub width: u32,
    pub height: u32,
    pub scale: u16,
}

impl CanvasPreviewViewport {
    pub fn new(
        device: CanvasPreviewDevice,
        width: u32,
        height: u32,
        scale: u16,
    ) -> Result<Self, CanvasPreviewError> {
        let viewport = Self {
            device,
            width,
            height,
            scale,
        };
        viewport.validate()?;
        Ok(viewport)
    }

    pub fn phone() -> Self {
        Self {
            device: CanvasPreviewDevice::Phone,
            width: 390,
            height: 844,
            scale: 2,
        }
    }

    pub fn tablet() -> Self {
        Self {
            device: CanvasPreviewDevice::Tablet,
            width: 834,
            height: 1_119,
            scale: 2,
        }
    }

    pub fn desktop() -> Self {
        Self {
            device: CanvasPreviewDevice::Desktop,
            width: 1_280,
            height: 800,
            scale: 1,
        }
    }

    pub fn validate(&self) -> Result<(), CanvasPreviewError> {
        if self.width == 0 || self.height == 0 {
            return Err(CanvasPreviewError::InvalidViewport(
                "viewport dimensions must be positive",
            ));
        }
        if self.width > CANVAS_PREVIEW_MAX_DIMENSION
            || self.height > CANVAS_PREVIEW_MAX_DIMENSION
        {
            return Err(CanvasPreviewError::InvalidViewport(
                "viewport dimension exceeds the preview bound",
            ));
        }
        if self.scale == 0 || self.scale > CANVAS_PREVIEW_MAX_SCALE {
            return Err(CanvasPreviewError::InvalidViewport(
                "viewport scale is outside the preview bound",
            ));
        }
        if let CanvasPreviewDevice::Custom(name) = &self.device {
            preview_text(name, "viewport device", 128)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CanvasPreviewTheme {
    System,
    Light,
    Dark,
    HighContrast,
    Custom(String),
}

impl CanvasPreviewTheme {
    pub fn as_str(&self) -> &str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
            Self::HighContrast => "high-contrast",
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CanvasPreviewAccessibility {
    Default,
    ReducedMotion,
    LargeText,
    HighContrast,
    ScreenReader,
    Custom(String),
}

impl CanvasPreviewAccessibility {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Default => "default",
            Self::ReducedMotion => "reduced-motion",
            Self::LargeText => "large-text",
            Self::HighContrast => "high-contrast",
            Self::ScreenReader => "screen-reader",
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CanvasPreviewLifecycle {
    Initial,
    Loading,
    Loaded,
    Empty,
    Error,
    Offline,
    Custom(String),
}

impl CanvasPreviewLifecycle {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Initial => "initial",
            Self::Loading => "loading",
            Self::Loaded => "loaded",
            Self::Empty => "empty",
            Self::Error => "error",
            Self::Offline => "offline",
            Self::Custom(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasPreviewTraits {
    pub theme: CanvasPreviewTheme,
    pub locale: String,
    pub accessibility: CanvasPreviewAccessibility,
    pub lifecycle: CanvasPreviewLifecycle,
}

impl Default for CanvasPreviewTraits {
    fn default() -> Self {
        Self {
            theme: CanvasPreviewTheme::System,
            locale: "en-US".to_string(),
            accessibility: CanvasPreviewAccessibility::Default,
            lifecycle: CanvasPreviewLifecycle::Initial,
        }
    }
}

impl CanvasPreviewTraits {
    pub fn validate(&self) -> Result<(), CanvasPreviewError> {
        preview_text(&self.locale, "locale", 128)?;
        for (value, field) in [
            (self.theme.as_str(), "theme"),
            (self.accessibility.as_str(), "accessibility"),
            (self.lifecycle.as_str(), "lifecycle"),
        ] {
            preview_text(value, field, 128)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CanvasPreviewEffect {
    Read(String),
    Write(String),
    Network(String),
    Clock,
    Custom(String),
}

impl CanvasPreviewEffect {
    pub fn capability(&self) -> &str {
        match self {
            Self::Read(value) => value,
            Self::Write(value) => value,
            Self::Network(value) => value,
            Self::Clock => "Clock.Read",
            Self::Custom(value) => value,
        }
    }

    pub fn validate(&self) -> Result<(), CanvasPreviewError> {
        preview_text(self.capability(), "preview effect", 256)
    }
}

/// Authority is explicit and attenuating.  A preview starts loopback-only
/// with no capabilities; declaring an effect does not grant it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasPreviewAuthority {
    loopback_only: bool,
    capabilities: BTreeSet<String>,
}

impl CanvasPreviewAuthority {
    pub fn development() -> Self {
        Self {
            loopback_only: true,
            capabilities: BTreeSet::new(),
        }
    }

    pub fn new(loopback_only: bool) -> Self {
        Self {
            loopback_only,
            capabilities: BTreeSet::new(),
        }
    }

    pub fn loopback_only(&self) -> bool {
        self.loopback_only
    }

    pub fn capabilities(&self) -> impl Iterator<Item = &str> {
        self.capabilities.iter().map(String::as_str)
    }

    pub fn with_capability(mut self, capability: impl Into<String>) -> Result<Self, CanvasPreviewError> {
        let capability = capability.into();
        preview_text(&capability, "authority capability", 256)?;
        self.capabilities.insert(capability);
        Ok(self)
    }

    pub fn allows_capability(&self, requested: &str) -> bool {
        self.capabilities.iter().any(|held| {
            held == requested
                || requested
                    .strip_prefix(held)
                    .is_some_and(|tail| tail.starts_with('.') || tail.starts_with(':') || tail.starts_with('/'))
        })
    }

    pub fn allows_effect(&self, effect: &CanvasPreviewEffect, loopback: bool) -> bool {
        (!self.loopback_only || loopback) && self.allows_capability(effect.capability())
    }

    pub fn attenuate<I>(&self, capabilities: I) -> Result<Self, CanvasPreviewError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut narrowed = Self::new(self.loopback_only);
        for capability in capabilities {
            preview_text(&capability, "authority capability", 256)?;
            if !self.allows_capability(&capability) {
                return Err(CanvasPreviewError::CapabilityDenied(capability));
            }
            narrowed.capabilities.insert(capability);
        }
        Ok(narrowed)
    }

    fn covers(&self, other: &Self) -> bool {
        (!self.loopback_only || other.loopback_only)
            && other
                .capabilities
                .iter()
                .all(|capability| self.allows_capability(capability))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CanvasPreviewInput {
    Text(String),
    Bool(bool),
    Integer(i64),
    Float(f64),
}

impl CanvasPreviewInput {
    pub fn validate(&self) -> Result<(), CanvasPreviewError> {
        match self {
            Self::Text(value) => preview_text(value, "preview input", CANVAS_PREVIEW_MAX_INPUT_BYTES),
            Self::Float(value) if !value.is_finite() => Err(CanvasPreviewError::InvalidInput(
                "preview float input must be finite",
            )),
            _ => Ok(()),
        }
    }

    fn encoded_len(&self) -> usize {
        match self {
            Self::Text(value) => value.len(),
            Self::Bool(_) => 1,
            Self::Integer(_) => 8,
            Self::Float(_) => 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasPreviewInputOverride {
    pub name: String,
    pub value: CanvasPreviewInput,
}

impl CanvasPreviewInputOverride {
    pub fn new(name: impl Into<String>, value: CanvasPreviewInput) -> Result<Self, CanvasPreviewError> {
        let input = Self {
            name: name.into(),
            value,
        };
        input.validate()?;
        Ok(input)
    }

    fn validate(&self) -> Result<(), CanvasPreviewError> {
        preview_text(&self.name, "preview input name", 256)?;
        self.value.validate()
    }
}

/// One immutable, explicitly named fixture context.  Variants may refer to
/// the same context id, but each render receives read-only cloned facts from
/// this value; mutable state never becomes ambient shared preview state.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasPreviewContext {
    pub id: String,
    pub source: CanvasPreviewSourceIdentity,
    values: BTreeMap<String, CanvasPreviewInput>,
    resident_bytes: usize,
}

impl CanvasPreviewContext {
    pub fn new<I>(
        id: impl Into<String>,
        source: CanvasPreviewSourceIdentity,
        values: I,
    ) -> Result<Self, CanvasPreviewError>
    where
        I: IntoIterator<Item = CanvasPreviewInputOverride>,
    {
        let id = id.into();
        preview_text(&id, "preview context id", 256)?;
        source.validate()?;
        let mut context = Self {
            id,
            source,
            values: BTreeMap::new(),
            resident_bytes: 0,
        };
        for value in values {
            value.validate()?;
            if context.values.len() >= CANVAS_PREVIEW_MAX_INPUTS {
                return Err(CanvasPreviewError::TooManyInputs);
            }
            if context.values.contains_key(&value.name) {
                return Err(CanvasPreviewError::DuplicateInput(value.name));
            }
            context.resident_bytes = context
                .resident_bytes
                .saturating_add(value.name.len())
                .saturating_add(value.value.encoded_len());
            if context.resident_bytes > CANVAS_PREVIEW_MAX_INPUT_BYTES {
                return Err(CanvasPreviewError::InputBudgetExceeded);
            }
            context.values.insert(value.name, value.value);
        }
        Ok(context)
    }

    pub fn values(&self) -> &BTreeMap<String, CanvasPreviewInput> {
        &self.values
    }

    pub fn resident_bytes(&self) -> usize {
        self.resident_bytes
    }
}

/// A checked callback identity.  The callback body itself stays in the
/// source/compiler runtime; Canvas only carries the identity for diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasPreviewCallback {
    pub id: String,
}

impl CanvasPreviewCallback {
    pub fn checked(id: impl Into<String>) -> Result<Self, CanvasPreviewError> {
        let id = id.into();
        preview_text(&id, "preview callback id", 256)?;
        Ok(Self { id })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasPreviewDescriptor {
    kind: CanvasPreviewKind,
    name: String,
    callback: CanvasPreviewCallback,
    source: CanvasPreviewSourceIdentity,
    viewport: CanvasPreviewViewport,
    traits: CanvasPreviewTraits,
    effects: BTreeSet<CanvasPreviewEffect>,
    authority: CanvasPreviewAuthority,
    inputs: BTreeMap<String, CanvasPreviewInput>,
    context_id: Option<String>,
}

impl CanvasPreviewDescriptor {
    pub fn new(
        kind: CanvasPreviewKind,
        name: impl Into<String>,
        callback: CanvasPreviewCallback,
        source: CanvasPreviewSourceIdentity,
    ) -> Self {
        Self {
            kind,
            name: name.into(),
            callback,
            source,
            viewport: CanvasPreviewViewport::desktop(),
            traits: CanvasPreviewTraits::default(),
            effects: BTreeSet::new(),
            authority: CanvasPreviewAuthority::development(),
            inputs: BTreeMap::new(),
            context_id: None,
        }
    }

    pub fn preview(
        name: impl Into<String>,
        callback: CanvasPreviewCallback,
        source: CanvasPreviewSourceIdentity,
    ) -> Self {
        Self::new(CanvasPreviewKind::Preview, name, callback, source)
    }

    pub fn playground(
        name: impl Into<String>,
        callback: CanvasPreviewCallback,
        source: CanvasPreviewSourceIdentity,
    ) -> Self {
        Self::new(CanvasPreviewKind::Playground, name, callback, source)
    }

    pub fn kind(&self) -> CanvasPreviewKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn callback(&self) -> &CanvasPreviewCallback {
        &self.callback
    }

    pub fn source(&self) -> &CanvasPreviewSourceIdentity {
        &self.source
    }

    pub fn viewport(&self) -> &CanvasPreviewViewport {
        &self.viewport
    }

    pub fn traits(&self) -> &CanvasPreviewTraits {
        &self.traits
    }

    pub fn effects(&self) -> impl Iterator<Item = &CanvasPreviewEffect> {
        self.effects.iter()
    }

    pub fn authority(&self) -> &CanvasPreviewAuthority {
        &self.authority
    }

    pub fn inputs(&self) -> &BTreeMap<String, CanvasPreviewInput> {
        &self.inputs
    }

    pub fn context_id(&self) -> Option<&str> {
        self.context_id.as_deref()
    }

    pub fn set_viewport(&mut self, viewport: CanvasPreviewViewport) -> Result<(), CanvasPreviewError> {
        viewport.validate()?;
        self.viewport = viewport;
        Ok(())
    }

    pub fn set_traits(&mut self, traits: CanvasPreviewTraits) -> Result<(), CanvasPreviewError> {
        traits.validate()?;
        self.traits = traits;
        Ok(())
    }

    pub fn set_authority(&mut self, authority: CanvasPreviewAuthority) {
        self.authority = authority;
    }

    pub fn add_effect(&mut self, effect: CanvasPreviewEffect) -> Result<(), CanvasPreviewError> {
        effect.validate()?;
        self.effects.insert(effect);
        Ok(())
    }

    pub fn set_input(&mut self, input: CanvasPreviewInputOverride) -> Result<(), CanvasPreviewError> {
        input.validate()?;
        if self.inputs.len() >= CANVAS_PREVIEW_MAX_INPUTS && !self.inputs.contains_key(&input.name) {
            return Err(CanvasPreviewError::TooManyInputs);
        }
        self.inputs.insert(input.name, input.value);
        Ok(())
    }

    pub fn set_context_id(&mut self, context_id: Option<String>) -> Result<(), CanvasPreviewError> {
        if let Some(context_id) = &context_id {
            preview_text(context_id, "preview context id", 256)?;
        }
        self.context_id = context_id;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), CanvasPreviewError> {
        preview_text(&self.name, "preview name", 256)?;
        self.callback.checked_id()?;
        self.source.validate()?;
        self.viewport.validate()?;
        self.traits.validate()?;
        for effect in &self.effects {
            effect.validate()?;
            if !self.authority.allows_capability(effect.capability()) {
                return Err(CanvasPreviewError::CapabilityDenied(
                    effect.capability().to_string(),
                ));
            }
        }
        for (name, value) in &self.inputs {
            CanvasPreviewInputOverride {
                name: name.clone(),
                value: value.clone(),
            }
            .validate()?;
        }
        if self.inputs.len() > CANVAS_PREVIEW_MAX_INPUTS {
            return Err(CanvasPreviewError::TooManyInputs);
        }
        if let Some(context_id) = &self.context_id {
            preview_text(context_id, "preview context id", 256)?;
        }
        Ok(())
    }
}

impl CanvasPreviewCallback {
    fn checked_id(&self) -> Result<(), CanvasPreviewError> {
        preview_text(&self.id, "preview callback id", 256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasPreviewPolicy {
    pub max_previews: usize,
    pub max_selected: usize,
    pub max_contexts: usize,
    pub max_inputs: usize,
    pub max_frame_bytes: usize,
    pub max_resident_bytes: usize,
    pub max_diagnostic_bytes: usize,
    pub max_clients: usize,
    pub client_idle_secs: u64,
}

impl Default for CanvasPreviewPolicy {
    fn default() -> Self {
        Self {
            max_previews: CANVAS_PREVIEW_MAX_PREVIEWS,
            max_selected: CANVAS_PREVIEW_MAX_SELECTED,
            max_contexts: CANVAS_PREVIEW_MAX_CONTEXTS,
            max_inputs: CANVAS_PREVIEW_MAX_INPUTS,
            max_frame_bytes: CANVAS_PREVIEW_MAX_FRAME_BYTES,
            max_resident_bytes: CANVAS_PREVIEW_MAX_RESIDENT_BYTES,
            max_diagnostic_bytes: CANVAS_PREVIEW_MAX_DIAGNOSTIC_BYTES,
            max_clients: CANVAS_PREVIEW_MAX_CLIENTS,
            client_idle_secs: CANVAS_PREVIEW_CLIENT_IDLE_SECS,
        }
    }
}

impl CanvasPreviewPolicy {
    pub fn validate(&self) -> Result<(), CanvasPreviewError> {
        if self.max_previews == 0
            || self.max_previews > CANVAS_PREVIEW_MAX_PREVIEWS
            || self.max_selected == 0
            || self.max_selected > CANVAS_PREVIEW_MAX_SELECTED
            || self.max_contexts > CANVAS_PREVIEW_MAX_CONTEXTS
            || self.max_inputs > CANVAS_PREVIEW_MAX_INPUTS
            || self.max_frame_bytes == 0
            || self.max_frame_bytes > CANVAS_PREVIEW_MAX_FRAME_BYTES
            || self.max_resident_bytes == 0
            || self.max_resident_bytes > CANVAS_PREVIEW_MAX_RESIDENT_BYTES
            || self.max_diagnostic_bytes == 0
            || self.max_diagnostic_bytes > CANVAS_PREVIEW_MAX_DIAGNOSTIC_BYTES
            || self.max_clients == 0
            || self.max_clients > CANVAS_PREVIEW_MAX_CLIENTS
            || self.client_idle_secs == 0
        {
            return Err(CanvasPreviewError::InvalidPolicy);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanvasPreviewStatus {
    Unrendered,
    Building,
    Ready,
    Error,
    Stale,
    Closed,
}

impl CanvasPreviewStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Unrendered => "unrendered",
            Self::Building => "building",
            Self::Ready => "ready",
            Self::Error => "error",
            Self::Stale => "stale",
            Self::Closed => "closed",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasPreviewDiagnostic {
    pub code: String,
    pub message: String,
    pub source: CanvasPreviewSourceIdentity,
}

impl CanvasPreviewDiagnostic {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        source: CanvasPreviewSourceIdentity,
        max_bytes: usize,
    ) -> Result<Self, CanvasPreviewError> {
        let diagnostic = Self {
            code: code.into(),
            message: message.into(),
            source,
        };
        preview_text(&diagnostic.code, "preview diagnostic code", 128)?;
        preview_text(&diagnostic.message, "preview diagnostic", max_bytes)?;
        diagnostic.source.validate()?;
        Ok(diagnostic)
    }
}

#[derive(Clone)]
pub struct CanvasPreviewRendered<T> {
    pub value: T,
    pub bytes: usize,
}

impl<T> CanvasPreviewRendered<T> {
    pub fn new(value: T, bytes: usize) -> Self {
        Self { value, bytes }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasPreviewRenderError {
    pub code: String,
    pub message: String,
}

impl CanvasPreviewRenderError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Result<Self, CanvasPreviewError> {
        let error = Self {
            code: code.into(),
            message: message.into(),
        };
        preview_text(&error.code, "preview render error code", 128)?;
        preview_text(&error.message, "preview render error", CANVAS_PREVIEW_MAX_DIAGNOSTIC_BYTES)?;
        Ok(error)
    }
}

#[derive(Clone)]
pub struct CanvasPreviewFrame<T> {
    pub value: T,
    pub source: CanvasPreviewSourceIdentity,
    pub bytes: usize,
    pub sequence: u64,
}

#[derive(Clone)]
pub struct CanvasPreviewSnapshot<T> {
    pub session_id: String,
    pub name: String,
    pub kind: CanvasPreviewKind,
    pub source: CanvasPreviewSourceIdentity,
    pub status: CanvasPreviewStatus,
    pub freshness: JetDevtoolsFreshnessState,
    /// `current` is deliberately `None` whenever status is Error/Stale.  Use
    /// `last_good` for the visible retained frame and inspect freshness.
    pub current: Option<CanvasPreviewFrame<T>>,
    pub last_good: Option<CanvasPreviewFrame<T>>,
    pub diagnostic: Option<CanvasPreviewDiagnostic>,
    pub sequence: u64,
}

impl<T: Clone> CanvasPreviewSnapshot<T> {
    pub fn display_frame(&self) -> Option<CanvasPreviewFrame<T>> {
        self.current.clone().or_else(|| self.last_good.clone())
    }

    pub fn has_current_output(&self) -> bool {
        self.current.is_some() && self.freshness == JetDevtoolsFreshnessState::Fresh
    }

    pub fn to_json(&self) -> String {
        let diagnostic = self
            .diagnostic
            .as_ref()
            .map(|value| {
                format!(
                    "{{\"code\":\"{}\",\"message\":\"{}\",\"source\":{}}}",
                    preview_json_escape(&value.code),
                    preview_json_escape(&value.message),
                    preview_source_json(&value.source),
                )
            })
            .unwrap_or_else(|| "null".to_string());
        let current_source = self
            .current
            .as_ref()
            .map(|frame| preview_source_json(&frame.source))
            .unwrap_or_else(|| "null".to_string());
        let last_good_source = self
            .last_good
            .as_ref()
            .map(|frame| preview_source_json(&frame.source))
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"schema_version\":{},\"session_id\":\"{}\",\"name\":\"{}\",\"kind\":\"{}\",\"status\":\"{}\",\"freshness\":\"{}\",\"current\":{},\"last_good\":{},\"current_source\":{},\"last_good_source\":{},\"diagnostic\":{},\"sequence\":{}}}",
            CANVAS_PREVIEW_SCHEMA_VERSION,
            preview_json_escape(&self.session_id),
            preview_json_escape(&self.name),
            self.kind.as_str(),
            self.status.as_str(),
            self.freshness.as_str(),
            self.current.is_some(),
            self.last_good.is_some(),
            current_source,
            last_good_source,
            diagnostic,
            self.sequence,
        )
    }
}

#[derive(Clone)]
struct PreviewRecord<T> {
    descriptor: CanvasPreviewDescriptor,
    status: CanvasPreviewStatus,
    freshness: JetDevtoolsFreshnessState,
    last_good: Option<CanvasPreviewFrame<T>>,
    diagnostic: Option<CanvasPreviewDiagnostic>,
    sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanvasPreviewConnection {
    pub session_id: String,
    pub client_id: String,
    pub generation: u64,
    pub cursor: u64,
}

struct PreviewClientLease {
    generation: u64,
    cursor: u64,
    loopback: bool,
    last_seen: Instant,
}

/// One bounded resident development session.  `T` is normally the canonical
/// `UiNode` or the renderer's output value; this host stores only one retained
/// frame per named scenario and never builds an alternate UI tree.
pub struct CanvasPreviewSession<T: Clone> {
    id: String,
    secret: String,
    authority: CanvasPreviewAuthority,
    policy: CanvasPreviewPolicy,
    registry: CanvasPreviewRegistry,
    records: BTreeMap<String, PreviewRecord<T>>,
    clients: BTreeMap<String, PreviewClientLease>,
    resident_bytes: usize,
    sequence: u64,
    closed: bool,
}

impl<T: Clone> CanvasPreviewSession<T> {
    pub fn new(
        id: impl Into<String>,
        secret: impl Into<String>,
        authority: CanvasPreviewAuthority,
    ) -> Result<Self, CanvasPreviewError> {
        Self::with_policy(id, secret, authority, CanvasPreviewPolicy::default())
    }

    pub fn with_policy(
        id: impl Into<String>,
        secret: impl Into<String>,
        authority: CanvasPreviewAuthority,
        policy: CanvasPreviewPolicy,
    ) -> Result<Self, CanvasPreviewError> {
        let id = id.into();
        let secret = secret.into();
        preview_text(&id, "preview session id", 256)?;
        preview_text(&secret, "preview session secret", 256)?;
        policy.validate()?;
        Ok(Self {
            id,
            secret,
            authority,
            policy,
            registry: CanvasPreviewRegistry::default(),
            records: BTreeMap::new(),
            clients: BTreeMap::new(),
            resident_bytes: 0,
            sequence: 0,
            closed: false,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn authority(&self) -> &CanvasPreviewAuthority {
        &self.authority
    }

    pub fn policy(&self) -> &CanvasPreviewPolicy {
        &self.policy
    }

    pub fn registry(&self) -> &CanvasPreviewRegistry {
        &self.registry
    }

    pub fn resident_bytes(&self) -> usize {
        self.resident_bytes
    }

    pub fn add_preview(&mut self, descriptor: CanvasPreviewDescriptor) -> Result<(), CanvasPreviewError> {
        self.ensure_open()?;
        descriptor.validate()?;
        if self.records.len() >= self.policy.max_previews {
            return Err(CanvasPreviewError::TooManyPreviews);
        }
        if descriptor.inputs().len() > self.policy.max_inputs {
            return Err(CanvasPreviewError::TooManyInputs);
        }
        if !self.authority.covers(descriptor.authority()) {
            return Err(CanvasPreviewError::AuthorityWidened);
        }
        self.registry.insert(descriptor.clone())?;
        self.records.insert(
            descriptor.name().to_string(),
            PreviewRecord {
                descriptor,
                status: CanvasPreviewStatus::Unrendered,
                freshness: JetDevtoolsFreshnessState::Unknown,
                last_good: None,
                diagnostic: None,
                sequence: self.sequence,
            },
        );
        Ok(())
    }

    pub fn replace_preview(&mut self, descriptor: CanvasPreviewDescriptor) -> Result<(), CanvasPreviewError> {
        self.ensure_open()?;
        descriptor.validate()?;
        if descriptor.inputs().len() > self.policy.max_inputs {
            return Err(CanvasPreviewError::TooManyInputs);
        }
        if !self.authority.covers(descriptor.authority()) {
            return Err(CanvasPreviewError::AuthorityWidened);
        }
        if !self.records.contains_key(descriptor.name()) {
            return self.add_preview(descriptor);
        }
        self.registry.replace(descriptor.clone())?;
        let sequence = self.bump_sequence();
        let record = self
            .records
            .get_mut(descriptor.name())
            .ok_or_else(|| CanvasPreviewError::UnknownPreview(descriptor.name().to_string()))?;
        record.descriptor = descriptor;
        record.status = if record.last_good.is_some() {
            CanvasPreviewStatus::Stale
        } else {
            CanvasPreviewStatus::Unrendered
        };
        record.freshness = if record.last_good.is_some() {
            JetDevtoolsFreshnessState::Stale
        } else {
            JetDevtoolsFreshnessState::Unknown
        };
        record.diagnostic = None;
        record.sequence = sequence;
        Ok(())
    }

    pub fn remove_preview(&mut self, name: &str) -> Result<(), CanvasPreviewError> {
        self.ensure_open()?;
        let record = self
            .records
            .remove(name)
            .ok_or_else(|| CanvasPreviewError::UnknownPreview(name.to_string()))?;
        self.resident_bytes = self
            .resident_bytes
            .saturating_sub(record.last_good.as_ref().map_or(0, |frame| frame.bytes));
        self.registry.remove(name)?;
        Ok(())
    }

    pub fn register_context(&mut self, context: CanvasPreviewContext) -> Result<(), CanvasPreviewError> {
        self.ensure_open()?;
        if self.registry.contexts_len() >= self.policy.max_contexts {
            return Err(CanvasPreviewError::TooManyContexts);
        }
        self.registry.insert_context(context)
    }

    pub fn select(&mut self, name: &str) -> Result<(), CanvasPreviewError> {
        self.ensure_open()?;
        if self.registry.selected_len() >= self.policy.max_selected
            && !self.registry.is_selected(name)
        {
            return Err(CanvasPreviewError::TooManySelected);
        }
        self.registry.select(name)
    }

    pub fn select_many<I>(&mut self, names: I) -> Result<(), CanvasPreviewError>
    where
        I: IntoIterator<Item = String>,
    {
        self.ensure_open()?;
        let names = names.into_iter().collect::<Vec<_>>();
        if names.len() > self.policy.max_selected {
            return Err(CanvasPreviewError::TooManySelected);
        }
        self.registry.select_many(names)
    }

    pub fn clear_selection(&mut self) {
        self.registry.clear_selection();
    }

    pub fn selected_names(&self) -> Vec<String> {
        self.registry.selected_names()
    }

    pub fn descriptor(&self, name: &str) -> Result<&CanvasPreviewDescriptor, CanvasPreviewError> {
        self.records
            .get(name)
            .map(|record| &record.descriptor)
            .ok_or_else(|| CanvasPreviewError::UnknownPreview(name.to_string()))
    }

    pub fn snapshot(&self, name: &str) -> Result<CanvasPreviewSnapshot<T>, CanvasPreviewError> {
        let record = self
            .records
            .get(name)
            .ok_or_else(|| CanvasPreviewError::UnknownPreview(name.to_string()))?;
        Ok(self.snapshot_record(name, record))
    }

    pub fn snapshots(&self) -> Vec<CanvasPreviewSnapshot<T>> {
        self.records
            .iter()
            .map(|(name, record)| self.snapshot_record(name, record))
            .collect()
    }

    pub fn selected_snapshots(&self) -> Vec<CanvasPreviewSnapshot<T>> {
        self.registry
            .selected_names()
            .into_iter()
            .filter_map(|name| self.records.get(&name).map(|record| self.snapshot_record(&name, record)))
            .collect()
    }

    pub fn mark_building(&mut self, name: &str) -> Result<(), CanvasPreviewError> {
        self.ensure_open()?;
        let sequence = self.bump_sequence();
        let record = self.record_mut(name)?;
        record.status = CanvasPreviewStatus::Building;
        record.freshness = if record.last_good.is_some() {
            JetDevtoolsFreshnessState::Stale
        } else {
            JetDevtoolsFreshnessState::Unknown
        };
        record.diagnostic = None;
        record.sequence = sequence;
        Ok(())
    }

    pub fn mark_stale(
        &mut self,
        name: &str,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), CanvasPreviewError> {
        self.ensure_open()?;
        let source = self.descriptor(name)?.source().clone();
        let diagnostic = CanvasPreviewDiagnostic::new(
            code,
            message,
            source,
            self.policy.max_diagnostic_bytes,
        )?;
        let sequence = self.bump_sequence();
        let record = self.record_mut(name)?;
        record.status = CanvasPreviewStatus::Stale;
        record.freshness = JetDevtoolsFreshnessState::Stale;
        record.diagnostic = Some(diagnostic);
        record.sequence = sequence;
        Ok(())
    }

    pub fn mark_error(
        &mut self,
        name: &str,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), CanvasPreviewError> {
        self.ensure_open()?;
        let source = self.descriptor(name)?.source().clone();
        let diagnostic = CanvasPreviewDiagnostic::new(
            code,
            message,
            source,
            self.policy.max_diagnostic_bytes,
        )?;
        let sequence = self.bump_sequence();
        let record = self.record_mut(name)?;
        record.status = CanvasPreviewStatus::Error;
        record.freshness = JetDevtoolsFreshnessState::Stale;
        record.diagnostic = Some(diagnostic);
        record.sequence = sequence;
        Ok(())
    }

    /// Pure previews can be evaluated directly.  A descriptor with effects
    /// must use `evaluate_authorized`, which ties the render to a live client
    /// connection and the exact declared effect set.
    pub fn evaluate<F>(
        &mut self,
        name: &str,
        render: F,
    ) -> Result<CanvasPreviewSnapshot<T>, CanvasPreviewError>
    where
        F: FnOnce(
            &CanvasPreviewDescriptor,
            Option<&CanvasPreviewContext>,
        ) -> Result<CanvasPreviewRendered<T>, CanvasPreviewRenderError>,
    {
        let descriptor = self.descriptor(name)?.clone();
        if descriptor.effects().next().is_some() {
            return Err(CanvasPreviewError::AuthorityRequired);
        }
        self.evaluate_inner(name, descriptor, render)
    }

    pub fn evaluate_authorized<F>(
        &mut self,
        connection: &CanvasPreviewConnection,
        name: &str,
        requested_effects: &[CanvasPreviewEffect],
        render: F,
    ) -> Result<CanvasPreviewSnapshot<T>, CanvasPreviewError>
    where
        F: FnOnce(
            &CanvasPreviewDescriptor,
            Option<&CanvasPreviewContext>,
        ) -> Result<CanvasPreviewRendered<T>, CanvasPreviewRenderError>,
    {
        self.touch(connection)?;
        let descriptor = self.descriptor(name)?.clone();
        self.authorize_descriptor(&descriptor, connection, requested_effects)?;
        self.evaluate_inner(name, descriptor, render)
    }

    fn evaluate_inner<F>(
        &mut self,
        name: &str,
        descriptor: CanvasPreviewDescriptor,
        render: F,
    ) -> Result<CanvasPreviewSnapshot<T>, CanvasPreviewError>
    where
        F: FnOnce(
            &CanvasPreviewDescriptor,
            Option<&CanvasPreviewContext>,
        ) -> Result<CanvasPreviewRendered<T>, CanvasPreviewRenderError>,
    {
        self.mark_building(name)?;
        let context = descriptor
            .context_id()
            .and_then(|id| self.registry.context(id).cloned());
        let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render(&descriptor, context.as_ref())
        }));
        match rendered {
            Ok(Ok(rendered)) => {
                self.publish_inner(name, descriptor.source().clone(), rendered)
            }
            Ok(Err(error)) => {
                self.mark_error(name, error.code, error.message)?;
                Err(CanvasPreviewError::RenderFailed)
            }
            Err(_) => {
                self.mark_error(name, "E_PREVIEW_PANIC", "preview callback panicked")?;
                Err(CanvasPreviewError::RenderFailed)
            }
        }
    }

    /// Publish a frame produced by the normal Canvas/UiNode renderer.  The
    /// zero-effect path stays convenient; effectful previews must call the
    /// connection-bound variant below.
    pub fn publish(
        &mut self,
        name: &str,
        source: CanvasPreviewSourceIdentity,
        rendered: CanvasPreviewRendered<T>,
    ) -> Result<CanvasPreviewSnapshot<T>, CanvasPreviewError> {
        let descriptor = self.descriptor(name)?.clone();
        if descriptor.effects().next().is_some() {
            return Err(CanvasPreviewError::AuthorityRequired);
        }
        self.publish_inner(name, source, rendered)
    }

    pub fn publish_authorized(
        &mut self,
        connection: &CanvasPreviewConnection,
        name: &str,
        requested_effects: &[CanvasPreviewEffect],
        source: CanvasPreviewSourceIdentity,
        rendered: CanvasPreviewRendered<T>,
    ) -> Result<CanvasPreviewSnapshot<T>, CanvasPreviewError> {
        self.touch(connection)?;
        let descriptor = self.descriptor(name)?.clone();
        self.authorize_descriptor(&descriptor, connection, requested_effects)?;
        self.publish_inner(name, source, rendered)
    }

    fn publish_inner(
        &mut self,
        name: &str,
        source: CanvasPreviewSourceIdentity,
        rendered: CanvasPreviewRendered<T>,
    ) -> Result<CanvasPreviewSnapshot<T>, CanvasPreviewError> {
        source.validate()?;
        let descriptor = self.descriptor(name)?.clone();
        if source != *descriptor.source() {
            return Err(CanvasPreviewError::SourceMismatch);
        }
        if rendered.bytes == 0 || rendered.bytes > self.policy.max_frame_bytes {
            return Err(CanvasPreviewError::FrameTooLarge);
        }
        let old_bytes = self
            .records
            .get(name)
            .and_then(|record| record.last_good.as_ref())
            .map_or(0, |frame| frame.bytes);
        let new_resident = self
            .resident_bytes
            .saturating_sub(old_bytes)
            .saturating_add(rendered.bytes);
        if new_resident > self.policy.max_resident_bytes {
            return Err(CanvasPreviewError::ResidentBudgetExceeded);
        }
        let sequence = self.bump_sequence();
        let frame = CanvasPreviewFrame {
            value: rendered.value,
            source,
            bytes: rendered.bytes,
            sequence,
        };
        let record = self.record_mut(name)?;
        record.status = CanvasPreviewStatus::Ready;
        record.freshness = JetDevtoolsFreshnessState::Fresh;
        record.last_good = Some(frame);
        record.diagnostic = None;
        record.sequence = sequence;
        self.resident_bytes = new_resident;
        Ok(self.snapshot(name)?)
    }

    fn authorize_descriptor(
        &self,
        descriptor: &CanvasPreviewDescriptor,
        connection: &CanvasPreviewConnection,
        requested_effects: &[CanvasPreviewEffect],
    ) -> Result<(), CanvasPreviewError> {
        if connection.session_id != self.id {
            return Err(CanvasPreviewError::Unauthorized);
        }
        if !connection_is_loopback(self.clients.get(&connection.client_id)) {
            return Err(CanvasPreviewError::LoopbackRequired);
        }
        let requested = requested_effects.iter().collect::<BTreeSet<_>>();
        let declared = descriptor.effects.iter().collect::<BTreeSet<_>>();
        if requested != declared {
            return Err(CanvasPreviewError::EffectSetMismatch);
        }
        for effect in &descriptor.effects {
            if !descriptor.authority.allows_effect(effect, true)
                || !self.authority.allows_effect(effect, true)
            {
                return Err(CanvasPreviewError::CapabilityDenied(
                    effect.capability().to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn connect(
        &mut self,
        client_id: impl Into<String>,
        presented_secret: &str,
        loopback: bool,
    ) -> Result<CanvasPreviewConnection, CanvasPreviewError> {
        self.ensure_open()?;
        self.prune_expired_at(Instant::now());
        let client_id = client_id.into();
        preview_text(&client_id, "preview client id", 256)?;
        self.authorize_connection(presented_secret, loopback)?;
        let cursor = 0;
        self.open_client(client_id, loopback, cursor)
    }

    pub fn reconnect(
        &mut self,
        connection: &CanvasPreviewConnection,
        presented_secret: &str,
        loopback: bool,
    ) -> Result<CanvasPreviewConnection, CanvasPreviewError> {
        self.ensure_open()?;
        self.authorize_connection(presented_secret, loopback)?;
        let generation = self
            .clients
            .get(&connection.client_id)
            .ok_or(CanvasPreviewError::StaleConnection)?
            .generation;
        if generation != connection.generation || connection.session_id != self.id {
            return Err(CanvasPreviewError::StaleConnection);
        }
        self.open_client(
            connection.client_id.clone(),
            loopback,
            connection.cursor.min(self.sequence),
        )
    }

    pub fn touch(&mut self, connection: &CanvasPreviewConnection) -> Result<(), CanvasPreviewError> {
        self.ensure_open()?;
        if connection.session_id != self.id {
            return Err(CanvasPreviewError::StaleConnection);
        }
        let lease = self
            .clients
            .get_mut(&connection.client_id)
            .ok_or(CanvasPreviewError::StaleConnection)?;
        if lease.generation != connection.generation {
            return Err(CanvasPreviewError::StaleConnection);
        }
        lease.last_seen = Instant::now();
        Ok(())
    }

    pub fn acknowledge(
        &mut self,
        connection: &CanvasPreviewConnection,
        cursor: u64,
    ) -> Result<(), CanvasPreviewError> {
        self.touch(connection)?;
        if cursor > self.sequence {
            return Err(CanvasPreviewError::InvalidCursor);
        }
        let lease = self
            .clients
            .get_mut(&connection.client_id)
            .ok_or(CanvasPreviewError::StaleConnection)?;
        lease.cursor = cursor;
        Ok(())
    }

    pub fn needs_snapshot(&self, connection: &CanvasPreviewConnection) -> Result<bool, CanvasPreviewError> {
        let lease = self
            .clients
            .get(&connection.client_id)
            .ok_or(CanvasPreviewError::StaleConnection)?;
        if connection.session_id != self.id || lease.generation != connection.generation {
            return Err(CanvasPreviewError::StaleConnection);
        }
        Ok(lease.cursor < self.sequence)
    }

    pub fn disconnect(&mut self, connection: &CanvasPreviewConnection) -> Result<(), CanvasPreviewError> {
        let lease = self
            .clients
            .get(&connection.client_id)
            .ok_or(CanvasPreviewError::StaleConnection)?;
        if connection.session_id != self.id || lease.generation != connection.generation {
            return Err(CanvasPreviewError::StaleConnection);
        }
        self.clients.remove(&connection.client_id);
        Ok(())
    }

    pub fn prune_expired(&mut self) -> usize {
        self.prune_expired_at(Instant::now())
    }

    pub fn prune_expired_at(&mut self, now: Instant) -> usize {
        let timeout = Duration::from_secs(self.policy.client_idle_secs);
        let before = self.clients.len();
        self.clients
            .retain(|_, lease| now.saturating_duration_since(lease.last_seen) <= timeout);
        before.saturating_sub(self.clients.len())
    }

    pub fn close(&mut self) {
        self.closed = true;
        self.clients.clear();
        self.records.clear();
        self.registry = CanvasPreviewRegistry::default();
        self.resident_bytes = 0;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn to_json(&self) -> String {
        let snapshots = self
            .snapshots()
            .iter()
            .map(CanvasPreviewSnapshot::to_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema_version\":{},\"session_id\":\"{}\",\"closed\":{},\"selected\":[{}],\"resident_bytes\":{},\"sequence\":{},\"previews\":[{}]}}",
            CANVAS_PREVIEW_SCHEMA_VERSION,
            preview_json_escape(&self.id),
            self.closed,
            self.registry
                .selected_names()
                .iter()
                .map(|name| format!("\"{}\"", preview_json_escape(name)))
                .collect::<Vec<_>>()
                .join(","),
            self.resident_bytes,
            self.sequence,
            snapshots,
        )
    }

    fn open_client(
        &mut self,
        client_id: String,
        loopback: bool,
        cursor: u64,
    ) -> Result<CanvasPreviewConnection, CanvasPreviewError> {
        let generation = self
            .clients
            .get(&client_id)
            .map_or(1, |lease| lease.generation.saturating_add(1));
        if !self.clients.contains_key(&client_id) && self.clients.len() >= self.policy.max_clients {
            return Err(CanvasPreviewError::TooManyClients);
        }
        self.clients.insert(
            client_id.clone(),
            PreviewClientLease {
                generation,
                cursor,
                loopback,
                last_seen: Instant::now(),
            },
        );
        Ok(CanvasPreviewConnection {
            session_id: self.id.clone(),
            client_id,
            generation,
            cursor,
        })
    }

    fn authorize_connection(&self, presented_secret: &str, loopback: bool) -> Result<(), CanvasPreviewError> {
        if !constant_time_equal(&self.secret, presented_secret) {
            return Err(CanvasPreviewError::Unauthorized);
        }
        if self.authority.loopback_only() && !loopback {
            return Err(CanvasPreviewError::LoopbackRequired);
        }
        Ok(())
    }

    fn record_mut(&mut self, name: &str) -> Result<&mut PreviewRecord<T>, CanvasPreviewError> {
        self.records
            .get_mut(name)
            .ok_or_else(|| CanvasPreviewError::UnknownPreview(name.to_string()))
    }

    fn snapshot_record(&self, name: &str, record: &PreviewRecord<T>) -> CanvasPreviewSnapshot<T> {
        CanvasPreviewSnapshot {
            session_id: self.id.clone(),
            name: name.to_string(),
            kind: record.descriptor.kind(),
            source: record.descriptor.source().clone(),
            status: record.status.clone(),
            freshness: record.freshness,
            current: (record.freshness == JetDevtoolsFreshnessState::Fresh)
                .then(|| record.last_good.clone())
                .flatten(),
            last_good: record.last_good.clone(),
            diagnostic: record.diagnostic.clone(),
            sequence: record.sequence,
        }
    }

    fn bump_sequence(&mut self) -> u64 {
        self.sequence = self.sequence.saturating_add(1);
        self.sequence
    }

    fn ensure_open(&self) -> Result<(), CanvasPreviewError> {
        if self.closed {
            Err(CanvasPreviewError::SessionClosed)
        } else {
            Ok(())
        }
    }
}

impl<T: Clone> Drop for CanvasPreviewSession<T> {
    fn drop(&mut self) {
        self.clients.clear();
        self.records.clear();
        self.registry.clear();
        self.resident_bytes = 0;
    }
}

/// The descriptor registry is intentionally independent from the session's
/// retained frames.  This lets a checker publish a complete registry first,
/// then attach one bounded resident session to Canvas and `jet dev`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasPreviewRegistry {
    descriptors: BTreeMap<String, CanvasPreviewDescriptor>,
    contexts: BTreeMap<String, CanvasPreviewContext>,
    selected: BTreeSet<String>,
}

impl CanvasPreviewRegistry {
    pub fn new<I>(descriptors: I) -> Result<Self, CanvasPreviewError>
    where
        I: IntoIterator<Item = CanvasPreviewDescriptor>,
    {
        let mut registry = Self::default();
        for descriptor in descriptors {
            registry.insert(descriptor)?;
        }
        Ok(registry)
    }

    pub fn insert(&mut self, descriptor: CanvasPreviewDescriptor) -> Result<(), CanvasPreviewError> {
        descriptor.validate()?;
        if self.descriptors.len() >= CANVAS_PREVIEW_MAX_PREVIEWS {
            return Err(CanvasPreviewError::TooManyPreviews);
        }
        if self.descriptors.contains_key(descriptor.name()) {
            return Err(CanvasPreviewError::DuplicatePreview(
                descriptor.name().to_string(),
            ));
        }
        self.descriptors
            .insert(descriptor.name().to_string(), descriptor);
        Ok(())
    }

    pub fn replace(&mut self, descriptor: CanvasPreviewDescriptor) -> Result<(), CanvasPreviewError> {
        descriptor.validate()?;
        if !self.descriptors.contains_key(descriptor.name()) {
            return self.insert(descriptor);
        }
        self.descriptors
            .insert(descriptor.name().to_string(), descriptor);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<(), CanvasPreviewError> {
        self.descriptors
            .remove(name)
            .ok_or_else(|| CanvasPreviewError::UnknownPreview(name.to_string()))?;
        self.selected.remove(name);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&CanvasPreviewDescriptor> {
        self.descriptors.get(name)
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &CanvasPreviewDescriptor> {
        self.descriptors.values()
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }

    pub fn select(&mut self, name: &str) -> Result<(), CanvasPreviewError> {
        if !self.descriptors.contains_key(name) {
            return Err(CanvasPreviewError::UnknownPreview(name.to_string()));
        }
        if self.selected.len() >= CANVAS_PREVIEW_MAX_SELECTED && !self.selected.contains(name) {
            return Err(CanvasPreviewError::TooManySelected);
        }
        self.selected.insert(name.to_string());
        Ok(())
    }

    pub fn select_many<I>(&mut self, names: I) -> Result<(), CanvasPreviewError>
    where
        I: IntoIterator<Item = String>,
    {
        let names = names.into_iter().collect::<Vec<_>>();
        if names.len() > CANVAS_PREVIEW_MAX_SELECTED {
            return Err(CanvasPreviewError::TooManySelected);
        }
        for name in &names {
            if !self.descriptors.contains_key(name) {
                return Err(CanvasPreviewError::UnknownPreview(name.clone()));
            }
        }
        self.selected.clear();
        self.selected.extend(names);
        Ok(())
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    pub fn selected_names(&self) -> Vec<String> {
        self.selected.iter().cloned().collect()
    }

    fn selected_len(&self) -> usize {
        self.selected.len()
    }

    fn is_selected(&self, name: &str) -> bool {
        self.selected.contains(name)
    }

    pub fn insert_context(&mut self, context: CanvasPreviewContext) -> Result<(), CanvasPreviewError> {
        if self.contexts.len() >= CANVAS_PREVIEW_MAX_CONTEXTS {
            return Err(CanvasPreviewError::TooManyContexts);
        }
        if self.contexts.contains_key(&context.id) {
            return Err(CanvasPreviewError::DuplicateContext(context.id));
        }
        context.source.validate()?;
        self.contexts.insert(context.id.clone(), context);
        Ok(())
    }

    pub fn context(&self, id: &str) -> Option<&CanvasPreviewContext> {
        self.contexts.get(id)
    }

    fn contexts_len(&self) -> usize {
        self.contexts.len()
    }

    pub fn validate(&self) -> Result<(), CanvasPreviewError> {
        for descriptor in self.descriptors.values() {
            descriptor.validate()?;
            if let Some(context_id) = descriptor.context_id() {
                let context = self
                    .contexts
                    .get(context_id)
                    .ok_or_else(|| CanvasPreviewError::UnknownContext(context_id.to_string()))?;
                if context.source != *descriptor.source() {
                    return Err(CanvasPreviewError::ContextSourceMismatch);
                }
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> String {
        let descriptors = self
            .descriptors
            .values()
            .map(|descriptor| {
                format!(
                    "{{\"name\":\"{}\",\"kind\":\"{}\",\"callback\":\"{}\",\"source\":{},\"viewport\":{{\"device\":\"{}\",\"width\":{},\"height\":{},\"scale\":{}}},\"selected\":{}}}",
                    preview_json_escape(descriptor.name()),
                    descriptor.kind().as_str(),
                    preview_json_escape(&descriptor.callback().id),
                    preview_source_json(descriptor.source()),
                    preview_json_escape(descriptor.viewport().device.as_str()),
                    descriptor.viewport().width,
                    descriptor.viewport().height,
                    descriptor.viewport().scale,
                    self.selected.contains(descriptor.name()),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema_version\":{},\"selected\":[{}],\"contexts\":{},\"descriptors\":[{}]}}",
            CANVAS_PREVIEW_SCHEMA_VERSION,
            self.selected
                .iter()
                .map(|name| format!("\"{}\"", preview_json_escape(name)))
                .collect::<Vec<_>>()
                .join(","),
            self.contexts.len(),
            descriptors,
        )
    }

    fn clear(&mut self) {
        self.descriptors.clear();
        self.contexts.clear();
        self.selected.clear();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanvasPreviewError {
    InvalidText { field: &'static str, reason: &'static str },
    InvalidSource(&'static str),
    InvalidViewport(&'static str),
    InvalidInput(&'static str),
    InvalidPolicy,
    TooManyPreviews,
    TooManySelected,
    TooManyContexts,
    TooManyInputs,
    DuplicatePreview(String),
    UnknownPreview(String),
    DuplicateContext(String),
    UnknownContext(String),
    DuplicateInput(String),
    InputBudgetExceeded,
    AuthorityWidened,
    AuthorityRequired,
    CapabilityDenied(String),
    EffectSetMismatch,
    LoopbackRequired,
    Unauthorized,
    TooManyClients,
    StaleConnection,
    InvalidCursor,
    FrameTooLarge,
    ResidentBudgetExceeded,
    SourceMismatch,
    ContextSourceMismatch,
    RenderFailed,
    SessionClosed,
}

impl fmt::Display for CanvasPreviewError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidText { field, reason } => write!(output, "invalid {field}: {reason}"),
            Self::InvalidSource(reason) => write!(output, "invalid preview source: {reason}"),
            Self::InvalidViewport(reason) => write!(output, "invalid preview viewport: {reason}"),
            Self::InvalidInput(reason) => write!(output, "invalid preview input: {reason}"),
            Self::InvalidPolicy => output.write_str("preview policy exceeds the bounded session contract"),
            Self::TooManyPreviews => output.write_str("preview registry is full"),
            Self::TooManySelected => output.write_str("preview selection exceeds the bounded limit"),
            Self::TooManyContexts => output.write_str("preview context registry is full"),
            Self::TooManyInputs => output.write_str("preview input overrides exceed the bounded limit"),
            Self::DuplicatePreview(name) => write!(output, "preview `{name}` is duplicated"),
            Self::UnknownPreview(name) => write!(output, "unknown preview `{name}`"),
            Self::DuplicateContext(id) => write!(output, "preview context `{id}` is duplicated"),
            Self::UnknownContext(id) => write!(output, "unknown preview context `{id}`"),
            Self::DuplicateInput(name) => write!(output, "preview input `{name}` is duplicated"),
            Self::InputBudgetExceeded => output.write_str("preview input budget exceeded"),
            Self::AuthorityWidened => output.write_str("preview authority widens the session authority"),
            Self::AuthorityRequired => output.write_str("preview effects require an authorized connection"),
            Self::CapabilityDenied(capability) => write!(output, "preview capability `{capability}` is denied"),
            Self::EffectSetMismatch => output.write_str("requested preview effects do not match the checked declaration"),
            Self::LoopbackRequired => output.write_str("preview session requires a loopback connection"),
            Self::Unauthorized => output.write_str("preview session authorization failed"),
            Self::TooManyClients => output.write_str("preview session client limit reached"),
            Self::StaleConnection => output.write_str("preview connection is stale; reconnect and resync"),
            Self::InvalidCursor => output.write_str("preview cursor is outside the session history"),
            Self::FrameTooLarge => output.write_str("preview frame exceeds the bounded frame limit"),
            Self::ResidentBudgetExceeded => output.write_str("preview resident frame budget exceeded"),
            Self::SourceMismatch => output.write_str("preview frame source identity does not match the checked declaration"),
            Self::ContextSourceMismatch => output.write_str("preview context source identity does not match the checked declaration"),
            Self::RenderFailed => output.write_str("preview callback failed; the last-good frame remains visible as stale"),
            Self::SessionClosed => output.write_str("preview session is closed"),
        }
    }
}

impl std::error::Error for CanvasPreviewError {}

fn preview_text(value: &str, field: &'static str, max_bytes: usize) -> Result<(), CanvasPreviewError> {
    if value.trim().is_empty() {
        return Err(CanvasPreviewError::InvalidText {
            field,
            reason: "must not be empty",
        });
    }
    if value.len() > max_bytes {
        return Err(CanvasPreviewError::InvalidText {
            field,
            reason: "exceeds the bounded text size",
        });
    }
    if value.chars().any(char::is_control) {
        return Err(CanvasPreviewError::InvalidText {
            field,
            reason: "must not contain control characters",
        });
    }
    Ok(())
}

fn preview_source_json(source: &CanvasPreviewSourceIdentity) -> String {
    format!(
        "{{\"source_id\":\"{}\",\"build_id\":\"{}\",\"revision\":\"{}\",\"span\":{{\"source_id\":\"{}\",\"file\":\"{}\",\"start_line\":{},\"start_column\":{},\"end_line\":{},\"end_column\":{}}}}}",
        preview_json_escape(&source.source_id),
        preview_json_escape(&source.build_id),
        preview_json_escape(&source.revision),
        preview_json_escape(&source.span.source_id),
        preview_json_escape(&source.span.file),
        source.span.start_line,
        source.span.start_column,
        source.span.end_line,
        source.span.end_column,
    )
}

fn preview_json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let mut difference = left.len() ^ right.len();
    let width = left.len().max(right.len());
    for index in 0..width {
        let left_byte = left.as_bytes().get(index).copied().unwrap_or(0);
        let right_byte = right.as_bytes().get(index).copied().unwrap_or(0);
        difference |= usize::from(left_byte ^ right_byte);
    }
    difference == 0
}

fn connection_is_loopback(lease: Option<&PreviewClientLease>) -> bool {
    lease.is_some_and(|lease| lease.loopback)
}
