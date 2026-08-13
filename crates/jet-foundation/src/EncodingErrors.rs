//! One parse-error vocabulary for text encodings.

pub const JSON_EXPECTED_VALUE: &str = "expected a JSON value";
pub const JSON_EXPECTED_WORD: &str = "expected a JSON word";
pub const JSON_EXPECTED_QUOTED_TEXT: &str = "expected quoted text";
pub const JSON_UNFINISHED_ESCAPE: &str = "unfinished escape";
pub const JSON_INVALID_ESCAPE: &str = "invalid escape in string";
pub const JSON_CONTROL_CHARACTER: &str = "control character in string";
pub const JSON_MISSING_CLOSING_QUOTE: &str = "missing closing quote";
pub const JSON_UNPAIRED_SURROGATE: &str = "unpaired surrogate in string";
pub const JSON_INVALID_UNICODE_ESCAPE: &str = "invalid unicode escape";
pub const JSON_TRUNCATED_UNICODE_ESCAPE: &str = "truncated unicode escape";
pub const JSON_BAD_NUMBER: &str = "bad number";
pub const JSON_EXPECTED_ARRAY_SEPARATOR: &str = "expected `,` or `]`";
pub const JSON_EXPECTED_OBJECT_SEPARATOR: &str = "expected `,` or `}`";
pub const JSON_EXPECTED_OBJECT_COLON: &str = "expected `:` after object key";
pub const JSON_DUPLICATE_OBJECT_KEY: &str = "duplicate JSON object key";
pub const JSON_EXTRA_TEXT: &str = "extra text after JSON value";

pub const TOML_EXPECTED_TABLE_CLOSE: &str = "expected `]` to close a table header";
pub const TOML_EXPECTED_ARRAY_TABLE_CLOSE: &str =
    "expected `]]` to close an array-of-tables header";
pub const TOML_TABLE_NAME_REQUIRED: &str = "a table header must name a table";
pub const TOML_EXPECTED_KEY: &str = "expected a key";
pub const TOML_EXPECTED_VALUE: &str = "expected a value";
pub const TOML_EXPECTED_BOOLEAN: &str = "expected `true` or `false`";
pub const TOML_UNTERMINATED_STRING: &str = "unterminated string";
pub const TOML_CONTROL_CHARACTER: &str = "control character in string";
pub const TOML_UNTERMINATED_MULTILINE_STRING: &str = "unterminated multi-line string";
pub const TOML_UNTERMINATED_ESCAPE: &str = "unterminated escape";
pub const TOML_TRUNCATED_UNICODE_ESCAPE: &str = "truncated unicode escape";
pub const TOML_INVALID_UNICODE_ESCAPE: &str = "invalid unicode escape";
pub const TOML_INVALID_UNICODE_SCALAR: &str = "invalid unicode scalar value";
pub const TOML_UNTERMINATED_LITERAL_STRING: &str = "unterminated literal string";
pub const TOML_UNTERMINATED_MULTILINE_LITERAL_STRING: &str =
    "unterminated multi-line literal string";
pub const TOML_UNTERMINATED_ARRAY: &str = "unterminated array";
pub const TOML_EXPECTED_INLINE_EQUALS: &str = "expected `=` in inline table";
pub const TOML_UNTERMINATED_INLINE_TABLE: &str = "unterminated inline table";
pub const TOML_EXPECTED_RADIX_DIGITS: &str = "expected digits after numeric base prefix";

pub fn toml_unexpected_after_value(c: char) -> String {
    format!("unexpected `{c}` after value")
}

pub fn toml_expected_equals_after_key(key: &str) -> String {
    format!("expected `=` after key `{key}`")
}

pub fn toml_invalid_key_character(c: char) -> String {
    format!("`{c}` is not a valid key character")
}

pub fn toml_invalid_value_start(c: char) -> String {
    format!("`{c}` does not start a valid value")
}

pub fn toml_invalid_escape(c: char) -> String {
    format!("invalid escape `\\{c}`")
}

pub fn toml_expected_array_separator(c: char) -> String {
    format!("expected `,` or `]` in array, found `{c}`")
}

pub fn toml_expected_inline_separator(c: char) -> String {
    format!("expected `,` or `}}` in inline table, found `{c}`")
}

pub fn toml_invalid_number(token: &str) -> String {
    format!("invalid number `{token}`")
}

pub fn toml_invalid_radix_integer(radix: u32, token: &str) -> String {
    format!("invalid base-{radix} integer `{token}`")
}

pub const YAML_EXPECTED_KEY_VALUE: &str = "expected `key: value`";
