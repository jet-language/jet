// D-DATAFLOW1 / card #2447: backend-neutral typed query-plan facts.
//
// This fragment is deliberately a semantic kernel, not a query engine. It
// owns typed identities, schemas, logical operations, validation, deterministic
// inspection, exact integer estimates, and the control state around bounded
// batches. It never owns rows, a source handle, a predicate, or an execution
// operator. Query adapters create these facts; the MIR adapter copies them
// without re-inferring meaning; each backend owns its own storage and operator
// implementation.
//
// Integration points (kept here as a reminder for the later assembly):
// * Query adapters: `Prelude/CoreLib/Top/Compute.rs` and
//   `Prelude/CoreLib/JetStd/CommonTypes.rs` marshal typed source/schema facts
//   into `JetTableSource` and `JetTablePlan`.
// * MIR: `Codegen/TIR/data_plan.rs` projects a validated plan into its MIR
//   carrier. MIR must consume `inspect()`/`facts()` and must not rebuild a
//   graph or choose a backend operator.
// * AOT/JIT/interpreter/web: consume the same plan and `JetTableBatchState`;
//   this file contains no target conditional and no second storage table.

use std::collections::VecDeque;
use std::fmt;

/// Version of this semantic fact shape.  A consumer should reject an unknown
/// version rather than silently guessing at a changed operation or estimate.
pub const JET_LAZY_TABLE_PLAN_SCHEMA_VERSION: u32 = 1;

/// A stable identity may be displayed in diagnostics, but it is not a path,
/// pointer, address, or backend handle.  The value is intentionally opaque to
/// this kernel and is only required to be non-empty and control-free.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableIdentityError {
    pub kind: &'static str,
    pub value: String,
    pub reason: &'static str,
}

impl JetTableIdentityError {
    fn empty(kind: &'static str, value: String) -> Self {
        Self {
            kind,
            value,
            reason: "must not be empty",
        }
    }

    fn control(kind: &'static str, value: String) -> Self {
        Self {
            kind,
            value,
            reason: "must not contain control characters",
        }
    }
}

impl fmt::Display for JetTableIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "table {} identity {:?} {}",
            self.kind, self.value, self.reason
        )
    }
}

impl std::error::Error for JetTableIdentityError {}

fn jet_table_validate_identity(
    value: &str,
    kind: &'static str,
) -> Result<(), JetTableIdentityError> {
    if value.trim().is_empty() {
        return Err(JetTableIdentityError::empty(kind, value.to_string()));
    }
    if value.chars().any(char::is_control) {
        return Err(JetTableIdentityError::control(kind, value.to_string()));
    }
    Ok(())
}

macro_rules! jet_table_string_identity {
    ($name:ident, $kind:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn try_new(value: impl Into<String>) -> Result<Self, JetTableIdentityError> {
                let value = value.into();
                jet_table_validate_identity(&value, $kind)?;
                Ok(Self(value))
            }

            pub fn from_name(value: impl Into<String>) -> Self {
                Self::new(value)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }

            pub fn validate(&self) -> Result<(), JetTableIdentityError> {
                jet_table_validate_identity(&self.0, $kind)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

jet_table_string_identity!(JetTablePlanId, "plan");
jet_table_string_identity!(JetTableSchemaId, "schema");
jet_table_string_identity!(JetTableColumnId, "column");
jet_table_string_identity!(JetTableSourceId, "source");
jet_table_string_identity!(JetTableTypeId, "type");

impl JetTableTypeId {
    pub fn boolean() -> Self {
        Self::new("Bool")
    }

    pub fn string() -> Self {
        Self::new("String")
    }

    pub fn integer() -> Self {
        Self::new("Int")
    }

    pub fn float() -> Self {
        Self::new("Float")
    }

    pub fn any() -> Self {
        Self::new("Any")
    }
}

impl JetTableColumnId {
    /// Derive a stable column identity from its name and type.  The identity
    /// bytes and representation are owned by the Foundation data-flow kernel.
    pub fn derived(name: &str, ty: &JetTableTypeId) -> Self {
        Self::new(jet_foundation::PreludeDataFlow::column_identity(
            name,
            ty.as_str(),
        ))
    }
}

/// Node identity is an insertion index, not a pointer or a hash-map key.  A
/// plan's validation requires IDs to match graph order, which makes a graph
/// inspectable without an unstable map iteration order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JetTableNodeId(pub usize);

impl JetTableNodeId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

impl From<usize> for JetTableNodeId {
    fn from(value: usize) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for JetTableNodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "node:{}", self.0)
    }
}

fn jet_table_fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3_u64);
    }
    hash
}

fn jet_table_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '|' => escaped.push_str("\\|"),
            ';' => escaped.push_str("\\;"),
            '=' => escaped.push_str("\\="),
            ':' => escaped.push_str("\\:"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn jet_table_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len().saturating_add(2));
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0c}' => escaped.push_str("\\f"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

/// A typed field in a row schema.  This is metadata only; it contains no row
/// value and no storage offset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableColumn {
    pub id: JetTableColumnId,
    pub name: String,
    pub ty: JetTableTypeId,
}

impl JetTableColumn {
    pub fn new(name: impl Into<String>, ty: JetTableTypeId) -> Self {
        let name = name.into();
        let id = JetTableColumnId::derived(&name, &ty);
        Self { id, name, ty }
    }

    pub fn with_id(
        id: impl Into<JetTableColumnId>,
        name: impl Into<String>,
        ty: JetTableTypeId,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ty,
        }
    }

    pub fn validate(&self) -> Result<(), JetTableSchemaError> {
        self.id
            .validate()
            .map_err(JetTableSchemaError::Identity)?;
        if self.name.trim().is_empty() {
            return Err(JetTableSchemaError::EmptyColumnName);
        }
        if self.name.chars().any(char::is_control) {
            return Err(JetTableSchemaError::InvalidColumnName(self.name.clone()));
        }
        self.ty
            .validate()
            .map_err(JetTableSchemaError::Identity)
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}",
            jet_table_escape(self.id.as_str()),
            jet_table_escape(&self.name),
            jet_table_escape(self.ty.as_str())
        )
    }
}

/// Schema errors are raised before a node can be admitted to a plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTableSchemaError {
    Identity(JetTableIdentityError),
    EmptyColumnName,
    InvalidColumnName(String),
    DuplicateColumnName(String),
    DuplicateColumnId(JetTableColumnId),
}

impl fmt::Display for JetTableSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(error) => error.fmt(formatter),
            Self::EmptyColumnName => formatter.write_str("table column name must not be empty"),
            Self::InvalidColumnName(name) => {
                write!(formatter, "table column name {:?} contains a control character", name)
            }
            Self::DuplicateColumnName(name) => {
                write!(formatter, "table schema repeats column name `{name}`")
            }
            Self::DuplicateColumnId(id) => {
                write!(formatter, "table schema repeats column identity `{id}`")
            }
        }
    }
}

impl std::error::Error for JetTableSchemaError {}

/// Ordered, typed row schema.  Column order is semantic and is retained by
/// inspection; no map is used for field storage or serialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableSchema {
    pub id: JetTableSchemaId,
    pub row_type: JetTableTypeId,
    pub columns: Vec<JetTableColumn>,
}

impl JetTableSchema {
    pub fn new(row_type: JetTableTypeId, columns: Vec<JetTableColumn>) -> Self {
        let id = Self::derived_id(&row_type, &columns);
        Self {
            id,
            row_type,
            columns,
        }
    }

    pub fn with_id(
        id: impl Into<JetTableSchemaId>,
        row_type: JetTableTypeId,
        columns: Vec<JetTableColumn>,
    ) -> Self {
        Self {
            id: id.into(),
            row_type,
            columns,
        }
    }

    pub fn derived_id(row_type: &JetTableTypeId, columns: &[JetTableColumn]) -> JetTableSchemaId {
        let key = format!(
            "row={};columns={}",
            jet_table_escape(row_type.as_str()),
            columns
                .iter()
                .map(JetTableColumn::canonical_key)
                .collect::<Vec<_>>()
                .join(";")
        );
        JetTableSchemaId::new(format!(
            "schema-{:016x}",
            jet_table_fnv1a64(key.as_bytes())
        ))
    }

    pub fn column(&self, id: &JetTableColumnId) -> Option<&JetTableColumn> {
        self.columns.iter().find(|column| &column.id == id)
    }

    pub fn column_named(&self, name: &str) -> Option<&JetTableColumn> {
        self.columns.iter().find(|column| column.name == name)
    }

    pub fn column_index(&self, id: &JetTableColumnId) -> Option<usize> {
        self.columns.iter().position(|column| &column.id == id)
    }

    pub fn validate(&self) -> Result<(), JetTableSchemaError> {
        self.id
            .validate()
            .map_err(JetTableSchemaError::Identity)?;
        self.row_type
            .validate()
            .map_err(JetTableSchemaError::Identity)?;
        for (index, column) in self.columns.iter().enumerate() {
            column.validate()?;
            if self
                .columns
                .iter()
                .take(index)
                .any(|previous| previous.name == column.name)
            {
                return Err(JetTableSchemaError::DuplicateColumnName(column.name.clone()));
            }
            if self
                .columns
                .iter()
                .take(index)
                .any(|previous| previous.id == column.id)
            {
                return Err(JetTableSchemaError::DuplicateColumnId(column.id.clone()));
            }
        }
        Ok(())
    }

    pub fn canonical_key(&self) -> String {
        format!(
            "schema={}:row={}:columns=[{}]",
            jet_table_escape(self.id.as_str()),
            jet_table_escape(self.row_type.as_str()),
            self.columns
                .iter()
                .map(JetTableColumn::canonical_key)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

/// Explicit estimates never use `None` for an unknown fact.  A reason travels
/// with an unavailable estimate so an inspector can distinguish "not measured"
/// from zero and a backend cannot silently substitute a guess.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTableEstimate<T> {
    Exact(T),
    Unavailable { reason: String },
}

impl<T> JetTableEstimate<T> {
    pub fn exact(value: T) -> Self {
        Self::Exact(value)
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }

    pub fn is_exact(&self) -> bool {
        matches!(self, Self::Exact(_))
    }

    pub fn exact_value(&self) -> Option<&T> {
        match self {
            Self::Exact(value) => Some(value),
            Self::Unavailable { .. } => None,
        }
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        match self {
            Self::Exact(_) => None,
            Self::Unavailable { reason } => Some(reason),
        }
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> JetTableEstimate<U> {
        match self {
            Self::Exact(value) => JetTableEstimate::Exact(map(value)),
            Self::Unavailable { reason } => JetTableEstimate::Unavailable { reason },
        }
    }

    pub fn validate(&self) -> Result<(), JetTableEstimateError> {
        if let Self::Unavailable { reason } = self {
            if reason.trim().is_empty() {
                return Err(JetTableEstimateError::MissingReason);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTableEstimateError {
    MissingReason,
}

impl fmt::Display for JetTableEstimateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingReason => formatter.write_str("unavailable estimate needs a reason"),
        }
    }
}

impl std::error::Error for JetTableEstimateError {}

impl<T: fmt::Display> JetTableEstimate<T> {
    fn canonical_key(&self) -> String {
        match self {
            Self::Exact(value) => format!("exact:{value}"),
            Self::Unavailable { reason } => {
                format!("unavailable:{}", jet_table_escape(reason))
            }
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Exact(value) => format!("exact({value})"),
            Self::Unavailable { reason } => {
                format!("unavailable({})", jet_table_escape(reason))
            }
        }
    }

    fn json(&self) -> String {
        match self {
            Self::Exact(value) => format!("{{\"status\":\"exact\",\"value\":{value}}}"),
            Self::Unavailable { reason } => format!(
                "{{\"status\":\"unavailable\",\"reason\":{}}}",
                jet_table_json_string(reason)
            ),
        }
    }
}

fn jet_table_estimate_add(
    left: &JetTableEstimate<u128>,
    right: &JetTableEstimate<u128>,
    reason: &str,
) -> JetTableEstimate<u128> {
    match (left, right) {
        (JetTableEstimate::Exact(left), JetTableEstimate::Exact(right)) => left
            .checked_add(*right)
            .map(JetTableEstimate::Exact)
            .unwrap_or_else(|| JetTableEstimate::unavailable("integer overflow")),
        _ => JetTableEstimate::unavailable(reason),
    }
}

fn jet_table_estimate_mul(
    left: &JetTableEstimate<u128>,
    right: &JetTableEstimate<u128>,
    reason: &str,
) -> JetTableEstimate<u128> {
    match (left, right) {
        (JetTableEstimate::Exact(left), JetTableEstimate::Exact(right)) => left
            .checked_mul(*right)
            .map(JetTableEstimate::Exact)
            .unwrap_or_else(|| JetTableEstimate::unavailable("integer overflow")),
        _ => JetTableEstimate::unavailable(reason),
    }
}

fn jet_table_estimate_min(
    left: &JetTableEstimate<u128>,
    right: u128,
    reason: &str,
) -> JetTableEstimate<u128> {
    match left {
        JetTableEstimate::Exact(left) => JetTableEstimate::Exact((*left).min(right)),
        JetTableEstimate::Unavailable { .. } => JetTableEstimate::unavailable(reason),
    }
}

/// Exact row/cardinality facts for one node.  `input_rows` follows the node's
/// input identity order; no source row is materialized to obtain these facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableCardinalityFacts {
    pub input_rows: Vec<JetTableEstimate<u128>>,
    pub output_rows: JetTableEstimate<u128>,
    pub rows_read: JetTableEstimate<u128>,
    pub rows_written: JetTableEstimate<u128>,
}

impl JetTableCardinalityFacts {
    pub fn unknown(input_count: usize) -> Self {
        Self {
            input_rows: (0..input_count)
                .map(|_| JetTableEstimate::unavailable("input cardinality unavailable"))
                .collect(),
            output_rows: JetTableEstimate::unavailable("output cardinality unavailable"),
            rows_read: JetTableEstimate::unavailable("rows read unavailable"),
            rows_written: JetTableEstimate::unavailable("rows written unavailable"),
        }
    }

    pub fn validate(&self, input_count: usize) -> Result<(), JetTableFactsError> {
        if self.input_rows.len() != input_count {
            return Err(JetTableFactsError::InputFactCount {
                expected: input_count,
                actual: self.input_rows.len(),
            });
        }
        for estimate in &self.input_rows {
            estimate.validate()?;
        }
        self.output_rows.validate()?;
        self.rows_read.validate()?;
        self.rows_written.validate()?;
        Ok(())
    }

    fn canonical_key(&self) -> String {
        format!(
            "in=[{}];out={};read={};written={}",
            self.input_rows
                .iter()
                .map(JetTableEstimate::canonical_key)
                .collect::<Vec<_>>()
                .join(","),
            self.output_rows.canonical_key(),
            self.rows_read.canonical_key(),
            self.rows_written.canonical_key()
        )
    }

    fn text(&self) -> String {
        format!(
            "input=[{}], output={}, read={}, written={}",
            self.input_rows
                .iter()
                .map(JetTableEstimate::text)
                .collect::<Vec<_>>()
                .join(","),
            self.output_rows.text(),
            self.rows_read.text(),
            self.rows_written.text()
        )
    }

    fn json(&self) -> String {
        format!(
            "{{\"input_rows\":[{}],\"output_rows\":{},\"rows_read\":{},\"rows_written\":{}}}",
            self.input_rows
                .iter()
                .map(JetTableEstimate::json)
                .collect::<Vec<_>>()
                .join(","),
            self.output_rows.json(),
            self.rows_read.json(),
            self.rows_written.json()
        )
    }
}

/// Exact integer cost facts.  These are semantic work units, not wall-clock
/// predictions.  Unknown costs remain explicitly unavailable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableCostFacts {
    pub work_units: JetTableEstimate<u128>,
    pub memory_bytes: JetTableEstimate<u128>,
    pub output_bytes: JetTableEstimate<u128>,
}

impl JetTableCostFacts {
    pub fn unknown() -> Self {
        Self {
            work_units: JetTableEstimate::unavailable("work cost unavailable"),
            memory_bytes: JetTableEstimate::unavailable("memory cost unavailable"),
            output_bytes: JetTableEstimate::unavailable("output byte cost unavailable"),
        }
    }

    pub fn validate(&self) -> Result<(), JetTableFactsError> {
        self.work_units.validate()?;
        self.memory_bytes.validate()?;
        self.output_bytes.validate()?;
        Ok(())
    }

    fn canonical_key(&self) -> String {
        format!(
            "work={};memory={};output={}",
            self.work_units.canonical_key(),
            self.memory_bytes.canonical_key(),
            self.output_bytes.canonical_key()
        )
    }

    fn text(&self) -> String {
        format!(
            "work={}, memory={}, output={}",
            self.work_units.text(),
            self.memory_bytes.text(),
            self.output_bytes.text()
        )
    }

    fn json(&self) -> String {
        format!(
            "{{\"work_units\":{},\"memory_bytes\":{},\"output_bytes\":{}}}",
            self.work_units.json(),
            self.memory_bytes.json(),
            self.output_bytes.json()
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableFacts {
    pub cardinality: JetTableCardinalityFacts,
    pub cost: JetTableCostFacts,
}

impl JetTableFacts {
    pub fn unknown(input_count: usize) -> Self {
        Self {
            cardinality: JetTableCardinalityFacts::unknown(input_count),
            cost: JetTableCostFacts::unknown(),
        }
    }

    pub fn validate(&self, input_count: usize) -> Result<(), JetTableFactsError> {
        self.cardinality.validate(input_count)?;
        self.cost.validate()
    }

    fn canonical_key(&self) -> String {
        format!(
            "cardinality{{{}}};cost{{{}}}",
            self.cardinality.canonical_key(),
            self.cost.canonical_key()
        )
    }

    fn text(&self) -> String {
        format!(
            "cardinality {{{}}}; cost {{{}}}",
            self.cardinality.text(),
            self.cost.text()
        )
    }

    fn json(&self) -> String {
        format!(
            "{{\"cardinality\":{},\"cost\":{}}}",
            self.cardinality.json(),
            self.cost.json()
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTableFactsError {
    InputFactCount { expected: usize, actual: usize },
    Estimate(JetTableEstimateError),
}

impl From<JetTableEstimateError> for JetTableFactsError {
    fn from(error: JetTableEstimateError) -> Self {
        Self::Estimate(error)
    }
}

impl fmt::Display for JetTableFactsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputFactCount { expected, actual } => write!(
                formatter,
                "table node has {actual} input facts; expected {expected}"
            ),
            Self::Estimate(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for JetTableFactsError {}

/// A callable's identity and checked type shape.  The callable itself lives in
/// sema/backend code; only this immutable fact crosses the plan boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableCallable {
    pub id: String,
    pub parameter: JetTableTypeId,
    pub result: JetTableTypeId,
}

impl JetTableCallable {
    pub fn new(
        id: impl Into<String>,
        parameter: JetTableTypeId,
        result: JetTableTypeId,
    ) -> Self {
        Self {
            id: id.into(),
            parameter,
            result,
        }
    }

    pub fn validate(&self) -> Result<(), JetTablePlanError> {
        if self.id.trim().is_empty() {
            return Err(JetTablePlanError::MissingCallableIdentity);
        }
        if self.id.chars().any(char::is_control) {
            return Err(JetTablePlanError::InvalidCallableIdentity(self.id.clone()));
        }
        self.parameter
            .validate()
            .map_err(JetTablePlanError::Identity)?;
        self.result
            .validate()
            .map_err(JetTablePlanError::Identity)
    }

    fn canonical_key(&self) -> String {
        format!(
            "{}:{}:{}",
            jet_table_escape(&self.id),
            jet_table_escape(self.parameter.as_str()),
            jet_table_escape(self.result.as_str())
        )
    }
}

/// Logical operation vocabulary.  Payloads such as a limit or join kind stay
/// on the node so this enum remains one canonical operation discriminant.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum JetTableOperation {
    Scan,
    Select,
    Filter,
    Map,
    Sort,
    Group,
    Join,
    Limit,
}

impl JetTableOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Select => "select",
            Self::Filter => "filter",
            Self::Map => "map",
            Self::Sort => "sort",
            Self::Group => "group",
            Self::Join => "join",
            Self::Limit => "limit",
        }
    }

    pub const fn input_arity(self) -> Option<usize> {
        match self {
            Self::Scan => Some(0),
            Self::Join => Some(2),
            Self::Select
            | Self::Filter
            | Self::Map
            | Self::Sort
            | Self::Group
            | Self::Limit => Some(1),
        }
    }

    pub fn default_stream(self) -> JetTableStreamMode {
        match self {
            Self::Sort => JetTableStreamMode::Materialized {
                reason: "sort requires a complete ordering".to_string(),
            },
            Self::Group => JetTableStreamMode::Materialized {
                reason: "group requires aggregate state".to_string(),
            },
            Self::Join => JetTableStreamMode::Materialized {
                reason: "join requires both input sides".to_string(),
            },
            Self::Scan | Self::Select | Self::Filter | Self::Map | Self::Limit => {
                JetTableStreamMode::Bounded { batch_rows: None }
            }
        }
    }
}

impl fmt::Display for JetTableOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum JetTableJoinKind {
    Inner,
    Left,
}

impl JetTableJoinKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inner => "inner",
            Self::Left => "left",
        }
    }
}

impl fmt::Display for JetTableJoinKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Streaming mode is a fact, not an instruction to an adapter to materialize.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTableStreamMode {
    Bounded { batch_rows: Option<u128> },
    Materialized { reason: String },
}

impl JetTableStreamMode {
    pub fn bounded(batch_rows: Option<u128>) -> Result<Self, JetTableStreamError> {
        if batch_rows == Some(0) {
            return Err(JetTableStreamError::ZeroBatchRows);
        }
        Ok(Self::Bounded { batch_rows })
    }

    pub fn materialized(reason: impl Into<String>) -> Result<Self, JetTableStreamError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(JetTableStreamError::MissingMaterializationReason);
        }
        Ok(Self::Materialized { reason })
    }

    pub const fn is_streaming(&self) -> bool {
        matches!(self, Self::Bounded { .. })
    }

    pub fn canonical_key(&self) -> String {
        match self {
            Self::Bounded { batch_rows: Some(rows) } => format!("bounded:{rows}"),
            Self::Bounded { batch_rows: None } => "bounded:host".to_string(),
            Self::Materialized { reason } => {
                format!("materialized:{}", jet_table_escape(reason))
            }
        }
    }

    pub fn text(&self) -> String {
        match self {
            Self::Bounded { batch_rows: Some(rows) } => format!("bounded(batch_rows={rows})"),
            Self::Bounded { batch_rows: None } => "bounded(batch_rows=host)".to_string(),
            Self::Materialized { reason } => {
                format!("materialized(reason={})", jet_table_escape(reason))
            }
        }
    }

    pub fn validate(&self) -> Result<(), JetTableStreamError> {
        match self {
            Self::Bounded { batch_rows: Some(0) } => Err(JetTableStreamError::ZeroBatchRows),
            Self::Bounded { .. } => Ok(()),
            Self::Materialized { reason } if reason.trim().is_empty() => {
                Err(JetTableStreamError::MissingMaterializationReason)
            }
            Self::Materialized { .. } => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTableStreamError {
    ZeroBatchRows,
    MissingMaterializationReason,
}

impl fmt::Display for JetTableStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroBatchRows => formatter.write_str("table batch row bound must be greater than zero"),
            Self::MissingMaterializationReason => {
                formatter.write_str("table materialization needs a reason")
            }
        }
    }
}

impl std::error::Error for JetTableStreamError {}

/// One logical plan node.  `facts` and all fields are metadata; no callback,
/// table row, reader, or physical operator is stored here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableNode {
    pub id: JetTableNodeId,
    pub operation: JetTableOperation,
    pub inputs: Vec<JetTableNodeId>,
    pub source: Option<JetTableSourceId>,
    pub schema: JetTableSchema,
    pub callable: Option<JetTableCallable>,
    pub stream: JetTableStreamMode,
    pub join_kind: Option<JetTableJoinKind>,
    pub join_keys: Vec<(JetTableColumnId, JetTableColumnId)>,
    pub limit: Option<u128>,
    pub selected_columns: Option<Vec<JetTableColumnId>>,
    pub facts: JetTableFacts,
    pub note: String,
}

impl JetTableNode {
    pub fn new(
        id: JetTableNodeId,
        operation: JetTableOperation,
        inputs: Vec<JetTableNodeId>,
        schema: JetTableSchema,
    ) -> Self {
        let stream = operation.default_stream();
        let facts = JetTableFacts::unknown(inputs.len());
        Self {
            id,
            operation,
            inputs,
            source: None,
            schema,
            callable: None,
            stream,
            join_kind: None,
            join_keys: Vec::new(),
            limit: None,
            selected_columns: None,
            facts,
            note: String::new(),
        }
    }

    pub fn scan(
        id: JetTableNodeId,
        source: JetTableSourceId,
        schema: JetTableSchema,
        facts: JetTableFacts,
    ) -> Self {
        let mut node = Self::new(id, JetTableOperation::Scan, Vec::new(), schema);
        node.source = Some(source);
        node.facts = facts;
        node
    }

    pub fn with_callable(mut self, callable: JetTableCallable) -> Self {
        self.callable = Some(callable);
        self
    }

    pub fn with_stream(mut self, stream: JetTableStreamMode) -> Self {
        self.stream = stream;
        self
    }

    pub fn with_join_kind(mut self, kind: JetTableJoinKind) -> Self {
        self.join_kind = Some(kind);
        self
    }

    pub fn with_join_keys(
        mut self,
        keys: Vec<(JetTableColumnId, JetTableColumnId)>,
    ) -> Self {
        self.join_keys = keys;
        self
    }

    pub fn with_limit(mut self, limit: u128) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_selected_columns(mut self, columns: Vec<JetTableColumnId>) -> Self {
        self.selected_columns = Some(columns);
        self
    }

    pub fn with_facts(mut self, facts: JetTableFacts) -> Self {
        self.facts = facts;
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    fn operation_key(&self) -> String {
        let join = self
            .join_kind
            .map(|kind| kind.as_str().to_string())
            .unwrap_or_else(|| "none".to_string());
        let keys = self
            .join_keys
            .iter()
            .map(|(left, right)| {
                format!(
                    "{}~{}",
                    jet_table_escape(left.as_str()),
                    jet_table_escape(right.as_str())
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let limit = self
            .limit
            .map(|limit| limit.to_string())
            .unwrap_or_else(|| "none".to_string());
        let selected = self
            .selected_columns
            .as_ref()
            .map(|columns| {
                columns
                    .iter()
                    .map(|column| jet_table_escape(column.as_str()))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| "none".to_string());
        format!(
            "{}:join={}:keys=[{}]:limit={}:select=[{}]",
            self.operation.as_str(), join, keys, limit, selected
        )
    }

    fn canonical_key(&self) -> String {
        format!(
            "id={}:op={}:inputs=[{}]:source={}:{}:callable={}:stream={}:facts={}:note={}",
            self.id.index(),
            self.operation_key(),
            self.inputs
                .iter()
                .map(|input| input.index().to_string())
                .collect::<Vec<_>>()
                .join(","),
            self.source
                .as_ref()
                .map(|source| jet_table_escape(source.as_str()))
                .unwrap_or_else(|| "none".to_string()),
            self.schema.canonical_key(),
            self.callable
                .as_ref()
                .map(JetTableCallable::canonical_key)
                .unwrap_or_else(|| "none".to_string()),
            self.stream.canonical_key(),
            self.facts.canonical_key(),
            jet_table_escape(&self.note)
        )
    }

    fn text(&self) -> String {
        format!(
            "node {} op={} inputs=[{}] source={} schema={} callable={} stream={} {} facts={} note={}",
            self.id.index(),
            self.operation_key(),
            self.inputs
                .iter()
                .map(|input| input.index().to_string())
                .collect::<Vec<_>>()
                .join(","),
            self.source
                .as_ref()
                .map(|source| jet_table_escape(source.as_str()))
                .unwrap_or_else(|| "none".to_string()),
            self.schema.canonical_key(),
            self.callable
                .as_ref()
                .map(JetTableCallable::canonical_key)
                .unwrap_or_else(|| "none".to_string()),
            self.stream.text(),
            if self.stream.is_streaming() {
                "fallback=none"
            } else {
                "fallback=materialized"
            },
            self.facts.text(),
            jet_table_escape(&self.note)
        )
    }

    fn json(&self) -> String {
        let inputs = self
            .inputs
            .iter()
            .map(|input| input.index().to_string())
            .collect::<Vec<_>>()
            .join(",");
        let source = self
            .source
            .as_ref()
            .map(|source| jet_table_json_string(source.as_str()))
            .unwrap_or_else(|| "null".to_string());
        let callable = self
            .callable
            .as_ref()
            .map(|callable| {
                format!(
                    "{{\"id\":{},\"parameter\":{},\"result\":{}}}",
                    jet_table_json_string(&callable.id),
                    jet_table_json_string(callable.parameter.as_str()),
                    jet_table_json_string(callable.result.as_str())
                )
            })
            .unwrap_or_else(|| "null".to_string());
        let selected = self
            .selected_columns
            .as_ref()
            .map(|columns| {
                format!(
                    "[{}]",
                    columns
                        .iter()
                        .map(|column| jet_table_json_string(column.as_str()))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .unwrap_or_else(|| "null".to_string());
        let join_kind = self
            .join_kind
            .map(|kind| jet_table_json_string(kind.as_str()))
            .unwrap_or_else(|| "null".to_string());
        let join_keys = self
            .join_keys
            .iter()
            .map(|(left, right)| {
                format!(
                    "[{},{}]",
                    jet_table_json_string(left.as_str()),
                    jet_table_json_string(right.as_str())
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let limit = self
            .limit
            .map(|limit| limit.to_string())
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"id\":{},\"operation\":{},\"inputs\":[{}],\"source\":{},\"schema\":{},\"callable\":{},\"stream\":{},\"join_kind\":{},\"join_keys\":[{}],\"limit\":{},\"selected_columns\":{},\"facts\":{},\"note\":{}}}",
            self.id.index(),
            jet_table_json_string(self.operation.as_str()),
            inputs,
            source,
            jet_table_json_string(&self.schema.canonical_key()),
            callable,
            jet_table_json_string(&self.stream.text()),
            join_kind,
            join_keys,
            limit,
            selected,
            self.facts.json(),
            jet_table_json_string(&self.note)
        )
    }
}

/// Source metadata is the only source-side fact held by this kernel.  A source
/// adapter may fill exact rows/width, but it never passes its storage handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableSource {
    pub id: JetTableSourceId,
    pub schema: JetTableSchema,
    pub rows: JetTableEstimate<u128>,
    pub row_width_bytes: JetTableEstimate<u128>,
}

impl JetTableSource {
    pub fn new(id: impl Into<JetTableSourceId>, schema: JetTableSchema) -> Self {
        Self {
            id: id.into(),
            schema,
            rows: JetTableEstimate::unavailable("source cardinality unavailable"),
            row_width_bytes: JetTableEstimate::unavailable("source row width unavailable"),
        }
    }

    pub fn with_rows(mut self, rows: u128) -> Self {
        self.rows = JetTableEstimate::exact(rows);
        self
    }

    pub fn with_row_width_bytes(mut self, bytes: u128) -> Self {
        self.row_width_bytes = JetTableEstimate::exact(bytes);
        self
    }

    pub fn validate(&self) -> Result<(), JetTablePlanError> {
        self.id.validate().map_err(JetTablePlanError::Identity)?;
        self.schema
            .validate()
            .map_err(JetTablePlanError::Schema)?;
        self.rows.validate().map_err(JetTablePlanError::Estimate)?;
        self.row_width_bytes
            .validate()
            .map_err(JetTablePlanError::Estimate)
    }

    fn canonical_key(&self) -> String {
        format!(
            "source={}:{}:rows={}:width={}",
            jet_table_escape(self.id.as_str()),
            self.schema.canonical_key(),
            self.rows.canonical_key(),
            self.row_width_bytes.canonical_key()
        )
    }
}
/// Build the canonical one-step query plan shared by native and JIT list
/// constructors. The descriptor order is the checked declaration order; the
/// helper deliberately owns all query identity derivation so adapters cannot
/// create incompatible schema, column, source, or plan identities.
pub(crate) fn jet_table_plan_from_descriptors(
    row_type: &str,
    columns: &[String],
    rows: u128,
) -> Result<JetTablePlan, String> {
    let mut pairs = columns.chunks_exact(2);
    let columns = pairs
        .by_ref()
        .map(|pair| {
            JetTableColumn::new(
                pair[0].clone(),
                JetTableTypeId::new(pair[1].clone()),
            )
        })
        .collect();
    if !pairs.remainder().is_empty() {
        return Err("table column metadata must contain alternating name/type pairs".to_string());
    }
    let row_type_id = JetTableTypeId::new(row_type);
    let schema = JetTableSchema::new(row_type_id, columns);
    let source = JetTableSource::new(
        format!("data.query.source::<{row_type}>"),
        schema,
    )
    .with_rows(rows);
    Ok(JetTablePlan::from_source(
        format!("data.query::<{row_type}>"),
        source,
    ))
}

fn jet_table_scan_facts(source: &JetTableSource) -> JetTableFacts {
    let output_bytes = jet_table_estimate_mul(
        &source.rows,
        &source.row_width_bytes,
        "source output bytes unavailable",
    );
    JetTableFacts {
        cardinality: JetTableCardinalityFacts {
            input_rows: Vec::new(),
            output_rows: source.rows.clone(),
            rows_read: source.rows.clone(),
            rows_written: JetTableEstimate::exact(0),
        },
        cost: JetTableCostFacts {
            work_units: source.rows.clone(),
            memory_bytes: JetTableEstimate::exact(0),
            output_bytes,
        },
    }
}

fn jet_table_derive_facts(
    operation: JetTableOperation,
    inputs: &[&JetTableNode],
    limit: Option<u128>,
) -> JetTableFacts {
    let input_rows = inputs
        .iter()
        .map(|node| node.facts.cardinality.output_rows.clone())
        .collect::<Vec<_>>();
    let unknown = |reason: &str| JetTableEstimate::unavailable(reason);
    let (output_rows, rows_read, rows_written, work_units, memory_bytes) = match operation {
        JetTableOperation::Scan => (
            unknown("scan facts require a source"),
            unknown("scan facts require a source"),
            JetTableEstimate::exact(0),
            unknown("scan facts require a source"),
            JetTableEstimate::exact(0),
        ),
        JetTableOperation::Select | JetTableOperation::Map => {
            let input = input_rows
                .first()
                .cloned()
                .unwrap_or_else(|| unknown("unary input cardinality unavailable"));
            (
                input.clone(),
                input.clone(),
                input.clone(),
                input,
                JetTableEstimate::exact(0),
            )
        }
        JetTableOperation::Filter => {
            let input = input_rows
                .first()
                .cloned()
                .unwrap_or_else(|| unknown("filter input cardinality unavailable"));
            (
                unknown("filter selectivity unavailable"),
                input.clone(),
                unknown("filter output cardinality unavailable"),
                input,
                JetTableEstimate::exact(0),
            )
        }
        JetTableOperation::Sort => {
            let input = input_rows
                .first()
                .cloned()
                .unwrap_or_else(|| unknown("sort input cardinality unavailable"));
            (
                input.clone(),
                input.clone(),
                input.clone(),
                unknown("sort comparison cost unavailable"),
                unknown("sort working memory unavailable"),
            )
        }
        JetTableOperation::Group => {
            let input = input_rows
                .first()
                .cloned()
                .unwrap_or_else(|| unknown("group input cardinality unavailable"));
            (
                unknown("group cardinality unavailable"),
                input,
                unknown("group output cardinality unavailable"),
                unknown("group aggregation cost unavailable"),
                unknown("group working memory unavailable"),
            )
        }
        JetTableOperation::Join => {
            let rows_read = match (input_rows.first(), input_rows.get(1)) {
                (Some(left), Some(right)) => {
                    jet_table_estimate_add(left, right, "join input cardinality unavailable")
                }
                _ => unknown("join input cardinality unavailable"),
            };
            (
                unknown("join cardinality unavailable"),
                rows_read,
                unknown("join output cardinality unavailable"),
                unknown("join matching cost unavailable"),
                unknown("join working memory unavailable"),
            )
        }
        JetTableOperation::Limit => {
            let input = input_rows
                .first()
                .cloned()
                .unwrap_or_else(|| unknown("limit input cardinality unavailable"));
            let limit = limit.unwrap_or(0);
            let output = jet_table_estimate_min(
                &input,
                limit,
                "limit output cardinality unavailable",
            );
            (
                output.clone(),
                output.clone(),
                output.clone(),
                output,
                JetTableEstimate::exact(0),
            )
        }
    };
    JetTableFacts {
        cardinality: JetTableCardinalityFacts {
            input_rows,
            output_rows,
            rows_read,
            rows_written,
        },
        cost: JetTableCostFacts {
            work_units,
            memory_bytes,
            output_bytes: unknown("output bytes unavailable without a row width"),
        },
    }
}

/// Plan validation errors are structured so a caller can report the operation,
/// node, input, schema, or identity that failed without parsing display text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTablePlanError {
    Identity(JetTableIdentityError),
    Schema(JetTableSchemaError),
    Estimate(JetTableEstimateError),
    Facts(JetTableFactsError),
    Stream(JetTableStreamError),
    Batch(JetTableBatchError),
    EmptyPlan,
    MissingScan,
    InvalidOutput(JetTableNodeId),
    NodeIdMismatch { expected: usize, actual: JetTableNodeId },
    UnknownInput { node: JetTableNodeId, input: JetTableNodeId },
    InputNotEarlier { node: JetTableNodeId, input: JetTableNodeId },
    Cycle(JetTableNodeId),
    WrongInputArity {
        node: JetTableNodeId,
        operation: JetTableOperation,
        expected: usize,
        actual: usize,
    },
    MissingSource(JetTableNodeId),
    UnexpectedSource(JetTableNodeId),
    UnexpectedCallable(JetTableNodeId),
    UnexpectedJoinMetadata(JetTableNodeId),
    SourceMismatch,
    SchemaMismatch {
        node: JetTableNodeId,
        input: JetTableNodeId,
    },
    SelectionSchemaMismatch(JetTableNodeId),
    MissingCallable(JetTableNodeId),
    MissingCallableIdentity,
    InvalidCallableIdentity(String),
    CallableParameterMismatch {
        node: JetTableNodeId,
        expected: JetTableTypeId,
        actual: JetTableTypeId,
    },
    CallableResultMismatch {
        node: JetTableNodeId,
        expected: JetTableTypeId,
        actual: JetTableTypeId,
    },
    MissingJoinKind(JetTableNodeId),
    InvalidJoinKey {
        node: JetTableNodeId,
        left: JetTableColumnId,
        right: JetTableColumnId,
    },
    MissingLimit(JetTableNodeId),
    UnexpectedLimit(JetTableNodeId),
    UnexpectedSelection(JetTableNodeId),
    UnknownSelectedColumn {
        node: JetTableNodeId,
        column: JetTableColumnId,
    },
    InvalidNote(JetTableNodeId),
    NotStreaming(JetTableNodeId),
}
impl fmt::Display for JetTablePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(error) => error.fmt(formatter),
            Self::Schema(error) => error.fmt(formatter),
            Self::Estimate(error) => error.fmt(formatter),
            Self::Facts(error) => error.fmt(formatter),
            Self::Stream(error) => error.fmt(formatter),
            Self::Batch(error) => error.fmt(formatter),
            Self::EmptyPlan => formatter.write_str("table plan has no nodes"),
            Self::MissingScan => formatter.write_str("table plan must start with a source scan"),
            Self::InvalidOutput(id) => {
                write!(formatter, "table plan output node {} is missing", id.index())
            }
            Self::NodeIdMismatch { expected, actual } => write!(
                formatter,
                "table node id {} does not match graph position {expected}",
                actual.index()
            ),
            Self::UnknownInput { node, input } => write!(
                formatter,
                "table node {} references unknown input {}",
                node.index(),
                input.index()
            ),
            Self::InputNotEarlier { node, input } => write!(
                formatter,
                "table node {} references non-previous input {}",
                node.index(),
                input.index()
            ),
            Self::Cycle(node) => {
                write!(formatter, "table plan contains a cycle at node {}", node.index())
            }
            Self::WrongInputArity {
                node,
                operation,
                expected,
                actual,
            } => write!(
                formatter,
                "table node {} operation `{operation}` needs {expected} inputs, got {actual}",
                node.index()
            ),
            Self::MissingSource(node) => {
                write!(formatter, "table scan node {} has no source", node.index())
            }
            Self::UnexpectedSource(node) => {
                write!(formatter, "non-scan table node {} carries a source", node.index())
            }
            Self::UnexpectedCallable(node) => {
                write!(formatter, "non-transform table node {} carries a callable", node.index())
            }
            Self::UnexpectedJoinMetadata(node) => {
                write!(formatter, "non-join table node {} carries join metadata", node.index())
            }
            Self::SourceMismatch => {
                formatter.write_str("table scan source or schema differs from plan source")
            }
            Self::SchemaMismatch { node, input } => write!(
                formatter,
                "table node {} schema does not preserve input node {} schema",
                node.index(),
                input.index()
            ),
            Self::SelectionSchemaMismatch(node) => write!(
                formatter,
                "table select node {} schema does not match selected columns",
                node.index()
            ),
            Self::MissingCallable(node) => {
                write!(formatter, "table node {} needs a checked callable", node.index())
            }
            Self::MissingCallableIdentity => {
                formatter.write_str("table callable identity must not be empty")
            }
            Self::InvalidCallableIdentity(id) => {
                write!(formatter, "table callable identity {:?} contains a control character", id)
            }
            Self::CallableParameterMismatch {
                node,
                expected,
                actual,
            } => write!(
                formatter,
                "table node {} callable takes {}, expected {}",
                node.index(),
                actual,
                expected
            ),
            Self::CallableResultMismatch {
                node,
                expected,
                actual,
            } => write!(
                formatter,
                "table node {} callable returns {}, expected {}",
                node.index(),
                actual,
                expected
            ),
            Self::MissingJoinKind(node) => {
                write!(formatter, "table join node {} has no join kind", node.index())
            }
            Self::InvalidJoinKey { node, left, right } => write!(
                formatter,
                "table join node {} has incompatible key pair {} / {}",
                node.index(),
                left,
                right
            ),
            Self::MissingLimit(node) => {
                write!(formatter, "table limit node {} has no row bound", node.index())
            }
            Self::UnexpectedLimit(node) => {
                write!(formatter, "non-limit table node {} carries a row bound", node.index())
            }
            Self::UnexpectedSelection(node) => {
                write!(formatter, "non-select table node {} carries selected columns", node.index())
            }
            Self::UnknownSelectedColumn { node, column } => write!(
                formatter,
                "table select node {} references unknown column {}",
                node.index(),
                column
            ),
            Self::InvalidNote(node) => {
                write!(formatter, "table node {} note contains a control character", node.index())
            }
            Self::NotStreaming(node) => {
                write!(formatter, "table node {} is explicitly materialized", node.index())
            }
        }
    }
}
impl std::error::Error for JetTablePlanError {}

/// A typed lazy plan.  The only owned collection is the bounded metadata graph
/// itself; rows remain with source/table adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTablePlan {
    pub id: JetTablePlanId,
    pub source: JetTableSource,
    pub nodes: Vec<JetTableNode>,
    pub output: JetTableNodeId,
}

impl JetTablePlan {
    pub fn new(
        id: impl Into<JetTablePlanId>,
        source: impl Into<JetTableSourceId>,
        schema: JetTableSchema,
    ) -> Self {
        Self::from_source(id, JetTableSource::new(source, schema))
    }

    pub fn from_source(id: impl Into<JetTablePlanId>, source: JetTableSource) -> Self {
        let id = id.into();
        let scan = JetTableNode::scan(
            JetTableNodeId::new(0),
            source.id.clone(),
            source.schema.clone(),
            jet_table_scan_facts(&source),
        );
        Self {
            id,
            source,
            nodes: vec![scan],
            output: JetTableNodeId::new(0),
        }
    }
    /// Append a second source scan without changing the current output node.
    ///
    /// Join plans use this to retain distinct left/right scan identities. The
    /// source schema and bounded cardinality facts remain on the scan node;
    /// rows and source handles stay with the adapter.
    pub fn append_source(
        &mut self,
        source: JetTableSource,
    ) -> Result<JetTableNodeId, JetTablePlanError> {
        self.validate()?;
        let id = JetTableNodeId::new(self.nodes.len());
        let node = JetTableNode::scan(
            id,
            source.id.clone(),
            source.schema.clone(),
            jet_table_scan_facts(&source),
        );
        let output = self.output;
        let node_id = self.append_node(node)?;
        self.output = output;
        Ok(node_id)
    }


    /// Construct a potentially invalid graph for a parser/MIR boundary test;
    /// callers must call `validate` before inspection or execution.
    pub fn from_parts(
        id: impl Into<JetTablePlanId>,
        source: JetTableSource,
        nodes: Vec<JetTableNode>,
        output: JetTableNodeId,
    ) -> Self {
        Self {
            id: id.into(),
            source,
            nodes,
            output,
        }
    }

    pub fn output_node(&self) -> Option<&JetTableNode> {
        self.nodes.get(self.output.index())
    }

    pub fn output_schema(&self) -> Option<&JetTableSchema> {
        self.output_node().map(|node| &node.schema)
    }

    pub fn append_node(&mut self, node: JetTableNode) -> Result<JetTableNodeId, JetTablePlanError> {
        let expected = self.nodes.len();
        if node.id.index() != expected {
            return Err(JetTablePlanError::NodeIdMismatch {
                expected,
                actual: node.id,
            });
        }
        let id = node.id;
        let old_output = self.output;
        self.nodes.push(node);
        self.output = id;
        if let Err(error) = self.validate() {
            self.nodes.pop();
            self.output = old_output;
            return Err(error);
        }
        Ok(id)
    }

    pub fn append(
        &mut self,
        operation: JetTableOperation,
        inputs: Vec<JetTableNodeId>,
        schema: JetTableSchema,
        callable: Option<JetTableCallable>,
    ) -> Result<JetTableNodeId, JetTablePlanError> {
        let id = JetTableNodeId::new(self.nodes.len());
        let mut node = JetTableNode::new(id, operation, inputs, schema);
        node.callable = callable;
        if operation != JetTableOperation::Scan {
            let input_nodes = node
                .inputs
                .iter()
                .filter_map(|input| self.nodes.get(input.index()))
                .collect::<Vec<_>>();
            if input_nodes.len() == node.inputs.len() {
                node.facts = jet_table_derive_facts(operation, &input_nodes, node.limit);
            }
        }
        self.append_node(node)
    }

    pub fn append_unary(
        &mut self,
        operation: JetTableOperation,
        schema: JetTableSchema,
        callable: Option<JetTableCallable>,
    ) -> Result<JetTableNodeId, JetTablePlanError> {
        let input = self.output;
        self.append(operation, vec![input], schema, callable)
    }

    pub fn append_filter(
        &mut self,
        schema: JetTableSchema,
        callable: JetTableCallable,
    ) -> Result<JetTableNodeId, JetTablePlanError> {
        self.append_unary(JetTableOperation::Filter, schema, Some(callable))
    }

    pub fn append_map(
        &mut self,
        schema: JetTableSchema,
        callable: JetTableCallable,
    ) -> Result<JetTableNodeId, JetTablePlanError> {
        self.append_unary(JetTableOperation::Map, schema, Some(callable))
    }

    pub fn append_sort(
        &mut self,
        schema: JetTableSchema,
        callable: JetTableCallable,
    ) -> Result<JetTableNodeId, JetTablePlanError> {
        self.append_unary(JetTableOperation::Sort, schema, Some(callable))
    }

    pub fn append_limit(
        &mut self,
        schema: JetTableSchema,
        limit: u128,
    ) -> Result<JetTableNodeId, JetTablePlanError> {
        self.validate()?;
        let id = JetTableNodeId::new(self.nodes.len());
        let input = self.output_node().ok_or(JetTablePlanError::InvalidOutput(self.output))?;
        let node = JetTableNode::new(id, JetTableOperation::Limit, vec![self.output], schema)
            .with_limit(limit)
            .with_facts(jet_table_derive_facts(
                JetTableOperation::Limit,
                &[input],
                Some(limit),
            ));
        self.append_node(node)
    }

    pub fn append_select(
        &mut self,
        schema: JetTableSchema,
        columns: Vec<JetTableColumnId>,
    ) -> Result<JetTableNodeId, JetTablePlanError> {
        self.validate()?;
        let id = JetTableNodeId::new(self.nodes.len());
        let input = self.output_node().ok_or(JetTablePlanError::InvalidOutput(self.output))?;
        let node = JetTableNode::new(id, JetTableOperation::Select, vec![self.output], schema)
            .with_selected_columns(columns)
            .with_facts(jet_table_derive_facts(
                JetTableOperation::Select,
                &[input],
                None,
            ));
        self.append_node(node)
    }

    pub fn append_join(
        &mut self,
        right: JetTableNodeId,
        schema: JetTableSchema,
        kind: JetTableJoinKind,
        keys: Vec<(JetTableColumnId, JetTableColumnId)>,
    ) -> Result<JetTableNodeId, JetTablePlanError> {
        self.validate()?;
        let id = JetTableNodeId::new(self.nodes.len());
        let left_node = self
            .output_node()
            .ok_or(JetTablePlanError::InvalidOutput(self.output))?;
        let right_node = self
            .nodes
            .get(right.index())
            .ok_or(JetTablePlanError::UnknownInput {
                node: id,
                input: right,
            })?;
        let node = JetTableNode::new(
            id,
            JetTableOperation::Join,
            vec![left_node.id, right],
            schema,
        )
        .with_join_kind(kind)
        .with_join_keys(keys)
        .with_facts(jet_table_derive_facts(
            JetTableOperation::Join,
            &[left_node, right_node],
            None,
        ));
        self.append_node(node)
    }

    /// Validate every identity, schema, graph edge, callable type, operation
    /// shape, stream boundary, and exact-fact shape before any consumer runs.
    pub fn validate(&self) -> Result<(), JetTablePlanError> {
        self.id.validate().map_err(JetTablePlanError::Identity)?;
        self.source.validate()?;
        if self.nodes.is_empty() {
            return Err(JetTablePlanError::EmptyPlan);
        }
        if self.output.index() >= self.nodes.len() {
            return Err(JetTablePlanError::InvalidOutput(self.output));
        }
        let first = &self.nodes[0];
        if first.operation != JetTableOperation::Scan {
            return Err(JetTablePlanError::MissingScan);
        }
        if first.source.as_ref() != Some(&self.source.id) || first.schema != self.source.schema {
            return Err(JetTablePlanError::SourceMismatch);
        }
        jet_table_detect_cycle(&self.nodes)?;
        for (index, node) in self.nodes.iter().enumerate() {
            if node.id.index() != index {
                return Err(JetTablePlanError::NodeIdMismatch {
                    expected: index,
                    actual: node.id,
                });
            }
            node.schema
                .validate()
                .map_err(JetTablePlanError::Schema)?;
            node.stream.validate().map_err(JetTablePlanError::Stream)?;
            node.facts
                .validate(node.inputs.len())
                .map_err(JetTablePlanError::Facts)?;
            if node.note.chars().any(char::is_control) {
                return Err(JetTablePlanError::InvalidNote(node.id));
            }
            if node.operation != JetTableOperation::Limit && node.limit.is_some() {
                return Err(JetTablePlanError::UnexpectedLimit(node.id));
            }
            if node.operation != JetTableOperation::Select && node.selected_columns.is_some() {
                return Err(JetTablePlanError::UnexpectedSelection(node.id));
            }
            if node.operation != JetTableOperation::Join
                && (node.join_kind.is_some() || !node.join_keys.is_empty())
            {
                return Err(JetTablePlanError::UnexpectedJoinMetadata(node.id));
            }
            for input in &node.inputs {
                if input.index() >= self.nodes.len() {
                    return Err(JetTablePlanError::UnknownInput {
                        node: node.id,
                        input: *input,
                    });
                }
                if input.index() >= index {
                    return Err(JetTablePlanError::InputNotEarlier {
                        node: node.id,
                        input: *input,
                    });
                }
            }
            self.validate_node(node)?;
        }
        Ok(())
    }

    fn validate_node(&self, node: &JetTableNode) -> Result<(), JetTablePlanError> {
        let expected = node.operation.input_arity().unwrap_or(node.inputs.len());
        if node.inputs.len() != expected {
            return Err(JetTablePlanError::WrongInputArity {
                node: node.id,
                operation: node.operation,
                expected,
                actual: node.inputs.len(),
            });
        }
        match node.operation {
            JetTableOperation::Scan => {
                if node.id.index() == 0 {
                    if node.source.is_none() {
                        return Err(JetTablePlanError::MissingSource(node.id));
                    }
                } else if node.source.is_none() {
                    return Err(JetTablePlanError::MissingSource(node.id));
                }
                if node.callable.is_some() {
                    return Err(JetTablePlanError::UnexpectedCallable(node.id));
                }
            }
            JetTableOperation::Select => {
                let input = self.unary_input(node)?;
                if node.source.is_some() {
                    return Err(JetTablePlanError::UnexpectedSource(node.id));
                }
                if node.callable.is_some() {
                    return Err(JetTablePlanError::UnexpectedCallable(node.id));
                }
                if let Some(columns) = &node.selected_columns {
                    if node.schema.columns.len() != columns.len() {
                        return Err(JetTablePlanError::SelectionSchemaMismatch(node.id));
                    }
                    for (output, column) in node.schema.columns.iter().zip(columns) {
                        let Some(input_column) = input.schema.column(column) else {
                            return Err(JetTablePlanError::UnknownSelectedColumn {
                                node: node.id,
                                column: column.clone(),
                            });
                        };
                        if output != input_column {
                            return Err(JetTablePlanError::SelectionSchemaMismatch(node.id));
                        }
                    }
                } else if node.schema != input.schema {
                    return Err(JetTablePlanError::SchemaMismatch {
                        node: node.id,
                        input: input.id,
                    });
                }
            }
            JetTableOperation::Filter => {
                self.validate_unary_schema(node)?;
                self.validate_callable(node, JetTableTypeId::boolean(), true)?;
            }
            JetTableOperation::Map => {
                if node.source.is_some() {
                    return Err(JetTablePlanError::UnexpectedSource(node.id));
                }
                let input = self.unary_input(node)?;
                let callable = node
                    .callable
                    .as_ref()
                    .ok_or(JetTablePlanError::MissingCallable(node.id))?;
                callable.validate()?;
                if callable.parameter != input.schema.row_type {
                    return Err(JetTablePlanError::CallableParameterMismatch {
                        node: node.id,
                        expected: input.schema.row_type.clone(),
                        actual: callable.parameter.clone(),
                    });
                }
                if callable.result != node.schema.row_type {
                    return Err(JetTablePlanError::CallableResultMismatch {
                        node: node.id,
                        expected: node.schema.row_type.clone(),
                        actual: callable.result.clone(),
                    });
                }
            }
            JetTableOperation::Sort => {
                self.validate_unary_schema(node)?;
                self.validate_callable(node, JetTableTypeId::string(), true)?;
            }
            JetTableOperation::Group => {
                if node.source.is_some() {
                    return Err(JetTablePlanError::UnexpectedSource(node.id));
                }
                let input = self.unary_input(node)?;
                let callable = node
                    .callable
                    .as_ref()
                    .ok_or(JetTablePlanError::MissingCallable(node.id))?;
                callable.validate()?;
                if callable.parameter != input.schema.row_type {
                    return Err(JetTablePlanError::CallableParameterMismatch {
                        node: node.id,
                        expected: input.schema.row_type.clone(),
                        actual: callable.parameter.clone(),
                    });
                }
            }
            JetTableOperation::Join => {
                if node.source.is_some() {
                    return Err(JetTablePlanError::UnexpectedSource(node.id));
                }
                let _ = node
                    .join_kind
                    .ok_or(JetTablePlanError::MissingJoinKind(node.id))?;
                let left = &self.nodes[node.inputs[0].index()];
                let right = &self.nodes[node.inputs[1].index()];
                for (left_key, right_key) in &node.join_keys {
                    let Some(left_column) = left.schema.column(left_key) else {
                        return Err(JetTablePlanError::InvalidJoinKey {
                            node: node.id,
                            left: left_key.clone(),
                            right: right_key.clone(),
                        });
                    };
                    let Some(right_column) = right.schema.column(right_key) else {
                        return Err(JetTablePlanError::InvalidJoinKey {
                            node: node.id,
                            left: left_key.clone(),
                            right: right_key.clone(),
                        });
                    };
                    if left_column.ty != right_column.ty {
                        return Err(JetTablePlanError::InvalidJoinKey {
                            node: node.id,
                            left: left_key.clone(),
                            right: right_key.clone(),
                        });
                    }
                }
                if node.callable.is_some() {
                    return Err(JetTablePlanError::UnexpectedCallable(node.id));
                }
            }
            JetTableOperation::Limit => {
                self.validate_unary_schema(node)?;
                if node.limit.is_none() {
                    return Err(JetTablePlanError::MissingLimit(node.id));
                }
                if node.callable.is_some() {
                    return Err(JetTablePlanError::UnexpectedCallable(node.id));
                }
            }
        }
        Ok(())
    }

    fn unary_input(&self, node: &JetTableNode) -> Result<&JetTableNode, JetTablePlanError> {
        node.inputs
            .first()
            .and_then(|input| self.nodes.get(input.index()))
            .ok_or(JetTablePlanError::WrongInputArity {
                node: node.id,
                operation: node.operation,
                expected: 1,
                actual: node.inputs.len(),
            })
    }

    fn validate_unary_schema(&self, node: &JetTableNode) -> Result<(), JetTablePlanError> {
        if node.source.is_some() {
            return Err(JetTablePlanError::UnexpectedSource(node.id));
        }
        let input = self.unary_input(node)?;
        if input.schema != node.schema
            && matches!(
                node.operation,
                JetTableOperation::Filter | JetTableOperation::Sort | JetTableOperation::Limit
            )
        {
            return Err(JetTablePlanError::SchemaMismatch {
                node: node.id,
                input: input.id,
            });
        }
        Ok(())
    }

    fn validate_callable(
        &self,
        node: &JetTableNode,
        expected_result: JetTableTypeId,
        result_is_fixed: bool,
    ) -> Result<(), JetTablePlanError> {
        let input = self.unary_input(node)?;
        let callable = node
            .callable
            .as_ref()
            .ok_or(JetTablePlanError::MissingCallable(node.id))?;
        callable.validate()?;
        if callable.parameter != input.schema.row_type {
            return Err(JetTablePlanError::CallableParameterMismatch {
                node: node.id,
                expected: input.schema.row_type.clone(),
                actual: callable.parameter.clone(),
            });
        }
        if result_is_fixed && callable.result != expected_result {
            return Err(JetTablePlanError::CallableResultMismatch {
                node: node.id,
                expected: expected_result,
                actual: callable.result.clone(),
            });
        }
        Ok(())
    }

    pub fn canonical_key(&self) -> String {
        format!(
            "version={};plan={};source={};output={};nodes=[{}]",
            JET_LAZY_TABLE_PLAN_SCHEMA_VERSION,
            jet_table_escape(self.id.as_str()),
            self.source.canonical_key(),
            self.output.index(),
            self.nodes
                .iter()
                .map(JetTableNode::canonical_key)
                .collect::<Vec<_>>()
                .join("|")
        )
    }

    pub fn inspect(&self) -> Result<JetTablePlanInspection, JetTablePlanError> {
        self.validate()?;
        Ok(JetTablePlanInspection {
            plan_id: self.id.clone(),
            source: self.source.clone(),
            output: self.output,
            nodes: self.nodes.clone(),
        })
    }

    pub fn inspect_text(&self) -> Result<String, JetTablePlanError> {
        Ok(self.inspect()?.to_text())
    }

    pub fn inspect_json(&self) -> Result<String, JetTablePlanError> {
        Ok(self.inspect()?.to_json())
    }

    /// Open a bounded metadata batch state only after plan validation.  The
    /// state carries batch descriptors, never row values, so a backend remains
    /// responsible for actual storage and operator execution.
    pub fn begin_batches(&self, capacity: usize) -> Result<JetTableBatchState, JetTablePlanError> {
        self.validate()?;
        let output = self
            .output_node()
            .ok_or(JetTablePlanError::InvalidOutput(self.output))?;
        let JetTableStreamMode::Bounded { batch_rows } = &output.stream else {
            return Err(JetTablePlanError::NotStreaming(output.id));
        };
        let max_rows = output.facts.cardinality.output_rows.exact_value().copied();
        JetTableBatchState::with_limits(capacity, *batch_rows, max_rows)
            .map_err(JetTablePlanError::Batch)
    }
}


/// A deterministic, owned inspection projection.  It preserves node order and
/// all typed identities exactly; it does not sort the graph or collapse nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTablePlanInspection {
    pub plan_id: JetTablePlanId,
    pub source: JetTableSource,
    pub output: JetTableNodeId,
    pub nodes: Vec<JetTableNode>,
}

impl JetTablePlanInspection {
    pub fn canonical_key(&self) -> String {
        format!(
            "version={};plan={};source={};output={};nodes=[{}]",
            JET_LAZY_TABLE_PLAN_SCHEMA_VERSION,
            jet_table_escape(self.plan_id.as_str()),
            self.source.canonical_key(),
            self.output.index(),
            self.nodes
                .iter()
                .map(JetTableNode::canonical_key)
                .collect::<Vec<_>>()
                .join("|")
        )
    }

    pub fn to_text(&self) -> String {
        let mut lines = Vec::with_capacity(self.nodes.len().saturating_add(1));
        lines.push(format!(
            "plan {} source={} output={} version={}",
            jet_table_escape(self.plan_id.as_str()),
            self.source.canonical_key(),
            self.output.index(),
            JET_LAZY_TABLE_PLAN_SCHEMA_VERSION
        ));
        lines.extend(self.nodes.iter().map(JetTableNode::text));
        lines.join("\n")
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"version\":{},\"plan_id\":{},\"source\":{},\"output\":{},\"nodes\":[{}]}}",
            JET_LAZY_TABLE_PLAN_SCHEMA_VERSION,
            jet_table_json_string(self.plan_id.as_str()),
            jet_table_json_string(&self.source.canonical_key()),
            self.output.index(),
            self.nodes
                .iter()
                .map(JetTableNode::json)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

fn jet_table_detect_cycle(nodes: &[JetTableNode]) -> Result<(), JetTablePlanError> {
    let mut colors = vec![0_u8; nodes.len()];
    for start in 0..nodes.len() {
        if colors[start] != 0 {
            continue;
        }
        colors[start] = 1;
        let mut stack = vec![(start, 0_usize)];
        while let Some((node, next_input)) = stack.last_mut() {
            if *next_input >= nodes[*node].inputs.len() {
                colors[*node] = 2;
                stack.pop();
                continue;
            }
            let input = nodes[*node].inputs[*next_input];
            *next_input += 1;
            let input_index = input.index();
            if input_index >= nodes.len() {
                continue;
            }
            match colors[input_index] {
                0 => {
                    colors[input_index] = 1;
                    stack.push((input_index, 0));
                }
                1 => return Err(JetTablePlanError::Cycle(input)),
                _ => {}
            }
        }
    }
    Ok(())
}

/// A batch descriptor is metadata only.  `rows` says how many rows a backend
/// accepted; it does not contain those rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetTableBatch {
    pub sequence: u128,
    pub rows: u128,
    pub truncated: bool,
}

impl JetTableBatch {
    pub fn new(sequence: u128, rows: u128) -> Self {
        Self {
            sequence,
            rows,
            truncated: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTableBatchOffer {
    Accepted(JetTableBatch),
    Truncated {
        batch: Option<JetTableBatch>,
        dropped_rows: u128,
    },
    Backpressure,
    Cancelled,
    Closed,
    ArithmeticOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetTableBatchStatus {
    Open,
    Backpressure,
    Truncated,
    Cancelled,
    Closed,
    Drained,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableBatchSnapshot {
    pub capacity: usize,
    pub queued_batches: usize,
    pub queued_rows: JetTableEstimate<u128>,
    pub accepted_rows: u128,
    pub emitted_rows: u128,
    pub dropped_rows: u128,
    pub backpressure_events: u128,
    pub next_sequence: u128,
    pub truncated: bool,
    pub cancelled: bool,
    pub closed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetTableBatchError {
    ZeroCapacity,
    ZeroBatchRows,
}

impl fmt::Display for JetTableBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => {
                formatter.write_str("table batch capacity must be greater than zero")
            }
            Self::ZeroBatchRows => {
                formatter.write_str("table batch row bound must be greater than zero")
            }
        }
    }
}
impl std::error::Error for JetTableBatchError {}

/// Bounded producer/consumer control state.  The queue stores at most
/// `capacity` descriptors, reports backpressure instead of blocking, and has a
/// terminal cancellation path.  Actual row buffers belong to the adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetTableBatchState {
    capacity: usize,
    max_batch_rows: Option<u128>,
    max_rows: Option<u128>,
    queue: VecDeque<JetTableBatch>,
    next_sequence: u128,
    accepted_rows: u128,
    emitted_rows: u128,
    dropped_rows: u128,
    backpressure_events: u128,
    truncated: bool,
    cancelled: Option<String>,
    closed: bool,
}

impl JetTableBatchState {
    pub fn new(capacity: usize) -> Result<Self, JetTableBatchError> {
        Self::with_limits(capacity, None, None)
    }

    pub fn with_limits(
        capacity: usize,
        max_batch_rows: Option<u128>,
        max_rows: Option<u128>,
    ) -> Result<Self, JetTableBatchError> {
        if capacity == 0 {
            return Err(JetTableBatchError::ZeroCapacity);
        }
        if max_batch_rows == Some(0) {
            return Err(JetTableBatchError::ZeroBatchRows);
        }
        Ok(Self {
            capacity,
            max_batch_rows,
            max_rows,
            queue: VecDeque::with_capacity(capacity),
            next_sequence: 0,
            accepted_rows: 0,
            emitted_rows: 0,
            dropped_rows: 0,
            backpressure_events: 0,
            truncated: false,
            cancelled: None,
            closed: false,
        })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn max_batch_rows(&self) -> Option<u128> {
        self.max_batch_rows
    }

    pub fn max_rows(&self) -> Option<u128> {
        self.max_rows
    }

    pub fn queued_batches(&self) -> usize {
        self.queue.len()
    }

    pub fn available_slots(&self) -> usize {
        self.capacity - self.queue.len()
    }

    pub fn accepted_rows(&self) -> u128 {
        self.accepted_rows
    }

    pub fn emitted_rows(&self) -> u128 {
        self.emitted_rows
    }

    pub fn dropped_rows(&self) -> u128 {
        self.dropped_rows
    }

    pub fn backpressure_events(&self) -> u128 {
        self.backpressure_events
    }

    pub fn is_backpressured(&self) -> bool {
        !self.is_cancelled() && !self.closed && self.queue.len() >= self.capacity
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.is_some()
    }

    pub fn cancellation_reason(&self) -> Option<&str> {
        self.cancelled.as_deref()
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn status(&self) -> JetTableBatchStatus {
        if self.cancelled.is_some() {
            JetTableBatchStatus::Cancelled
        } else if self.truncated {
            if self.queue.is_empty() {
                JetTableBatchStatus::Drained
            } else {
                JetTableBatchStatus::Truncated
            }
        } else if self.closed {
            if self.queue.is_empty() {
                JetTableBatchStatus::Drained
            } else {
                JetTableBatchStatus::Closed
            }
        } else if self.is_backpressured() {
            JetTableBatchStatus::Backpressure
        } else {
            JetTableBatchStatus::Open
        }
    }

    /// Offer only a count.  A full queue returns `Backpressure` without
    /// blocking or consuming input.  A row/batch bound returns `Truncated` and
    /// closes the state so dropped rows are never silently accepted later.
    pub fn offer_rows(&mut self, rows: u128) -> JetTableBatchOffer {
        if self.is_cancelled() {
            return JetTableBatchOffer::Cancelled;
        }
        if self.closed {
            return JetTableBatchOffer::Closed;
        }
        if self.queue.len() >= self.capacity {
            self.backpressure_events = match self.backpressure_events.checked_add(1) {
                Some(value) => value,
                None => return JetTableBatchOffer::ArithmeticOverflow,
            };
            return JetTableBatchOffer::Backpressure;
        }

        let mut accepted = rows;
        let mut dropped = 0_u128;
        if let Some(max_batch_rows) = self.max_batch_rows {
            if accepted > max_batch_rows {
                dropped = accepted - max_batch_rows;
                accepted = max_batch_rows;
            }
        }
        if let Some(max_rows) = self.max_rows {
            if self.accepted_rows > max_rows {
                return JetTableBatchOffer::ArithmeticOverflow;
            }
            let remaining = max_rows - self.accepted_rows;
            if accepted > remaining {
                let newly_dropped = accepted - remaining;
                dropped = match dropped.checked_add(newly_dropped) {
                    Some(value) => value,
                    None => return JetTableBatchOffer::ArithmeticOverflow,
                };
                accepted = remaining;
            }
        }
        let dropped_rows = match self.dropped_rows.checked_add(dropped) {
            Some(value) => value,
            None => return JetTableBatchOffer::ArithmeticOverflow,
        };
        if accepted == 0 {
            if dropped > 0 {
                self.truncated = true;
                self.closed = true;
                self.dropped_rows = dropped_rows;
            }
            return JetTableBatchOffer::Truncated {
                batch: None,
                dropped_rows: dropped,
            };
        }

        let sequence = self.next_sequence;
        let Some(next_sequence) = self.next_sequence.checked_add(1) else {
            return JetTableBatchOffer::ArithmeticOverflow;
        };
        let Some(accepted_rows) = self.accepted_rows.checked_add(accepted) else {
            return JetTableBatchOffer::ArithmeticOverflow;
        };

        self.next_sequence = next_sequence;
        self.accepted_rows = accepted_rows;
        if dropped > 0 {
            self.truncated = true;
            self.closed = true;
            self.dropped_rows = dropped_rows;
        }
        let batch = JetTableBatch {
            sequence,
            rows: accepted,
            truncated: dropped > 0,
        };
        self.queue.push_back(batch);
        if dropped > 0 {
            JetTableBatchOffer::Truncated {
                batch: Some(batch),
                dropped_rows: dropped,
            }
        } else {
            JetTableBatchOffer::Accepted(batch)
        }
    }

    pub fn push(&mut self, rows: u128) -> JetTableBatchOffer {
        self.offer_rows(rows)
    }

    pub fn pop(&mut self) -> Option<JetTableBatch> {
        if self.is_cancelled() {
            return None;
        }
        let batch = self.queue.pop_front()?;
        self.emitted_rows = self
            .emitted_rows
            .checked_add(batch.rows)
            .expect("accepted/emitted row counters cannot overflow");
        Some(batch)
    }
    pub fn close(&mut self) {
        if !self.is_cancelled() {
            self.closed = true;
        }
    }

    pub fn finish(&mut self) {
        self.close();
    }

    pub fn cancel(&mut self, reason: impl Into<String>) {
        if self.cancelled.is_none() {
            let reason = reason.into();
            self.cancelled = Some(if reason.trim().is_empty() {
                "cancelled".to_string()
            } else {
                reason
            });
            self.queue.clear();
            self.closed = true;
        }
    }

    pub fn snapshot(&self) -> JetTableBatchSnapshot {
        let queued_rows = self.queue.iter().try_fold(0_u128, |total, batch| {
            total.checked_add(batch.rows)
        });
        JetTableBatchSnapshot {
            capacity: self.capacity,
            queued_batches: self.queue.len(),
            queued_rows: queued_rows
                .map(JetTableEstimate::exact)
                .unwrap_or_else(|| JetTableEstimate::unavailable("queued row count overflow")),
            accepted_rows: self.accepted_rows,
            emitted_rows: self.emitted_rows,
            dropped_rows: self.dropped_rows,
            backpressure_events: self.backpressure_events,
            next_sequence: self.next_sequence,
            truncated: self.truncated,
            cancelled: self.is_cancelled(),
            closed: self.closed,
        }
    }
}

