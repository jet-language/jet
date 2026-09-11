// The one backend-neutral shape fact used by text, environment, and argument
// projections.
//
// This file is dependency-free on purpose. The compiler, AOT Prelude, and
// resident adapters consume the same vocabulary instead of defining a second
// shape enum in one of the execution tiers.


/// The source identity carried by every shape and every projection.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeIdentity {
    /// Stable source/module identity, such as a package-qualified file name.
    pub source: String,
    /// Declared type identity inside `source`.
    pub type_name: String,
}

impl ShapeIdentity {
    pub fn new(source: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            type_name: type_name.into(),
        }
    }

    pub fn validate(&self) -> Result<(), ShapeError> {
        if self.source.trim().is_empty() {
            return Err(ShapeError::EmptyIdentity { part: "source" });
        }
        if self.type_name.trim().is_empty() {
            return Err(ShapeError::EmptyIdentity { part: "type_name" });
        }
        if self.source.contains('\0') || self.type_name.contains('\0') {
            return Err(ShapeError::InvalidIdentity {
                source: self.source.clone(),
                type_name: self.type_name.clone(),
            });
        }
        Ok(())
    }

    /// Stable human-readable identity for caches and inspection output.
    pub fn key(&self) -> String {
        format!("{}::{}", self.source, self.type_name)
    }
}

/// A source span retained as provenance without depending on a compiler AST.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeSpan {
    pub start: usize,
    pub end: usize,
}

impl ShapeSpan {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    fn validate(self) -> Result<(), ShapeError> {
        if self.start > self.end {
            return Err(ShapeError::InvalidProvenance);
        }
        Ok(())
    }
}

/// The typed source carrier of one field.
///
/// `Unknown` is a real state. It is never treated as text or as an empty
/// object by a projection.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShapeType {
    Null,
    Bool,
    Int,
    Float,
    Number,
    Text,
    Bytes,
    Array(Box<ShapeType>),
    Optional(Box<ShapeType>),
    Object(ShapeIdentity),
    Unknown(String),
}

impl ShapeType {
    fn validate(&self) -> Result<(), ShapeError> {
        match self {
            Self::Array(inner) | Self::Optional(inner) => inner.validate(),
            Self::Object(identity) => identity.validate(),
            Self::Unknown(name) if name.trim().is_empty() || name.contains('\0') => {
                Err(ShapeError::UnknownType {
                    value: name.clone(),
                })
            }
            _ => Ok(()),
        }
    }
}

/// A typed declaration default retained by every projection.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShapeDefault {
    Null,
    Bool(bool),
    Int(i128),
    /// The source spelling is retained so an exact decimal is not rounded.
    Float(String),
    Text(String),
    Bytes(Vec<u8>),
    Unknown(String),
}

impl ShapeDefault {
    fn validate(&self) -> Result<(), ShapeError> {
        if let Self::Float(value) | Self::Unknown(value) = self {
            if value.trim().is_empty() || value.contains('\0') {
                return Err(ShapeError::InvalidDefault);
            }
        }
        Ok(())
    }
}

/// A source-level origin for a shape fact.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShapeOrigin {
    Declared,
    Derived { from: ShapeIdentity },
    Imported { package: String },
}

/// Provenance retained beside the shape instead of reconstructed from display
/// text or backend metadata.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeProvenance {
    pub origin: ShapeOrigin,
    pub span: Option<ShapeSpan>,
    pub declaration: Option<String>,
}

impl ShapeProvenance {
    pub fn declared() -> Self {
        Self {
            origin: ShapeOrigin::Declared,
            span: None,
            declaration: None,
        }
    }

    pub fn with_span(mut self, span: ShapeSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_declaration(mut self, declaration: impl Into<String>) -> Self {
        self.declaration = Some(declaration.into());
        self
    }

    fn validate(&self) -> Result<(), ShapeError> {
        if self
            .declaration
            .as_deref()
            .is_some_and(|declaration| declaration.contains('\0'))
        {
            return Err(ShapeError::InvalidProvenance);
        }
        if let Some(span) = self.span {
            span.validate()?;
        }
        match &self.origin {
            ShapeOrigin::Declared => Ok(()),
            ShapeOrigin::Derived { from } => from.validate(),
            ShapeOrigin::Imported { package }
                if !package.trim().is_empty() && !package.contains('\0') => Ok(()),
            ShapeOrigin::Imported { .. } => Err(ShapeError::InvalidProvenance),
        }
    }
}

/// The known outer wire carrier of one field or shape.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShapeEncoding {
    Null,
    Bool,
    Int,
    Float,
    Number,
    Text,
    Array,
    Object,
    Unknown(String),
}

impl ShapeEncoding {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Null => "Null",
            Self::Bool => "Bool",
            Self::Int => "Int",
            Self::Float => "Float",
            Self::Number => "Number",
            Self::Text => "Text",
            Self::Array => "Array",
            Self::Object => "Object",
            Self::Unknown(name) => name.as_str(),
        }
    }

    fn validate(&self) -> Result<(), ShapeError> {
        if let Self::Unknown(name) = self {
            if name.trim().is_empty() || name.contains('\0') {
                return Err(ShapeError::UnknownEncoding {
                    value: name.clone(),
                });
            }
        }
        Ok(())
    }
}
/// Physical layout metadata retained by the canonical shape fact.
///
/// `CAligned` is represented as C layout plus an explicit power-of-two
/// alignment. The metadata is carried into every projection so a resident
/// adapter cannot silently discard an ABI promise while selecting field names.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShapeLayoutKind {
    Default,
    C,
    Columnar,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeLayoutFact {
    pub kind: ShapeLayoutKind,
    pub alignment: Option<u64>,
    /// True when the declaration used `align(target, N)` rather than the
    /// portable `align(N)` form.
    pub target_alignment: bool,
}

impl Default for ShapeLayoutFact {
    fn default() -> Self {
        Self {
            kind: ShapeLayoutKind::Default,
            alignment: None,
            target_alignment: false,
        }
    }
}

impl ShapeLayoutFact {
    pub fn c() -> Self {
        Self {
            kind: ShapeLayoutKind::C,
            alignment: None,
            target_alignment: false,
        }
    }

    pub fn c_aligned(alignment: u64) -> Self {
        Self::c_aligned_with_mode(alignment, false)
    }

    pub fn c_aligned_target(alignment: u64) -> Self {
        Self::c_aligned_with_mode(alignment, true)
    }

    pub fn c_aligned_with_mode(alignment: u64, target_alignment: bool) -> Self {
        Self {
            kind: ShapeLayoutKind::C,
            alignment: Some(alignment),
            target_alignment,
        }
    }

    pub fn columnar() -> Self {
        Self {
            kind: ShapeLayoutKind::Columnar,
            alignment: None,
            target_alignment: false,
        }
    }

    pub fn validate(&self) -> Result<(), ShapeError> {
        if self.target_alignment && self.alignment.is_none() {
            return Err(ShapeError::InvalidLayoutAlignment { alignment: 0 });
        }
        if self.alignment.is_some_and(|alignment| {
            alignment == 0
                || !alignment.is_power_of_two()
                || !matches!(&self.kind, ShapeLayoutKind::C)
        }) {
            return Err(ShapeError::InvalidLayoutAlignment {
                alignment: self.alignment.unwrap_or_default(),
            });
        }
        Ok(())
    }
}


/// Resolved names for every supported projection.
///
/// This is a typed record instead of a format-keyed map. The compiler resolves
/// default names and marker overrides once; each projection only selects one
/// slot. `text` is the shared fallback for all text and storage formats.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeFieldNames {
    pub text: String,
    pub json: Option<String>,
    pub cbor: Option<String>,
    pub csv: Option<String>,
    pub toml: Option<String>,
    pub yaml: Option<String>,
    pub xml: Option<String>,
    pub args: String,
    pub env: String,
    pub db: Option<String>,
    pub layout: Option<String>,
}

impl ShapeFieldNames {
    pub fn from_source(source: &str) -> Self {
        Self {
            text: source.to_string(),
            json: None,
            cbor: None,
            csv: None,
            toml: None,
            yaml: None,
            xml: None,
            args: shape_args_name(source),
            env: shape_env_name(source),
            db: None,
            layout: None,
        }
    }

    pub fn shared(mut self, name: impl Into<String>) -> Self {
        self.text = name.into();
        self
    }

    pub fn json(mut self, name: impl Into<String>) -> Self {
        self.json = Some(name.into());
        self
    }

    pub fn cbor(mut self, name: impl Into<String>) -> Self {
        self.cbor = Some(name.into());
        self
    }

    pub fn csv(mut self, name: impl Into<String>) -> Self {
        self.csv = Some(name.into());
        self
    }

    pub fn toml(mut self, name: impl Into<String>) -> Self {
        self.toml = Some(name.into());
        self
    }

    pub fn yaml(mut self, name: impl Into<String>) -> Self {
        self.yaml = Some(name.into());
        self
    }

    pub fn xml(mut self, name: impl Into<String>) -> Self {
        self.xml = Some(name.into());
        self
    }

    pub fn args(mut self, name: impl Into<String>) -> Self {
        self.args = name.into();
        self
    }

    pub fn env(mut self, name: impl Into<String>) -> Self {
        self.env = name.into();
        self
    }

    pub fn db(mut self, name: impl Into<String>) -> Self {
        self.db = Some(name.into());
        self
    }

    pub fn layout(mut self, name: impl Into<String>) -> Self {
        self.layout = Some(name.into());
        self
    }

    /// Return the resolved name for a consumer. Unsupported per-format facts
    /// stay in the canonical record; a projection only selects its slot and
    /// falls back to the shared source name.
    pub fn name_for(&self, projection: ShapeProjectionKind) -> Option<&str> {
        match projection {
            ShapeProjectionKind::Json => self.json.as_deref().or(Some(self.text.as_str())),
            ShapeProjectionKind::Cbor => self.cbor.as_deref().or(Some(self.text.as_str())),
            ShapeProjectionKind::Csv => self.csv.as_deref().or(Some(self.text.as_str())),
            ShapeProjectionKind::Toml => self.toml.as_deref().or(Some(self.text.as_str())),
            ShapeProjectionKind::Yaml => self.yaml.as_deref().or(Some(self.text.as_str())),
            ShapeProjectionKind::Xml => self.xml.as_deref().or(Some(self.text.as_str())),
            ShapeProjectionKind::Args => Some(self.args.as_str()),
            ShapeProjectionKind::Env => Some(self.env.as_str()),
            ShapeProjectionKind::Db => self.db.as_deref().or(Some(self.text.as_str())),
            ShapeProjectionKind::Layout => self.layout.as_deref().or(Some(self.text.as_str())),
        }
    }

    fn validate(&self, source: &str) -> Result<(), ShapeError> {
        for (projection, name) in [
            ("text", Some(self.text.as_str())),
            ("json", self.json.as_deref()),
            ("cbor", self.cbor.as_deref()),
            ("csv", self.csv.as_deref()),
            ("toml", self.toml.as_deref()),
            ("yaml", self.yaml.as_deref()),
            ("xml", self.xml.as_deref()),
            ("args", Some(self.args.as_str())),
            ("env", Some(self.env.as_str())),
            ("db", self.db.as_deref()),
            ("layout", self.layout.as_deref()),
        ] {
            if name.is_some_and(|name| name.trim().is_empty() || name.contains('\0')) {
                return Err(ShapeError::InvalidFieldName {
                    field: source.to_string(),
                    projection: Some(projection.to_string()),
                });
            }
        }
        Ok(())
    }
}

/// One source field fact. All projection-specific marker data is retained in
/// this record, even when a projection ignores it.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeFieldFact {
    pub name: String,
    pub ty: ShapeType,
    /// Source declaration order. It is part of the fact, not inferred from a
    /// backend container's iteration order.
    pub order: usize,
    pub names: ShapeFieldNames,
    pub encoding: ShapeEncoding,
    pub default: Option<ShapeDefault>,
    pub skip: bool,
    pub short: Option<String>,
    pub doc: Option<String>,
}


impl ShapeFieldFact {
    pub fn new(name: impl Into<String>, ty: ShapeType, order: usize) -> Self {
        let name = name.into();
        Self {
            names: ShapeFieldNames::from_source(&name),
            name,
            ty,
            order,
            encoding: ShapeEncoding::Unknown("unresolved".to_string()),
            default: None,
            skip: false,
            short: None,
            doc: None,
        }
    }

    pub fn with_names(mut self, names: ShapeFieldNames) -> Self {
        self.names = names;
        self
    }

    pub fn with_encoding(mut self, encoding: ShapeEncoding) -> Self {
        self.encoding = encoding;
        self
    }

    pub fn with_default(mut self, default: ShapeDefault) -> Self {
        self.default = Some(default);
        self
    }

    pub fn skipped(mut self) -> Self {
        self.skip = true;
        self
    }

    pub fn with_short(mut self, short: impl Into<String>) -> Self {
        self.short = Some(short.into());
        self
    }

    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }

    fn validate(&self) -> Result<(), ShapeError> {
        if self.name.trim().is_empty() || self.name.contains('\0') {
            return Err(ShapeError::InvalidFieldName {
                field: self.name.clone(),
                projection: None,
            });
        }
        self.ty.validate()?;
        self.names.validate(&self.name)?;
        self.encoding.validate()?;
        if let Some(default) = &self.default {
            default.validate()?;
        }
        if self
            .short
            .as_deref()
            .is_some_and(|short| short.trim().is_empty() || short.contains('\0'))
            || self
                .doc
                .as_deref()
                .is_some_and(|doc| doc.contains('\0'))
        {
            return Err(ShapeError::InvalidFieldMetadata {
                field: self.name.clone(),
            });
        }
        Ok(())
    }
}

/// A field after one projection has selected its already-resolved name.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeProjectedField {
    pub source_name: String,
    /// Name consumed by the selected projection.
    pub name: String,
    /// Canonical JSON/DataTree decoder key for the same source field. A
    /// non-JSON projection uses this when it materializes a DataTree for the
    /// shared `Decode` implementation.
    pub decode_name: String,
    pub ty: ShapeType,
    pub order: usize,
    pub encoding: ShapeEncoding,
    pub default: Option<ShapeDefault>,
    /// Only the args projection understands these two fields. Other
    /// projections retain the source facts but expose `None` here.
    pub short: Option<String>,
    pub doc: Option<String>,
}

/// Which consumer is reading a shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShapeProjectionKind {
    Json,
    Cbor,
    Csv,
    Toml,
    Yaml,
    Xml,
    Args,
    Env,
    Db,
    Layout,
}

impl ShapeProjectionKind {
    pub const ALL: [Self; 10] = [
        Self::Json,
        Self::Cbor,
        Self::Csv,
        Self::Toml,
        Self::Yaml,
        Self::Xml,
        Self::Args,
        Self::Env,
        Self::Db,
        Self::Layout,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Cbor => "cbor",
            Self::Csv => "csv",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Xml => "xml",
            Self::Args => "args",
            Self::Env => "env",
            Self::Db => "db",
            Self::Layout => "layout",
        }
    }

    pub const fn is_text(self) -> bool {
        matches!(
            self,
            Self::Json
                | Self::Cbor
                | Self::Csv
                | Self::Toml
                | Self::Yaml
                | Self::Xml
        )
    }
}

/// One typed projection of a canonical shape fact.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeProjection {
    pub kind: ShapeProjectionKind,
    pub identity: ShapeIdentity,
    pub dimensions: ShapeDimensions,
    pub encoding: ShapeEncoding,
    pub layout: ShapeLayoutFact,
    pub fields: Vec<ShapeProjectedField>,
    pub provenance: ShapeProvenance,
}

impl ShapeProjection {
    pub fn field(&self, name: &str) -> Option<&ShapeProjectedField> {
        self.fields.iter().find(|field| field.name == name)
    }
}

/// The encoded result of applying one shape projection.
///
/// Text formats and JSON return their rendered text. CBOR returns raw bytes so
/// binary output is never routed through a string. Adapters must not add a
/// third shape-specific carrier.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShapeProjectionResult {
    Text(String),
    Bytes(Vec<u8>),
}

/// Shape extent known by the front end.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShapeDimensions {
    Known { rank: usize, width: usize },
    Unknown { reason: String },
}

impl ShapeDimensions {
    pub fn record(width: usize) -> Self {
        Self::Known { rank: 1, width }
    }

    pub fn known(rank: usize, width: usize) -> Result<Self, ShapeError> {
        if rank == 0 {
            return Err(ShapeError::InvalidDimensions {
                reason: "rank must be greater than zero".to_string(),
            });
        }
        Ok(Self::Known { rank, width })
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Self::Unknown {
            reason: reason.into(),
        }
    }

    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known { .. })
    }
}

/// The complete canonical fact. `fields` is sorted by source order during
/// construction, and duplicate source/projection names are rejected before a
/// projection can observe them.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ShapeFact {
    pub identity: ShapeIdentity,
    pub dimensions: ShapeDimensions,
    pub fields: Vec<ShapeFieldFact>,
    pub encoding: ShapeEncoding,
    pub layout: ShapeLayoutFact,
    pub provenance: ShapeProvenance,
}

impl ShapeFact {
    pub fn new(
        identity: ShapeIdentity,
        dimensions: ShapeDimensions,
        mut fields: Vec<ShapeFieldFact>,
        encoding: ShapeEncoding,
        provenance: ShapeProvenance,
    ) -> Result<Self, ShapeError> {
        fields.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.name.cmp(&right.name))
        });
        let fact = Self {
            identity,
            dimensions,
            fields,
            encoding,
            layout: ShapeLayoutFact::default(),
            provenance,
        };
        fact.validate()?;
        Ok(fact)
    }

    pub fn with_layout(mut self, layout: ShapeLayoutFact) -> Result<Self, ShapeError> {
        layout.validate()?;
        self.layout = layout;
        Ok(self)
    }

    pub fn record(
        identity: ShapeIdentity,
        fields: Vec<ShapeFieldFact>,
        provenance: ShapeProvenance,
    ) -> Result<Self, ShapeError> {
        let width = fields.len();
        Self::new(
            identity,
            ShapeDimensions::record(width),
            fields,
            ShapeEncoding::Object,
            provenance,
        )
    }

    /// Preserve an incomplete source fact without guessing its carrier or
    /// fields. Every projection returns `UnknownShape` until the front end
    /// supplies a resolved fact.
    pub fn unknown(
        identity: ShapeIdentity,
        reason: impl Into<String>,
        provenance: ShapeProvenance,
    ) -> Result<Self, ShapeError> {
        Self::new(
            identity,
            ShapeDimensions::unknown(reason),
            Vec::new(),
            ShapeEncoding::Unknown("unresolved".to_string()),
            provenance,
        )
    }

    pub fn validate(&self) -> Result<(), ShapeError> {
        self.identity.validate()?;
        self.provenance.validate()?;
        self.encoding.validate()?;
        self.layout.validate()?;
        match &self.dimensions {
            ShapeDimensions::Known { rank, width } => {
                if *rank == 0 || *width != self.fields.len() {
                    return Err(ShapeError::InvalidDimensions {
                        reason: format!("rank {rank} and width {width} do not match fields"),
                    });
                }
            }
            ShapeDimensions::Unknown { reason } if reason.trim().is_empty() => {
                return Err(ShapeError::InvalidDimensions {
                    reason: "unknown dimensions need a reason".to_string(),
                });
            }
            ShapeDimensions::Unknown { .. } => {}
        }
        if !self.fields.is_empty()
            && !matches!(self.encoding, ShapeEncoding::Object | ShapeEncoding::Unknown(_))
        {
            return Err(ShapeError::EncodingMismatch {
                identity: self.identity.key(),
                encoding: self.encoding.as_str().to_string(),
            });
        }

        let mut source_names = std::collections::BTreeSet::new();
        let mut orders = std::collections::BTreeSet::new();
        for field in &self.fields {
            field.validate()?;
            if let ShapeEncoding::Unknown(value) = &field.encoding {
                return Err(ShapeError::UnknownFieldEncoding {
                    field: field.name.clone(),
                    value: value.clone(),
                });
            }
            if !source_names.insert(field.name.clone()) {
                return Err(ShapeError::DuplicateField {
                    name: field.name.clone(),
                });
            }
            if !orders.insert(field.order) {
                return Err(ShapeError::AmbiguousFieldOrder { order: field.order });
            }
        }
        for projection in ShapeProjectionKind::ALL {
            let mut names = std::collections::BTreeSet::new();
            for field in self.fields.iter().filter(|field| !field.skip) {
                let name = field
                    .names
                    .name_for(projection)
                    .ok_or_else(|| ShapeError::MissingProjectionName {
                        field: field.name.clone(),
                        projection: projection.as_str().to_string(),
                    })?;
                if !names.insert(name.to_string()) {
                    return Err(ShapeError::DuplicateProjectionName {
                        projection: projection.as_str().to_string(),
                        name: name.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Project one consumer view without changing the canonical fact.
    pub fn project(&self, kind: ShapeProjectionKind) -> Result<ShapeProjection, ShapeError> {
        self.validate()?;
        if !self.dimensions.is_known() || matches!(self.encoding, ShapeEncoding::Unknown(_)) {
            return Err(ShapeError::UnknownShape {
                identity: self.identity.key(),
            });
        }
        let fields = self
            .fields
            .iter()
            .filter(|field| !field.skip)
            .map(|field| {
                let name = field
                    .names
                    .name_for(kind)
                    .ok_or_else(|| ShapeError::MissingProjectionName {
                        field: field.name.clone(),
                        projection: kind.as_str().to_string(),
                    })?;
                let decode_name = field
                    .names
                    .name_for(ShapeProjectionKind::Json)
                    .ok_or_else(|| ShapeError::MissingProjectionName {
                        field: field.name.clone(),
                        projection: ShapeProjectionKind::Json.as_str().to_string(),
                    })?
                    .to_string();
                Ok(ShapeProjectedField {
                    source_name: field.name.clone(),
                    name: name.to_string(),
                    decode_name,
                    ty: field.ty.clone(),
                    order: field.order,
                    encoding: field.encoding.clone(),
                    default: field.default.clone(),
                    short: (kind == ShapeProjectionKind::Args)
                        .then(|| field.short.clone())
                        .flatten(),
                    doc: (kind == ShapeProjectionKind::Args)
                        .then(|| field.doc.clone())
                        .flatten(),
                })
            })
            .collect::<Result<Vec<_>, ShapeError>>()?;
        Ok(ShapeProjection {
            kind,
            identity: self.identity.clone(),
            dimensions: self.dimensions.clone(),
            encoding: self.encoding.clone(),
            layout: self.layout.clone(),
            fields,
            provenance: self.provenance.clone(),
        })
    }
}

/// Explicit failures from shape construction and projection.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShapeError {
    EmptyIdentity { part: &'static str },
    InvalidIdentity { source: String, type_name: String },
    InvalidProvenance,
    InvalidDimensions { reason: String },
    InvalidLayoutAlignment { alignment: u64 },
    EncodingMismatch { identity: String, encoding: String },
    InvalidFieldName { field: String, projection: Option<String> },
    InvalidFieldMetadata { field: String },
    InvalidDefault,
    UnknownType { value: String },
    UnknownEncoding { value: String },
    UnknownFieldEncoding { field: String, value: String },
    DuplicateField { name: String },
    AmbiguousFieldOrder { order: usize },
    MissingProjectionName { field: String, projection: String },
    DuplicateProjectionName { projection: String, name: String },
    UnknownShape { identity: String },
}

impl std::fmt::Display for ShapeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentity { part } => write!(formatter, "shape {part} is empty"),
            Self::InvalidIdentity { source, type_name } => {
                write!(formatter, "invalid shape identity {source}::{type_name}")
            }
            Self::InvalidProvenance => formatter.write_str("invalid shape provenance"),
            Self::InvalidDimensions { reason } => {
                write!(formatter, "invalid shape dimensions: {reason}")
            }
            Self::InvalidLayoutAlignment { alignment } => {
                write!(formatter, "invalid shape layout alignment {alignment}")
            }
            Self::EncodingMismatch {
                identity,
                encoding,
            } => write!(formatter, "shape {identity} cannot carry fields as {encoding}"),
            Self::InvalidFieldName { field, projection } => match projection {
                Some(projection) => {
                    write!(formatter, "invalid {projection} name for field {field}")
                }
                None => write!(formatter, "invalid shape field name {field}"),
            },
            Self::InvalidFieldMetadata { field } => {
                write!(formatter, "invalid metadata for field {field}")
            }
            Self::InvalidDefault => formatter.write_str("invalid shape default"),
            Self::UnknownType { value } => write!(formatter, "unknown shape type {value}"),
            Self::UnknownEncoding { value } => write!(formatter, "unknown shape encoding {value}"),
            Self::UnknownFieldEncoding { field, value } => {
                write!(formatter, "field {field} has unknown shape encoding {value}")
            }
            Self::DuplicateField { name } => write!(formatter, "duplicate shape field {name}"),
            Self::AmbiguousFieldOrder { order } => {
                write!(formatter, "ambiguous shape field order {order}")
            }
            Self::MissingProjectionName { field, projection } => {
                write!(formatter, "field {field} has no {projection} projection name")
            }
            Self::DuplicateProjectionName { projection, name } => {
                write!(formatter, "duplicate {projection} projection name {name}")
            }
            Self::UnknownShape { identity } => write!(formatter, "shape {identity} is unknown"),
        }
    }
}

impl std::error::Error for ShapeError {}

fn shape_args_name(source: &str) -> String {
    source.replace('_', "-")
}

fn shape_env_name(source: &str) -> String {
    source.replace('-', "_").to_ascii_uppercase()
}
