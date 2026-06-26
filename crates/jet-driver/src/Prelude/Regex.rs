// jet.regex runtime (D-REGEX1) — the ONLY code that touches the `regex` crate.
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs). The compiler crate (`Source/`) never depends on `regex`;
// it only ships this text. Owner-approved I6 bootstrap exception: replace with
// an in-house RE2-style engine before the end of Epoch 3.
//
// The `regex` crate is a DFA/NFA hybrid: linear-time, no catastrophic
// backtracking (no ReDoS). We expose none of the features it forbids
// (backreferences, lookaround) because the crate simply doesn't have them —
// the safety property is preserved by construction.
//
// A `Match` is represented to Jet as `Vec<Option<String>>`: index 0 is the
// whole match, index n is capture group n (None if the group did not
// participate). `Match.group(n)` is plain indexing emitted inline by codegen,
// so it needs no regex dependency.

fn jet_regex_compile(pattern: &str) -> Result<regex::Regex, String> {
    regex::Regex::new(pattern).map_err(|e| {
        // Collapse the multi-line crate error into one user-facing line.
        let msg = e.to_string();
        let first = msg.lines().find(|l| !l.trim().is_empty()).unwrap_or("invalid pattern");
        format!("invalid regex `{}`: {}", pattern, first.trim())
    })
}

fn jet_regex_captures_to_match(caps: &regex::Captures) -> Vec<Option<String>> {
    caps.iter()
        .map(|m| m.map(|x| x.as_str().to_string()))
        .collect()
}

pub fn jet_regex_is_match(pattern: &str, text: &str) -> Result<bool, String> {
    Ok(jet_regex_compile(pattern)?.is_match(text))
}

/// First match anywhere in `text`, with its capture groups; `None` if no match.
pub fn jet_regex_match(pattern: &str, text: &str) -> Result<Option<Vec<Option<String>>>, String> {
    let re = jet_regex_compile(pattern)?;
    Ok(re.captures(text).map(|c| jet_regex_captures_to_match(&c)))
}

/// The substring of the first match, or `None`.
pub fn jet_regex_find(pattern: &str, text: &str) -> Result<Option<String>, String> {
    let re = jet_regex_compile(pattern)?;
    Ok(re.find(text).map(|m| m.as_str().to_string()))
}

/// Every non-overlapping matched substring, left to right.
pub fn jet_regex_find_all(pattern: &str, text: &str) -> Result<Vec<String>, String> {
    let re = jet_regex_compile(pattern)?;
    Ok(re.find_iter(text).map(|m| m.as_str().to_string()).collect())
}

/// Replace the first match. `repl` may reference groups with `$1`, `${name}`.
pub fn jet_regex_replace(pattern: &str, text: &str, repl: &str) -> Result<String, String> {
    let re = jet_regex_compile(pattern)?;
    Ok(re.replace(text, repl).into_owned())
}

/// Replace every match.
pub fn jet_regex_replace_all(pattern: &str, text: &str, repl: &str) -> Result<String, String> {
    let re = jet_regex_compile(pattern)?;
    Ok(re.replace_all(text, repl).into_owned())
}

/// Split `text` on every match of `pattern`.
pub fn jet_regex_split(pattern: &str, text: &str) -> Result<Vec<String>, String> {
    let re = jet_regex_compile(pattern)?;
    Ok(re.split(text).map(|s| s.to_string()).collect())
}
