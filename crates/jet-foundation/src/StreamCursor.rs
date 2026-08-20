// ── binary.Reader / text.Cursor (D-SHIFT1, c7shift) ──────────────────────────
// The "shift" kernel from linear stream parsing (Jai's `shift` primitive),
// without a dedicated operator (D-SHIFT1=A rejected that). `Reader` owns a
// copy of its byte buffer plus a read position; every read is fallible —
// a bounds miss is an ordinary `Err` string, never a panic or silent
// truncation.
//
// I9: this file is the ONE implementation. `Codegen/mod.rs` splices it verbatim
// into the emitted AOT prelude (`include_str!`), and the canonical TIR
// evaluator calls the same functions directly (`eval/handles.rs`), so quick-run
// and full build read the same bytes and produce the same error text.
// It must stay std-only and free of `crate::` paths: the AOT copy compiles at
// the root of a generated program, not inside this crate.
#[derive(Clone)]
pub struct JetReader {
    pub buf: Vec<u8>,
    pub pos: usize,
}
#[derive(Clone)]
pub struct JetCursor {
    pub buf: String,
    pub pos: usize,
}

pub fn jet_reader_over(bytes: &Vec<u8>) -> JetReader {
    JetReader {
        buf: bytes.clone(),
        pos: 0,
    }
}

pub fn jet_reader_bounds_error(method: &str, need: usize, r: &JetReader) -> String {
    format!(
        "Reader.{}: needed {} byte{} at position {}, only {} remain",
        method,
        need,
        if need == 1 { "" } else { "s" },
        r.pos,
        r.buf.len().saturating_sub(r.pos),
    )
}

pub fn jet_reader_take_fixed(r: &mut JetReader, n: usize, method: &str) -> Result<Vec<u8>, String> {
    if r.pos + n > r.buf.len() {
        return Err(jet_reader_bounds_error(method, n, r));
    }
    let out = r.buf[r.pos..r.pos + n].to_vec();
    r.pos += n;
    Ok(out)
}

pub fn jet_reader_read_u8(r: &mut JetReader) -> Result<u8, String> {
    jet_reader_take_fixed(r, 1, "read_u8").map(|b| b[0])
}
pub fn jet_reader_read_u16_le(r: &mut JetReader) -> Result<u16, String> {
    jet_reader_take_fixed(r, 2, "read_u16_le").map(|b| u16::from_le_bytes([b[0], b[1]]))
}
pub fn jet_reader_read_u16_be(r: &mut JetReader) -> Result<u16, String> {
    jet_reader_take_fixed(r, 2, "read_u16_be").map(|b| u16::from_be_bytes([b[0], b[1]]))
}
pub fn jet_reader_read_u32_le(r: &mut JetReader) -> Result<u32, String> {
    jet_reader_take_fixed(r, 4, "read_u32_le").map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
pub fn jet_reader_read_u32_be(r: &mut JetReader) -> Result<u32, String> {
    jet_reader_take_fixed(r, 4, "read_u32_be").map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
pub fn jet_reader_read_u64_le(r: &mut JetReader) -> Result<u64, String> {
    jet_reader_take_fixed(r, 8, "read_u64_le")
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
pub fn jet_reader_read_u64_be(r: &mut JetReader) -> Result<u64, String> {
    jet_reader_take_fixed(r, 8, "read_u64_be")
        .map(|b| u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

pub fn jet_reader_take(r: &mut JetReader, n: i64) -> Result<Vec<u8>, String> {
    if n < 0 {
        return Err(format!(
            "Reader.take: length must not be negative, got {}",
            n
        ));
    }
    jet_reader_take_fixed(r, n as usize, "take")
}

pub fn jet_reader_remaining(r: &JetReader) -> i64 {
    (r.buf.len() - r.pos) as i64
}

pub fn jet_reader_at_end(r: &JetReader) -> bool {
    r.pos >= r.buf.len()
}

pub fn jet_cursor_over(s: &String) -> JetCursor {
    JetCursor {
        buf: s.clone(),
        pos: 0,
    }
}

pub fn jet_cursor_take_until(c: &mut JetCursor, delim: &String) -> Result<String, String> {
    let tail = &c.buf[c.pos..];
    match tail.find(delim.as_str()) {
        Some(i) => {
            let out = tail[..i].to_string();
            c.pos += i;
            Ok(out)
        }
        None => Err(format!(
            "Cursor.take_until: {:?} not found in the remaining text",
            delim
        )),
    }
}

pub fn jet_cursor_skip_ws(c: &mut JetCursor) {
    let tail = jet_cursor_tail(c);
    let skipped = tail.len() - tail.trim_start().len();
    c.pos += skipped;
}

// `take_pattern` runs a different match engine on every tier: AOT inlines a
// closure specialized to the pattern, the Cranelift host and the interpreter
// walk the pattern as data. What surrounds the match is the same everywhere,
// so it lives here: what the scan may look at, how far a hit advances, and
// what a miss reports. A tier supplies the match and projects the bindings;
// it decides nothing else.

pub fn jet_cursor_tail(c: &JetCursor) -> &str {
    &c.buf[c.pos..]
}

pub fn jet_reader_tail(r: &JetReader) -> &[u8] {
    &r.buf[r.pos..]
}

/// A matched prefix is consumed. `consumed` counts bytes from the tail.
pub fn jet_cursor_take_pattern(c: &mut JetCursor, consumed: usize) {
    c.pos += consumed;
}

pub fn jet_reader_take_pattern(r: &mut JetReader, consumed: usize) {
    r.pos += consumed;
}

/// A miss leaves the position untouched and names it, so a caller can see
/// where the parse stalled.
pub fn jet_cursor_pattern_miss(c: &JetCursor) -> String {
    format!("pattern did not match at cursor position {}", c.pos)
}

pub fn jet_reader_pattern_miss(r: &JetReader) -> String {
    format!("pattern did not match at reader position {}", r.pos)
}

#[cfg(test)]
mod stream_cursor_tests {
    use super::*;

    #[test]
    fn reads_advance_and_report_bounds() {
        let mut r = jet_reader_over(&vec![42, 0, 0, 0, 3, 0, 16, 32, 48]);
        assert_eq!(jet_reader_read_u32_le(&mut r), Ok(42));
        assert_eq!(jet_reader_read_u16_le(&mut r), Ok(3));
        assert_eq!(jet_reader_take(&mut r, 3), Ok(vec![16, 32, 48]));
        assert!(jet_reader_at_end(&r));
        assert_eq!(jet_reader_remaining(&r), 0);
        assert_eq!(
            jet_reader_read_u8(&mut r),
            Err("Reader.read_u8: needed 1 byte at position 9, only 0 remain".to_string())
        );
        assert_eq!(
            jet_reader_take(&mut r, -1),
            Err("Reader.take: length must not be negative, got -1".to_string())
        );
    }

    #[test]
    fn take_pattern_advances_on_a_hit_and_names_the_position_on_a_miss() {
        let mut c = jet_cursor_over(&"abcdef".to_string());
        assert_eq!(jet_cursor_tail(&c), "abcdef");
        jet_cursor_take_pattern(&mut c, 3);
        assert_eq!(jet_cursor_tail(&c), "def");
        assert_eq!(
            jet_cursor_pattern_miss(&c),
            "pattern did not match at cursor position 3"
        );

        let mut r = jet_reader_over(&vec![1, 2, 3, 4]);
        jet_reader_take_pattern(&mut r, 2);
        assert_eq!(jet_reader_tail(&r), &[3, 4]);
        assert_eq!(
            jet_reader_pattern_miss(&r),
            "pattern did not match at reader position 2"
        );
    }

    #[test]
    fn cursor_skips_space_and_takes_until() {
        let mut c = jet_cursor_over(&"   ab|cd".to_string());
        jet_cursor_skip_ws(&mut c);
        assert_eq!(c.pos, 3);
        assert_eq!(jet_cursor_take_until(&mut c, &"|".to_string()), Ok("ab".to_string()));
        assert!(jet_cursor_take_until(&mut c, &"!".to_string()).is_err());
    }
}
