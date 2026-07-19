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
    lines: Vec<(usize, usize)>,
}

impl LineTable {
    pub(crate) fn new(src: &str) -> Self {
        let mut lines = Vec::new();
        let mut start = 0;
        let mut i = 0;
        let bytes = src.as_bytes();
        while i < bytes.len() {
            if matches!(bytes[i], b'\r' | b'\n') {
                lines.push((start, i));
                if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                start = i + 1;
            }
            i += 1;
        }
        lines.push((start, src.len()));
        LineTable { lines }
    }

    pub(crate) fn offset(&self, src: &str, pos: LspPos) -> usize {
        self.checked_offset(src, pos).unwrap_or(src.len())
    }

    fn checked_offset(&self, src: &str, pos: LspPos) -> Option<usize> {
        let (line_start, line_end) = match self.lines.get(pos.line as usize) {
            Some(line) => *line,
            None => return None,
        };
        let line_text = &src[line_start..line_end];
        let mut utf16_count = 0u32;
        let mut byte_off = line_start;
        for c in line_text.chars() {
            if utf16_count == pos.character {
                return Some(byte_off);
            }
            let next = utf16_count + c.len_utf16() as u32;
            if pos.character < next {
                return None;
            }
            utf16_count = next;
            byte_off += c.len_utf8();
        }
        Some(byte_off)
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
    let table = LineTable::new(src);
    for (line, &(start, end)) in table.lines.iter().enumerate() {
        let next_start = table
            .lines
            .get(line + 1)
            .map(|&(next, _)| next)
            .unwrap_or(src.len() + 1);
        if offset < next_start {
            return LspPos {
                line: line as u32,
                character: src[start..offset.min(end)].encode_utf16().count() as u32,
            };
        }
    }
    unreachable!()
}

/// Convert an LSP (line, UTF-16 char) position back to a byte offset.
pub fn lsp_pos_to_offset(src: &str, pos: LspPos) -> usize {
    LineTable::new(src).offset(src, pos)
}

pub(crate) fn apply_lsp_edit(
    src: &str,
    range: LspRange,
    expected_utf16_len: Option<u32>,
    new_text: &str,
) -> Option<String> {
    let table = LineTable::new(src);
    let start = table.checked_offset(src, range.start)?;
    let end = table.checked_offset(src, range.end)?;
    if end < start
        || expected_utf16_len
            .is_some_and(|len| src[start..end].encode_utf16().count() != len as usize)
    {
        return None;
    }
    let mut out = String::with_capacity(src.len() + new_text.len());
    out.push_str(&src[..start]);
    out.push_str(new_text);
    out.push_str(&src[end..]);
    Some(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validated_edits_follow_utf16_and_lsp_line_rules() {
        let src = "a😀\r\nbc\rd\n";
        let table = LineTable::new(src);
        assert_eq!(
            table.checked_offset(
                src,
                LspPos {
                    line: 0,
                    character: 2,
                }
            ),
            None
        );
        assert_eq!(
            table.checked_offset(
                src,
                LspPos {
                    line: 0,
                    character: 99,
                }
            ),
            Some(5)
        );
        assert_eq!(
            table.checked_offset(
                src,
                LspPos {
                    line: 2,
                    character: 0,
                }
            ),
            Some(10)
        );
        assert_eq!(
            byte_offset_to_lsp(src, 6),
            LspPos {
                line: 0,
                character: 3,
            }
        );
        assert_eq!(
            byte_offset_to_lsp(src, 10),
            LspPos {
                line: 2,
                character: 0,
            }
        );

        let edited = apply_lsp_edit(
            src,
            LspRange {
                start: LspPos {
                    line: 1,
                    character: 1,
                },
                end: LspPos {
                    line: 1,
                    character: 99,
                },
            },
            Some(1),
            "X",
        );
        assert_eq!(edited.as_deref(), Some("a😀\r\nbX\rd\n"));
    }
}
