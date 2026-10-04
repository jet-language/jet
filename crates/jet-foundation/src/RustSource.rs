//! Text-level scans of generated Rust source: the code mask the runtime crate
//! export and the bootstrap backend split read, and the removal of test-only
//! items from the runtime/Core text every program embeds.

/// `source` with every comment, string and char literal blanked to spaces
/// (newlines kept), byte for byte the same length: brackets and keywords in
/// the mask are code. The bootstrap backend split scans emitted items with it.
pub fn rust_code_mask(source: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Code,
        LineComment,
        BlockComment(usize),
        String(bool),
        RawString(usize),
        Char(bool),
    }

    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut state = State::Code;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        match state {
            State::Code => {
                if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
                    out.extend_from_slice(b"  ");
                    index += 2;
                    state = State::LineComment;
                } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    out.extend_from_slice(b"  ");
                    index += 2;
                    state = State::BlockComment(1);
                } else if let Some((prefix_len, hashes)) = raw_string_start(&bytes[index..]) {
                    out.extend(std::iter::repeat(b' ').take(prefix_len));
                    index += prefix_len;
                    state = State::RawString(hashes);
                } else if byte == b'"' || (byte == b'b' && bytes.get(index + 1) == Some(&b'"')) {
                    let len = if byte == b'b' { 2 } else { 1 };
                    out.extend(std::iter::repeat(b' ').take(len));
                    index += len;
                    state = State::String(false);
                } else if byte == b'\'' && char_literal_end(&bytes[index..]).is_some() {
                    out.push(b' ');
                    index += 1;
                    state = State::Char(false);
                } else {
                    out.push(byte);
                    index += 1;
                }
            }
            State::LineComment => {
                if byte == b'\n' {
                    out.push(byte);
                    state = State::Code;
                } else {
                    out.push(b' ');
                }
                index += 1;
            }
            State::BlockComment(depth) => {
                if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    out.extend_from_slice(b"  ");
                    index += 2;
                    state = State::BlockComment(depth + 1);
                } else if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    out.extend_from_slice(b"  ");
                    index += 2;
                    state = if depth == 1 {
                        State::Code
                    } else {
                        State::BlockComment(depth - 1)
                    };
                } else {
                    out.push(if byte == b'\n' { b'\n' } else { b' ' });
                    index += 1;
                }
            }
            State::String(escaped) => {
                out.push(if byte == b'\n' { b'\n' } else { b' ' });
                index += 1;
                if escaped {
                    state = State::String(false);
                } else if byte == b'\\' {
                    state = State::String(true);
                } else if byte == b'"' {
                    state = State::Code;
                }
            }
            State::RawString(hashes) => {
                let closes = byte == b'"'
                    && bytes
                        .get(index + 1..index + 1 + hashes)
                        .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'));
                out.push(if byte == b'\n' { b'\n' } else { b' ' });
                index += 1;
                if closes {
                    for _ in 0..hashes {
                        out.push(b' ');
                        index += 1;
                    }
                    state = State::Code;
                }
            }
            State::Char(escaped) => {
                out.push(b' ');
                index += 1;
                if escaped {
                    state = State::Char(false);
                } else if byte == b'\\' {
                    state = State::Char(true);
                } else if byte == b'\'' {
                    state = State::Code;
                }
            }
        }
    }
    String::from_utf8(out).expect("Rust source mask preserves UTF-8 bytes")
}

fn raw_string_start(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut index = match bytes.first() {
        Some(b'r') => 1,
        Some(b'b') if bytes.get(1) == Some(&b'r') => 2,
        _ => return None,
    };
    let start = index;
    while bytes.get(index) == Some(&b'#') {
        index += 1;
    }
    (bytes.get(index) == Some(&b'"')).then_some((index + 1, index - start))
}

fn char_literal_end(bytes: &[u8]) -> Option<usize> {
    if bytes.first() != Some(&b'\'') {
        return None;
    }
    if bytes.get(1) != Some(&b'\\') {
        // Decode only the one character: validating the whole remaining
        // source per quote made the mask quadratic on large programs.
        let width = match *bytes.get(1)? {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return None,
        };
        let text = std::str::from_utf8(bytes.get(1..1 + width)?).ok()?;
        let character = text.chars().next()?;
        let end = 1 + character.len_utf8();
        return (bytes.get(end) == Some(&b'\'')).then_some(end);
    }

    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'\'' {
            return Some(index);
        } else if byte == b'\n' {
            return None;
        }
    }
    None
}

/// `source` without the items rustc compiles only under `--test`: every item
/// whose outer attributes start with a line-leading `#[cfg(test)]` or
/// `#[test]`, together with the doc comments and attributes written above
/// it. Generated programs are never built with `--test`, so the result
/// compiles exactly as `source` does; it is smaller to write, hash and parse.
/// Attributes on statements, fields, variants and match arms are left alone,
/// as is any item whose extent the scan cannot delimit.
pub fn strip_test_items(source: &str) -> String {
    let mask = rust_code_mask(source);
    let code = mask.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut kept = 0usize;
    let mut line_start = 0usize;
    while line_start < code.len() {
        let line_end = code[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(code.len(), |offset| line_start + offset + 1);
        let mut at = line_start;
        while at < line_end && matches!(code[at], b' ' | b'\t') {
            at += 1;
        }
        let attribute = &code[at..line_end];
        if attribute.starts_with(b"#[cfg(test)]") || attribute.starts_with(b"#[test]") {
            let removed = test_item_end(code, at)
                .and_then(|end| Some((leading_item_start(source.as_bytes(), code, line_start, kept)?, end)));
            if let Some((start, end)) = removed {
                out.push_str(&source[kept..start]);
                kept = end;
                line_start = end;
                continue;
            }
        }
        line_start = line_end;
    }
    out.push_str(&source[kept..]);
    out
}

/// Items that end at the brace closing their body (or at `;` when they have
/// none: `struct S;`, `struct S(u8);`, an extern declaration).
const BODY_ITEMS: [&[u8]; 17] = [
    b"fn ",
    b"mod ",
    b"impl ",
    b"impl<",
    b"trait ",
    b"struct ",
    b"enum ",
    b"union ",
    b"macro_rules!",
    b"thread_local!",
    b"unsafe fn ",
    b"unsafe impl",
    b"unsafe trait ",
    b"async fn ",
    b"const fn ",
    b"const unsafe fn ",
    b"extern ",
];
/// Items that end only at their `;`: a brace inside them belongs to an
/// initializer or a use tree.
const SEMICOLON_ITEMS: [&[u8]; 4] = [b"const ", b"static ", b"use ", b"type "];

/// The end (past its line break) of the item whose attributes start at `at`
/// in the masked `code`, or `None` when it is not a delimitable item.
fn test_item_end(code: &[u8], at: usize) -> Option<usize> {
    let mut index = at;
    loop {
        index = skip_whitespace(code, index);
        if code.get(index) == Some(&b'#') && code.get(index + 1) == Some(&b'[') {
            index = closing_bracket(code, index + 1)? + 1;
        } else {
            break;
        }
    }
    if code[index..].starts_with(b"pub(") {
        index = skip_whitespace(code, closing_bracket(code, index + 3)? + 1);
    } else if code[index..].starts_with(b"pub ") {
        index = skip_whitespace(code, index + 4);
    }
    let header = &code[index..];
    let body_item = if BODY_ITEMS.iter().any(|keyword| header.starts_with(keyword)) {
        true
    } else if SEMICOLON_ITEMS.iter().any(|keyword| header.starts_with(keyword)) {
        false
    } else {
        return None;
    };
    // A body item's body is the first `{` outside every bracket and every
    // generic argument list: a brace inside `<…>` (a const-generic argument
    // or default such as `<const N: usize = { 3 }>`, `where T: Trait<{ N }>`)
    // belongs to the header. Angle brackets count only in such a header and
    // outside other brackets, where a `>` is never a comparison (`->` is not
    // one). A semicolon item ends at its first top-level `;` whatever its
    // initializer compares.
    let mut depth = 0usize;
    let mut angles = 0usize;
    let mut in_body = false;
    for (offset, byte) in header.iter().enumerate() {
        let generic_header = body_item && !in_body && depth == 0;
        match byte {
            b'<' if generic_header => angles += 1,
            b'>' if generic_header && offset > 0 && header[offset - 1] != b'-' => angles = angles.checked_sub(1)?,
            b'{' => {
                in_body |= depth == 0 && angles == 0;
                depth += 1;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.checked_sub(1)?,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 && body_item && in_body {
                    return Some(past_line_end(code, index + offset + 1));
                }
            }
            b';' if depth == 0 && angles == 0 => return Some(past_line_end(code, index + offset + 1)),
            _ => {}
        }
    }
    None
}

/// The first line of the item's leading doc comments and outer attributes
/// above `line_start`, never before `floor`; `None` when a multi-line
/// attribute ends just above them (it would otherwise move to the next item).
fn leading_item_start(source: &[u8], code: &[u8], line_start: usize, floor: usize) -> Option<usize> {
    let mut start = line_start;
    while start > floor {
        let mut previous = start - 1;
        while previous > floor && source[previous - 1] != b'\n' {
            previous -= 1;
        }
        let indent = |bytes: &[u8]| {
            let mut index = previous;
            while index < start && matches!(bytes[index], b' ' | b'\t') {
                index += 1;
            }
            index
        };
        if source[indent(source)..start].starts_with(b"///") || code[indent(code)..start].starts_with(b"#[") {
            start = previous;
        } else {
            break;
        }
    }
    let mut above = start;
    while above > floor && code[above - 1].is_ascii_whitespace() {
        above -= 1;
    }
    (above == floor || code[above - 1] != b']').then_some(start)
}

/// Index of the bracket closing the one at `open`.
fn closing_bracket(code: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in code.iter().enumerate().skip(open) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn skip_whitespace(code: &[u8], mut index: usize) -> usize {
    while code.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

/// `end`, or past the line break when only blanks follow it on its line.
fn past_line_end(code: &[u8], end: usize) -> usize {
    let mut index = end;
    while index < code.len() && matches!(code[index], b' ' | b'\t' | b'\r') {
        index += 1;
    }
    match code.get(index) {
        Some(b'\n') => index + 1,
        None => index,
        Some(_) => end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_test_items_removes_test_items_with_their_docs_and_attributes() {
        let source = "fn kept() {}\n\
                      /// Helper docs.\n\
                      #[allow(dead_code)]\n\
                      #[cfg(test)]\n\
                      fn helper() -> [u8; 2] { [0; 2] }\n\
                      #[cfg(test)]\n\
                      mod tests {\n    #[test]\n    fn inner() { let _ = \"}\"; }\n}\n\
                      #[cfg(test)]\n\
                      const TABLE: Pair = Pair { a: 1 };\n\
                      #[cfg(test)]\n\
                      pub(crate) use std::fmt;\n\
                      #[cfg(not(test))]\n\
                      fn production() {}\n";
        assert_eq!(
            strip_test_items(source),
            "fn kept() {}\n#[cfg(not(test))]\nfn production() {}\n"
        );
    }

    #[test]
    fn strip_test_items_reads_braces_in_generic_headers_as_header() {
        // Braces in a const-generic default, a const-generic argument in a
        // where clause and a return type must not end the item early; the
        // whole item goes, and the following item stays.
        let source = "#[cfg(test)]\n\
                      fn sized<const N: usize = { 2 + 1 }>() -> [u8; N] where Pad<{ N }>: Fill {\n    [0; N]\n}\n\
                      #[cfg(test)]\n\
                      impl<T: Into<u8>> Probe<{ 3 }> for T where T: Fn() -> u8 {\n    fn run(&self) {}\n}\n\
                      #[cfg(test)]\n\
                      struct Pair<T>(T, [u8; 2]);\n\
                      #[cfg(test)]\n\
                      const SMALL: bool = 1 < 2;\n\
                      fn kept() {}\n";
        assert_eq!(strip_test_items(source), "fn kept() {}\n");
    }

    #[test]
    fn strip_test_items_keeps_what_it_cannot_remove_safely() {
        // Statement, field and variant attributes; attribute text in strings
        // and comments; an item under a multi-line attribute.
        let source = "fn f() {\n    #[cfg(test)]\n    let x = 1;\n}\n\
                      struct S {\n    #[cfg(test)]\n    field: u8,\n}\n\
                      enum E {\n    #[cfg(test)]\n    Variant,\n}\n\
                      const TEXT: &str = \"\n#[cfg(test)]\nfn fake() {}\n\";\n\
                      // #[cfg(test)]\n\
                      #[derive(\n    Debug,\n)]\n#[cfg(test)]\nstruct T;\n";
        assert_eq!(strip_test_items(source), source);
    }
}
