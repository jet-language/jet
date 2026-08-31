// ASCII case conversion is deliberately narrower than Unicode case mapping.
// Only the 26 ASCII letters change; every other UTF-8 byte sequence is copied
// unchanged.  This is the one semantic kernel used by AOT, JIT, and TIR eval.

pub(crate) fn jet_text_ascii_lower(s: &String) -> String {
    s.chars()
        .map(|character| match character {
            'A'..='Z' => ((character as u8) + (b'a' - b'A')) as char,
            other => other,
        })
        .collect()
}

pub(crate) fn jet_text_ascii_upper(s: &String) -> String {
    s.chars()
        .map(|character| match character {
            'a'..='z' => ((character as u8) - (b'a' - b'A')) as char,
            other => other,
        })
        .collect()
}
