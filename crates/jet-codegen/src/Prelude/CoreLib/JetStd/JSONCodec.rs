    // Canonical std-only JSON codec shared by AOT, JIT encoding, and tier 0.
    pub fn parse_json(text: &str) -> Result<DataTree, EncodingError> {
        crate::jet_encoding_json::parse_json(text, false).map_err(json_error_from_shared)
    }

    pub fn parse_json_strict(text: &str) -> Result<DataTree, EncodingError> {
        crate::jet_encoding_json::parse_json(text, true).map_err(json_error_from_shared)
    }

    fn json_error_from_shared(error: crate::jet_encoding_json::Error) -> EncodingError {
        EncodingError::new(
            EncodingFormat::JSON,
            EncodingErrorKind::Syntax,
            0,
            Ok(error.line),
            Err(JetAbsent),
            "",
            error.message,
        )
    }

    pub fn is_json_structural_whitespace(c: char) -> bool {
        crate::jet_encoding_json::is_json_structural_whitespace(c)
    }

    pub use crate::jet_encoding_json::render_json;
