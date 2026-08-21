//! Shared validation for the typed service-tree builder.

pub const MAX_NAME_BYTES: usize = 256;

pub fn valid_name(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.chars().any(char::is_control)
        && value.len() <= MAX_NAME_BYTES
}
