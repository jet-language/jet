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

#[cold]
#[inline(never)]
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
    let Some(end) = r.pos.checked_add(n) else {
        return Err(jet_reader_bounds_error(method, n, r));
    };
    if end > r.buf.len() {
        return Err(jet_reader_bounds_error(method, n, r));
    }
    let out = r.buf[r.pos..end].to_vec();
    r.pos = end;
    Ok(out)
}

#[inline(always)]
pub fn jet_reader_read_u8_fast(r: &mut JetReader) -> Option<u8> {
    if r.buf.len().saturating_sub(r.pos) < 1 {
        return None;
    }
    let pos = r.pos;
    let value = r.buf[pos];
    r.pos = pos + 1;
    Some(value)
}

/// Prove that a fixed-width read region is resident before entering its hot
/// loop. The caller must consume exactly `count` bytes from the returned
/// half-open interval without changing the reader through another operation;
/// the region emitter enforces that condition from TIR. A miss deliberately
/// returns `None` without changing the reader so the ordinary fallible loop
/// can preserve its partial-progress and bounds-error behavior.
#[inline(always)]
pub fn jet_reader_region_bounds(r: &JetReader, count: i64) -> Option<(usize, usize)> {
    let count = usize::try_from(count).ok()?;
    let end = r.pos.checked_add(count)?;
    (end <= r.buf.len()).then_some((r.pos, end))
}

#[inline(always)]
pub fn jet_reader_read_u8(r: &mut JetReader) -> Result<u8, String> {
    jet_reader_read_u8_fast(r).ok_or_else(|| jet_reader_bounds_error("read_u8", 1, r))
}

#[inline(always)]
pub fn jet_reader_read_i8_fast(r: &mut JetReader) -> Option<i8> {
    jet_reader_read_u8_fast(r).map(|value| value as i8)
}

#[inline(always)]
pub fn jet_reader_read_i8(r: &mut JetReader) -> Result<i8, String> {
    jet_reader_read_i8_fast(r).ok_or_else(|| jet_reader_bounds_error("read_i8", 1, r))
}

#[inline(always)]
pub fn jet_reader_read_u16_le_fast(r: &mut JetReader) -> Option<u16> {
    if r.buf.len().saturating_sub(r.pos) < 2 {
        return None;
    }
    let pos = r.pos;
    let value = u16::from_le_bytes([r.buf[pos], r.buf[pos + 1]]);
    r.pos = pos + 2;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_u16_le(r: &mut JetReader) -> Result<u16, String> {
    jet_reader_read_u16_le_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_u16_le", 2, r))
}

#[inline(always)]
pub fn jet_reader_read_u16_be_fast(r: &mut JetReader) -> Option<u16> {
    if r.buf.len().saturating_sub(r.pos) < 2 {
        return None;
    }
    let pos = r.pos;
    let value = u16::from_be_bytes([r.buf[pos], r.buf[pos + 1]]);
    r.pos = pos + 2;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_u16_be(r: &mut JetReader) -> Result<u16, String> {
    jet_reader_read_u16_be_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_u16_be", 2, r))
}

#[inline(always)]
pub fn jet_reader_read_i16_le_fast(r: &mut JetReader) -> Option<i16> {
    jet_reader_read_u16_le_fast(r).map(|value| value as i16)
}

#[inline(always)]
pub fn jet_reader_read_i16_le(r: &mut JetReader) -> Result<i16, String> {
    jet_reader_read_i16_le_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_i16_le", 2, r))
}

#[inline(always)]
pub fn jet_reader_read_i16_be_fast(r: &mut JetReader) -> Option<i16> {
    jet_reader_read_u16_be_fast(r).map(|value| value as i16)
}

#[inline(always)]
pub fn jet_reader_read_i16_be(r: &mut JetReader) -> Result<i16, String> {
    jet_reader_read_i16_be_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_i16_be", 2, r))
}

#[inline(always)]
pub fn jet_reader_read_u32_le_fast(r: &mut JetReader) -> Option<u32> {
    if r.buf.len().saturating_sub(r.pos) < 4 {
        return None;
    }
    let pos = r.pos;
    let value = u32::from_le_bytes([
        r.buf[pos],
        r.buf[pos + 1],
        r.buf[pos + 2],
        r.buf[pos + 3],
    ]);
    r.pos = pos + 4;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_u32_le(r: &mut JetReader) -> Result<u32, String> {
    jet_reader_read_u32_le_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_u32_le", 4, r))
}

#[inline(always)]
pub fn jet_reader_read_u32_be_fast(r: &mut JetReader) -> Option<u32> {
    if r.buf.len().saturating_sub(r.pos) < 4 {
        return None;
    }
    let pos = r.pos;
    let value = u32::from_be_bytes([
        r.buf[pos],
        r.buf[pos + 1],
        r.buf[pos + 2],
        r.buf[pos + 3],
    ]);
    r.pos = pos + 4;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_u32_be(r: &mut JetReader) -> Result<u32, String> {
    jet_reader_read_u32_be_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_u32_be", 4, r))
}

#[inline(always)]
pub fn jet_reader_read_i32_le_fast(r: &mut JetReader) -> Option<i32> {
    jet_reader_read_u32_le_fast(r).map(|value| value as i32)
}

#[inline(always)]
pub fn jet_reader_read_i32_le(r: &mut JetReader) -> Result<i32, String> {
    jet_reader_read_i32_le_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_i32_le", 4, r))
}

#[inline(always)]
pub fn jet_reader_read_i32_be_fast(r: &mut JetReader) -> Option<i32> {
    jet_reader_read_u32_be_fast(r).map(|value| value as i32)
}

#[inline(always)]
pub fn jet_reader_read_i32_be(r: &mut JetReader) -> Result<i32, String> {
    jet_reader_read_i32_be_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_i32_be", 4, r))
}

#[inline(always)]
pub fn jet_reader_read_u64_le_fast(r: &mut JetReader) -> Option<u64> {
    if r.buf.len().saturating_sub(r.pos) < 8 {
        return None;
    }
    let pos = r.pos;
    let value = u64::from_le_bytes([
        r.buf[pos],
        r.buf[pos + 1],
        r.buf[pos + 2],
        r.buf[pos + 3],
        r.buf[pos + 4],
        r.buf[pos + 5],
        r.buf[pos + 6],
        r.buf[pos + 7],
    ]);
    r.pos = pos + 8;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_u64_le(r: &mut JetReader) -> Result<u64, String> {
    jet_reader_read_u64_le_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_u64_le", 8, r))
}

#[inline(always)]
pub fn jet_reader_read_u64_be_fast(r: &mut JetReader) -> Option<u64> {
    if r.buf.len().saturating_sub(r.pos) < 8 {
        return None;
    }
    let pos = r.pos;
    let value = u64::from_be_bytes([
        r.buf[pos],
        r.buf[pos + 1],
        r.buf[pos + 2],
        r.buf[pos + 3],
        r.buf[pos + 4],
        r.buf[pos + 5],
        r.buf[pos + 6],
        r.buf[pos + 7],
    ]);
    r.pos = pos + 8;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_u64_be(r: &mut JetReader) -> Result<u64, String> {
    jet_reader_read_u64_be_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_u64_be", 8, r))
}

#[inline(always)]
pub fn jet_reader_read_i64_le_fast(r: &mut JetReader) -> Option<i64> {
    jet_reader_read_u64_le_fast(r).map(|value| value as i64)
}

#[inline(always)]
pub fn jet_reader_read_i64_le(r: &mut JetReader) -> Result<i64, String> {
    jet_reader_read_i64_le_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_i64_le", 8, r))
}

#[inline(always)]
pub fn jet_reader_read_i64_be_fast(r: &mut JetReader) -> Option<i64> {
    jet_reader_read_u64_be_fast(r).map(|value| value as i64)
}

#[inline(always)]
pub fn jet_reader_read_i64_be(r: &mut JetReader) -> Result<i64, String> {
    jet_reader_read_i64_be_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_i64_be", 8, r))
}

#[inline(always)]
pub fn jet_reader_read_f32_le_fast(r: &mut JetReader) -> Option<f32> {
    if r.buf.len().saturating_sub(r.pos) < 4 {
        return None;
    }
    let pos = r.pos;
    let value = f32::from_le_bytes([
        r.buf[pos],
        r.buf[pos + 1],
        r.buf[pos + 2],
        r.buf[pos + 3],
    ]);
    r.pos = pos + 4;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_f32_le(r: &mut JetReader) -> Result<f32, String> {
    jet_reader_read_f32_le_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_f32_le", 4, r))
}

#[inline(always)]
pub fn jet_reader_read_f32_be_fast(r: &mut JetReader) -> Option<f32> {
    if r.buf.len().saturating_sub(r.pos) < 4 {
        return None;
    }
    let pos = r.pos;
    let value = f32::from_be_bytes([
        r.buf[pos],
        r.buf[pos + 1],
        r.buf[pos + 2],
        r.buf[pos + 3],
    ]);
    r.pos = pos + 4;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_f32_be(r: &mut JetReader) -> Result<f32, String> {
    jet_reader_read_f32_be_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_f32_be", 4, r))
}

#[inline(always)]
pub fn jet_reader_read_f64_le_fast(r: &mut JetReader) -> Option<f64> {
    if r.buf.len().saturating_sub(r.pos) < 8 {
        return None;
    }
    let pos = r.pos;
    let value = f64::from_le_bytes([
        r.buf[pos],
        r.buf[pos + 1],
        r.buf[pos + 2],
        r.buf[pos + 3],
        r.buf[pos + 4],
        r.buf[pos + 5],
        r.buf[pos + 6],
        r.buf[pos + 7],
    ]);
    r.pos = pos + 8;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_f64_le(r: &mut JetReader) -> Result<f64, String> {
    jet_reader_read_f64_le_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_f64_le", 8, r))
}

#[inline(always)]
pub fn jet_reader_read_f64_be_fast(r: &mut JetReader) -> Option<f64> {
    if r.buf.len().saturating_sub(r.pos) < 8 {
        return None;
    }
    let pos = r.pos;
    let value = f64::from_be_bytes([
        r.buf[pos],
        r.buf[pos + 1],
        r.buf[pos + 2],
        r.buf[pos + 3],
        r.buf[pos + 4],
        r.buf[pos + 5],
        r.buf[pos + 6],
        r.buf[pos + 7],
    ]);
    r.pos = pos + 8;
    Some(value)
}

#[inline(always)]
pub fn jet_reader_read_f64_be(r: &mut JetReader) -> Result<f64, String> {
    jet_reader_read_f64_be_fast(r)
        .ok_or_else(|| jet_reader_bounds_error("read_f64_be", 8, r))
}

pub fn jet_reader_peek(r: &JetReader) -> Result<u8, String> {
    r.buf
        .get(r.pos)
        .copied()
        .ok_or_else(|| jet_reader_bounds_error("peek", 1, r))
}

pub fn jet_reader_seek(r: &mut JetReader, position: i64) -> Result<(), String> {
    if position < 0 {
        return Err(format!(
            "Reader.seek: position must not be negative, got {}",
            position
        ));
    }
    let position = usize::try_from(position).map_err(|_| {
        format!(
            "Reader.seek: position {} is outside the addressable buffer",
            position
        )
    })?;
    if position > r.buf.len() {
        return Err(format!(
            "Reader.seek: position {} is beyond end {}",
            position,
            r.buf.len()
        ));
    }
    r.pos = position;
    Ok(())
}

pub fn jet_reader_skip(r: &mut JetReader, count: i64) -> Result<(), String> {
    if count < 0 {
        return Err(format!(
            "Reader.skip: length must not be negative, got {}",
            count
        ));
    }
    let count = usize::try_from(count).map_err(|_| {
        format!(
            "Reader.skip: length {} is outside the addressable buffer",
            count
        )
    })?;
    let end = r.pos.checked_add(count).ok_or_else(|| {
        format!(
            "Reader.skip: position overflow at {} plus {}",
            r.pos, count
        )
    })?;
    if end > r.buf.len() {
        return Err(jet_reader_bounds_error("skip", count, r));
    }
    r.pos = end;
    Ok(())
}

pub fn jet_reader_take(r: &mut JetReader, n: i64) -> Result<Vec<u8>, String> {
    if n < 0 {
        return Err(format!(
            "Reader.take: length must not be negative, got {}",
            n
        ));
    }
    let n = usize::try_from(n).map_err(|_| {
        format!(
            "Reader.take: length {} is outside the addressable buffer",
            n
        )
    })?;
    jet_reader_take_fixed(r, n, "take")
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
    fn fast_fixed_reads_advance_and_leave_misses_cold() {
        let mut r = jet_reader_over(&vec![0x2a, 0x00, 0x03]);
        assert_eq!(jet_reader_read_u8_fast(&mut r), Some(0x2a));
        assert_eq!(r.pos, 1);
        assert_eq!(jet_reader_read_u16_le_fast(&mut r), Some(0x0300));
        assert_eq!(r.pos, 3);
        assert_eq!(jet_reader_read_u8_fast(&mut r), None);
        assert_eq!(r.pos, 3);
    }

    #[test]
    fn region_bounds_checks_without_advancing() {
        let r = jet_reader_over(&vec![1, 2, 3]);
        assert_eq!(jet_reader_region_bounds(&r, 2), Some((0, 2)));
        assert_eq!(r.pos, 0);
        assert_eq!(jet_reader_region_bounds(&r, 4), None);
        assert_eq!(r.pos, 0);
        assert_eq!(jet_reader_region_bounds(&r, -1), None);
        assert_eq!(r.pos, 0);
    }

    #[test]
    fn reads_little_endian_floats() {
        let mut r = jet_reader_over(&vec![
            0x00, 0x00, 0xc0, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x40,
        ]);
        assert_eq!(jet_reader_read_f32_le(&mut r), Ok(1.5));
        assert_eq!(jet_reader_read_f64_le(&mut r), Ok(2.5));
        assert!(jet_reader_at_end(&r));
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
        assert_eq!(
            jet_cursor_take_until(&mut c, &"|".to_string()),
            Ok("ab".to_string())
        );
        assert!(jet_cursor_take_until(&mut c, &"!".to_string()).is_err());
    }
}
