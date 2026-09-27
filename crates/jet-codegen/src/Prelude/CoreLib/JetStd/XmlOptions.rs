// D-ENC-XML-SURFACE1=A: checked XML stream policy and limits are shared by
// the AOT Prelude and the resident JIT stream host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XMLLimits {
    pub max_depth: i64,
    pub max_nodes: i64,
    pub max_attributes_per_element: i64,
    pub max_name_bytes: i64,
    pub max_text_bytes: i64,
    pub max_entity_declarations: i64,
    pub max_entity_depth: i64,
    pub max_entity_replacement_bytes: i64,
}

impl XMLLimits {
    pub fn safe() -> Self {
        Self {
            max_depth: 256,
            max_nodes: 1_000_000,
            max_attributes_per_element: 1024,
            max_name_bytes: 4096,
            max_text_bytes: 16_777_216,
            max_entity_declarations: 1024,
            max_entity_depth: 32,
            max_entity_replacement_bytes: 8_388_608,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XMLEntityPolicy {
    Preserve,
    Reject,
    Resolve(std::collections::BTreeMap<String, String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XMLParseOptions {
    pub entities: XMLEntityPolicy,
    pub limits: XMLLimits,
}

impl XMLParseOptions {
    pub fn safe() -> Self {
        Self {
            entities: XMLEntityPolicy::Preserve,
            limits: XMLLimits::safe(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XMLEncoding {
    UTF8,
    UTF8BOM,
    UTF16LE,
    UTF16BE,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XMLLexicalPolicy {
    PreserveValid,
    Deterministic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XMLRenderOptions {
    pub encoding: XMLEncoding,
    pub lexical: XMLLexicalPolicy,
}

impl XMLRenderOptions {
    pub fn safe() -> Self {
        Self {
            encoding: XMLEncoding::UTF8,
            lexical: XMLLexicalPolicy::PreserveValid,
        }
    }
}
