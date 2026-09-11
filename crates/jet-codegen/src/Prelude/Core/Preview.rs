// D-DX-PREVIEW1=A / card #2464: named previews and playgrounds are ordinary
// typed core.ui values.  The checked callback returns the canonical UiNode;
// host adapters retain only bounded metadata and invoke the normal renderer.

pub const JET_UI_PREVIEW_MAX_PREVIEWS: usize = 64;
pub const JET_UI_PREVIEW_MAX_SELECTED: usize = 16;
pub const JET_UI_PREVIEW_MAX_CONTEXTS: usize = 16;
pub const JET_UI_PREVIEW_MAX_INPUTS: usize = 32;
pub const JET_UI_PREVIEW_MAX_TEXT_BYTES: usize = 16 * 1024;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetUiPreviewError {
    EmptyName,
    TextTooLong(&'static str),
    InvalidSource(&'static str),
    InvalidViewport(&'static str),
    InvalidInput(&'static str),
    TooManyPreviews,
    TooManySelected,
    TooManyContexts,
    TooManyInputs,
    DuplicatePreview(String),
    UnknownPreview(String),
    DuplicateInput(String),
    MissingSource(String),
    MissingContext(String),
    CallbackFailed(String),
    CapabilityDenied(String),
}

impl std::fmt::Display for JetUiPreviewError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => output.write_str("UI preview name must not be empty"),
            Self::TextTooLong(field) => write!(output, "UI preview {field} exceeds its text bound"),
            Self::InvalidSource(reason) => write!(output, "UI preview source is invalid: {reason}"),
            Self::InvalidViewport(reason) => write!(output, "UI preview viewport is invalid: {reason}"),
            Self::InvalidInput(reason) => write!(output, "UI preview input is invalid: {reason}"),
            Self::TooManyPreviews => output.write_str("UI preview registry is full"),
            Self::TooManySelected => output.write_str("UI preview selection is full"),
            Self::TooManyContexts => output.write_str("UI preview context registry is full"),
            Self::TooManyInputs => output.write_str("UI preview inputs exceed the bounded limit"),
            Self::DuplicatePreview(name) => write!(output, "UI preview `{name}` is duplicated"),
            Self::UnknownPreview(name) => write!(output, "unknown UI preview `{name}`"),
            Self::DuplicateInput(name) => write!(output, "UI preview input `{name}` is duplicated"),
            Self::MissingSource(name) => write!(output, "UI preview `{name}` has no source identity"),
            Self::MissingContext(id) => write!(output, "UI preview context `{id}` is not registered"),
            Self::CallbackFailed(name) => write!(output, "UI preview callback `{name}` failed"),
            Self::CapabilityDenied(capability) => write!(output, "UI preview capability `{capability}` is denied"),
        }
    }
}

impl std::error::Error for JetUiPreviewError {}

fn jet_ui_preview_text(value: &str, field: &'static str, max_bytes: usize) -> Result<(), JetUiPreviewError> {
    if value.trim().is_empty() {
        return Err(if field == "preview name" {
            JetUiPreviewError::EmptyName
        } else {
            JetUiPreviewError::InvalidSource("text must not be empty")
        });
    }
    if value.len() > max_bytes {
        return Err(JetUiPreviewError::TextTooLong(field));
    }
    if value.chars().any(char::is_control) {
        return Err(JetUiPreviewError::InvalidSource("text contains a control character"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetUiPreviewKind {
    Preview,
    Playground,
}

impl JetUiPreviewKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Playground => "playground",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetUiPreviewDevice {
    Phone,
    Tablet,
    Desktop,
    Custom(String),
}

impl JetUiPreviewDevice {
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
pub struct JetUiPreviewViewport {
    pub device: JetUiPreviewDevice,
    pub width: u32,
    pub height: u32,
    pub scale: u16,
}

impl JetUiPreviewViewport {
    pub fn new(
        device: JetUiPreviewDevice,
        width: u32,
        height: u32,
        scale: u16,
    ) -> Result<Self, JetUiPreviewError> {
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
            device: JetUiPreviewDevice::Phone,
            width: 390,
            height: 844,
            scale: 2,
        }
    }

    pub fn tablet() -> Self {
        Self {
            device: JetUiPreviewDevice::Tablet,
            width: 834,
            height: 1_119,
            scale: 2,
        }
    }

    pub fn desktop() -> Self {
        Self {
            device: JetUiPreviewDevice::Desktop,
            width: 1_280,
            height: 800,
            scale: 1,
        }
    }

    pub fn validate(&self) -> Result<(), JetUiPreviewError> {
        if self.width == 0 || self.height == 0 {
            return Err(JetUiPreviewError::InvalidViewport(
                "viewport dimensions must be positive",
            ));
        }
        if self.width > 16_384 || self.height > 16_384 {
            return Err(JetUiPreviewError::InvalidViewport(
                "viewport dimension exceeds the preview bound",
            ));
        }
        if self.scale == 0 || self.scale > 8 {
            return Err(JetUiPreviewError::InvalidViewport(
                "viewport scale exceeds the preview bound",
            ));
        }
        if let JetUiPreviewDevice::Custom(name) = &self.device {
            jet_ui_preview_text(name, "viewport device", 128)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetUiPreviewTheme {
    System,
    Light,
    Dark,
    HighContrast,
    Custom(String),
}

impl JetUiPreviewTheme {
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
pub enum JetUiPreviewAccessibility {
    Default,
    ReducedMotion,
    LargeText,
    HighContrast,
    ScreenReader,
    Custom(String),
}

impl JetUiPreviewAccessibility {
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
pub enum JetUiPreviewLifecycle {
    Initial,
    Loading,
    Loaded,
    Empty,
    Error,
    Offline,
    Custom(String),
}

impl JetUiPreviewLifecycle {
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
pub struct JetUiPreviewTraits {
    pub theme: JetUiPreviewTheme,
    pub locale: String,
    pub accessibility: JetUiPreviewAccessibility,
    pub lifecycle: JetUiPreviewLifecycle,
}

impl Default for JetUiPreviewTraits {
    fn default() -> Self {
        Self {
            theme: JetUiPreviewTheme::System,
            locale: "en-US".to_string(),
            accessibility: JetUiPreviewAccessibility::Default,
            lifecycle: JetUiPreviewLifecycle::Initial,
        }
    }
}

impl JetUiPreviewTraits {
    pub fn validate(&self) -> Result<(), JetUiPreviewError> {
        jet_ui_preview_text(&self.locale, "locale", 128)?;
        for value in [
            self.theme.as_str(),
            self.accessibility.as_str(),
            self.lifecycle.as_str(),
        ] {
            jet_ui_preview_text(value, "preview trait", 128)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetUiPreviewEffect {
    Read(String),
    Write(String),
    Network(String),
    Clock,
    Custom(String),
}

impl JetUiPreviewEffect {
    pub fn capability(&self) -> &str {
        match self {
            Self::Read(value) | Self::Write(value) | Self::Network(value) | Self::Custom(value) => value,
            Self::Clock => "Clock.Read",
        }
    }

    pub fn validate(&self) -> Result<(), JetUiPreviewError> {
        jet_ui_preview_text(self.capability(), "preview effect", 256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetUiPreviewAuthority {
    loopback_only: bool,
    capabilities: std::collections::BTreeSet<String>,
}

impl JetUiPreviewAuthority {
    pub fn development() -> Self {
        Self {
            loopback_only: true,
            capabilities: std::collections::BTreeSet::new(),
        }
    }

    pub fn new(loopback_only: bool) -> Self {
        Self {
            loopback_only,
            capabilities: std::collections::BTreeSet::new(),
        }
    }

    pub fn with_capability(mut self, capability: impl Into<String>) -> Result<Self, JetUiPreviewError> {
        let capability = capability.into();
        jet_ui_preview_text(&capability, "authority capability", 256)?;
        self.capabilities.insert(capability);
        Ok(self)
    }

    pub fn loopback_only(&self) -> bool {
        self.loopback_only
    }

    pub fn allows(&self, capability: &str, loopback: bool) -> bool {
        (!self.loopback_only || loopback)
            && self.capabilities.iter().any(|held| {
                held == capability
                    || capability
                        .strip_prefix(held)
                        .is_some_and(|tail| tail.starts_with('.') || tail.starts_with(':') || tail.starts_with('/'))
            })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetUiPreviewInputValue {
    Text(String),
    Bool(bool),
    Integer(i64),
    Float(f64),
}

impl JetUiPreviewInputValue {
    pub fn validate(&self) -> Result<(), JetUiPreviewError> {
        match self {
            Self::Text(value) => jet_ui_preview_text(value, "preview input", JET_UI_PREVIEW_MAX_TEXT_BYTES),
            Self::Float(value) if !value.is_finite() => Err(JetUiPreviewError::InvalidInput(
                "preview float input must be finite",
            )),
            _ => Ok(()),
        }
    }
}

pub trait JetUiPreviewInputValueLike {
    fn into_jet_ui_preview_input(self) -> JetUiPreviewInputValue;
}

impl JetUiPreviewInputValueLike for JetUiPreviewInputValue {
    fn into_jet_ui_preview_input(self) -> JetUiPreviewInputValue {
        self
    }
}
impl JetUiPreviewInputValueLike for bool {
    fn into_jet_ui_preview_input(self) -> JetUiPreviewInputValue {
        JetUiPreviewInputValue::Bool(self)
    }
}
impl JetUiPreviewInputValueLike for i64 {
    fn into_jet_ui_preview_input(self) -> JetUiPreviewInputValue {
        JetUiPreviewInputValue::Integer(self)
    }
}
impl JetUiPreviewInputValueLike for i32 {
    fn into_jet_ui_preview_input(self) -> JetUiPreviewInputValue {
        JetUiPreviewInputValue::Integer(i64::from(self))
    }
}
impl JetUiPreviewInputValueLike for usize {
    fn into_jet_ui_preview_input(self) -> JetUiPreviewInputValue {
        JetUiPreviewInputValue::Integer(self as i64)
    }
}
impl JetUiPreviewInputValueLike for f64 {
    fn into_jet_ui_preview_input(self) -> JetUiPreviewInputValue {
        JetUiPreviewInputValue::Float(self)
    }
}
impl JetUiPreviewInputValueLike for String {
    fn into_jet_ui_preview_input(self) -> JetUiPreviewInputValue {
        JetUiPreviewInputValue::Text(self)
    }
}
impl JetUiPreviewInputValueLike for &str {
    fn into_jet_ui_preview_input(self) -> JetUiPreviewInputValue {
        JetUiPreviewInputValue::Text(self.to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetUiPreviewInputOverride {
    pub name: String,
    pub value: JetUiPreviewInputValue,
}

impl JetUiPreviewInputOverride {
    pub fn new<Value>(name: impl Into<String>, value: Value) -> Result<Self, JetUiPreviewError>
    where
        Value: JetUiPreviewInputValueLike,
    {
        let input = Self {
            name: name.into(),
            value: value.into_jet_ui_preview_input(),
        };
        jet_ui_preview_text(&input.name, "preview input name", 256)?;
        input.value.validate()?;
        Ok(input)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetUiPreviewSource {
    pub source_id: String,
    pub build_id: String,
    pub revision: String,
    pub span: JetDevtoolsSourceSpan,
}

impl JetUiPreviewSource {
    pub fn new(
        source_id: impl Into<String>,
        build_id: impl Into<String>,
        revision: impl Into<String>,
        span: JetDevtoolsSourceSpan,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            build_id: build_id.into(),
            revision: revision.into(),
            span,
        }
    }

    pub fn validate(&self) -> Result<(), JetUiPreviewError> {
        jet_ui_preview_text(&self.source_id, "source id", JET_UI_PREVIEW_MAX_TEXT_BYTES)
            .map_err(|_| JetUiPreviewError::InvalidSource("source id is invalid"))?;
        jet_ui_preview_text(&self.build_id, "build id", JET_UI_PREVIEW_MAX_TEXT_BYTES)
            .map_err(|_| JetUiPreviewError::InvalidSource("build id is invalid"))?;
        jet_ui_preview_text(&self.revision, "revision", JET_UI_PREVIEW_MAX_TEXT_BYTES)
            .map_err(|_| JetUiPreviewError::InvalidSource("revision is invalid"))?;
        self.span
            .validate()
            .map_err(|_| JetUiPreviewError::InvalidSource("source span is invalid"))?;
        if self.span.source_id != self.source_id {
            return Err(JetUiPreviewError::InvalidSource(
                "source span identity does not match source",
            ));
        }
        Ok(())
    }
}

pub type JetUiPreviewCallback = std::sync::Arc<dyn Fn() -> JetUiNode + Send + Sync>;

pub struct JetUiPreview {
    kind: JetUiPreviewKind,
    name: String,
    callback: JetUiPreviewCallback,
    callback_id: String,
    source: Option<JetUiPreviewSource>,
    viewport: JetUiPreviewViewport,
    traits: JetUiPreviewTraits,
    effects: std::collections::BTreeSet<JetUiPreviewEffect>,
    authority: JetUiPreviewAuthority,
    inputs: std::collections::BTreeMap<String, JetUiPreviewInputValue>,
    context_id: Option<String>,
}

impl Clone for JetUiPreview {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            name: self.name.clone(),
            callback: self.callback.clone(),
            callback_id: self.callback_id.clone(),
            source: self.source.clone(),
            viewport: self.viewport.clone(),
            traits: self.traits.clone(),
            effects: self.effects.clone(),
            authority: self.authority.clone(),
            inputs: self.inputs.clone(),
            context_id: self.context_id.clone(),
        }
    }
}

impl std::fmt::Debug for JetUiPreview {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output
            .debug_struct("JetUiPreview")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("callback_id", &self.callback_id)
            .field("source", &self.source)
            .field("viewport", &self.viewport)
            .field("traits", &self.traits)
            .field("effects", &self.effects)
            .field("authority", &self.authority)
            .field("inputs", &self.inputs)
            .field("context_id", &self.context_id)
            .finish()
    }
}

impl JetUiPreview {
    pub fn new<F>(name: impl Into<String>, callback: F) -> Self
    where
        F: Fn() -> JetUiNode + Send + Sync + 'static,
    {
        let name = name.into();
        Self {
            kind: JetUiPreviewKind::Preview,
            callback: std::sync::Arc::new(callback),
            callback_id: name.clone(),
            name,
            source: None,
            viewport: JetUiPreviewViewport::desktop(),
            traits: JetUiPreviewTraits::default(),
            effects: std::collections::BTreeSet::new(),
            authority: JetUiPreviewAuthority::development(),
            inputs: std::collections::BTreeMap::new(),
            context_id: None,
        }
    }

    pub fn playground<F>(name: impl Into<String>, callback: F) -> Self
    where
        F: Fn() -> JetUiNode + Send + Sync + 'static,
    {
        let mut preview = Self::new(name, callback);
        preview.kind = JetUiPreviewKind::Playground;
        preview
    }

    pub fn with_source(mut self, source: JetUiPreviewSource) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_callback_id(mut self, callback_id: impl Into<String>) -> Self {
        self.callback_id = callback_id.into();
        self
    }

    pub fn with_viewport(mut self, viewport: JetUiPreviewViewport) -> Self {
        self.viewport = viewport;
        self
    }

    pub fn with_traits(mut self, traits: JetUiPreviewTraits) -> Self {
        self.traits = traits;
        self
    }

    pub fn with_authority(mut self, authority: JetUiPreviewAuthority) -> Self {
        self.authority = authority;
        self
    }

    pub fn with_effect(mut self, effect: JetUiPreviewEffect) -> Self {
        self.effects.insert(effect);
        self
    }

    pub fn with_context(mut self, context_id: impl Into<String>) -> Self {
        self.context_id = Some(context_id.into());
        self
    }

    pub fn with_input<Value>(mut self, name: impl Into<String>, value: Value) -> Result<Self, JetUiPreviewError>
    where
        Value: JetUiPreviewInputValueLike,
    {
        let input = JetUiPreviewInputOverride::new(name, value)?;
        if self.inputs.len() >= JET_UI_PREVIEW_MAX_INPUTS && !self.inputs.contains_key(&input.name) {
            return Err(JetUiPreviewError::TooManyInputs);
        }
        self.inputs.insert(input.name, input.value);
        Ok(self)
    }

    pub fn kind(&self) -> JetUiPreviewKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn callback_id(&self) -> &str {
        &self.callback_id
    }

    pub fn source(&self) -> Option<&JetUiPreviewSource> {
        self.source.as_ref()
    }

    pub fn viewport(&self) -> &JetUiPreviewViewport {
        &self.viewport
    }

    pub fn traits(&self) -> &JetUiPreviewTraits {
        &self.traits
    }

    pub fn effects(&self) -> impl Iterator<Item = &JetUiPreviewEffect> {
        self.effects.iter()
    }

    pub fn authority(&self) -> &JetUiPreviewAuthority {
        &self.authority
    }

    pub fn inputs(&self) -> &std::collections::BTreeMap<String, JetUiPreviewInputValue> {
        &self.inputs
    }

    pub fn context_id(&self) -> Option<&str> {
        self.context_id.as_deref()
    }

    pub fn render(&self) -> Result<JetUiNode, JetUiPreviewError> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.callback)()))
            .map_err(|_| JetUiPreviewError::CallbackFailed(self.callback_id.clone()))
    }

    pub fn validate(&self) -> Result<(), JetUiPreviewError> {
        jet_ui_preview_text(&self.name, "preview name", 256)?;
        jet_ui_preview_text(&self.callback_id, "callback id", 256)?;
        let source = self
            .source
            .as_ref()
            .ok_or_else(|| JetUiPreviewError::MissingSource(self.name.clone()))?;
        source.validate()?;
        self.viewport.validate()?;
        self.traits.validate()?;
        for effect in &self.effects {
            effect.validate()?;
            if !self.authority.allows(effect.capability(), true) {
                return Err(JetUiPreviewError::CapabilityDenied(
                    effect.capability().to_string(),
                ));
            }
        }
        if self.inputs.len() > JET_UI_PREVIEW_MAX_INPUTS {
            return Err(JetUiPreviewError::TooManyInputs);
        }
        for (name, value) in &self.inputs {
            JetUiPreviewInputOverride {
                name: name.clone(),
                value: value.clone(),
            }
            .validate()?;
        }
        if let Some(context_id) = &self.context_id {
            jet_ui_preview_text(context_id, "context id", 256)?;
        }
        Ok(())
    }
}

impl JetUiPreviewInputOverride {
    fn validate(&self) -> Result<(), JetUiPreviewError> {
        jet_ui_preview_text(&self.name, "preview input name", 256)?;
        self.value.validate()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetUiPreviewContext {
    pub id: String,
    pub source: JetUiPreviewSource,
    values: std::collections::BTreeMap<String, JetUiPreviewInputValue>,
}

impl JetUiPreviewContext {
    pub fn new<I>(
        id: impl Into<String>,
        source: JetUiPreviewSource,
        values: I,
    ) -> Result<Self, JetUiPreviewError>
    where
        I: IntoIterator<Item = JetUiPreviewInputOverride>,
    {
        let id = id.into();
        jet_ui_preview_text(&id, "context id", 256)?;
        source.validate()?;
        let mut context = Self {
            id,
            source,
            values: std::collections::BTreeMap::new(),
        };
        for value in values {
            value.validate()?;
            if context.values.len() >= JET_UI_PREVIEW_MAX_INPUTS {
                return Err(JetUiPreviewError::TooManyInputs);
            }
            if context.values.contains_key(&value.name) {
                return Err(JetUiPreviewError::DuplicateInput(value.name));
            }
            context.values.insert(value.name, value.value);
        }
        Ok(context)
    }

    pub fn values(&self) -> &std::collections::BTreeMap<String, JetUiPreviewInputValue> {
        &self.values
    }
}

#[derive(Clone, Debug, Default)]
pub struct JetUiPreviewRegistry {
    previews: std::collections::BTreeMap<String, JetUiPreview>,
    selected: std::collections::BTreeSet<String>,
    contexts: std::collections::BTreeMap<String, JetUiPreviewContext>,
}

impl JetUiPreviewRegistry {
    pub fn new<I>(previews: I) -> Result<Self, JetUiPreviewError>
    where
        I: IntoIterator<Item = JetUiPreview>,
    {
        let mut registry = Self::default();
        for preview in previews {
            registry.insert(preview)?;
        }
        registry.validate()?;
        Ok(registry)
    }

    pub fn insert(&mut self, preview: JetUiPreview) -> Result<(), JetUiPreviewError> {
        if self.previews.len() >= JET_UI_PREVIEW_MAX_PREVIEWS {
            return Err(JetUiPreviewError::TooManyPreviews);
        }
        preview.validate()?;
        if self.previews.contains_key(preview.name()) {
            return Err(JetUiPreviewError::DuplicatePreview(preview.name().to_string()));
        }
        self.previews.insert(preview.name().to_string(), preview);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&JetUiPreview> {
        self.previews.get(name)
    }

    pub fn previews(&self) -> impl Iterator<Item = &JetUiPreview> {
        self.previews.values()
    }

    pub fn select(&mut self, name: &str) -> Result<(), JetUiPreviewError> {
        if !self.previews.contains_key(name) {
            return Err(JetUiPreviewError::UnknownPreview(name.to_string()));
        }
        if self.selected.len() >= JET_UI_PREVIEW_MAX_SELECTED && !self.selected.contains(name) {
            return Err(JetUiPreviewError::TooManySelected);
        }
        self.selected.insert(name.to_string());
        Ok(())
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    pub fn register_context(&mut self, context: JetUiPreviewContext) -> Result<(), JetUiPreviewError> {
        if self.contexts.len() >= JET_UI_PREVIEW_MAX_CONTEXTS {
            return Err(JetUiPreviewError::TooManyContexts);
        }
        if self.contexts.contains_key(&context.id) {
            return Err(JetUiPreviewError::DuplicatePreview(context.id));
        }
        self.contexts.insert(context.id.clone(), context);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), JetUiPreviewError> {
        for preview in self.previews.values() {
            preview.validate()?;
            if let Some(context_id) = preview.context_id() {
                let context = self
                    .contexts
                    .get(context_id)
                    .ok_or_else(|| JetUiPreviewError::MissingContext(context_id.to_string()))?;
                if preview.source() != Some(&context.source) {
                    return Err(JetUiPreviewError::InvalidSource(
                        "preview context source does not match preview source",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn render(&self, name: &str) -> Result<JetUiNode, JetUiPreviewError> {
        self.previews
            .get(name)
            .ok_or_else(|| JetUiPreviewError::UnknownPreview(name.to_string()))?
            .render()
    }

    pub fn render_selected(&self) -> Result<Vec<(String, JetUiNode)>, JetUiPreviewError> {
        self.selected
            .iter()
            .map(|name| self.render(name).map(|node| (name.clone(), node)))
            .collect()
    }
}

pub type JetUiPlayground = JetUiPreview;

pub fn jet_ui_preview<F>(name: impl Into<String>, callback: F) -> JetUiPreview
where
    F: Fn() -> JetUiNode + Send + Sync + 'static,
{
    JetUiPreview::new(name, callback)
}

pub fn jet_ui_preview_with_viewport<F>(
    name: impl Into<String>,
    viewport: JetOutcome<JetUiPreviewViewport, JetAbsent>,
    callback: F,
) -> JetUiPreview
where
    F: Fn() -> JetUiNode + Send + Sync + 'static,
{
    let preview = JetUiPreview::new(name, callback);
    if let Ok(viewport) = viewport {
        preview.with_viewport(viewport)
    } else {
        preview
    }
}

pub fn jet_ui_playground_with_viewport<F>(
    name: impl Into<String>,
    viewport: JetOutcome<JetUiPreviewViewport, JetAbsent>,
    callback: F,
) -> JetUiPreview
where
    F: Fn() -> JetUiNode + Send + Sync + 'static,
{
    let preview = JetUiPreview::playground(name, callback);
    if let Ok(viewport) = viewport {
        preview.with_viewport(viewport)
    } else {
        preview
    }
}

pub fn jet_ui_playground<F>(name: impl Into<String>, callback: F) -> JetUiPreview
where
    F: Fn() -> JetUiNode + Send + Sync + 'static,
{
    JetUiPreview::playground(name, callback)
}

pub fn jet_ui_previews<I>(previews: I) -> Result<JetUiPreviewRegistry, JetUiPreviewError>
where
    I: IntoIterator<Item = JetUiPreview>,
{
    JetUiPreviewRegistry::new(previews)
}
pub fn jet_ui_playgrounds<I>(previews: I) -> Result<JetUiPreviewRegistry, JetUiPreviewError>
where
    I: IntoIterator<Item = JetUiPreview>,
{
    JetUiPreviewRegistry::new(previews)
}
pub fn jet_ui_phone() -> JetUiPreviewViewport {
    JetUiPreviewViewport::phone()
}

pub fn jet_ui_tablet() -> JetUiPreviewViewport {
    JetUiPreviewViewport::tablet()
}

pub fn jet_ui_desktop() -> JetUiPreviewViewport {
    JetUiPreviewViewport::desktop()
}

/// Compiler-only provenance attachment for inline preview calls.  The public
/// `core.ui.preview` signature stays name/viewport/callback; lowering supplies
/// this checked source/build identity after the ordinary constructor runs.
#[allow(clippy::too_many_arguments)]
fn jet_ui_preview_attach_compiler_source(
    preview: JetUiPreview,
    source_id: &str,
    source_file: &str,
    build_id: &str,
    revision: &str,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
) -> JetUiPreview {
    let source = JetUiPreviewSource::new(
        source_id,
        build_id,
        revision,
        JetDevtoolsSourceSpan::new(
            source_id,
            source_file,
            start_line,
            start_column,
            end_line,
            end_column,
        ),
    );
    preview.with_source(source)
}
