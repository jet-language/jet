//! LSP positions (UTF-16 code units) + range conversion.

use crate::Diagnostics::Span;

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

#[derive(Clone, Debug)]
pub(crate) struct LineTable {
    starts: Vec<usize>,
}

impl LineTable {
    pub(crate) fn new(src: &str) -> Self {
        let mut starts = vec![0];
        for (i, c) in src.char_indices() {
            if c == '\n' {
                starts.push(i + 1);
            }
        }
        LineTable { starts }
    }

    pub(crate) fn offset(&self, src: &str, pos: LspPos) -> usize {
        let line_start = match self.starts.get(pos.line as usize) {
            Some(start) => *start,
            None => return src.len(),
        };
        let line_end = self
            .starts
            .get(pos.line as usize + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or(src.len());
        let line_text = &src[line_start..line_end.min(src.len())];
        let mut utf16_count = 0u32;
        let mut byte_off = line_start;
        for c in line_text.chars() {
            if utf16_count >= pos.character {
                break;
            }
            utf16_count += c.len_utf16() as u32;
            byte_off += c.len_utf8();
        }
        byte_off.min(src.len())
    }
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
    LineTable::new(src).offset(src, pos)
}

pub(crate) fn apply_lsp_edit(src: &str, range: LspRange, new_text: &str) -> String {
    let table = LineTable::new(src);
    let start = table.offset(src, range.start);
    let end = table.offset(src, range.end).max(start).min(src.len());
    let mut out = String::with_capacity(src.len() + new_text.len());
    out.push_str(&src[..start]);
    out.push_str(new_text);
    out.push_str(&src[end..]);
    out
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
