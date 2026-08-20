// D-ENCSTREAM-SURFACE1=A: shared, handle-free encoding ABI types.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodingLimits {
    pub buffer_bytes: i64,
    pub max_depth: i64,
    pub max_item_bytes: i64,
    pub max_total_bytes: JetOutcome<i64, JetAbsent>,
    pub max_expansion_depth: i64,
    pub max_expansion_bytes: i64,
}

impl EncodingLimits {
    pub fn safe() -> Self {
        Self {
            buffer_bytes: 65536,
            max_depth: 256,
            max_item_bytes: 16777216,
            max_total_bytes: Err(JetAbsent),
            max_expansion_depth: 32,
            max_expansion_bytes: 8388608,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodingFormat {
    JSON,
    JSONL,
    CSV,
    XML,
    CBOR,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodingErrorKind {
    Syntax,
    Truncated,
    Unsupported,
    Limit,
    IO,
    State,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodingCause {
    pub kind: String,
    pub os_code: JetOutcome<i64, JetAbsent>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodingError {
    pub format: EncodingFormat,
    pub kind: EncodingErrorKind,
    pub byte_offset: i64,
    pub line: JetOutcome<i64, JetAbsent>,
    pub column: JetOutcome<i64, JetAbsent>,
    pub path: String,
    pub reason: String,
    pub cause: JetOutcome<EncodingCause, JetAbsent>,
}
