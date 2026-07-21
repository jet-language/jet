// D-ENCSTREAM-SURFACE1=A: bounded pull JSON over owned file handles.
// Reader consumes at most one scalar/container boundary per `next`; no read-to-end
// or delimiter transcript sits behind this API.

enum JetJsonReadFrame {
    ArrayValueOrEnd { first: bool, index: usize },
    ArrayCommaOrEnd { index: usize },
    ObjectKeyOrEnd { first: bool },
    ObjectColonValue { key: String, key_heap: usize },
    ObjectCommaOrEnd { key: String, key_heap: usize },
}

#[derive(Clone, Copy)]
enum JetJsonReadState {
    ArrayValueOrEnd { first: bool, index: usize },
    ArrayCommaOrEnd { index: usize },
    ObjectKeyOrEnd { first: bool },
    ObjectColonValue,
    ObjectCommaOrEnd,
}

#[derive(Clone)]
struct JetJsonAllocationBudget {
    inner: std::rc::Rc<std::cell::RefCell<JetJsonAllocationState>>,
}

struct JetJsonAllocationState {
    used: usize,
    limit: usize,
}

impl JetJsonAllocationBudget {
    fn new(limit: usize) -> Self {
        Self { inner: std::rc::Rc::new(std::cell::RefCell::new(JetJsonAllocationState { used: 0, limit })) }
    }

    fn charge(&self, bytes: usize) -> bool {
        let mut state = self.inner.borrow_mut();
        let Some(next) = state.used.checked_add(bytes) else { return false };
        if next > state.limit { return false; }
        state.used = next;
        true
    }

    fn would_fit(&self, bytes: usize) -> bool {
        let state = self.inner.borrow();
        state.used.checked_add(bytes).is_some_and(|next| next <= state.limit)
    }

    fn release(&self, bytes: usize) {
        let mut state = self.inner.borrow_mut();
        state.used = state.used.saturating_sub(bytes);
    }
}

#[derive(Clone, Copy)]
enum JetJsonWriteFrame {
    Array { first: bool },
    ObjectKey { first: bool },
    ObjectValue,
}

enum JetJsonCanonicalFrame {
    Array { first: bool },
    Object {
        entries: Vec<(String, Vec<u8>)>,
        key: Option<String>,
        value: Vec<u8>,
        retained: usize,
    },
}

fn jet_json_big_pow(mut base: jet_std::JetBigInt, mut exponent: usize) -> jet_std::JetBigInt {
    let mut out = jet_std::JetBigInt::from_int(1);
    while exponent > 0 {
        if exponent & 1 == 1 {
            out = out.mul(&base);
        }
        exponent >>= 1;
        if exponent > 0 {
            base = base.mul(&base);
        }
    }
    out
}

fn jet_json_decimal_ratio(text: &str) -> (jet_std::JetBigInt, jet_std::JetBigInt) {
    let (mantissa, exponent) = text
        .split_once('e')
        .map(|(mantissa, exponent)| (mantissa, exponent.parse::<i32>().expect("Rust float exponent")))
        .unwrap_or((text, 0));
    let fraction = mantissa.split_once('.').map_or(0, |(_, fraction)| fraction.len()) as i32;
    let digits = mantissa.replace('.', "");
    let mut numerator = jet_std::JetBigInt::from_str(&digits).expect("Rust float digits");
    let mut denominator = jet_std::JetBigInt::from_int(1);
    let decimal_exponent = exponent - fraction;
    if decimal_exponent >= 0 {
        numerator = numerator.mul(&jet_json_big_pow(jet_std::JetBigInt::from_int(10), decimal_exponent as usize));
    } else {
        denominator = jet_json_big_pow(jet_std::JetBigInt::from_int(10), (-decimal_exponent) as usize);
    }
    (numerator, denominator)
}

fn jet_json_float_ratio(value: f64) -> (jet_std::JetBigInt, jet_std::JetBigInt) {
    let bits = value.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (significand, exponent) = if biased == 0 {
        (fraction, -1074)
    } else {
        ((1u64 << 52) | fraction, biased - 1023 - 52)
    };
    let mut numerator = jet_std::JetBigInt::from_int(significand as i64);
    let mut denominator = jet_std::JetBigInt::from_int(1);
    if exponent >= 0 {
        numerator = numerator.mul(&jet_json_big_pow(jet_std::JetBigInt::from_int(2), exponent as usize));
    } else {
        denominator = jet_json_big_pow(jet_std::JetBigInt::from_int(2), (-exponent) as usize);
    }
    (numerator, denominator)
}

fn jet_json_decimal_distance(text: &str, exact: &(jet_std::JetBigInt, jet_std::JetBigInt)) -> jet_std::JetBigInt {
    let candidate = jet_json_decimal_ratio(text);
    let distance = candidate.0.mul(&exact.1).sub(&exact.0.mul(&candidate.1));
    let rendered = distance.to_string_rep();
    jet_std::JetBigInt::from_str(rendered.strip_prefix('-').unwrap_or(&rendered)).expect("absolute decimal distance")
}

fn jet_json_positive_big_cmp(left: &jet_std::JetBigInt, right: &jet_std::JetBigInt) -> std::cmp::Ordering {
    let left = left.to_string_rep();
    let right = right.to_string_rep();
    left.len().cmp(&right.len()).then_with(|| left.cmp(&right))
}

fn jet_json_jcs_shortest(value: f64) -> String {
    let shortest = format!("{:?}", value);
    let mantissa_end = shortest.find('e').unwrap_or(shortest.len());
    let Some(index) = shortest[..mantissa_end].rfind(|ch: char| ch.is_ascii_digit()) else { return shortest };
    let digit = shortest.as_bytes()[index] - b'0';
    let mut candidates = vec![(shortest.clone(), digit)];
    for replacement in [digit.checked_sub(1), digit.checked_add(1).filter(|next| *next < 10)].into_iter().flatten() {
        let mut candidate = shortest.clone().into_bytes();
        candidate[index] = b'0' + replacement;
        let candidate = String::from_utf8(candidate).expect("ASCII digit replacement");
        if candidate.parse::<f64>().is_ok_and(|parsed| parsed.to_bits() == value.to_bits()) {
            candidates.push((candidate, replacement));
        }
    }
    if candidates.len() == 1 {
        return shortest;
    }
    let exact = jet_json_float_ratio(value);
    candidates
        .into_iter()
        .map(|(candidate, last)| {
            let distance = jet_json_decimal_distance(&candidate, &exact);
            (candidate, last, distance)
        })
        .min_by(|left, right| {
            jet_json_positive_big_cmp(&left.2, &right.2)
                .then_with(|| (left.1 & 1).cmp(&(right.1 & 1)))
        })
        .map(|(candidate, _, _)| candidate)
        .expect("at least one shortest float")
}

fn jet_json_jcs_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let negative = value.is_sign_negative();
    let shortest = jet_json_jcs_shortest(value.abs());
    let (mantissa, exponent) = shortest
        .split_once('e')
        .map(|(mantissa, exponent)| (mantissa, exponent.parse::<i32>().expect("Rust float exponent")))
        .unwrap_or((&shortest, 0));
    let decimal = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let mut digits = mantissa.bytes().filter(|byte| *byte != b'.').collect::<Vec<_>>();
    let leading = digits.iter().take_while(|byte| **byte == b'0').count();
    digits.drain(..leading);
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }
    let n = decimal + exponent - leading as i32;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if n > 0 && n <= 21 {
        if digits.len() <= n as usize {
            out.extend(digits.iter().map(|byte| *byte as char));
            out.extend(std::iter::repeat_n('0', n as usize - digits.len()));
        } else {
            let split = n as usize;
            out.extend(digits[..split].iter().map(|byte| *byte as char));
            out.push('.');
            out.extend(digits[split..].iter().map(|byte| *byte as char));
        }
    } else if n > -6 && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.extend(digits.iter().map(|byte| *byte as char));
    } else {
        out.push(digits[0] as char);
        if digits.len() > 1 {
            out.push('.');
            out.extend(digits[1..].iter().map(|byte| *byte as char));
        }
        out.push('e');
        let exponent = n - 1;
        if exponent >= 0 {
            out.push('+');
        }
        out.push_str(&exponent.to_string());
    }
    out
}

fn jet_json_jcs_key_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn jet_encoding_error(
    kind: jet_std::EncodingErrorKind,
    offset: i64,
    line: i64,
    column: i64,
    reason: impl Into<String>,
) -> jet_std::EncodingError {
    jet_std::EncodingError {
        format: jet_std::EncodingFormat::JSON,
        kind,
        byte_offset: offset,
        line: Some(line),
        column: Some(column),
        path: String::new(),
        reason: reason.into(),
        cause: None,
    }
}

fn jet_encoding_io_error(
    offset: i64,
    line: i64,
    column: i64,
    error: std::io::Error,
) -> jet_std::EncodingError {
    jet_std::EncodingError {
        format: jet_std::EncodingFormat::JSON,
        kind: jet_std::EncodingErrorKind::IO,
        byte_offset: offset,
        line: Some(line),
        column: Some(column),
        path: String::new(),
        reason: "file IO failed".to_string(),
        cause: Some(jet_std::EncodingCause {
            kind: format!("{:?}", error.kind()),
            os_code: error.raw_os_error().map(i64::from),
            message: error.to_string(),
        }),
    }
}

fn jet_encoding_validate_limits(
    limits: &jet_std::EncodingLimits,
) -> Result<(), jet_std::EncodingError> {
    let invalid = if !(4096..=16777216).contains(&limits.buffer_bytes) {
        Some(format!("buffer_bytes {} is outside 4096..16777216", limits.buffer_bytes))
    } else if !(1..=4096).contains(&limits.max_depth) {
        Some(format!("max_depth {} is outside 1..4096", limits.max_depth))
    } else if !(1..=1073741824).contains(&limits.max_item_bytes) {
        Some(format!("max_item_bytes {} is outside 1..1073741824", limits.max_item_bytes))
    } else if limits.max_total_bytes.is_some_and(|n| n < 0) {
        Some(format!("max_total_bytes {} is outside 0..Int.max", limits.max_total_bytes.unwrap_or(0)))
    } else if !(0..=256).contains(&limits.max_expansion_depth) {
        Some(format!("max_expansion_depth {} is outside 0..256", limits.max_expansion_depth))
    } else if !(0..=1073741824).contains(&limits.max_expansion_bytes) {
        Some(format!("max_expansion_bytes {} is outside 0..1073741824", limits.max_expansion_bytes))
    } else {
        None
    };
    match invalid {
        Some(reason) => Err(jet_std::EncodingError {
            format: jet_std::EncodingFormat::JSON,
            kind: jet_std::EncodingErrorKind::Limit,
            byte_offset: 0,
            line: Some(1),
            column: Some(1),
            path: String::new(),
            reason,
            cause: None,
        }),
        None => Ok(()),
    }
}

// D-ENCSTREAM-SURFACE1=A codec-owned live heap ceiling.
fn jet_encoding_codec_heap_ceiling(limits: &jet_std::EncodingLimits) -> usize {
    (limits.buffer_bytes as usize)
        .saturating_add(limits.max_item_bytes as usize)
        .saturating_add(limits.max_expansion_bytes as usize)
        .saturating_add((limits.max_depth as usize).saturating_mul(256))
        .saturating_add(65_536)
}

fn jet_enc_json_reader(
    input: JetFileReader,
    limits: jet_std::EncodingLimits,
) -> Result<jet_std::JSONReader, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits)?;
    let allocation_budget = Some(JetJsonAllocationBudget::new(jet_encoding_codec_heap_ceiling(&limits)));
    Ok(jet_std::JSONReader {
        input,
        limits,
        total: 0,
        offset: 0,
        line: 1,
        column: 1,
        lookahead: None,
        frames: Vec::new(),
        root_started: false,
        root_done: false,
        terminal: None,
        eof: false,
        record_mode: false,
        allocation_budget,
    })
}

fn jet_enc_json_writer(
    output: JetFileWriter,
    limits: jet_std::EncodingLimits,
    canonical: bool,
) -> Result<jet_std::JSONWriter, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits)?;
    Ok(jet_std::JSONWriter {
        output,
        limits,
        frames: Vec::new(),
        root_written: false,
        finished: false,
        terminal: None,
        total: 0,
        canonical,
        canonical_frames: Vec::new(),
        canonical_retained: 0,
    })
}

impl jet_std::JSONReader {
    fn path(&self) -> String {
        let mut path = "$".to_string();
        for frame in &self.frames {
            match frame {
                JetJsonReadFrame::ArrayValueOrEnd { index, .. }
                | JetJsonReadFrame::ArrayCommaOrEnd { index } => {
                    path.push('[');
                    path.push_str(&index.to_string());
                    path.push(']');
                }
                JetJsonReadFrame::ObjectColonValue { key, .. }
                | JetJsonReadFrame::ObjectCommaOrEnd { key, .. } => {
                    path.push('[');
                    path.push_str(&format!("{:?}", key));
                    path.push(']');
                }
                JetJsonReadFrame::ObjectKeyOrEnd { .. } => {}
            }
        }
        path
    }

    fn fail<T>(&mut self, mut error: jet_std::EncodingError) -> Result<T, jet_std::EncodingError> {
        if error.path.is_empty() { error.path = self.path(); }
        self.terminal = Some(error.clone());
        Err(error)
    }

    fn append_item_bytes(&self, bytes: &mut Vec<u8>, append: &[u8], item: &str) -> Result<(), jet_std::EncodingError> {
        let limit = self.limits.max_item_bytes as usize;
        let Some(new_len) = bytes.len().checked_add(append.len()) else {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.offset, self.line, self.column, format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)));
        };
        if new_len > limit {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.offset, self.line, self.column, format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)));
        }
        if new_len > bytes.capacity() {
            let next_capacity = bytes.capacity().max(1).saturating_mul(2).max(new_len);
            let growth = next_capacity.saturating_sub(bytes.capacity());
            if self.allocation_budget.as_ref().is_some_and(|budget| !budget.charge(growth)) {
                return Err(self.item_allocation_error(item));
            }
            if bytes.try_reserve_exact(next_capacity - bytes.len()).is_err() {
                return Err(self.item_allocation_error(item));
            }
            debug_assert_eq!(bytes.capacity(), next_capacity);
        }
        bytes.extend_from_slice(append);
        Ok(())
    }

    fn item_allocation_error(&self, item: &str) -> jet_std::EncodingError {
        jet_encoding_error(
            jet_std::EncodingErrorKind::Limit,
            self.offset,
            self.line,
            self.column,
            format!("JSON {item} allocation exceeded the bounded codec heap ceiling"),
        )
    }

    fn clone_key_for_frame(&self, key: &str) -> Result<(String, usize), jet_std::EncodingError> {
        let Some(budget) = &self.allocation_budget else { return Ok((key.to_string(), 0)) };
        let capacity = key.len();
        if !budget.charge(capacity) { return Err(self.item_allocation_error("string")); }
        let mut cloned = String::new();
        if cloned.try_reserve_exact(capacity).is_err() { return Err(self.item_allocation_error("string")); }
        cloned.push_str(key);
        debug_assert_eq!(cloned.capacity(), capacity);
        Ok((cloned, capacity))
    }

    fn release_item_heap(&self, bytes: usize) {
        if let Some(budget) = &self.allocation_budget { budget.release(bytes); }
    }

    fn frame_allocation_error(&self) -> jet_std::EncodingError {
        jet_encoding_error(
            jet_std::EncodingErrorKind::Limit,
            self.offset,
            self.line,
            self.column,
            "JSON reader frame allocation exceeded the bounded codec heap ceiling",
        )
    }

    fn reserve_read_frame(&mut self) -> Result<(), jet_std::EncodingError> {
        if self.frames.len() < self.frames.capacity() {
            return Ok(());
        }
        let old = self.frames.capacity();
        let next = if old == 0 { 1 } else { old.saturating_mul(2).max(self.frames.len().saturating_add(1)) };
        let growth = next.saturating_sub(old).saturating_mul(std::mem::size_of::<JetJsonReadFrame>());
        if growth > 0 && self.allocation_budget.as_ref().is_some_and(|budget| !budget.charge(growth)) {
            return Err(self.frame_allocation_error());
        }
        if self.frames.try_reserve_exact(next.saturating_sub(self.frames.len())).is_err() {
            return Err(self.frame_allocation_error());
        }
        Ok(())
    }

    fn fill(&mut self) -> Result<Option<u8>, jet_std::EncodingError> {
        if let Some(byte) = self.lookahead {
            return Ok(Some(byte));
        }
        use std::io::Read;
        let mut byte = [0u8; 1];
        match self.input.inner.read(&mut byte) {
            Ok(0) => Ok(None),
            Ok(_) => {
                self.lookahead = Some(byte[0]);
                Ok(Some(byte[0]))
            }
            Err(error) => Err(jet_encoding_io_error(self.offset, self.line, self.column, error)),
        }
    }

    fn take(&mut self) -> Result<Option<u8>, jet_std::EncodingError> {
        let Some(byte) = self.fill()? else { return Ok(None) };
        if self.limits.max_total_bytes.is_some_and(|max| self.total >= max) {
            return Err(jet_encoding_error(
                jet_std::EncodingErrorKind::Limit,
                self.offset,
                self.line,
                self.column,
                format!("max_total_bytes {} exceeded", self.limits.max_total_bytes.unwrap_or(0)),
            ));
        }
        self.lookahead = None;
        self.total += 1;
        self.offset += 1;
        if byte == b'\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Ok(Some(byte))
    }

    fn skip_ws(&mut self) -> Result<(), jet_std::EncodingError> {
        while if self.record_mode {
            matches!(self.fill()?, Some(b' ' | b'\t'))
        } else {
            matches!(self.fill()?, Some(b' ' | b'\n' | b'\r' | b'\t'))
        } {
            self.take()?;
        }
        Ok(())
    }

    fn expect_byte(&mut self, want: u8, context: &str) -> Result<(), jet_std::EncodingError> {
        match self.take()? {
            Some(got) if got == want => Ok(()),
            Some(got) => Err(jet_encoding_error(
                jet_std::EncodingErrorKind::Syntax,
                self.offset - 1,
                self.line,
                self.column.saturating_sub(1),
                format!("expected {} but found {:?}", context, got as char),
            )),
            None => Err(jet_encoding_error(
                jet_std::EncodingErrorKind::Truncated,
                self.offset,
                self.line,
                self.column,
                format!("expected {} before end of input", context),
            )),
        }
    }

    fn read_hex4(&mut self) -> Result<u16, jet_std::EncodingError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let Some(byte) = self.take()? else {
                return Err(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "incomplete Unicode escape"));
            };
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), "Unicode escape contains a non-hex digit")),
            };
            value = value * 16 + u16::from(digit);
        }
        Ok(value)
    }

    fn read_string(&mut self) -> Result<String, jet_std::EncodingError> {
        self.expect_byte(b'"', "a string")?;
        let mut bytes = Vec::new();
        loop {
            let Some(byte) = self.take()? else {
                return Err(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "unterminated JSON string"));
            };
            match byte {
                b'"' => break,
                0..=0x1f => return Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), "unescaped control byte in JSON string")),
                b'\\' => {
                    let Some(escaped) = self.take()? else {
                        return Err(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "incomplete JSON escape"));
                    };
                    match escaped {
                        b'"' | b'\\' | b'/' => self.append_item_bytes(&mut bytes, &[escaped], "string")?,
                        b'b' => self.append_item_bytes(&mut bytes, &[8], "string")?,
                        b'f' => self.append_item_bytes(&mut bytes, &[12], "string")?,
                        b'n' => self.append_item_bytes(&mut bytes, b"\n", "string")?,
                        b'r' => self.append_item_bytes(&mut bytes, b"\r", "string")?,
                        b't' => self.append_item_bytes(&mut bytes, b"\t", "string")?,
                        b'u' => {
                            let first = self.read_hex4()?;
                            let scalar = if (0xd800..=0xdbff).contains(&first) {
                                self.expect_byte(b'\\', "a low-surrogate escape")?;
                                self.expect_byte(b'u', "a low-surrogate escape")?;
                                let low = self.read_hex4()?;
                                if !(0xdc00..=0xdfff).contains(&low) {
                                    return Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - 4, self.line, self.column.saturating_sub(4), "high surrogate is not followed by a low surrogate"));
                                }
                                0x10000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(low) - 0xdc00)
                            } else if (0xdc00..=0xdfff).contains(&first) {
                                return Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - 4, self.line, self.column.saturating_sub(4), "unpaired low surrogate"));
                            } else {
                                u32::from(first)
                            };
                            let ch = char::from_u32(scalar).ok_or_else(|| jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "invalid Unicode scalar"))?;
                            let mut buf = [0u8; 4];
                            self.append_item_bytes(&mut bytes, ch.encode_utf8(&mut buf).as_bytes(), "string")?;
                        }
                        _ => return Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), "unknown JSON escape")),
                    }
                }
                _ => self.append_item_bytes(&mut bytes, &[byte], "string")?,
            }
        }
        String::from_utf8(bytes).map_err(|_| jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "JSON string is not valid UTF-8"))
    }

    fn read_literal(&mut self, tail: &[u8], event: jet_std::DataEvent) -> Result<jet_std::DataEvent, jet_std::EncodingError> {
        for byte in tail {
            self.expect_byte(*byte, "JSON literal")?;
        }
        Ok(event)
    }

    fn read_number(&mut self, first: u8) -> Result<jet_std::DataEvent, jet_std::EncodingError> {
        let mut bytes = Vec::new();
        self.append_item_bytes(&mut bytes, &[first], "number")?;
        while let Some(byte @ (b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) = self.fill()? {
            self.append_item_bytes(&mut bytes, &[byte], "number")?;
            self.take()?;
        }
        let text = std::str::from_utf8(&bytes).unwrap_or("");
        let valid = {
            let b = text.as_bytes();
            let mut i = usize::from(b.first() == Some(&b'-'));
            let int_ok = if b.get(i) == Some(&b'0') { i += 1; true } else if matches!(b.get(i), Some(b'1'..=b'9')) { i += 1; while matches!(b.get(i), Some(b'0'..=b'9')) { i += 1; } true } else { false };
            if int_ok && b.get(i) == Some(&b'.') { i += 1; let start = i; while matches!(b.get(i), Some(b'0'..=b'9')) { i += 1; } if i == start { i = usize::MAX; } }
            if i != usize::MAX && matches!(b.get(i), Some(b'e' | b'E')) { i += 1; if matches!(b.get(i), Some(b'+' | b'-')) { i += 1; } let start = i; while matches!(b.get(i), Some(b'0'..=b'9')) { i += 1; } if i == start { i = usize::MAX; } }
            i == b.len()
        };
        let result = if !valid {
            Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - bytes.len() as i64, self.line, self.column, format!("invalid JSON number `{}`", text)))
        } else if !text.contains(['.', 'e', 'E']) {
            match text.parse::<i64>() {
                Ok(value) => Ok(jet_std::DataEvent::Int(value)),
                Err(_) => match text.parse::<f64>() {
                    Ok(value) if value.is_finite() => Ok(jet_std::DataEvent::Float(value)),
                    _ => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.offset - bytes.len() as i64, self.line, self.column, "JSON number is outside the DataTree numeric range")),
                },
            }
        } else {
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(jet_std::DataEvent::Float(value)),
                _ => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.offset - bytes.len() as i64, self.line, self.column, "JSON number is outside the DataTree numeric range")),
            }
        };
        self.release_item_heap(bytes.capacity());
        result
    }

    fn parse_value(&mut self) -> Result<jet_std::DataEvent, jet_std::EncodingError> {
        self.skip_ws()?;
        let Some(first) = self.take()? else {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "expected a JSON value"));
        };
        match first {
            b'n' => self.read_literal(b"ull", jet_std::DataEvent::Null),
            b't' => self.read_literal(b"rue", jet_std::DataEvent::Bool(true)),
            b'f' => self.read_literal(b"alse", jet_std::DataEvent::Bool(false)),
            b'"' => { self.lookahead = Some(b'"'); self.offset -= 1; self.total -= 1; self.column -= 1; Ok(jet_std::DataEvent::Text(self.read_string()?)) }
            b'[' => {
                if self.frames.len() as i64 >= self.limits.max_depth { return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.offset - 1, self.line, self.column.saturating_sub(1), format!("max_depth {} exceeded", self.limits.max_depth))); }
                self.reserve_read_frame()?;
                self.frames.push(JetJsonReadFrame::ArrayValueOrEnd { first: true, index: 0 });
                Ok(jet_std::DataEvent::ArrayStart)
            }
            b'{' => {
                if self.frames.len() as i64 >= self.limits.max_depth { return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.offset - 1, self.line, self.column.saturating_sub(1), format!("max_depth {} exceeded", self.limits.max_depth))); }
                self.reserve_read_frame()?;
                self.frames.push(JetJsonReadFrame::ObjectKeyOrEnd { first: true });
                Ok(jet_std::DataEvent::ObjectStart)
            }
            b'-' | b'0'..=b'9' => self.read_number(first),
            _ => Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), format!("unexpected byte {:?} while reading a JSON value", first as char))),
        }
    }

    fn next_event(&mut self) -> Result<Option<jet_std::DataEvent>, jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        match self.next_event_inner() {
            Ok(event) => Ok(event),
            Err(error) => self.fail(error),
        }
    }

    fn next_event_inner(&mut self) -> Result<Option<jet_std::DataEvent>, jet_std::EncodingError> {
        if self.eof { return Ok(None); }
        loop {
            self.skip_ws()?;
            let state = self.frames.last().map(|frame| match frame {
                JetJsonReadFrame::ArrayValueOrEnd { first, index } => JetJsonReadState::ArrayValueOrEnd { first: *first, index: *index },
                JetJsonReadFrame::ArrayCommaOrEnd { index } => JetJsonReadState::ArrayCommaOrEnd { index: *index },
                JetJsonReadFrame::ObjectKeyOrEnd { first } => JetJsonReadState::ObjectKeyOrEnd { first: *first },
                JetJsonReadFrame::ObjectColonValue { .. } => JetJsonReadState::ObjectColonValue,
                JetJsonReadFrame::ObjectCommaOrEnd { .. } => JetJsonReadState::ObjectCommaOrEnd,
            });
            match state {
                Some(JetJsonReadState::ArrayValueOrEnd { first, index }) => {
                    if self.fill()? == Some(b']') {
                        if !first { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "expected a JSON value after `,`")); }
                        self.take()?; self.frames.pop(); return Ok(Some(jet_std::DataEvent::ArrayEnd));
                    }
                    if !first && self.fill()?.is_none() { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "unterminated JSON array")); }
                    *self.frames.last_mut().unwrap() = JetJsonReadFrame::ArrayCommaOrEnd { index };
                    return self.parse_value().map(Some).or_else(|e| self.fail(e));
                }
                Some(JetJsonReadState::ArrayCommaOrEnd { index }) => match self.fill()? {
                    Some(b',') => { self.take()?; *self.frames.last_mut().unwrap() = JetJsonReadFrame::ArrayValueOrEnd { first: false, index: index + 1 }; }
                    Some(b']') => { self.take()?; self.frames.pop(); return Ok(Some(jet_std::DataEvent::ArrayEnd)); }
                    Some(_) => return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "expected `,` or `]` after array value")),
                    None => return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "unterminated JSON array")),
                },
                Some(JetJsonReadState::ObjectKeyOrEnd { first }) => {
                    if self.fill()? == Some(b'}') {
                        if !first { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "expected an object key after `,`")); }
                        self.take()?; self.frames.pop(); return Ok(Some(jet_std::DataEvent::ObjectEnd));
                    }
                    if !first && self.fill()?.is_none() { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "unterminated JSON object")); }
                    if self.fill()? != Some(b'"') { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "expected a quoted object key")); }
                    let key = match self.read_string() { Ok(v) => v, Err(e) => return self.fail(e) };
                    let (frame_key, key_heap) = match self.clone_key_for_frame(&key) { Ok(value) => value, Err(error) => return self.fail(error) };
                    *self.frames.last_mut().unwrap() = JetJsonReadFrame::ObjectColonValue { key: frame_key, key_heap };
                    return Ok(Some(jet_std::DataEvent::Key(key)));
                }
                Some(JetJsonReadState::ObjectColonValue) => {
                    if let Err(e) = self.expect_byte(b':', "`:` after object key") { return self.fail(e); }
                    let frame = self.frames.last_mut().unwrap();
                    let previous = std::mem::replace(frame, JetJsonReadFrame::ObjectKeyOrEnd { first: false });
                    let JetJsonReadFrame::ObjectColonValue { key, key_heap } = previous else { unreachable!() };
                    *frame = JetJsonReadFrame::ObjectCommaOrEnd { key, key_heap };
                    return self.parse_value().map(Some).or_else(|e| self.fail(e));
                }
                Some(JetJsonReadState::ObjectCommaOrEnd) => match self.fill()? {
                    Some(b',') => {
                        self.take()?;
                        let previous = std::mem::replace(self.frames.last_mut().unwrap(), JetJsonReadFrame::ObjectKeyOrEnd { first: false });
                        let JetJsonReadFrame::ObjectCommaOrEnd { key_heap, .. } = previous else { unreachable!() };
                        self.release_item_heap(key_heap);
                    }
                    Some(b'}') => {
                        self.take()?;
                        let Some(JetJsonReadFrame::ObjectCommaOrEnd { key_heap, .. }) = self.frames.pop() else { unreachable!() };
                        self.release_item_heap(key_heap);
                        return Ok(Some(jet_std::DataEvent::ObjectEnd));
                    }
                    Some(_) => return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "expected `,` or `}` after object value")),
                    None => return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "unterminated JSON object")),
                },
                None if !self.root_started => {
                    self.root_started = true;
                    self.root_done = true;
                    return self.parse_value().map(Some).or_else(|e| self.fail(e));
                }
                None if self.record_mode => match self.fill()? {
                    Some(b'\n') => {
                        self.take()?;
                        self.root_started = false;
                        self.root_done = false;
                        return Ok(None);
                    }
                    Some(b'\r') => {
                        self.take()?;
                        if self.fill()? != Some(b'\n') {
                            return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), "JSONL records use LF or CRLF; bare CR is not a record ending"));
                        }
                        self.take()?;
                        self.root_started = false;
                        self.root_done = false;
                        return Ok(None);
                    }
                    None => { self.eof = true; return Ok(None); }
                    Some(_) => return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "trailing input after JSONL record value")),
                },
                None => match self.fill()? {
                    None => { self.eof = true; return Ok(None); }
                    Some(_) => return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "trailing input after JSON root")),
                },
            }
        }
    }
}

fn jet_enc_json_reader_next(reader: &mut jet_std::JSONReader) -> Result<Option<jet_std::DataEvent>, jet_std::EncodingError> {
    reader.next_event()
}

impl jet_std::JSONWriter {
    fn fail<T>(&mut self, error: jet_std::EncodingError) -> Result<T, jet_std::EncodingError> { self.terminal = Some(error.clone()); Err(error) }
    fn state_error(&self, reason: &str) -> jet_std::EncodingError { jet_encoding_error(jet_std::EncodingErrorKind::State, self.total, 1, self.total + 1, reason) }
    fn ensure_total(&self, additional: usize) -> Result<(), jet_std::EncodingError> {
        let additional = i64::try_from(additional).unwrap_or(i64::MAX);
        if self.limits.max_total_bytes.is_some_and(|max| self.total.saturating_add(additional) > max) {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_total_bytes {} exceeded", self.limits.max_total_bytes.unwrap_or(0))));
        }
        Ok(())
    }
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), jet_std::EncodingError> {
        self.ensure_total(bytes.len())?;
        use std::io::Write;
        self.output.inner.write_all(bytes).map_err(|e| jet_encoding_io_error(self.total, 1, self.total + 1, e))?;
        self.total += bytes.len() as i64;
        Ok(())
    }
    fn quoted_len(&self, text: &str) -> Result<usize, jet_std::EncodingError> {
        let mut len = 0usize;
        for ch in text.chars() {
            let add = match ch { '"' | '\\' | '\n' | '\r' | '\t' | '\u{08}' | '\u{0c}' => 2, c if c <= '\u{1f}' => 6, c => c.len_utf8() };
            len = len.checked_add(add).ok_or_else(|| jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)))?;
        }
        if len > self.limits.max_item_bytes as usize {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)));
        }
        Ok(len)
    }
    fn write_quoted(&mut self, text: &str) -> Result<(), jet_std::EncodingError> {
        self.quoted_len(text)?;
        self.write_bytes(b"\"")?;
        for ch in text.chars() {
            match ch {
                '"' => self.write_bytes(b"\\\"")?,
                '\\' => self.write_bytes(b"\\\\")?,
                '\n' => self.write_bytes(b"\\n")?,
                '\r' => self.write_bytes(b"\\r")?,
                '\t' => self.write_bytes(b"\\t")?,
                '\u{08}' => self.write_bytes(b"\\b")?,
                '\u{0c}' => self.write_bytes(b"\\f")?,
                c if c <= '\u{1f}' => {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    let value = c as usize;
                    let escaped = [b'\\', b'u', HEX[(value >> 12) & 15], HEX[(value >> 8) & 15], HEX[(value >> 4) & 15], HEX[value & 15]];
                    self.write_bytes(&escaped)?;
                }
                c => {
                    let mut buf = [0u8; 4];
                    self.write_bytes(c.encode_utf8(&mut buf).as_bytes())?;
                }
            }
        }
        self.write_bytes(b"\"")
    }
    fn before_value(&mut self) -> Result<(), jet_std::EncodingError> {
        match self.frames.last().copied() {
            Some(JetJsonWriteFrame::Array { first }) => {
                if !first { self.write_bytes(b",")?; }
                *self.frames.last_mut().unwrap() = JetJsonWriteFrame::Array { first: false };
            }
            Some(JetJsonWriteFrame::ObjectValue) => { let len = self.frames.len(); self.frames[len - 1] = JetJsonWriteFrame::ObjectKey { first: false }; }
            Some(JetJsonWriteFrame::ObjectKey { .. }) => return Err(self.state_error("JSON object expects Key before a value")),
            None if self.root_written => return Err(self.state_error("JSON writer accepts exactly one root")),
            None => self.root_written = true,
        }
        Ok(())
    }
    fn value_prefix_len(&self) -> Result<usize, jet_std::EncodingError> {
        match self.frames.last().copied() {
            Some(JetJsonWriteFrame::Array { first }) => Ok(usize::from(!first)),
            Some(JetJsonWriteFrame::ObjectValue) => Ok(0),
            Some(JetJsonWriteFrame::ObjectKey { .. }) => Err(self.state_error("JSON object expects Key before a value")),
            None if self.root_written => Err(self.state_error("JSON writer accepts exactly one root")),
            None => Ok(0),
        }
    }
    fn write_event(&mut self, event: jet_std::DataEvent) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        let result = if self.canonical {
            self.write_canonical_event_inner(event)
        } else {
            self.write_event_inner(event)
        };
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error),
        }
    }

    fn canonical_workspace_limit(&self) -> usize {
        (self.limits.max_item_bytes as usize)
            .saturating_mul(2)
            .saturating_add(self.limits.buffer_bytes as usize)
            .saturating_add((self.limits.max_depth as usize).saturating_mul(128))
    }

    fn canonical_charge(&mut self, frame: usize, bytes: usize) -> Result<(), jet_std::EncodingError> {
        let Some(next) = self.canonical_retained.checked_add(bytes) else {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, "canonical JSON workspace size overflow"));
        };
        if next > self.canonical_workspace_limit() {
            return Err(jet_encoding_error(
                jet_std::EncodingErrorKind::Limit,
                self.total,
                1,
                self.total + 1,
                format!("canonical JSON workspace exceeded bounded max_item_bytes {}", self.limits.max_item_bytes),
            ));
        }
        let JetJsonCanonicalFrame::Object { retained, .. } = &mut self.canonical_frames[frame] else { unreachable!() };
        *retained = retained.saturating_add(bytes);
        self.canonical_retained = next;
        Ok(())
    }

    fn canonical_object_sink(&self) -> Option<usize> {
        self.canonical_frames.iter().rposition(|frame| matches!(frame, JetJsonCanonicalFrame::Object { .. }))
    }

    fn canonical_emit(&mut self, bytes: &[u8]) -> Result<(), jet_std::EncodingError> {
        if let Some(frame) = self.canonical_object_sink() {
            let current_len = match &self.canonical_frames[frame] {
                JetJsonCanonicalFrame::Object { value, .. } => value.len(),
                _ => unreachable!(),
            };
            let Some(next_len) = current_len.checked_add(bytes.len()) else {
                return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, "canonical JSON object size overflow"));
            };
            if next_len > self.limits.max_item_bytes as usize {
                return Err(jet_encoding_error(
                    jet_std::EncodingErrorKind::Limit,
                    self.total,
                    1,
                    self.total + 1,
                    format!("max_item_bytes {} exceeded by canonical object", self.limits.max_item_bytes),
                ));
            }
            let growth = match &self.canonical_frames[frame] {
                JetJsonCanonicalFrame::Object { value, .. } if next_len > value.capacity() => next_len - value.capacity(),
                _ => 0,
            };
            self.canonical_charge(frame, growth)?;
            let JetJsonCanonicalFrame::Object { value, .. } = &mut self.canonical_frames[frame] else { unreachable!() };
            if growth > 0 && value.try_reserve_exact(growth).is_err() {
                return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, "canonical JSON object allocation failed within configured bounds"));
            }
            value.extend_from_slice(bytes);
            Ok(())
        } else {
            self.write_bytes(bytes)
        }
    }

    fn canonical_quote(&self, text: &str) -> Result<Vec<u8>, jet_std::EncodingError> {
        let inner = self.quoted_len(text)?;
        let total = inner.checked_add(2).ok_or_else(|| jet_encoding_error(
            jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1,
            format!("max_item_bytes {} exceeded", self.limits.max_item_bytes),
        ))?;
        let mut out = Vec::new();
        if out.try_reserve_exact(total).is_err() {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, "canonical JSON string allocation failed within configured bounds"));
        }
        out.push(b'"');
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for ch in text.chars() {
            match ch {
                '"' => out.extend_from_slice(b"\\\""),
                '\\' => out.extend_from_slice(b"\\\\"),
                '\n' => out.extend_from_slice(b"\\n"),
                '\r' => out.extend_from_slice(b"\\r"),
                '\t' => out.extend_from_slice(b"\\t"),
                '\u{08}' => out.extend_from_slice(b"\\b"),
                '\u{0c}' => out.extend_from_slice(b"\\f"),
                c if c <= '\u{1f}' => {
                    let value = c as usize;
                    out.extend_from_slice(&[b'\\', b'u', HEX[(value >> 12) & 15], HEX[(value >> 8) & 15], HEX[(value >> 4) & 15], HEX[value & 15]]);
                }
                c => {
                    let mut encoded = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut encoded).as_bytes());
                }
            }
        }
        out.push(b'"');
        Ok(out)
    }

    fn canonical_before_value(&mut self) -> Result<(), jet_std::EncodingError> {
        match self.canonical_frames.last() {
            Some(JetJsonCanonicalFrame::Array { first }) => {
                let first = *first;
                if !first { self.canonical_emit(b",")?; }
                let Some(JetJsonCanonicalFrame::Array { first }) = self.canonical_frames.last_mut() else { unreachable!() };
                *first = false;
            }
            Some(JetJsonCanonicalFrame::Object { key: Some(_), value, .. }) if value.is_empty() => {}
            Some(JetJsonCanonicalFrame::Object { key: None, .. }) => return Err(self.state_error("canonical JSON object expects Key before a value")),
            Some(JetJsonCanonicalFrame::Object { .. }) => return Err(self.state_error("canonical JSON object value is already active")),
            None if self.root_written => return Err(self.state_error("JSON writer accepts exactly one root")),
            None => self.root_written = true,
        }
        Ok(())
    }

    fn canonical_complete_value(&mut self) -> Result<(), jet_std::EncodingError> {
        let Some(frame) = self.canonical_frames.len().checked_sub(1) else { return Ok(()) };
        let (key, value) = match &mut self.canonical_frames[frame] {
            JetJsonCanonicalFrame::Object { key, value, .. } => {
                let Some(key) = key.take() else { return Ok(()) };
                (key, std::mem::take(value))
            }
            JetJsonCanonicalFrame::Array { .. } => return Ok(()),
        };
        let old_capacity = value.capacity();
        let entry_size = std::mem::size_of::<(String, Vec<u8>)>();
        let growth = match &self.canonical_frames[frame] {
            JetJsonCanonicalFrame::Object { entries, .. } if entries.len() == entries.capacity() => {
                entries.capacity().max(1).saturating_mul(2).saturating_sub(entries.capacity()).saturating_mul(entry_size)
            }
            _ => 0,
        };
        self.canonical_charge(frame, growth)?;
        let JetJsonCanonicalFrame::Object { entries, .. } = &mut self.canonical_frames[frame] else { unreachable!() };
        if growth > 0 && entries.try_reserve_exact(growth / entry_size).is_err() {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, "canonical JSON entry allocation failed within configured bounds"));
        }
        entries.push((key, value));
        debug_assert!(old_capacity > 0 || entries.last().is_some_and(|(_, value)| value.is_empty()));
        Ok(())
    }

    fn canonical_render_object(&mut self) -> Result<Vec<u8>, jet_std::EncodingError> {
        let Some(JetJsonCanonicalFrame::Object { mut entries, key: None, value, retained }) = self.canonical_frames.pop() else {
            return Err(self.state_error("ObjectEnd does not match a complete canonical object"));
        };
        if !value.is_empty() {
            return Err(self.state_error("canonical JSON object value was not completed"));
        }
        entries.sort_by(|left, right| jet_json_jcs_key_cmp(&left.0, &right.0));
        for pair in entries.windows(2) {
            if pair[0].0 == pair[1].0 {
                self.canonical_retained = self.canonical_retained.saturating_sub(retained);
                return Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.total, 1, self.total + 1, "JCS requires unique object keys"));
            }
        }
        let mut total = 2usize;
        let mut quoted = Vec::with_capacity(entries.len());
        for (index, (key, value)) in entries.iter().enumerate() {
            let key = self.canonical_quote(key)?;
            total = total.checked_add(usize::from(index > 0)).and_then(|n| n.checked_add(key.len())).and_then(|n| n.checked_add(1)).and_then(|n| n.checked_add(value.len()))
                .ok_or_else(|| jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, "canonical JSON object size overflow"))?;
            quoted.push(key);
        }
        if total > self.limits.max_item_bytes as usize {
            self.canonical_retained = self.canonical_retained.saturating_sub(retained);
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_item_bytes {} exceeded by canonical object", self.limits.max_item_bytes)));
        }
        let mut out = Vec::new();
        if out.try_reserve_exact(total).is_err() {
            self.canonical_retained = self.canonical_retained.saturating_sub(retained);
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, "canonical JSON object allocation failed within configured bounds"));
        }
        out.push(b'{');
        for (index, ((_, value), key)) in entries.into_iter().zip(quoted).enumerate() {
            if index > 0 { out.push(b','); }
            out.extend_from_slice(&key);
            out.push(b':');
            out.extend_from_slice(&value);
        }
        out.push(b'}');
        self.canonical_retained = self.canonical_retained.saturating_sub(retained);
        Ok(out)
    }

    fn write_canonical_event_inner(&mut self, event: jet_std::DataEvent) -> Result<(), jet_std::EncodingError> {
        if self.finished { return Err(self.state_error("write called after finish")); }
        match event {
            jet_std::DataEvent::Key(key) => {
                let Some(JetJsonCanonicalFrame::Object { entries, key: current, .. }) = self.canonical_frames.last() else {
                    return Err(self.state_error("Key is only valid while an object expects a key"));
                };
                if current.is_some() { return Err(self.state_error("canonical JSON object key has no value")); }
                if entries.iter().any(|(old, _)| old == &key) {
                    return Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.total, 1, self.total + 1, "JCS requires unique object keys"));
                }
                let frame = self.canonical_frames.len() - 1;
                self.canonical_charge(frame, key.capacity())?;
                let JetJsonCanonicalFrame::Object { key: current, .. } = &mut self.canonical_frames[frame] else { unreachable!() };
                *current = Some(key);
                Ok(())
            }
            jet_std::DataEvent::ArrayEnd => {
                if !matches!(self.canonical_frames.last(), Some(JetJsonCanonicalFrame::Array { .. })) {
                    return Err(self.state_error("ArrayEnd does not match an open array"));
                }
                self.canonical_emit(b"]")?;
                self.canonical_frames.pop();
                self.canonical_complete_value()
            }
            jet_std::DataEvent::ObjectEnd => {
                if matches!(self.canonical_frames.last(), Some(JetJsonCanonicalFrame::Object { key: Some(_), .. })) {
                    return Err(self.state_error("object key has no value"));
                }
                if !matches!(self.canonical_frames.last(), Some(JetJsonCanonicalFrame::Object { .. })) {
                    return Err(self.state_error("ObjectEnd does not match an open object"));
                }
                let object = self.canonical_render_object()?;
                self.canonical_emit(&object)?;
                self.canonical_complete_value()
            }
            jet_std::DataEvent::Bytes(_) => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.total, 1, self.total + 1, "JSON cannot encode Bytes; encode bytes as Text explicitly")),
            jet_std::DataEvent::Float(value) if !value.is_finite() => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.total, 1, self.total + 1, "JCS cannot encode a non-finite Float")),
            jet_std::DataEvent::Int(value) if (value as f64) as i128 != value as i128 => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.total, 1, self.total + 1, "JCS requires Int exactly representable as IEEE 754 binary64; encode this integer as Text")),
            value => {
                if matches!(value, jet_std::DataEvent::ArrayStart | jet_std::DataEvent::ObjectStart)
                    && self.canonical_frames.len() as i64 >= self.limits.max_depth
                {
                    return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_depth {} exceeded", self.limits.max_depth)));
                }
                let encoded = match &value {
                    jet_std::DataEvent::Null => Some(b"null".to_vec()),
                    jet_std::DataEvent::Bool(true) => Some(b"true".to_vec()),
                    jet_std::DataEvent::Bool(false) => Some(b"false".to_vec()),
                    jet_std::DataEvent::Int(value) => Some(jet_json_jcs_number(*value as f64).into_bytes()),
                    jet_std::DataEvent::Float(value) => Some(jet_json_jcs_number(*value).into_bytes()),
                    jet_std::DataEvent::Text(value) => Some(self.canonical_quote(value)?),
                    jet_std::DataEvent::ArrayStart | jet_std::DataEvent::ObjectStart => None,
                    _ => unreachable!(),
                };
                self.canonical_before_value()?;
                match value {
                    jet_std::DataEvent::ArrayStart => {
                        self.canonical_emit(b"[")?;
                        self.canonical_frames.push(JetJsonCanonicalFrame::Array { first: true });
                        Ok(())
                    }
                    jet_std::DataEvent::ObjectStart => {
                        self.canonical_frames.push(JetJsonCanonicalFrame::Object { entries: Vec::new(), key: None, value: Vec::new(), retained: 0 });
                        Ok(())
                    }
                    _ => {
                        self.canonical_emit(encoded.as_deref().unwrap())?;
                        self.canonical_complete_value()
                    }
                }
            }
        }
    }
    fn write_event_inner(&mut self, event: jet_std::DataEvent) -> Result<(), jet_std::EncodingError> {
        if self.finished { return Err(self.state_error("write called after finish")); }
        let result = match event {
            jet_std::DataEvent::Key(key) => {
                let first = match self.frames.last().copied() { Some(JetJsonWriteFrame::ObjectKey { first }) => first, _ => return self.fail(self.state_error("Key is only valid while an object expects a key")) };
                let key_len = self.quoted_len(&key)?.checked_add(2).and_then(|n| n.checked_add(1 + usize::from(!first)))
                    .ok_or_else(|| jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)))?;
                self.ensure_total(key_len)?;
                if !first { self.write_bytes(b",")?; }
                self.write_quoted(&key)?; self.write_bytes(b":")?;
                *self.frames.last_mut().unwrap() = JetJsonWriteFrame::ObjectValue; Ok(())
            }
            jet_std::DataEvent::ArrayEnd => match self.frames.last().copied() { Some(JetJsonWriteFrame::Array { .. }) => { self.ensure_total(1)?; self.write_bytes(b"]")?; self.frames.pop(); Ok(()) }, _ => Err(self.state_error("ArrayEnd does not match an open array")) },
            jet_std::DataEvent::ObjectEnd => match self.frames.last().copied() { Some(JetJsonWriteFrame::ObjectKey { .. }) => { self.ensure_total(1)?; self.write_bytes(b"}")?; self.frames.pop(); Ok(()) }, Some(JetJsonWriteFrame::ObjectValue) => Err(self.state_error("object key has no value")), _ => Err(self.state_error("ObjectEnd does not match an open object")) },
            jet_std::DataEvent::Bytes(_) => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.total, 1, self.total + 1, "JSON cannot encode Bytes; encode bytes as Text explicitly")),
            jet_std::DataEvent::Float(value) if !value.is_finite() => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.total, 1, self.total + 1, "JSON cannot encode a non-finite Float")),
            value => {
                if matches!(value, jet_std::DataEvent::ArrayStart | jet_std::DataEvent::ObjectStart)
                    && self.frames.len() as i64 >= self.limits.max_depth
                {
                    return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_depth {} exceeded", self.limits.max_depth)));
                }
                let payload_len = match &value {
                    jet_std::DataEvent::Null => 4,
                    jet_std::DataEvent::Bool(true) => 4,
                    jet_std::DataEvent::Bool(false) => 5,
                    jet_std::DataEvent::Int(v) => v.to_string().len(),
                    jet_std::DataEvent::Float(v) => v.to_string().len(),
                    jet_std::DataEvent::Text(v) => self.quoted_len(v)?.checked_add(2).ok_or_else(|| jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)))?,
                    jet_std::DataEvent::ArrayStart | jet_std::DataEvent::ObjectStart => 1,
                    _ => return Err(self.state_error("unsupported JSON writer event")),
                };
                let event_len = self.value_prefix_len()?.checked_add(payload_len)
                    .ok_or_else(|| jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, "JSON event wire size overflow"))?;
                self.ensure_total(event_len)?;
                self.before_value()?;
                match value {
                    jet_std::DataEvent::Null => self.write_bytes(b"null"),
                    jet_std::DataEvent::Bool(v) => self.write_bytes(if v { b"true" } else { b"false" }),
                    jet_std::DataEvent::Int(v) => self.write_bytes(v.to_string().as_bytes()),
                    jet_std::DataEvent::Float(v) => self.write_bytes(v.to_string().as_bytes()),
                    jet_std::DataEvent::Text(v) => self.write_quoted(&v),
                    jet_std::DataEvent::ArrayStart => { self.write_bytes(b"[")?; self.frames.push(JetJsonWriteFrame::Array { first: true }); Ok(()) },
                    jet_std::DataEvent::ObjectStart => { self.write_bytes(b"{")?; self.frames.push(JetJsonWriteFrame::ObjectKey { first: true }); Ok(()) },
                    _ => unreachable!(),
                }
            }
        };
        result
    }
    fn flush_output(&mut self) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        use std::io::Write;
        self.output.inner.flush().map_err(|e| jet_encoding_io_error(self.total, 1, self.total + 1, e)).or_else(|e| self.fail(e))
    }
    fn finish_output(&mut self) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        if self.finished { return Ok(()); }
        let open = if self.canonical { !self.canonical_frames.is_empty() } else { !self.frames.is_empty() };
        if !self.root_written || open { return self.fail(self.state_error("finish requires one structurally complete JSON root")); }
        self.flush_output()?; self.finished = true; Ok(())
    }
}

fn jet_enc_json_writer_write(writer: &mut jet_std::JSONWriter, event: jet_std::DataEvent) -> Result<(), jet_std::EncodingError> { writer.write_event(event) }
fn jet_enc_json_writer_flush(writer: &mut jet_std::JSONWriter) -> Result<(), jet_std::EncodingError> { writer.flush_output() }
fn jet_enc_json_writer_finish(writer: &mut jet_std::JSONWriter) -> Result<(), jet_std::EncodingError> { writer.finish_output() }

// D-ENCSTREAM-SURFACE1=A: JSONL shares the byte-at-a-time JSON tokenizer and
// event writer above. Only record framing and DataEvent<->DataTree folding are
// format-specific; no line buffer or second JSON parser exists here.
enum JetJsonlFoldFrame {
    Array(Vec<jet_std::DataTree>),
    Object {
        entries: Vec<(String, jet_std::DataTree)>,
        key: Option<String>,
    },
}

struct JetJsonlHeapBudget {
    allocation: JetJsonAllocationBudget,
    decoded: usize,
    decoded_limit: usize,
}

impl JetJsonlHeapBudget {
    fn charge(&mut self, bytes: usize) -> bool {
        self.allocation.charge(bytes)
    }

    fn charge_decoded(&mut self, bytes: usize) -> bool {
        let Some(next) = self.decoded.checked_add(bytes) else { return false };
        if next > self.decoded_limit { return false; }
        self.decoded = next;
        true
    }
}

// Charge capacity growth before asking the allocator. `reserve_exact` makes
// the prospective geometric capacity deterministic, with no
// allocate-then-check window. Existing spare capacity was charged when it was
// created, so pushes inside it need no new charge.
fn jet_jsonl_reserve_push<T>(heap: &mut JetJsonlHeapBudget, values: &mut Vec<T>) -> bool {
    if values.len() < values.capacity() { return true; }
    let Some(minimum) = values.len().checked_add(1) else { return false };
    let next_capacity = if values.capacity() == 0 {
        1
    } else {
        let Some(doubled) = values.capacity().checked_mul(2) else { return false };
        doubled.max(minimum)
    };
    let Some(old_bytes) = values.capacity().checked_mul(std::mem::size_of::<T>()) else { return false };
    let Some(next_bytes) = next_capacity.checked_mul(std::mem::size_of::<T>()) else { return false };
    if !heap.charge(next_bytes - old_bytes) { return false; }
    if values.try_reserve_exact(next_capacity - values.len()).is_err() { return false; }
    debug_assert_eq!(values.capacity(), next_capacity);
    true
}

fn jet_jsonl_project_error(
    mut error: jet_std::EncodingError,
    record_index: Option<i64>,
) -> jet_std::EncodingError {
    error.format = jet_std::EncodingFormat::JSONL;
    if let Some(index) = record_index {
        let suffix = error.path.strip_prefix('$').unwrap_or(&error.path);
        error.path = format!("$[{index}]{suffix}");
    }
    error
}

fn jet_enc_jsonl_reader(
    input: JetFileReader,
    limits: jet_std::EncodingLimits,
) -> Result<jet_std::JSONLReader, jet_std::EncodingError> {
    let mut json = jet_enc_json_reader(input, limits)
        .map_err(|error| jet_jsonl_project_error(error, None))?;
    json.record_mode = true;
    Ok(jet_std::JSONLReader {
        json,
        terminal: None,
        record_index: 0,
    })
}

impl jet_std::JSONLReader {
    fn fail<T>(&mut self, error: jet_std::EncodingError) -> Result<T, jet_std::EncodingError> {
        let error = jet_jsonl_project_error(error, Some(self.record_index));
        self.terminal = Some(error.clone());
        Err(error)
    }

    fn skip_blank_records(&mut self) -> Result<(), jet_std::EncodingError> {
        loop {
            self.json.skip_ws()?;
            match self.json.fill()? {
                Some(b'\n') => {
                    self.json.take()?;
                }
                Some(b'\r') => {
                    self.json.take()?;
                    if self.json.fill()? != Some(b'\n') {
                        return Err(jet_encoding_error(
                            jet_std::EncodingErrorKind::Syntax,
                            self.json.offset - 1,
                            self.json.line,
                            self.json.column.saturating_sub(1),
                            "JSONL records use LF or CRLF; bare CR is not a record ending",
                        ));
                    }
                    self.json.take()?;
                }
                None => {
                    self.json.eof = true;
                    return Ok(());
                }
                Some(_) => return Ok(()),
            }
        }
    }

    fn limit_error(&self, heap: &mut JetJsonlHeapBudget, heap_limit: bool) -> jet_std::EncodingError {
        // Reserve the terminal path before constructing it. Four bytes per key
        // byte covers Rust Debug escaping; indices need at most 20 digits.
        let path_capacity = 1usize.saturating_add(self.json.frames.iter().map(|frame| match frame {
            JetJsonReadFrame::ArrayValueOrEnd { .. } | JetJsonReadFrame::ArrayCommaOrEnd { .. } => 22,
            JetJsonReadFrame::ObjectColonValue { key, .. } | JetJsonReadFrame::ObjectCommaOrEnd { key, .. } => key.len().saturating_mul(4).saturating_add(4),
            JetJsonReadFrame::ObjectKeyOrEnd { .. } => 0,
        }).sum::<usize>());
        // JSONL projection prefixes the record index into a replacement path;
        // reserve both live buffers before either allocation.
        let path_reservation = path_capacity.saturating_mul(2).saturating_add(24);
        let path = if heap.charge(path_reservation) { self.json.path() } else { String::new() };
        let mut error = jet_encoding_error(
            jet_std::EncodingErrorKind::Limit,
            self.json.offset,
            self.json.line,
            self.json.column,
            if heap_limit {
                format!("JSONL record heap exceeded the bounded allocator ceiling for max_item_bytes {}", self.json.limits.max_item_bytes)
            } else {
                format!("max_item_bytes {} exceeded", self.json.limits.max_item_bytes)
            },
        );
        error.path = path;
        error
    }

    fn push_value(
        &mut self,
        heap: &mut JetJsonlHeapBudget,
        root: &mut Option<jet_std::DataTree>,
        frames: &mut Vec<JetJsonlFoldFrame>,
        value: jet_std::DataTree,
    ) -> Result<(), jet_std::EncodingError> {
        match frames.last_mut() {
            Some(JetJsonlFoldFrame::Array(items)) => {
                if !jet_jsonl_reserve_push(heap, items) { return Err(self.limit_error(heap, true)); }
                items.push(value);
            }
            Some(JetJsonlFoldFrame::Object { entries, key }) => {
                let Some(key) = key.take() else {
                    return Err(jet_encoding_error(
                        jet_std::EncodingErrorKind::State,
                        self.json.offset,
                        self.json.line,
                        self.json.column,
                        "JSON event stream produced an object value without a key",
                    ));
                };
                if !jet_jsonl_reserve_push(heap, entries) { return Err(self.limit_error(heap, true)); }
                entries.push((key, value));
            }
            None if root.is_none() => *root = Some(value),
            None => {
                return Err(jet_encoding_error(
                    jet_std::EncodingErrorKind::State,
                    self.json.offset,
                    self.json.line,
                    self.json.column,
                    "JSON event stream produced two roots for one JSONL record",
                ));
            }
        }
        Ok(())
    }

    fn next_record(&mut self) -> Result<Option<jet_std::DataTree>, jet_std::EncodingError> {
        if let Some(error) = &self.terminal {
            return Err(error.clone());
        }
        if self.json.eof {
            return Ok(None);
        }
        if let Err(error) = self.skip_blank_records() {
            return self.fail(error);
        }
        if self.json.eof {
            return Ok(None);
        }

        let mut root = None;
        let mut frames = Vec::new();
        let allocation = JetJsonAllocationBudget::new(jet_encoding_codec_heap_ceiling(&self.json.limits));
        self.json.allocation_budget = Some(allocation.clone());
        let mut heap = JetJsonlHeapBudget {
            allocation,
            decoded: 0,
            decoded_limit: self.json.limits.max_item_bytes as usize,
        };
        loop {
            let event = match self.json.next_event() {
                Ok(event) => event,
                Err(error) => return self.fail(error),
            };
            let Some(event) = event else {
                let Some(value) = root else {
                    return self.fail(jet_encoding_error(
                        jet_std::EncodingErrorKind::Truncated,
                        self.json.offset,
                        self.json.line,
                        self.json.column,
                        "JSONL record ended before a complete value",
                    ));
                };
                if !frames.is_empty() {
                    return self.fail(jet_encoding_error(
                        jet_std::EncodingErrorKind::Truncated,
                        self.json.offset,
                        self.json.line,
                        self.json.column,
                        "JSONL record ended before its value was closed",
                    ));
                }
                self.record_index += 1;
                return Ok(Some(value));
            };
            let result = (|| -> Result<(), jet_std::EncodingError> { match event {
                jet_std::DataEvent::Null => {
                    if !heap.charge_decoded(1) { return Err(self.limit_error(&mut heap, false)); }
                    self.push_value(&mut heap, &mut root, &mut frames, jet_std::DataTree::Null)
                }
                jet_std::DataEvent::Bool(value) => {
                    if !heap.charge_decoded(1) { return Err(self.limit_error(&mut heap, false)); }
                    self.push_value(&mut heap, &mut root, &mut frames, jet_std::DataTree::Bool(value))
                }
                jet_std::DataEvent::Int(value) => {
                    if !heap.charge_decoded(8) { return Err(self.limit_error(&mut heap, false)); }
                    self.push_value(&mut heap, &mut root, &mut frames, jet_std::DataTree::Int(value))
                }
                jet_std::DataEvent::Float(value) => {
                    if !heap.charge_decoded(8) { return Err(self.limit_error(&mut heap, false)); }
                    self.push_value(&mut heap, &mut root, &mut frames, jet_std::DataTree::Float(value))
                }
                jet_std::DataEvent::Text(value) => {
                    if !heap.charge_decoded(value.len()) { return Err(self.limit_error(&mut heap, false)); }
                    self.push_value(&mut heap, &mut root, &mut frames, jet_std::DataTree::Text(value))
                }
                jet_std::DataEvent::Bytes(_) => Err(jet_encoding_error(
                    jet_std::EncodingErrorKind::State,
                    self.json.offset,
                    self.json.line,
                    self.json.column,
                    "JSON tokenizer produced Bytes",
                )),
                jet_std::DataEvent::ArrayStart => {
                    if !heap.charge_decoded(1) { return Err(self.limit_error(&mut heap, false)); }
                    if !jet_jsonl_reserve_push(&mut heap, &mut frames) {
                        return Err(self.limit_error(&mut heap, true));
                    }
                    frames.push(JetJsonlFoldFrame::Array(Vec::new()));
                    Ok(())
                }
                jet_std::DataEvent::ObjectStart => {
                    if !heap.charge_decoded(1) { return Err(self.limit_error(&mut heap, false)); }
                    if !jet_jsonl_reserve_push(&mut heap, &mut frames) {
                        return Err(self.limit_error(&mut heap, true));
                    }
                    frames.push(JetJsonlFoldFrame::Object { entries: Vec::new(), key: None });
                    Ok(())
                }
                jet_std::DataEvent::Key(value) => {
                    match frames.last_mut() {
                        Some(JetJsonlFoldFrame::Object { key, .. }) if key.is_none() => {
                            if !heap.charge_decoded(value.len()) { return Err(self.limit_error(&mut heap, false)); }
                            *key = Some(value);
                            Ok(())
                        }
                        _ => Err(jet_encoding_error(
                            jet_std::EncodingErrorKind::State,
                            self.json.offset,
                            self.json.line,
                            self.json.column,
                            "JSON event stream produced a key outside an object",
                        )),
                    }
                }
                jet_std::DataEvent::ArrayEnd => match frames.pop() {
                    Some(JetJsonlFoldFrame::Array(items)) => self.push_value(
                        &mut heap,
                        &mut root,
                        &mut frames,
                        jet_std::DataTree::Array(items),
                    ),
                    _ => Err(jet_encoding_error(
                        jet_std::EncodingErrorKind::State,
                        self.json.offset,
                        self.json.line,
                        self.json.column,
                        "JSON event stream closed the wrong container",
                    )),
                },
                jet_std::DataEvent::ObjectEnd => match frames.pop() {
                    Some(JetJsonlFoldFrame::Object { entries, key: None }) => self.push_value(
                        &mut heap,
                        &mut root,
                        &mut frames,
                        jet_std::DataTree::Object(entries),
                    ),
                    _ => Err(jet_encoding_error(
                        jet_std::EncodingErrorKind::State,
                        self.json.offset,
                        self.json.line,
                        self.json.column,
                        "JSON event stream closed an incomplete object",
                    )),
                },
            } })();
            if let Err(error) = result {
                return self.fail(error);
            }
        }
    }
}

fn jet_enc_jsonl_reader_next(
    reader: &mut jet_std::JSONLReader,
) -> Result<Option<jet_std::DataTree>, jet_std::EncodingError> {
    reader.next_record()
}

fn jet_enc_jsonl_writer(
    output: JetFileWriter,
    limits: jet_std::EncodingLimits,
) -> Result<jet_std::JSONLWriter, jet_std::EncodingError> {
    let json = jet_enc_json_writer(output, limits, false)
        .map_err(|error| jet_jsonl_project_error(error, None))?;
    Ok(jet_std::JSONLWriter {
        json,
        terminal: None,
        record_index: 0,
        finished: false,
        pending_lf: false,
    })
}

fn jet_jsonl_tree_size(
    value: &jet_std::DataTree,
    depth: i64,
    path: &str,
    limits: &jet_std::EncodingLimits,
) -> Result<usize, jet_std::EncodingError> {
    let is_container = matches!(
        value,
        jet_std::DataTree::Array(_) | jet_std::DataTree::Object(_)
    );
    if is_container && depth >= limits.max_depth {
        let mut error = jet_encoding_error(
            jet_std::EncodingErrorKind::Limit,
            0,
            1,
            1,
            format!("max_depth {} exceeded", limits.max_depth),
        );
        error.path = path.to_string();
        return Err(error);
    }
    let size = match value {
        jet_std::DataTree::Null | jet_std::DataTree::Bool(_) => 1,
        jet_std::DataTree::Int(_) => 8,
        jet_std::DataTree::Float(value) if value.is_finite() => 8,
        jet_std::DataTree::Float(_) => {
            let mut error = jet_encoding_error(
                jet_std::EncodingErrorKind::Unsupported,
                0,
                1,
                1,
                "JSON cannot encode a non-finite Float",
            );
            error.path = path.to_string();
            return Err(error);
        }
        jet_std::DataTree::Text(text) => text.len(),
        jet_std::DataTree::Bytes(_) => {
            let mut error = jet_encoding_error(
                jet_std::EncodingErrorKind::Unsupported,
                0,
                1,
                1,
                "JSON cannot encode Bytes; encode bytes as Text explicitly",
            );
            error.path = path.to_string();
            return Err(error);
        }
        jet_std::DataTree::Array(items) => {
            let mut size = 1usize;
            for (index, item) in items.iter().enumerate() {
                size = size.checked_add(jet_jsonl_tree_size(
                    item,
                    depth + 1,
                    &format!("{path}[{index}]"),
                    limits,
                )?).ok_or_else(|| jet_encoding_error(jet_std::EncodingErrorKind::Limit, 0, 1, 1, format!("max_item_bytes {} exceeded", limits.max_item_bytes)))?;
            }
            size
        }
        jet_std::DataTree::Object(entries) => {
            let mut size = 1usize;
            for (key, item) in entries {
                let child = jet_jsonl_tree_size(
                    item,
                    depth + 1,
                    &format!("{path}[{:?}]", key),
                    limits,
                )?;
                size = size
                    .checked_add(key.len())
                    .and_then(|n| n.checked_add(child))
                    .ok_or_else(|| jet_encoding_error(jet_std::EncodingErrorKind::Limit, 0, 1, 1, format!("max_item_bytes {} exceeded", limits.max_item_bytes)))?;
            }
            size
        }
    };
    if size > limits.max_item_bytes as usize {
        let mut error = jet_encoding_error(
            jet_std::EncodingErrorKind::Limit,
            0,
            1,
            1,
            format!("max_item_bytes {} exceeded", limits.max_item_bytes),
        );
        error.path = path.to_string();
        return Err(error);
    }
    Ok(size)
}

fn jet_jsonl_wire_size(
    writer: &jet_std::JSONWriter,
    value: &jet_std::DataTree,
) -> Result<usize, jet_std::EncodingError> {
    let checked_sum = |parts: &[usize]| {
        parts.iter().try_fold(0usize, |sum, part| sum.checked_add(*part))
            .ok_or_else(|| jet_encoding_error(jet_std::EncodingErrorKind::Limit, writer.total, 1, writer.total + 1, "JSONL record wire size overflow"))
    };
    match value {
        jet_std::DataTree::Null => Ok(4),
        jet_std::DataTree::Bool(true) => Ok(4),
        jet_std::DataTree::Bool(false) => Ok(5),
        jet_std::DataTree::Int(value) => Ok(value.to_string().len()),
        jet_std::DataTree::Float(value) => Ok(value.to_string().len()),
        jet_std::DataTree::Text(value) => checked_sum(&[writer.quoted_len(value)?, 2]),
        jet_std::DataTree::Bytes(_) => Err(jet_encoding_error(
            jet_std::EncodingErrorKind::Unsupported,
            writer.total,
            1,
            writer.total + 1,
            "JSON cannot encode Bytes; encode bytes as Text explicitly",
        )),
        jet_std::DataTree::Array(items) => {
            let mut size = 2usize;
            for (index, item) in items.iter().enumerate() {
                size = checked_sum(&[size, usize::from(index > 0), jet_jsonl_wire_size(writer, item)?])?;
            }
            Ok(size)
        }
        jet_std::DataTree::Object(entries) => {
            let mut size = 2usize;
            for (index, (key, item)) in entries.iter().enumerate() {
                size = checked_sum(&[
                    size,
                    usize::from(index > 0),
                    writer.quoted_len(key)?,
                    3,
                    jet_jsonl_wire_size(writer, item)?,
                ])?;
            }
            Ok(size)
        }
    }
}

fn jet_jsonl_write_tree(
    writer: &mut jet_std::JSONWriter,
    value: &jet_std::DataTree,
) -> Result<(), jet_std::EncodingError> {
    match value {
        jet_std::DataTree::Null => writer.write_event(jet_std::DataEvent::Null),
        jet_std::DataTree::Bool(value) => writer.write_event(jet_std::DataEvent::Bool(*value)),
        jet_std::DataTree::Int(value) => writer.write_event(jet_std::DataEvent::Int(*value)),
        jet_std::DataTree::Float(value) => writer.write_event(jet_std::DataEvent::Float(*value)),
        jet_std::DataTree::Text(value) => writer.write_event(jet_std::DataEvent::Text(value.clone())),
        jet_std::DataTree::Bytes(value) => writer.write_event(jet_std::DataEvent::Bytes(value.clone())),
        jet_std::DataTree::Array(items) => {
            writer.write_event(jet_std::DataEvent::ArrayStart)?;
            for item in items {
                jet_jsonl_write_tree(writer, item)?;
            }
            writer.write_event(jet_std::DataEvent::ArrayEnd)
        }
        jet_std::DataTree::Object(entries) => {
            writer.write_event(jet_std::DataEvent::ObjectStart)?;
            for (key, value) in entries {
                writer.write_event(jet_std::DataEvent::Key(key.clone()))?;
                jet_jsonl_write_tree(writer, value)?;
            }
            writer.write_event(jet_std::DataEvent::ObjectEnd)
        }
    }
}

impl jet_std::JSONLWriter {
    fn fail<T>(&mut self, error: jet_std::EncodingError) -> Result<T, jet_std::EncodingError> {
        let error = jet_jsonl_project_error(error, Some(self.record_index));
        self.terminal = Some(error.clone());
        Err(error)
    }

    fn write_record(&mut self, value: jet_std::DataTree) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal {
            return Err(error.clone());
        }
        if self.finished {
            return self.fail(self.json.state_error("write called after finish"));
        }
        if self.pending_lf {
            // Prior record's LF: next write closes the previous line on the wire.
            if let Err(error) = self.json.ensure_total(1) {
                return self.fail(error);
            }
            if let Err(error) = self.json.write_bytes(b"\n") {
                return self.fail(error);
            }
            self.pending_lf = false;
        }
        if let Err(mut error) = jet_jsonl_tree_size(&value, 0, "$", &self.json.limits) {
            error.byte_offset = self.json.total;
            error.line = Some(self.record_index + 1);
            error.column = Some(1);
            return self.fail(error);
        }
        // Value bytes now; record LF is deferred to next write or finish so
        // Drop-without-finish leaves an incomplete line (≠ finished wire).
        let wire = match jet_jsonl_wire_size(&self.json, &value) {
            Ok(wire) => wire,
            Err(error) => return self.fail(error),
        };
        let reserved = match wire.checked_add(1) {
            Some(n) => n,
            None => {
                return self.fail(jet_encoding_error(
                    jet_std::EncodingErrorKind::Limit,
                    self.json.total,
                    self.record_index + 1,
                    1,
                    "JSONL record wire size overflow",
                ));
            }
        };
        if let Err(error) = self.json.ensure_total(reserved) {
            return self.fail(error);
        }
        if let Err(error) = jet_jsonl_write_tree(&mut self.json, &value) {
            return self.fail(error);
        }
        if !self.json.frames.is_empty() {
            return self.fail(self.json.state_error("JSONL record writer left an open container"));
        }
        self.json.root_written = false;
        self.pending_lf = true;
        self.record_index += 1;
        Ok(())
    }

    fn flush_output(&mut self) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal {
            return Err(error.clone());
        }
        if let Err(error) = self.json.flush_output() {
            return self.fail(error);
        }
        Ok(())
    }

    fn finish_output(&mut self) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal {
            return Err(error.clone());
        }
        if self.finished {
            return Ok(());
        }
        if self.pending_lf {
            if let Err(error) = self.json.ensure_total(1) {
                return self.fail(error);
            }
            if let Err(error) = self.json.write_bytes(b"\n") {
                return self.fail(error);
            }
            self.pending_lf = false;
        }
        self.flush_output()?;
        self.finished = true;
        Ok(())
    }
}

fn jet_enc_jsonl_writer_write(
    writer: &mut jet_std::JSONLWriter,
    value: jet_std::DataTree,
) -> Result<(), jet_std::EncodingError> {
    writer.write_record(value)
}
fn jet_enc_jsonl_writer_flush(writer: &mut jet_std::JSONLWriter) -> Result<(), jet_std::EncodingError> {
    writer.flush_output()
}
fn jet_enc_jsonl_writer_finish(writer: &mut jet_std::JSONLWriter) -> Result<(), jet_std::EncodingError> {
    writer.finish_output()
}

fn jet_csv_error(
    kind: jet_std::EncodingErrorKind,
    offset: i64,
    line: i64,
    column: i64,
    path: String,
    reason: impl Into<String>,
) -> jet_std::EncodingError {
    jet_std::EncodingError {
        format: jet_std::EncodingFormat::CSV,
        kind,
        byte_offset: offset,
        line: Some(line),
        column: Some(column),
        path,
        reason: reason.into(),
        cause: None,
    }
}

fn jet_csv_io_error(error: std::io::Error, offset: i64, line: i64, column: i64, path: String) -> jet_std::EncodingError {
    let mut out = jet_csv_error(jet_std::EncodingErrorKind::IO, offset, line, column, path, "file IO failed");
    out.cause = Some(jet_std::EncodingCause {
        kind: format!("{:?}", error.kind()),
        os_code: error.raw_os_error().map(i64::from),
        message: error.to_string(),
    });
    out
}

fn jet_csv_path(record: i64, field: usize) -> String { format!("$[{record}][{field}]") }

fn jet_enc_csv_reader(input: JetFileReader, limits: jet_std::EncodingLimits) -> Result<jet_std::CSVReader, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits).map_err(|mut error| { error.format = jet_std::EncodingFormat::CSV; error })?;
    Ok(jet_std::CSVReader { input, limits, total: 0, offset: 0, line: 1, column: 1, terminal: None, eof: false, record_index: 0 })
}

impl jet_std::CSVReader {
    fn fail<T>(&mut self, error: jet_std::EncodingError) -> Result<T, jet_std::EncodingError> {
        self.terminal = Some(error.clone());
        Err(error)
    }

    fn byte(&mut self, field: usize) -> Result<Option<u8>, jet_std::EncodingError> {
        if self.limits.max_total_bytes.is_some_and(|limit| self.total >= limit) {
            return Err(jet_csv_error(jet_std::EncodingErrorKind::Limit, self.offset, self.line, self.column,
                jet_csv_path(self.record_index, field), format!("max_total_bytes {} exceeded", self.limits.max_total_bytes.unwrap())));
        }
        let mut one = [0u8; 1];
        match std::io::Read::read(&mut self.input.inner, &mut one) {
            Ok(0) => Ok(None),
            Ok(_) => {
                self.total += 1;
                self.offset += 1;
                if one[0] == b'\n' { self.line += 1; self.column = 1; } else { self.column += 1; }
                Ok(Some(one[0]))
            }
            Err(error) => Err(jet_csv_io_error(error, self.offset, self.line, self.column, jet_csv_path(self.record_index, field))),
        }
    }

    fn next_record(&mut self) -> Result<Option<Vec<String>>, jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        if self.eof { return Ok(None); }
        let budget = JetJsonAllocationBudget::new(jet_encoding_codec_heap_ceiling(&self.limits));
        let mut row: Vec<String> = Vec::new();
        let mut field: Vec<u8> = Vec::new();
        let mut decoded = 0usize;
        let mut quoted = false;
        let mut after_quote = false;
        let mut saw_any = false;
        loop {
            let field_index = row.len();
            let byte = match self.byte(field_index) { Ok(v) => v, Err(e) => return self.fail(e) };
            match byte {
                None if !saw_any && row.is_empty() && field.is_empty() => { self.eof = true; return Ok(None); }
                None if quoted && !after_quote => return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Truncated,
                    self.offset, self.line, self.column, jet_csv_path(self.record_index, field_index), "quoted CSV field ended before its closing quote")),
                None => {
                    self.eof = true;
                    if let Err(e) = jet_csv_finish_field(&budget, &mut row, &mut field, &mut decoded, &self.limits, self.record_index, self.offset, self.line, self.column) { return self.fail(e); }
                    self.record_index += 1;
                    return Ok(Some(row));
                }
                Some(b) => {
                    saw_any = true;
                    if quoted {
                        if after_quote {
                            if b == b'"' { after_quote = false; if let Err(e) = jet_csv_push_byte(&budget, &mut field, b'"', &mut decoded, &self.limits, self.record_index, field_index, self.offset, self.line, self.column) { return self.fail(e); } }
                            else if b == b',' { quoted = false; after_quote = false; if let Err(e) = jet_csv_finish_field(&budget, &mut row, &mut field, &mut decoded, &self.limits, self.record_index, self.offset - 1, self.line, self.column.saturating_sub(1)) { return self.fail(e); } }
                            else if b == b'\r' { quoted = false; after_quote = false; match self.byte(field_index) { Ok(Some(b'\n')) => {}, Ok(None) => return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, jet_csv_path(self.record_index, field_index), "CSV CR record ending is missing LF")), Ok(Some(_)) => return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), jet_csv_path(self.record_index, field_index), "CSV records use CRLF; bare CR is not a record ending")), Err(e) => return self.fail(e) }; if let Err(e) = jet_csv_finish_field(&budget, &mut row, &mut field, &mut decoded, &self.limits, self.record_index, self.offset - 1, self.line.saturating_sub(1), 1) { return self.fail(e); } self.record_index += 1; return Ok(Some(row)); }
                            else { return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), jet_csv_path(self.record_index, field_index), "only quote, comma, CRLF, or EOF may follow a closing CSV quote")); }
                        } else if b == b'"' { after_quote = true; }
                        else if let Err(e) = jet_csv_push_byte(&budget, &mut field, b, &mut decoded, &self.limits, self.record_index, field_index, self.offset, self.line, self.column) { return self.fail(e); }
                    } else if b == b'"' {
                        if field.is_empty() { quoted = true; } else { return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), jet_csv_path(self.record_index, field_index), "quote inside an unquoted CSV field")); }
                    } else if b == b',' {
                        if let Err(e) = jet_csv_finish_field(&budget, &mut row, &mut field, &mut decoded, &self.limits, self.record_index, self.offset - 1, self.line, self.column.saturating_sub(1)) { return self.fail(e); }
                    } else if b == b'\r' {
                        match self.byte(field_index) { Ok(Some(b'\n')) => {}, Ok(None) => return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, jet_csv_path(self.record_index, field_index), "CSV CR record ending is missing LF")), Ok(Some(_)) => return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), jet_csv_path(self.record_index, field_index), "CSV records use CRLF; bare CR is not a record ending")), Err(e) => return self.fail(e) }
                        if let Err(e) = jet_csv_finish_field(&budget, &mut row, &mut field, &mut decoded, &self.limits, self.record_index, self.offset - 1, self.line.saturating_sub(1), 1) { return self.fail(e); }
                        self.record_index += 1;
                        return Ok(Some(row));
                    } else if b == b'\n' { return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line.saturating_sub(1), 1, jet_csv_path(self.record_index, field_index), "CSV records use CRLF; bare LF is not a record ending")); }
                    else if let Err(e) = jet_csv_push_byte(&budget, &mut field, b, &mut decoded, &self.limits, self.record_index, field_index, self.offset, self.line, self.column) { return self.fail(e); }
                }
            }
        }
    }
}

fn jet_csv_push_byte(budget: &JetJsonAllocationBudget, field: &mut Vec<u8>, byte: u8, decoded: &mut usize, limits: &jet_std::EncodingLimits, record: i64, index: usize, offset: i64, line: i64, column: i64) -> Result<(), jet_std::EncodingError> {
    if *decoded >= limits.max_item_bytes as usize { return Err(jet_csv_error(jet_std::EncodingErrorKind::Limit, offset, line, column, jet_csv_path(record, index), format!("max_item_bytes {} exceeded", limits.max_item_bytes))); }
    if field.len() == field.capacity() {
        let old = field.capacity();
        let next = if old == 0 { 8 } else { old.saturating_mul(2) };
        if !budget.charge(next.saturating_sub(old)) { return Err(jet_csv_error(jet_std::EncodingErrorKind::Limit, offset, line, column, jet_csv_path(record, index), "CSV record heap exceeded the bounded codec heap ceiling")); }
        field.try_reserve_exact(next.saturating_sub(old)).map_err(|_| jet_csv_error(jet_std::EncodingErrorKind::Limit, offset, line, column, jet_csv_path(record, index), "CSV record allocation failed"))?;
    }
    field.push(byte); *decoded += 1; Ok(())
}

fn jet_csv_finish_field(budget: &JetJsonAllocationBudget, row: &mut Vec<String>, field: &mut Vec<u8>, _decoded: &mut usize, _limits: &jet_std::EncodingLimits, record: i64, offset: i64, line: i64, column: i64) -> Result<(), jet_std::EncodingError> {
    let index = row.len();
    if row.len() == row.capacity() {
        let old = row.capacity(); let next = if old == 0 { 4 } else { old.saturating_mul(2) };
        let bytes = next.saturating_sub(old).saturating_mul(std::mem::size_of::<String>());
        if !budget.charge(bytes) { return Err(jet_csv_error(jet_std::EncodingErrorKind::Limit, offset, line, column, jet_csv_path(record, index), "CSV record heap exceeded the bounded codec heap ceiling")); }
        row.try_reserve_exact(next.saturating_sub(old)).map_err(|_| jet_csv_error(jet_std::EncodingErrorKind::Limit, offset, line, column, jet_csv_path(record, index), "CSV record allocation failed"))?;
    }
    let bytes = std::mem::take(field);
    row.push(String::from_utf8(bytes).map_err(|_| jet_csv_error(jet_std::EncodingErrorKind::Syntax, offset, line, column, jet_csv_path(record, index), "CSV field is not valid UTF-8"))?);
    Ok(())
}

fn jet_enc_csv_reader_next(reader: &mut jet_std::CSVReader) -> Result<Option<Vec<String>>, jet_std::EncodingError> { reader.next_record() }

fn jet_enc_csv_writer(output: JetFileWriter, limits: jet_std::EncodingLimits) -> Result<jet_std::CSVWriter, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits).map_err(|mut error| { error.format = jet_std::EncodingFormat::CSV; error })?;
    Ok(jet_std::CSVWriter { output, limits, terminal: None, total: 0, record_index: 0, finished: false, pending_crlf: false })
}

impl jet_std::CSVWriter {
    fn fail<T>(&mut self, error: jet_std::EncodingError) -> Result<T, jet_std::EncodingError> { self.terminal = Some(error.clone()); Err(error) }
    fn write_record(&mut self, row: Vec<String>) -> Result<(), jet_std::EncodingError> {
        if let Some(e) = &self.terminal { return Err(e.clone()); }
        if self.finished { return self.fail(jet_csv_error(jet_std::EncodingErrorKind::State, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, 0), "write called after finish")); }
        if self.pending_crlf {
            // Prior record's CRLF: next write closes the previous row on the wire.
            if self.limits.max_total_bytes.is_some_and(|limit| self.total.saturating_add(2) > limit) {
                return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Limit, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, 0), format!("max_total_bytes {} exceeded", self.limits.max_total_bytes.unwrap())));
            }
            if let Err(e) = std::io::Write::write_all(&mut self.output.inner, b"\r\n") {
                return self.fail(jet_csv_io_error(e, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, 0)));
            }
            self.total += 2;
            self.pending_crlf = false;
        }
        let mut decoded = 0usize; let mut wire = 0usize;
        for (i, field) in row.iter().enumerate() {
            decoded = decoded.checked_add(field.len()).ok_or_else(|| jet_csv_error(jet_std::EncodingErrorKind::Limit, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, i), "CSV record size overflow"))?;
            if decoded > self.limits.max_item_bytes as usize { return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Limit, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, i), format!("max_item_bytes {} exceeded", self.limits.max_item_bytes))); }
            let quoted = field.bytes().any(|b| matches!(b, b',' | b'"' | b'\r' | b'\n'));
            let size = field.len().saturating_add(if quoted { 2 + field.bytes().filter(|b| *b == b'"').count() } else { 0 }).saturating_add(usize::from(i > 0));
            wire = wire.checked_add(size).ok_or_else(|| jet_csv_error(jet_std::EncodingErrorKind::Limit, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, i), "CSV wire size overflow"))?;
        }
        // Prospectively reserve the deferred CRLF so Drop-without-finish cannot sneak past max_total.
        if self.limits.max_total_bytes.is_some_and(|limit| self.total.saturating_add(wire as i64).saturating_add(2) > limit) {
            return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Limit, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, row.len().saturating_sub(1)), format!("max_total_bytes {} exceeded", self.limits.max_total_bytes.unwrap())));
        }
        let mut write = |bytes: &[u8]| std::io::Write::write_all(&mut self.output.inner, bytes);
        let result = (|| -> std::io::Result<()> {
            for (i, field) in row.iter().enumerate() {
                if i > 0 { write(b",")?; }
                let quoted = field.bytes().any(|b| matches!(b, b',' | b'"' | b'\r' | b'\n'));
                if quoted {
                    write(b"\"")?;
                    for byte in field.as_bytes() {
                        if *byte == b'"' { write(b"\"\"")?; } else { write(std::slice::from_ref(byte))?; }
                    }
                    write(b"\"")?;
                } else {
                    write(field.as_bytes())?;
                }
            }
            Ok(())
        })();
        if let Err(e) = result { return self.fail(jet_csv_io_error(e, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, 0))); }
        self.total += wire as i64;
        self.pending_crlf = true;
        self.record_index += 1;
        Ok(())
    }
    fn flush_output(&mut self) -> Result<(), jet_std::EncodingError> { if let Some(e) = &self.terminal { return Err(e.clone()); } if let Err(e) = std::io::Write::flush(&mut self.output.inner) { return self.fail(jet_csv_io_error(e, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, 0))); } Ok(()) }
    fn finish_output(&mut self) -> Result<(), jet_std::EncodingError> {
        if let Some(e) = &self.terminal { return Err(e.clone()); }
        if self.finished { return Ok(()); }
        if self.pending_crlf {
            if self.limits.max_total_bytes.is_some_and(|limit| self.total.saturating_add(2) > limit) {
                return self.fail(jet_csv_error(jet_std::EncodingErrorKind::Limit, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, 0), format!("max_total_bytes {} exceeded", self.limits.max_total_bytes.unwrap())));
            }
            if let Err(e) = std::io::Write::write_all(&mut self.output.inner, b"\r\n") {
                return self.fail(jet_csv_io_error(e, self.total, self.record_index + 1, 1, jet_csv_path(self.record_index, 0)));
            }
            self.total += 2;
            self.pending_crlf = false;
        }
        self.flush_output()?;
        self.finished = true;
        Ok(())
    }
}

fn jet_enc_csv_writer_write(writer: &mut jet_std::CSVWriter, row: Vec<String>) -> Result<(), jet_std::EncodingError> { writer.write_record(row) }
fn jet_enc_csv_writer_flush(writer: &mut jet_std::CSVWriter) -> Result<(), jet_std::EncodingError> { writer.flush_output() }
fn jet_enc_csv_writer_finish(writer: &mut jet_std::CSVWriter) -> Result<(), jet_std::EncodingError> { writer.finish_output() }

enum JetCborReadFrame {
    Array { remaining: Option<u64>, index: u64 },
    Object { remaining: Option<u64>, expecting_key: bool, key: Option<usize>, keys: Vec<String> },
}

enum JetCborWriteFrame {
    Array { items: Vec<Vec<u8>> },
    Object { entries: Vec<(Vec<u8>, Vec<u8>)>, key: Option<Vec<u8>> },
}

fn jet_cbor_stream_error(kind: jet_std::EncodingErrorKind, offset: i64, path: String, reason: impl Into<String>) -> jet_std::EncodingError {
    jet_std::EncodingError { format: jet_std::EncodingFormat::CBOR, kind, byte_offset: offset, line: None, column: None, path, reason: reason.into(), cause: None }
}

fn jet_cbor_stream_io(error: std::io::Error, offset: i64, path: String) -> jet_std::EncodingError {
    let mut out = jet_cbor_stream_error(jet_std::EncodingErrorKind::IO, offset, path, "file IO failed");
    out.cause = Some(jet_std::EncodingCause { kind: format!("{:?}", error.kind()), os_code: error.raw_os_error().map(i64::from), message: error.to_string() });
    out
}

// Live Vec/String backing and frame tables share the D-ENCSTREAM-SURFACE1
// codec heap ceiling via JetJsonAllocationBudget (counting allocator parity).
fn jet_cbor_heap_error(offset: i64, path: String) -> jet_std::EncodingError {
    jet_cbor_stream_error(
        jet_std::EncodingErrorKind::Limit,
        offset,
        path,
        "CBOR stream heap exceeded the bounded codec heap ceiling",
    )
}

fn jet_cbor_charge(
    budget: &JetJsonAllocationBudget,
    bytes: usize,
    offset: i64,
    path: String,
) -> Result<(), jet_std::EncodingError> {
    if bytes == 0 {
        return Ok(());
    }
    if !budget.charge(bytes) {
        return Err(jet_cbor_heap_error(offset, path));
    }
    Ok(())
}

fn jet_cbor_ensure_fit(
    budget: &JetJsonAllocationBudget,
    bytes: usize,
    offset: i64,
    path: String,
) -> Result<(), jet_std::EncodingError> {
    if bytes == 0 || budget.would_fit(bytes) {
        return Ok(());
    }
    Err(jet_cbor_heap_error(offset, path))
}

fn jet_xml_stream_error(error: crate::jet_xml_pull::Error) -> jet_std::EncodingError {
    use crate::jet_xml_pull::Reason;
    let kind = match error.kind {
        Reason::EntityCycle | Reason::Limit => jet_std::EncodingErrorKind::Limit,
        Reason::Canonicalization | Reason::Unsupported => jet_std::EncodingErrorKind::Unsupported,
        Reason::Malformed
            if error.reason.contains("ended")
                || error.reason.contains("unterminated")
                || error.reason.contains("truncated") =>
        {
            jet_std::EncodingErrorKind::Truncated
        }
        _ => jet_std::EncodingErrorKind::Syntax,
    };
    jet_std::EncodingError {
        format: jet_std::EncodingFormat::XML,
        kind,
        byte_offset: error.offset as i64,
        line: error.line.map(|value| value as i64),
        column: error.column.map(|value| value as i64),
        path: error.path,
        reason: error.reason,
        cause: None,
    }
}
fn jet_xml_io_error(offset: i64, error: std::io::Error) -> jet_std::EncodingError {
    jet_std::EncodingError {
        format: jet_std::EncodingFormat::XML,
        kind: jet_std::EncodingErrorKind::IO,
        byte_offset: offset,
        line: None,
        column: None,
        path: String::new(),
        reason: "file IO failed".to_string(),
        cause: Some(jet_std::EncodingCause {
            kind: format!("{:?}", error.kind()),
            os_code: error.raw_os_error().map(i64::from),
            message: error.to_string(),
        }),
    }
}
fn jet_enc_xml_reader(
    input: JetFileReader,
    limits: jet_std::EncodingLimits,
    xml: jet_std::XMLParseOptions,
) -> Result<jet_std::XMLReader, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits).map_err(|mut error| {
        error.format = jet_std::EncodingFormat::XML;
        error.line = None;
        error.column = None;
        error
    })?;
    let mut options = jet_xml_options(&xml);
    options.limits.validate().map_err(jet_xml_stream_error)?;
    options.limits.max_depth = options.limits.max_depth.min(limits.max_depth as usize);
    options.limits.max_entity_depth = options
        .limits
        .max_entity_depth
        .min(limits.max_expansion_depth as usize)
        .min(options.limits.max_depth);
    options.limits.max_entity_replacement_bytes = options
        .limits
        .max_entity_replacement_bytes
        .min(limits.max_expansion_bytes as usize)
        .min(options.limits.max_text_bytes);
    let scanner = crate::jet_xml_pull::StreamScanner::new(
        limits.max_item_bytes as usize,
        options,
    )
    .map_err(jet_xml_stream_error)?;
    let allocation = JetJsonAllocationBudget::new(jet_encoding_codec_heap_ceiling(&limits));
    // Scratch read buffer is codec-owned for the reader lifetime.
    if !allocation.charge(limits.buffer_bytes as usize) {
        return Err(jet_std::EncodingError {
            format: jet_std::EncodingFormat::XML,
            kind: jet_std::EncodingErrorKind::Limit,
            byte_offset: 0,
            line: None,
            column: None,
            path: "$".to_string(),
            reason: "XML event heap exceeded the bounded codec heap ceiling".to_string(),
            cause: None,
        });
    }
    Ok(jet_std::XMLReader {
        input,
        limits,
        scanner,
        terminal: None,
        total: 0,
        eof: false,
        allocation,
    })
}

fn jet_xml_value_heap_cost(value: &crate::jet_xml_pull::Value) -> usize {
    match value {
        crate::jet_xml_pull::Value::Null
        | crate::jet_xml_pull::Value::Bool(_)
        | crate::jet_xml_pull::Value::Int(_) => 0,
        crate::jet_xml_pull::Value::Text(text) => text.len(),
        crate::jet_xml_pull::Value::Array(items) => items
            .len()
            .saturating_mul(std::mem::size_of::<jet_std::DataTree>())
            .saturating_add(items.iter().map(jet_xml_value_heap_cost).fold(0usize, usize::saturating_add)),
        crate::jet_xml_pull::Value::Object(entries) => entries
            .len()
            .saturating_mul(std::mem::size_of::<(String, jet_std::DataTree)>())
            .saturating_add(
                entries
                    .iter()
                    .map(|(key, value)| key.len().saturating_add(jet_xml_value_heap_cost(value)))
                    .fold(0usize, usize::saturating_add),
            ),
    }
}

fn jet_xml_heap_error(offset: i64) -> jet_std::EncodingError {
    jet_std::EncodingError {
        format: jet_std::EncodingFormat::XML,
        kind: jet_std::EncodingErrorKind::Limit,
        byte_offset: offset,
        line: None,
        column: None,
        path: "$".to_string(),
        reason: "XML event heap exceeded the bounded codec heap ceiling".to_string(),
        cause: None,
    }
}

fn jet_enc_xml_reader_next(
    reader: &mut jet_std::XMLReader,
) -> Result<Option<jet_std::DataTree>, jet_std::EncodingError> {
    if let Some(error) = &reader.terminal {
        return Err(error.clone());
    }
    loop {
        match reader.scanner.next() {
            Ok(Some(event)) => {
                // Lexical raw_bytes / BOM project to Array<Int> DataTree slots.
                // Charge that projection before building so a hostile token fails
                // closed without retaining the projected tree.
                let slot = std::mem::size_of::<jet_std::DataTree>();
                let prospective = event
                    .raw_bytes
                    .len()
                    .saturating_mul(slot)
                    .saturating_add(event.bom.len().saturating_mul(slot));
                if !reader.allocation.charge(prospective) {
                    let error = jet_xml_heap_error(reader.total);
                    reader.terminal = Some(error.clone());
                    return Err(error);
                }
                let value = crate::jet_xml_pull::stream_event_value(event);
                let full = jet_xml_value_heap_cost(&value);
                let extra = full.saturating_sub(prospective);
                if extra > 0 && !reader.allocation.charge(extra) {
                    reader.allocation.release(prospective);
                    let error = jet_xml_heap_error(reader.total);
                    reader.terminal = Some(error.clone());
                    return Err(error);
                }
                let tree = jet_xml_to_data_tree(value);
                // Returned DataTree leaves codec ownership (D-ENCSTREAM-SURFACE1).
                reader.allocation.release(prospective.saturating_add(extra));
                return Ok(Some(tree));
            }
            Ok(None) if reader.eof => return Ok(None),
            Ok(None) => {}
            Err(error) => {
                let error = jet_xml_stream_error(error);
                reader.terminal = Some(error.clone());
                return Err(error);
            }
        }
        let read_bytes = match reader.limits.max_total_bytes {
            Some(maximum) => reader
                .limits
                .buffer_bytes
                .min(maximum.saturating_sub(reader.total).saturating_add(1)),
            None => reader.limits.buffer_bytes,
        };
        let mut bytes = vec![0u8; read_bytes as usize];
        let count = match std::io::Read::read(&mut reader.input.inner, &mut bytes) {
            Ok(count) => count,
            Err(error) => {
                let error = jet_xml_io_error(reader.total, error);
                reader.terminal = Some(error.clone());
                return Err(error);
            }
        };
        bytes.truncate(count);
        if count == 0 {
            reader.eof = true;
            if let Err(error) = reader.scanner.finish_input() {
                let error = jet_xml_stream_error(error);
                reader.terminal = Some(error.clone());
                return Err(error);
            }
        } else {
            reader.total = reader.total.saturating_add(count as i64);
            if let Some(maximum) = reader.limits.max_total_bytes {
                if reader.total > maximum {
                    let error = jet_std::EncodingError {
                        format: jet_std::EncodingFormat::XML,
                        kind: jet_std::EncodingErrorKind::Limit,
                        byte_offset: reader.total,
                        line: None,
                        column: None,
                        path: String::new(),
                        reason: format!("max_total_bytes {maximum} exceeded"),
                        cause: None,
                    };
                    reader.terminal = Some(error.clone());
                    return Err(error);
                }
            }
            if let Err(error) = reader.scanner.push(&bytes) {
                let error = jet_xml_stream_error(error);
                reader.terminal = Some(error.clone());
                return Err(error);
            }
        }
    }
}

fn jet_xml_render_encoding(value: &jet_std::XMLEncoding) -> crate::jet_xml_pull::RenderEncoding {
    match value {
        jet_std::XMLEncoding::UTF8 => crate::jet_xml_pull::RenderEncoding::Utf8,
        jet_std::XMLEncoding::UTF8BOM => crate::jet_xml_pull::RenderEncoding::Utf8Bom,
        jet_std::XMLEncoding::UTF16LE => crate::jet_xml_pull::RenderEncoding::Utf16Le,
        jet_std::XMLEncoding::UTF16BE => crate::jet_xml_pull::RenderEncoding::Utf16Be,
    }
}

fn jet_xml_lexical_policy(value: &jet_std::XMLLexicalPolicy) -> crate::jet_xml_pull::LexicalPolicy {
    match value {
        jet_std::XMLLexicalPolicy::PreserveValid => crate::jet_xml_pull::LexicalPolicy::PreserveValid,
        jet_std::XMLLexicalPolicy::Deterministic => crate::jet_xml_pull::LexicalPolicy::Deterministic,
    }
}

fn jet_enc_xml_writer(
    output: JetFileWriter,
    limits: jet_std::EncodingLimits,
    xml: jet_std::XMLRenderOptions,
) -> Result<jet_std::XMLWriter, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits).map_err(|mut error| {
        error.format = jet_std::EncodingFormat::XML;
        error.line = None;
        error.column = None;
        error
    })?;
    let renderer = crate::jet_xml_pull::StreamWriter::new(
        jet_xml_render_encoding(&xml.encoding),
        jet_xml_lexical_policy(&xml.lexical),
    );
    Ok(jet_std::XMLWriter {
        output,
        limits,
        renderer,
        buffer: Vec::new(),
        terminal: None,
        total: 0,
        finished: false,
    })
}

impl jet_std::XMLWriter {
    fn fail<T>(&mut self, error: jet_std::EncodingError) -> Result<T, jet_std::EncodingError> {
        self.terminal = Some(error.clone());
        Err(error)
    }

    fn io_error(&self, error: std::io::Error) -> jet_std::EncodingError {
        jet_xml_io_error(self.total, error)
    }

    fn flush_buffer(&mut self) -> Result<(), jet_std::EncodingError> {
        if self.buffer.is_empty() { return Ok(()); }
        if let Err(error) = std::io::Write::write_all(&mut self.output.inner, &self.buffer) {
            let error = self.io_error(error);
            return self.fail(error);
        }
        self.buffer.clear();
        Ok(())
    }

    fn write_event(&mut self, event: jet_std::DataTree) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        if self.finished {
            return self.fail(jet_std::EncodingError { format: jet_std::EncodingFormat::XML, kind: jet_std::EncodingErrorKind::State, byte_offset: self.total, line: None, column: None, path: String::new(), reason: "write called after finish".to_string(), cause: None });
        }
        let value = match jet_xml_from_data_tree(&event) {
            Ok(value) => value,
            Err(reason) => {
                let error = jet_std::EncodingError { format: jet_std::EncodingFormat::XML, kind: jet_std::EncodingErrorKind::Syntax, byte_offset: self.total, line: None, column: None, path: String::new(), reason, cause: None };
                return self.fail(error);
            }
        };
        let bytes = match self.renderer.write(&value) {
            Ok(bytes) => bytes,
            Err(error) => {
                let mut error = jet_xml_stream_error(error);
                error.byte_offset = self.total;
                if let Some(reason) = error.reason.strip_prefix("[state] ") {
                    error.kind = jet_std::EncodingErrorKind::State;
                    error.reason = reason.to_string();
                }
                return self.fail(error);
            }
        };
        if bytes.len() > self.limits.max_item_bytes as usize {
            return self.fail(jet_std::EncodingError { format: jet_std::EncodingFormat::XML, kind: jet_std::EncodingErrorKind::Limit, byte_offset: self.total, line: None, column: None, path: String::new(), reason: format!("max_item_bytes {} exceeded", self.limits.max_item_bytes), cause: None });
        }
        let next_total = self.total.saturating_add(bytes.len() as i64);
        if self.limits.max_total_bytes.is_some_and(|maximum| next_total > maximum) {
            return self.fail(jet_std::EncodingError { format: jet_std::EncodingFormat::XML, kind: jet_std::EncodingErrorKind::Limit, byte_offset: self.total, line: None, column: None, path: String::new(), reason: format!("max_total_bytes {} exceeded", self.limits.max_total_bytes.unwrap_or(0)), cause: None });
        }
        let capacity = self.limits.buffer_bytes as usize;
        if self.buffer.len().saturating_add(bytes.len()) > capacity { self.flush_buffer()?; }
        if bytes.len() > capacity {
            if let Err(error) = std::io::Write::write_all(&mut self.output.inner, &bytes) {
                let error = self.io_error(error);
                return self.fail(error);
            }
        } else {
            self.buffer.extend_from_slice(&bytes);
        }
        self.total = next_total;
        Ok(())
    }

    fn flush_output(&mut self) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        self.flush_buffer()?;
        if let Err(error) = std::io::Write::flush(&mut self.output.inner) {
            let error = self.io_error(error);
            return self.fail(error);
        }
        Ok(())
    }

    fn finish_output(&mut self) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        if self.finished { return Ok(()); }
        if !self.renderer.is_finished() {
            return self.fail(jet_std::EncodingError { format: jet_std::EncodingFormat::XML, kind: jet_std::EncodingErrorKind::State, byte_offset: self.total, line: None, column: None, path: String::new(), reason: "finish requires document_end".to_string(), cause: None });
        }
        self.flush_output()?;
        self.finished = true;
        Ok(())
    }
}

fn jet_enc_xml_writer_write(writer: &mut jet_std::XMLWriter, event: jet_std::DataTree) -> Result<(), jet_std::EncodingError> { writer.write_event(event) }
fn jet_enc_xml_writer_flush(writer: &mut jet_std::XMLWriter) -> Result<(), jet_std::EncodingError> { writer.flush_output() }
fn jet_enc_xml_writer_finish(writer: &mut jet_std::XMLWriter) -> Result<(), jet_std::EncodingError> { writer.finish_output() }

fn jet_enc_cbor_reader(input: JetFileReader, limits: jet_std::EncodingLimits) -> Result<jet_std::CBORReader, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits).map_err(|mut e| { e.format = jet_std::EncodingFormat::CBOR; e.line = None; e.column = None; e })?;
    let allocation = JetJsonAllocationBudget::new(jet_encoding_codec_heap_ceiling(&limits));
    // Scratch / wire-adjacent budget is codec-owned for the reader lifetime.
    if !allocation.charge(limits.buffer_bytes as usize) {
        return Err(jet_cbor_heap_error(0, "$".to_string()));
    }
    Ok(jet_std::CBORReader {
        input,
        limits,
        total: 0,
        terminal: None,
        eof: false,
        root_done: false,
        lookahead: None,
        frames: Vec::new(),
        retained: 0,
        workspace: 0,
        allocation,
    })
}

impl jet_std::CBORReader {
    fn fail<T>(&mut self, error: jet_std::EncodingError) -> Result<T, jet_std::EncodingError> { self.terminal = Some(error.clone()); Err(error) }
    fn path(&self) -> String {
        let mut path = "$".to_string();
        for frame in &self.frames {
            match frame {
                JetCborReadFrame::Array { index, .. } => path.push_str(&format!("[{index}]")),
                JetCborReadFrame::Object { key: Some(key), keys, expecting_key: false, .. } => path.push_str(&format!("[{:?}]", keys[*key])),
                _ => {}
            }
        }
        path
    }
    fn raw(&mut self) -> Result<Option<u8>, jet_std::EncodingError> {
        if let Some(byte) = self.lookahead.take() { return Ok(Some(byte)); }
        if self.limits.max_total_bytes.is_some_and(|n| self.total >= n) {
            return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, self.total, self.path(), format!("max_total_bytes {} exceeded", self.limits.max_total_bytes.unwrap())));
        }
        let mut byte = [0u8; 1];
        match std::io::Read::read(&mut self.input.inner, &mut byte) {
            Ok(0) => Ok(None),
            Ok(_) => { self.total += 1; Ok(Some(byte[0])) }
            Err(error) => Err(jet_cbor_stream_io(error, self.total, self.path())),
        }
    }
    fn required(&mut self, start: i64, reason: &str) -> Result<u8, jet_std::EncodingError> {
        self.raw()?.ok_or_else(|| jet_cbor_stream_error(jet_std::EncodingErrorKind::Truncated, self.total.max(start), self.path(), reason))
    }
    fn arg(&mut self, add: u8, start: i64) -> Result<Option<u64>, jet_std::EncodingError> {
        let need = match add { n @ 0..=23 => return Ok(Some(n as u64)), 24 => 1, 25 => 2, 26 => 4, 27 => 8, 31 => return Ok(None), _ => return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Syntax, start, self.path(), format!("reserved CBOR additional value {add}"))) };
        let mut value = 0u64;
        for _ in 0..need { value = (value << 8) | self.required(start, "CBOR argument ended before all bytes were present")? as u64; }
        Ok(Some(value))
    }
    fn payload(&mut self, len: u64, start: i64) -> Result<Vec<u8>, jet_std::EncodingError> {
        if len > self.limits.max_item_bytes as u64 || self.retained.saturating_add(len as usize) > self.limits.max_item_bytes as usize {
            return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, start, self.path(), format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)));
        }
        let mut out = Vec::new();
        let mut charged = 0usize;
        for _ in 0..len {
            if out.len() == out.capacity() {
                let old = out.capacity();
                let next = if old == 0 { 8 } else { old.saturating_mul(2) };
                let growth = next.saturating_sub(old);
                if !self.allocation.charge(growth) {
                    return Err(jet_cbor_heap_error(self.total, self.path()));
                }
                charged = charged.saturating_add(growth);
                if out.try_reserve_exact(growth).is_err() {
                    self.allocation.release(charged);
                    return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, start, self.path(), "CBOR payload allocation failed"));
                }
            }
            match self.required(start, "CBOR payload ended before its declared length") {
                Ok(byte) => out.push(byte),
                Err(error) => {
                    self.allocation.release(charged);
                    return Err(error);
                }
            }
        }
        // Returned Text/Bytes leave codec ownership (D-ENCSTREAM-SURFACE1).
        self.allocation.release(charged);
        Ok(out)
    }
    fn string(&mut self, major: u8, add: u8, start: i64) -> Result<Vec<u8>, jet_std::EncodingError> {
        if let Some(len) = self.arg(add, start)? { return self.payload(len, start); }
        if self.frames.len() + 1 > self.limits.max_depth as usize { return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, start, self.path(), format!("max_depth {} exceeded", self.limits.max_depth))); }
        let mut out = Vec::new();
        let mut charged = 0usize;
        loop {
            let chunk_start = self.total;
            let head = match self.required(chunk_start, "indefinite CBOR string ended before break") {
                Ok(head) => head,
                Err(error) => {
                    self.allocation.release(charged);
                    return Err(error);
                }
            };
            if head == 0xff { break; }
            if head >> 5 != major || head & 31 == 31 {
                self.allocation.release(charged);
                return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Syntax, chunk_start, self.path(), "indefinite CBOR string contains a wrong or indefinite chunk"));
            }
            let len = match self.arg(head & 31, chunk_start) {
                Ok(Some(len)) => len,
                Ok(None) => {
                    self.allocation.release(charged);
                    return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Syntax, chunk_start, self.path(), "indefinite CBOR string contains a wrong or indefinite chunk"));
                }
                Err(error) => {
                    self.allocation.release(charged);
                    return Err(error);
                }
            };
            let next = self.retained.checked_add(out.len()).and_then(|n| n.checked_add(len as usize));
            if next.is_none_or(|n| n > self.limits.max_item_bytes as usize) {
                self.allocation.release(charged);
                return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, chunk_start, self.path(), format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)));
            }
            for _ in 0..len {
                if out.len() == out.capacity() {
                    let old = out.capacity();
                    let next_cap = if old == 0 { 8 } else { old.saturating_mul(2) };
                    let growth = next_cap.saturating_sub(old);
                    if !self.allocation.charge(growth) {
                        return Err(jet_cbor_heap_error(self.total, self.path()));
                    }
                    charged = charged.saturating_add(growth);
                    if out.try_reserve_exact(growth).is_err() {
                        self.allocation.release(charged);
                        return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, chunk_start, self.path(), "CBOR payload allocation failed"));
                    }
                }
                match self.required(chunk_start, "CBOR payload ended before its declared length") {
                    Ok(byte) => out.push(byte),
                    Err(error) => {
                        self.allocation.release(charged);
                        return Err(error);
                    }
                }
            }
        }
        self.allocation.release(charged);
        Ok(out)
    }
    fn reserve_frame(&mut self, start: i64) -> Result<(), jet_std::EncodingError> {
        if self.frames.len() < self.frames.capacity() { return Ok(()); }
        let old = self.frames.capacity();
        let slot = std::mem::size_of::<JetCborReadFrame>();
        jet_cbor_charge(&self.allocation, slot, start, self.path())?;
        self.frames.try_reserve_exact(1).map_err(|_| jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, start, self.path(), "CBOR reader frame allocation failed"))?;
        self.workspace = self.workspace.saturating_add(self.frames.capacity().saturating_sub(old).saturating_mul(slot));
        Ok(())
    }
    fn retain_key(&mut self, text: &str, start: i64) -> Result<usize, jet_std::EncodingError> {
        let duplicate = matches!(self.frames.last(), Some(JetCborReadFrame::Object { keys, .. }) if keys.iter().any(|key| key == text));
        if duplicate { return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Unsupported, start, self.path(), "duplicate CBOR text map key")); }
        let needs_slot = matches!(self.frames.last(), Some(JetCborReadFrame::Object { keys, .. }) if keys.len() == keys.capacity());
        let slot = std::mem::size_of::<String>();
        let estimated = text.len().saturating_add(if needs_slot { slot } else { 0 });
        jet_cbor_charge(&self.allocation, estimated, start, self.path())?;
        let mut stored = String::new();
        stored.try_reserve_exact(text.len()).map_err(|_| jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, start, self.path(), "CBOR map key allocation failed"))?;
        stored.push_str(text);
        let stored_capacity = stored.capacity();
        let mut slot_bytes = 0usize;
        if let Some(JetCborReadFrame::Object { keys, .. }) = self.frames.last_mut() {
            if keys.len() == keys.capacity() {
                let old = keys.capacity();
                keys.try_reserve_exact(1).map_err(|_| jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, start, "$".to_string(), "CBOR map key table allocation failed"))?;
                slot_bytes = keys.capacity().saturating_sub(old).saturating_mul(slot);
            }
            let index = keys.len();
            keys.push(stored);
            self.workspace = self.workspace.saturating_add(stored_capacity).saturating_add(slot_bytes);
            return Ok(index);
        }
        self.allocation.release(estimated);
        Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::State, start, self.path(), "CBOR map key outside object"))
    }
    fn close_event(&mut self) -> Result<Option<jet_std::DataEvent>, jet_std::EncodingError> {
        let close = match self.frames.last() {
            Some(JetCborReadFrame::Array { remaining: Some(0), .. }) => Some(false),
            Some(JetCborReadFrame::Object { remaining: Some(0), expecting_key: true, .. }) => Some(true),
            Some(JetCborReadFrame::Array { remaining: None, .. }) => { let b = self.raw()?; if b == Some(0xff) { Some(false) } else { self.lookahead = b; None } }
            Some(JetCborReadFrame::Object { remaining: None, expecting_key: true, .. }) => { let b = self.raw()?; if b == Some(0xff) { Some(true) } else { self.lookahead = b; None } }
            _ => None,
        };
        if let Some(object) = close {
            match self.frames.pop() {
                Some(JetCborReadFrame::Object { keys, .. }) => {
                    let released = keys.capacity().saturating_mul(std::mem::size_of::<String>()).saturating_add(keys.iter().map(String::capacity).sum::<usize>());
                    self.retained = self.retained.saturating_sub(keys.iter().map(String::len).sum::<usize>());
                    self.workspace = self.workspace.saturating_sub(released);
                    self.allocation.release(released);
                }
                Some(_) | None => {}
            }
            self.complete_parent();
            if self.frames.is_empty() { self.root_done = true; }
            return Ok(Some(if object { jet_std::DataEvent::ObjectEnd } else { jet_std::DataEvent::ArrayEnd }));
        }
        Ok(None)
    }
    fn complete_parent(&mut self) {
        if let Some(frame) = self.frames.last_mut() {
            match frame {
                JetCborReadFrame::Array { remaining, index } => { if let Some(n) = remaining { *n = n.saturating_sub(1); } *index += 1; }
                JetCborReadFrame::Object { remaining, expecting_key, key, .. } if !*expecting_key => { if let Some(n) = remaining { *n = n.saturating_sub(1); } *expecting_key = true; *key = None; }
                _ => {}
            }
        }
    }
    fn next_event_inner(&mut self) -> Result<Option<jet_std::DataEvent>, jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        if self.eof { return Ok(None); }
        if let Some(event) = self.close_event()? { return Ok(Some(event)); }
        if self.root_done {
            match self.raw() { Ok(None) => { self.eof = true; return Ok(None); }, Ok(Some(_)) => return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Syntax, self.total - 1, "$".to_string(), "trailing CBOR data after root value")), Err(e) => return self.fail(e) }
        }
        let start = if self.lookahead.is_some() { self.total.saturating_sub(1) } else { self.total };
        let head = match self.raw() { Ok(Some(b)) => b, Ok(None) if self.frames.is_empty() => { self.eof = true; return Ok(None); }, Ok(None) => return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Truncated, self.total, self.path(), "CBOR container ended before all items were present")), Err(e) => return self.fail(e) };
        if head == 0xff { return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Syntax, start, self.path(), "CBOR break outside an indefinite container")); }
        let major = head >> 5; let add = head & 31;
        let expecting_key = matches!(self.frames.last(), Some(JetCborReadFrame::Object { expecting_key: true, .. }));
        if expecting_key && major != 3 { return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Unsupported, start, self.path(), "CBOR map key must be text")); }
        let event = match major {
            0 | 1 => { let Some(n) = self.arg(add, start)? else { return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Syntax, start, self.path(), "indefinite CBOR integer")); }; let value = if major == 0 { i64::try_from(n).ok() } else { i64::try_from(n).ok().and_then(|n| n.checked_neg()?.checked_sub(1)) }; let Some(value) = value else { return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Unsupported, start, self.path(), "CBOR integer is outside Jet Int")); }; jet_std::DataEvent::Int(value) }
            2 | 3 => { let bytes = match self.string(major, add, start) { Ok(v) => v, Err(e) => return self.fail(e) }; if major == 2 { jet_std::DataEvent::Bytes(bytes) } else { let text = match String::from_utf8(bytes) { Ok(v) => v, Err(_) => return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Syntax, start, self.path(), "CBOR text is not UTF-8")) }; if expecting_key { let prospective = self.retained.checked_add(text.len().saturating_mul(2)); if prospective.is_none_or(|n| n > self.limits.max_item_bytes as usize) { return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, start, self.path(), format!("max_item_bytes {} exceeded", self.limits.max_item_bytes))); } let index = match self.retain_key(&text, start) { Ok(index) => index, Err(error) => return self.fail(error) }; self.retained = self.retained.saturating_add(text.len()); if let Some(JetCborReadFrame::Object { expecting_key, key, .. }) = self.frames.last_mut() { *key = Some(index); *expecting_key = false; } return Ok(Some(jet_std::DataEvent::Key(text))); } jet_std::DataEvent::Text(text) } }
            4 | 5 => { if self.frames.len() + 1 > self.limits.max_depth as usize { return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit, start, self.path(), format!("max_depth {} exceeded", self.limits.max_depth))); } let count = match self.arg(add, start) { Ok(v) => v, Err(e) => return self.fail(e) }; if let Err(error) = self.reserve_frame(start) { return self.fail(error); } if major == 4 { self.frames.push(JetCborReadFrame::Array { remaining: count, index: 0 }); jet_std::DataEvent::ArrayStart } else { self.frames.push(JetCborReadFrame::Object { remaining: count, expecting_key: true, key: None, keys: Vec::new() }); jet_std::DataEvent::ObjectStart } }
            6 => return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Unsupported, start, self.path(), "CBOR tags are outside DataEvent")),
            7 => match add { 20 => jet_std::DataEvent::Bool(false), 21 => jet_std::DataEvent::Bool(true), 22 => jet_std::DataEvent::Null, 25 => { let bits=u16::from_be_bytes([self.required(start,"truncated CBOR Float16")?,self.required(start,"truncated CBOR Float16")?]); jet_std::DataEvent::Float(jet_cbor_half_to_f64(bits)) }, 26 => { let mut b=[0u8;4]; for x in &mut b { *x=self.required(start,"truncated CBOR Float32")?; } jet_std::DataEvent::Float(f32::from_be_bytes(b) as f64) }, 27 => { let mut b=[0u8;8]; for x in &mut b { *x=self.required(start,"truncated CBOR Float64")?; } jet_std::DataEvent::Float(f64::from_be_bytes(b)) }, _ => return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Unsupported, start, self.path(), format!("unsupported CBOR simple value {add}"))) },
            _ => unreachable!(),
        };
        if !matches!(event, jet_std::DataEvent::ArrayStart | jet_std::DataEvent::ObjectStart) { self.complete_parent(); if self.frames.is_empty() { self.root_done = true; } }
        Ok(Some(event))
    }
    fn next_event(&mut self) -> Result<Option<jet_std::DataEvent>, jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        let result = self.next_event_inner();
        if let Err(error) = &result { self.terminal = Some(error.clone()); }
        result
    }
}

fn jet_enc_cbor_reader_next(reader: &mut jet_std::CBORReader) -> Result<Option<jet_std::DataEvent>, jet_std::EncodingError> { reader.next_event() }

fn jet_cbor_stream_len(out: &mut Vec<u8>, major: u8, n: u64) { if n < 24 { out.push((major << 5) | n as u8); } else if n <= 255 { out.extend_from_slice(&[(major << 5) | 24, n as u8]); } else if n <= 65535 { out.push((major << 5) | 25); out.extend_from_slice(&(n as u16).to_be_bytes()); } else if n <= u32::MAX as u64 { out.push((major << 5) | 26); out.extend_from_slice(&(n as u32).to_be_bytes()); } else { out.push((major << 5) | 27); out.extend_from_slice(&n.to_be_bytes()); } }
fn jet_cbor_stream_len_size(n: u64) -> usize { if n < 24 { 1 } else if n <= u8::MAX as u64 { 2 } else if n <= u16::MAX as u64 { 3 } else if n <= u32::MAX as u64 { 5 } else { 9 } }

fn jet_cbor_half_to_f64(bits:u16)->f64{let sign=((bits>>15)as u64)<<63;let exp=(bits>>10)&31;let frac=bits&1023;if exp==0{if frac==0{return f64::from_bits(sign)}let mut mant=frac as u64;let mut exponent=-14i32;while mant&1024==0{mant<<=1;exponent-=1}mant&=1023;f64::from_bits(sign|(((exponent+1023)as u64)<<52)|(mant<<42))}else if exp==31{f64::from_bits(sign|(0x7ffu64<<52)|((frac as u64)<<42))}else{f64::from_bits(sign|(((exp as i32-15+1023)as u64)<<52)|((frac as u64)<<42))}}
fn jet_cbor_f32_to_half_bits(value:f32)->u16{let bits=value.to_bits();let sign=((bits>>16)&0x8000)as u16;let exp=((bits>>23)&255)as i32;let frac=bits&0x7fffff;if exp==255{return sign|0x7c00|if frac==0{0}else{0x0200}}let half_exp=exp-127+15;if half_exp>=31{return sign|0x7c00}if half_exp<=0{if half_exp< -10{return sign}let mant=frac|0x800000;let shift=(14-half_exp)as u32;let mut rounded=mant>>shift;let rem=mant&((1u32<<shift)-1);let halfway=1u32<<(shift-1);if rem>halfway||(rem==halfway&&rounded&1!=0){rounded+=1}return sign|rounded as u16}let mut rounded=frac>>13;let rem=frac&0x1fff;if rem>0x1000||(rem==0x1000&&rounded&1!=0){rounded+=1}if rounded==0x0400{return sign|(((half_exp+1)as u16)<<10)}sign|((half_exp as u16)<<10)|rounded as u16}
fn jet_cbor_half_exact(value:f64)->Option<u16>{if value.is_nan(){return Some(0x7e00)}let narrowed=value as f32;if(narrowed as f64).to_bits()!=value.to_bits(){return None}let bits=jet_cbor_f32_to_half_bits(narrowed);(jet_cbor_half_to_f64(bits).to_bits()==value.to_bits()).then_some(bits)}
fn jet_cbor_push_preferred_float(out:&mut Vec<u8>,value:f64){if let Some(bits)=jet_cbor_half_exact(value){out.push(0xf9);out.extend_from_slice(&bits.to_be_bytes())}else if((value as f32)as f64).to_bits()==value.to_bits(){out.push(0xfa);out.extend_from_slice(&(value as f32).to_bits().to_be_bytes())}else{out.push(0xfb);out.extend_from_slice(&value.to_bits().to_be_bytes())}}

fn jet_enc_cbor_writer(output: JetFileWriter, limits: jet_std::EncodingLimits) -> Result<jet_std::CBORWriter, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits).map_err(|mut e| {
        e.format = jet_std::EncodingFormat::CBOR;
        e.line = None;
        e.column = None;
        e
    })?;
    let allocation = JetJsonAllocationBudget::new(jet_encoding_codec_heap_ceiling(&limits));
    Ok(jet_std::CBORWriter {
        output,
        limits,
        terminal: None,
        total: 0,
        frames: Vec::new(),
        root_written: false,
        finished: false,
        retained: 0,
        workspace: 0,
        allocation,
    })
}
impl jet_std::CBORWriter {
    fn fail<T>(&mut self,e:jet_std::EncodingError)->Result<T,jet_std::EncodingError>{self.terminal=Some(e.clone());Err(e)}
    fn item_limit(&self)->usize{self.limits.max_item_bytes as usize}
    fn check_replacement(&self,old:usize,new:usize)->Result<(),jet_std::EncodingError>{
        let retained=self.retained.checked_sub(old).and_then(|n|n.checked_add(new));
        if retained.is_none_or(|n|n>self.item_limit()){return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),format!("max_item_bytes {} exceeded",self.limits.max_item_bytes)));}
        Ok(())
    }
    fn allocate(&self,size:usize)->Result<Vec<u8>,jet_std::EncodingError>{let mut out=Vec::new();out.try_reserve_exact(size).map_err(|_|jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR item allocation failed"))?;Ok(out)}
    fn reserve_frame(&mut self)->Result<(),jet_std::EncodingError>{
        if self.frames.len()<self.frames.capacity(){return Ok(())}
        let old=self.frames.capacity();let slot=std::mem::size_of::<JetCborWriteFrame>();
        jet_cbor_charge(&self.allocation,slot,self.total,"$".to_string())?;
        self.frames.try_reserve_exact(1).map_err(|_|jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR writer frame allocation failed"))?;
        self.workspace=self.workspace.saturating_add(self.frames.capacity().saturating_sub(old).saturating_mul(slot));Ok(())
    }
    fn scalar(&self,event:jet_std::DataEvent)->Result<Vec<u8>,jet_std::EncodingError>{
        let payload=match &event{jet_std::DataEvent::Text(s)=>s.len(),jet_std::DataEvent::Bytes(b)=>b.len(),jet_std::DataEvent::Float(f)=>if jet_cbor_half_exact(*f).is_some(){2}else if ((*f as f32)as f64).to_bits()==f.to_bits(){4}else{8},_=>0};
        let header=match &event{jet_std::DataEvent::Text(s)=>jet_cbor_stream_len_size(s.len()as u64),jet_std::DataEvent::Bytes(b)=>jet_cbor_stream_len_size(b.len()as u64),jet_std::DataEvent::Float(_)=>1,jet_std::DataEvent::Null|jet_std::DataEvent::Bool(_)=>1,jet_std::DataEvent::Int(n)if*n>=0=>jet_cbor_stream_len_size(*n as u64),jet_std::DataEvent::Int(n)=>jet_cbor_stream_len_size((-1-*n)as u64),_=>return Err(jet_cbor_stream_error(jet_std::EncodingErrorKind::State,self.total,"$".to_string(),"CBOR container event reached scalar encoder"))};
        let size=header.checked_add(payload).ok_or_else(||jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR item size overflow"))?;
        self.check_replacement(0,size)?;
        let mut out=self.allocate(size)?;
        match event{jet_std::DataEvent::Null=>out.push(0xf6),jet_std::DataEvent::Bool(false)=>out.push(0xf4),jet_std::DataEvent::Bool(true)=>out.push(0xf5),jet_std::DataEvent::Int(n)if n>=0=>jet_cbor_stream_len(&mut out,0,n as u64),jet_std::DataEvent::Int(n)=>jet_cbor_stream_len(&mut out,1,(-1-n)as u64),jet_std::DataEvent::Float(f)=>jet_cbor_push_preferred_float(&mut out,f),jet_std::DataEvent::Text(s)=>{jet_cbor_stream_len(&mut out,3,s.len()as u64);out.extend_from_slice(s.as_bytes());},jet_std::DataEvent::Bytes(b)=>{jet_cbor_stream_len(&mut out,2,b.len()as u64);out.extend_from_slice(&b);},_=>unreachable!()}
        Ok(out)
    }
    fn accept(&mut self,bytes:Vec<u8>)->Result<(),jet_std::EncodingError>{
        let size=bytes.len();let capacity=bytes.capacity();self.check_replacement(0,size)?;
        if let Some(frame)=self.frames.last(){
            let slot=match frame{JetCborWriteFrame::Array{items}if items.len()==items.capacity()=>std::mem::size_of::<Vec<u8>>(),JetCborWriteFrame::Object{entries,key:Some(_)}if entries.len()==entries.capacity()=>std::mem::size_of::<(Vec<u8>,Vec<u8>)>(),JetCborWriteFrame::Object{key:None,..}=>return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::State,self.total,"$".to_string(),"CBOR object value written before Key")),_=>0};
            if let Err(error)=jet_cbor_charge(&self.allocation,capacity.saturating_add(slot),self.total,"$".to_string()){return self.fail(error)}
            let mut slot_bytes=0usize;
            if let Some(frame)=self.frames.last_mut(){match frame{
                JetCborWriteFrame::Array{items}=>{if items.len()==items.capacity(){let old=items.capacity();if items.try_reserve_exact(1).is_err(){return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR array table allocation failed"))}slot_bytes=items.capacity().saturating_sub(old).saturating_mul(std::mem::size_of::<Vec<u8>>());}items.push(bytes)},
                JetCborWriteFrame::Object{entries,key}=>{let k=key.take().expect("CBOR object key checked");if entries.len()==entries.capacity(){let old=entries.capacity();if entries.try_reserve_exact(1).is_err(){return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR object table allocation failed"))}slot_bytes=entries.capacity().saturating_sub(old).saturating_mul(std::mem::size_of::<(Vec<u8>,Vec<u8>)>());}entries.push((k,bytes))}
            }}
            self.workspace=self.workspace.saturating_add(capacity).saturating_add(slot_bytes);self.retained+=size;return Ok(());
        }
        if self.root_written{return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::State,self.total,"$".to_string(),"CBOR writer accepts exactly one root"));}
        if self.limits.max_total_bytes.is_some_and(|n|self.total.saturating_add(size as i64)>n){return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),format!("max_total_bytes {} exceeded",self.limits.max_total_bytes.unwrap())));}
        if let Err(e)=std::io::Write::write_all(&mut self.output.inner,&bytes){return self.fail(jet_cbor_stream_io(e,self.total,"$".to_string()));}self.total+=size as i64;self.root_written=true;Ok(())
    }
    fn write_key(&mut self,key_text:String)->Result<(),jet_std::EncodingError>{
        let valid=matches!(self.frames.last(),Some(JetCborWriteFrame::Object{key:None,..}));if !valid{return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::State,self.total,"$".to_string(),"Key outside CBOR object or before prior value"));}
        let size=jet_cbor_stream_len_size(key_text.len()as u64).checked_add(key_text.len()).ok_or_else(||jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR key size overflow"))?;if let Err(e)=self.check_replacement(0,size){return self.fail(e)}
        let mut encoded=match self.allocate(size){Ok(v)=>v,Err(e)=>return self.fail(e)};jet_cbor_stream_len(&mut encoded,3,key_text.len()as u64);encoded.extend_from_slice(key_text.as_bytes());
        let capacity=encoded.capacity();if let Err(error)=jet_cbor_charge(&self.allocation,capacity,self.total,"$".to_string()){return self.fail(error)}
        if let Some(JetCborWriteFrame::Object{key,entries})=self.frames.last_mut(){if entries.iter().any(|(old,_)|*old==encoded){return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Unsupported,self.total,"$".to_string(),"duplicate CBOR text map key"));}*key=Some(encoded);self.retained+=size;self.workspace=self.workspace.saturating_add(capacity);}
        Ok(())
    }
    fn close_array(&mut self,items:Vec<Vec<u8>>)->Result<(),jet_std::EncodingError>{
        let old=items.iter().try_fold(0usize,|n,item|n.checked_add(item.len())).ok_or_else(||jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR array size overflow"))?;
        let old_workspace=items.capacity().saturating_mul(std::mem::size_of::<Vec<u8>>()).saturating_add(items.iter().map(Vec::capacity).sum::<usize>());
        let size=jet_cbor_stream_len_size(items.len()as u64).checked_add(old).ok_or_else(||jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR array size overflow"))?;
        if let Err(e)=self.check_replacement(old,size){return self.fail(e)}
        if let Err(e)=jet_cbor_ensure_fit(&self.allocation,size,self.total,"$".to_string()){return self.fail(e)}
        let mut out=match self.allocate(size){Ok(v)=>v,Err(e)=>return self.fail(e)};
        self.retained-=old;self.workspace=self.workspace.saturating_sub(old_workspace);self.allocation.release(old_workspace);
        jet_cbor_stream_len(&mut out,4,items.len()as u64);for item in items{out.extend_from_slice(&item);}self.accept(out)
    }
    fn close_object(&mut self,mut entries:Vec<(Vec<u8>,Vec<u8>)>)->Result<(),jet_std::EncodingError>{
        let old=entries.iter().try_fold(0usize,|n,(key,value)|n.checked_add(key.len())?.checked_add(value.len())).ok_or_else(||jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR object size overflow"))?;
        let old_workspace=entries.capacity().saturating_mul(std::mem::size_of::<(Vec<u8>,Vec<u8>)>()).saturating_add(entries.iter().map(|(key,value)|key.capacity().saturating_add(value.capacity())).sum::<usize>());
        let size=jet_cbor_stream_len_size(entries.len()as u64).checked_add(old).ok_or_else(||jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),"CBOR object size overflow"))?;
        if let Err(e)=self.check_replacement(old,size){return self.fail(e)}
        if let Err(e)=jet_cbor_ensure_fit(&self.allocation,size,self.total,"$".to_string()){return self.fail(e)}
        let mut out=match self.allocate(size){Ok(v)=>v,Err(e)=>return self.fail(e)};
        self.retained-=old;self.workspace=self.workspace.saturating_sub(old_workspace);self.allocation.release(old_workspace);
        entries.sort_by(|a,b|a.0.cmp(&b.0));jet_cbor_stream_len(&mut out,5,entries.len()as u64);for(key,value)in entries{out.extend_from_slice(&key);out.extend_from_slice(&value);}self.accept(out)
    }
    fn write_event(&mut self,event:jet_std::DataEvent)->Result<(),jet_std::EncodingError>{if let Some(e)=&self.terminal{return Err(e.clone())}if self.finished{return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::State,self.total,"$".to_string(),"write called after finish"));}match event{jet_std::DataEvent::ArrayStart=>{if self.frames.len()+1>self.limits.max_depth as usize{return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),format!("max_depth {} exceeded",self.limits.max_depth)));}if let Err(error)=self.reserve_frame(){return self.fail(error)}self.frames.push(JetCborWriteFrame::Array{items:Vec::new()});Ok(())},jet_std::DataEvent::ObjectStart=>{if self.frames.len()+1>self.limits.max_depth as usize{return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::Limit,self.total,"$".to_string(),format!("max_depth {} exceeded",self.limits.max_depth)));}if let Err(error)=self.reserve_frame(){return self.fail(error)}self.frames.push(JetCborWriteFrame::Object{entries:Vec::new(),key:None});Ok(())},jet_std::DataEvent::Key(key)=>self.write_key(key),jet_std::DataEvent::ArrayEnd=>{let Some(JetCborWriteFrame::Array{items})=self.frames.pop()else{return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::State,self.total,"$".to_string(),"ArrayEnd does not match open CBOR container"));};self.close_array(items)},jet_std::DataEvent::ObjectEnd=>{let Some(JetCborWriteFrame::Object{entries,key:None})=self.frames.pop()else{return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::State,self.total,"$".to_string(),"ObjectEnd does not match complete CBOR object"));};self.close_object(entries)},scalar=>match self.scalar(scalar){Ok(v)=>self.accept(v),Err(e)=>self.fail(e)}}}
    fn flush_output(&mut self)->Result<(),jet_std::EncodingError>{if let Some(e)=&self.terminal{return Err(e.clone())}if let Err(e)=std::io::Write::flush(&mut self.output.inner){return self.fail(jet_cbor_stream_io(e,self.total,"$".to_string()));}Ok(())}
    fn finish_output(&mut self)->Result<(),jet_std::EncodingError>{if let Some(e)=&self.terminal{return Err(e.clone())}if self.finished{return Ok(())}if !self.frames.is_empty()||!self.root_written{return self.fail(jet_cbor_stream_error(jet_std::EncodingErrorKind::State,self.total,"$".to_string(),"finish requires one complete CBOR root"));}self.flush_output()?;self.finished=true;Ok(())}
}
fn jet_enc_cbor_writer_write(writer:&mut jet_std::CBORWriter,event:jet_std::DataEvent)->Result<(),jet_std::EncodingError>{writer.write_event(event)}
fn jet_enc_cbor_writer_flush(writer:&mut jet_std::CBORWriter)->Result<(),jet_std::EncodingError>{writer.flush_output()}
fn jet_enc_cbor_writer_finish(writer:&mut jet_std::CBORWriter)->Result<(),jet_std::EncodingError>{writer.finish_output()}
