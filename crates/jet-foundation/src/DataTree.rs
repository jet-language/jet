//! Canonical dependency-free data tree shared by Foundation and the embedded Prelude.
//!
//! Every text format and protocol parser projects into this ordered carrier.  The
//! object representation deliberately stays a list so insertion order is retained;
//! parsers enforce duplicate-key policy at their boundary instead of collapsing data
//! through a map.

#[derive(Clone, Debug, PartialEq)]
pub enum DataTree {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// Exact numeric lexeme retained for typed decoding before a target type is known.
    Number(String),
    /// Text retained by typed decoding before a target type is known.
    TypedText(String),
    Text(String),
    Bytes(Vec<u8>),
    Array(Vec<DataTree>),
    Object(Vec<(String, DataTree)>),
}

impl DataTree {
    pub fn as_array(&self) -> Result<&Vec<DataTree>, String> {
        match self {
            Self::Array(values) => Ok(values),
            _ => Err("expected a DataTree array".to_string()),
        }
    }

    pub fn as_object(&self) -> Result<&Vec<(String, DataTree)>, String> {
        match self {
            Self::Object(values) => Ok(values),
            _ => Err("expected a DataTree object".to_string()),
        }
    }

    pub fn as_str(&self) -> Result<&str, String> {
        match self {
            Self::Text(value) | Self::TypedText(value) => Ok(value),
            _ => Err("expected a DataTree string".to_string()),
        }
    }

    pub fn get<'a>(&'a self, key: &str) -> Result<&'a DataTree, String> {
        self.get_opt(key)
            .ok_or_else(|| format!("missing key `{key}`"))
    }

    pub fn get_opt(&self, key: &str) -> Option<&DataTree> {
        match self {
            Self::Object(values) => values
                .iter()
                .find_map(|(name, value)| (name == key).then_some(value)),
            _ => None,
        }
    }

    pub fn object_entries(&self) -> Option<&[(String, DataTree)]> {
        match self {
            Self::Object(values) => Some(values),
            _ => None,
        }
    }
}
