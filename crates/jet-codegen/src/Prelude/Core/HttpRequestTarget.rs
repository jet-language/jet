/// Project the one origin-form request target whose path is carried by an HTTP request.
/// Core owns validation; this helper only keeps the native and interpreter shapes aligned.
pub(crate) fn jet_http_request_target_path(url: &str) -> String {
    if url.starts_with('/') {
        url.to_string()
    } else {
        String::new()
    }
}

pub(crate) fn jet_http_header_name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

pub(crate) fn jet_http_header_value_valid(value: &str) -> bool {
    value.chars().all(|character| {
        (!character.is_control() || character == '\t')
            && (!character.is_whitespace() || matches!(character, ' ' | '\t'))
    })
}
