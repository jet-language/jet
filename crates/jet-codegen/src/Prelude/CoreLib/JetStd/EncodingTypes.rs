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

// BEGIN GENERATED CORE ENCODING FORMATS
// Source: crates/jet-codegen/src/Prelude/Core.jet
// Source SHA-256: 899a77cc91eadc9185e289e89a0dec593cbb1ce6f0834dadf77fdac0ae061648
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum EncodingFormat {
    JSON,
    JSONL,
    CSV,
    TOML,
    YAML,
    XML,
    CBOR,
}
impl EncodingFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::JSON => "JSON",
            Self::JSONL => "JSONL",
            Self::CSV => "CSV",
            Self::TOML => "TOML",
            Self::YAML => "YAML",
            Self::XML => "XML",
            Self::CBOR => "CBOR",
        }
    }
}
// END GENERATED CORE ENCODING FORMATS
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
impl EncodingError {
    /// Build the seven always-present error fields. `cause` is populated only
    /// by the shared IO projection, so whole-value codecs cannot invent a
    /// format-specific error carrier.
    pub fn new(
        format: EncodingFormat,
        kind: EncodingErrorKind,
        byte_offset: i64,
        line: JetOutcome<i64, JetAbsent>,
        column: JetOutcome<i64, JetAbsent>,
        path: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            format,
            kind,
            byte_offset,
            line,
            column,
            path: path.into(),
            reason: reason.into(),
            cause: Err(JetAbsent),
        }
    }

    /// Attach the handle-free cause snapshot used only by IO failures.
    pub fn with_cause(mut self, cause: EncodingCause) -> Self {
        self.cause = Ok(cause);
        self
    }
}

/// Decode CSV data rows through a caller-supplied typed carrier.
///
/// The first row supplies object field names. Every following row is decoded,
/// including rows after the first failure, so accumulated errors retain source
/// order. `frame_errors` supplies the carrier-specific row-path projection;
/// `make_text` and `make_object` keep this kernel independent of the carrier.
pub(crate) fn jet_enc_csv_decode_rows<T, I, V, F, E, P, C, O>(
    rows: I,
    mut decode: F,
    mut frame_errors: P,
    mut make_text: C,
    mut make_object: O,
) -> Result<Vec<T>, Vec<E>>
where
    I: IntoIterator<Item = Vec<String>>,
    F: FnMut(V) -> Result<T, Vec<E>>,
    P: FnMut(&str, Vec<E>) -> Vec<E>,
    C: FnMut(String) -> V,
    O: FnMut(Vec<(String, V)>) -> V,
{
    let mut rows = rows.into_iter();
    let Some(header) = rows.next() else {
        return Ok(Vec::new());
    };
    let mut values = Vec::new();
    let mut errors = Vec::new();
    for (index, row) in rows.enumerate() {
        let mut cells = row.into_iter();
        let object = header
            .iter()
            .map(|name| (name.clone(), make_text(cells.next().unwrap_or_default())))
            .collect();
        match decode(make_object(object)) {
            Ok(value) => values.push(value),
            Err(error) => {
                let row = format!("row {}", index + 1);
                errors.extend(frame_errors(&row, error));
            }
        }
    }
    if errors.is_empty() {
        Ok(values)
    } else {
        Err(errors)
    }
}
