//! LSP positions (UTF-16 code units) + range conversion.

use crate::diag::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LspPos {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LspRange {
    pub(crate) start: LspPos,
    pub(crate) end: LspPos,
}

pub(crate) fn byte_span_to_range(src: &str, span: Span) -> LspRange {
    LspRange {
        start: byte_offset_to_lsp(src, span.start),
        end: byte_offset_to_lsp(src, span.end),
    }
}

pub fn byte_offset_to_lsp(src: &str, offset: usize) -> LspPos {
    let offset = offset.min(src.len());
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (i, c) in src.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let line_text = &src[line_start..offset];
    let character = line_text.encode_utf16().count() as u32;
    LspPos { line, character }
}

/// Convert an LSP (line, UTF-16 char) position back to a byte offset.
pub fn lsp_pos_to_offset(src: &str, pos: LspPos) -> usize {
    let mut cur_line = 0u32;
    let mut line_byte_start = 0usize;
    for (i, c) in src.char_indices() {
        if cur_line == pos.line {
            break;
        }
        if c == '\n' {
            cur_line += 1;
            line_byte_start = i + 1;
        }
    }
    if cur_line < pos.line {
        return src.len();
    }
    let line_text = &src[line_byte_start..];
    let mut utf16_count = 0u32;
    let mut byte_off = line_byte_start;
    for c in line_text.chars() {
        if utf16_count >= pos.character {
            break;
        }
        utf16_count += c.len_utf16() as u32;
        byte_off += c.len_utf8();
    }
    byte_off.min(src.len())
}

pub(crate) fn full_document_range(src: &str) -> LspRange {
    let end = byte_offset_to_lsp(src, src.len());
    LspRange {
        start: LspPos {
            line: 0,
            character: 0,
        },
        end,
    }
}

pub(crate) fn range_json(r: LspRange) -> String {
    format!(
        r#"{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}}"#,
        r.start.line, r.start.character, r.end.line, r.end.character
    )
}
