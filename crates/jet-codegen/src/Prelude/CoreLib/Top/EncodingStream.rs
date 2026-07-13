// D-ENCSTREAM-SURFACE1=A: bounded pull JSON over owned file handles.
// Reader consumes at most one scalar/container boundary per `next`; no read-to-end
// or delimiter transcript sits behind this API.

#[derive(Clone)]
enum JetJsonReadFrame {
    ArrayValueOrEnd { first: bool, index: usize },
    ArrayCommaOrEnd { index: usize },
    ObjectKeyOrEnd { first: bool },
    ObjectColonValue { key: String },
    ObjectCommaOrEnd { key: String },
}

#[derive(Clone, Copy)]
enum JetJsonWriteFrame {
    Array { first: bool },
    ObjectKey { first: bool },
    ObjectValue,
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

fn jet_enc_json_reader(
    input: JetFileReader,
    limits: jet_std::EncodingLimits,
) -> Result<jet_std::JSONReader, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits)?;
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
    })
}

fn jet_enc_json_writer(
    output: JetFileWriter,
    limits: jet_std::EncodingLimits,
    canonical: bool,
) -> Result<jet_std::JSONWriter, jet_std::EncodingError> {
    jet_encoding_validate_limits(&limits)?;
    if canonical {
        return Err(jet_encoding_error(
            jet_std::EncodingErrorKind::Unsupported,
            0,
            1,
            1,
            "canonical streaming JSON is not available until object buffering can preserve max_item_bytes",
        ));
    }
    Ok(jet_std::JSONWriter {
        output,
        limits,
        frames: Vec::new(),
        root_written: false,
        finished: false,
        terminal: None,
        total: 0,
        canonical,
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
                JetJsonReadFrame::ObjectColonValue { key }
                | JetJsonReadFrame::ObjectCommaOrEnd { key } => {
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

    fn append_string_bytes(&self, bytes: &mut Vec<u8>, append: &[u8]) -> Result<(), jet_std::EncodingError> {
        let limit = self.limits.max_item_bytes as usize;
        let Some(new_len) = bytes.len().checked_add(append.len()) else {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.offset, self.line, self.column, format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)));
        };
        if new_len > limit {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.offset, self.line, self.column, format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)));
        }
        bytes.reserve_exact(new_len.saturating_sub(bytes.capacity()));
        bytes.extend_from_slice(append);
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
                        b'"' | b'\\' | b'/' => self.append_string_bytes(&mut bytes, &[escaped])?,
                        b'b' => self.append_string_bytes(&mut bytes, &[8])?,
                        b'f' => self.append_string_bytes(&mut bytes, &[12])?,
                        b'n' => self.append_string_bytes(&mut bytes, b"\n")?,
                        b'r' => self.append_string_bytes(&mut bytes, b"\r")?,
                        b't' => self.append_string_bytes(&mut bytes, b"\t")?,
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
                            self.append_string_bytes(&mut bytes, ch.encode_utf8(&mut buf).as_bytes())?;
                        }
                        _ => return Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - 1, self.line, self.column.saturating_sub(1), "unknown JSON escape")),
                    }
                }
                _ => self.append_string_bytes(&mut bytes, &[byte])?,
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
        let mut bytes = vec![first];
        while let Some(byte @ (b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) = self.fill()? {
            bytes.push(byte);
            self.take()?;
            if bytes.len() as i64 > self.limits.max_item_bytes {
                return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.offset, self.line, self.column, format!("max_item_bytes {} exceeded", self.limits.max_item_bytes)));
            }
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
        if !valid {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset - bytes.len() as i64, self.line, self.column, format!("invalid JSON number `{}`", text)));
        }
        if !text.contains(['.', 'e', 'E']) {
            if let Ok(value) = text.parse::<i64>() {
                return Ok(jet_std::DataEvent::Int(value));
            }
        }
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(jet_std::DataEvent::Float(value)),
            _ => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.offset - bytes.len() as i64, self.line, self.column, "JSON number is outside the DataTree numeric range")),
        }
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
                self.frames.push(JetJsonReadFrame::ArrayValueOrEnd { first: true, index: 0 });
                Ok(jet_std::DataEvent::ArrayStart)
            }
            b'{' => {
                if self.frames.len() as i64 >= self.limits.max_depth { return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.offset - 1, self.line, self.column.saturating_sub(1), format!("max_depth {} exceeded", self.limits.max_depth))); }
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
            let state = self.frames.last().cloned();
            match state {
                Some(JetJsonReadFrame::ArrayValueOrEnd { first, index }) => {
                    if self.fill()? == Some(b']') {
                        if !first { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "expected a JSON value after `,`")); }
                        self.take()?; self.frames.pop(); return Ok(Some(jet_std::DataEvent::ArrayEnd));
                    }
                    if !first && self.fill()?.is_none() { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "unterminated JSON array")); }
                    *self.frames.last_mut().unwrap() = JetJsonReadFrame::ArrayCommaOrEnd { index };
                    return self.parse_value().map(Some).or_else(|e| self.fail(e));
                }
                Some(JetJsonReadFrame::ArrayCommaOrEnd { index }) => match self.fill()? {
                    Some(b',') => { self.take()?; *self.frames.last_mut().unwrap() = JetJsonReadFrame::ArrayValueOrEnd { first: false, index: index + 1 }; }
                    Some(b']') => { self.take()?; self.frames.pop(); return Ok(Some(jet_std::DataEvent::ArrayEnd)); }
                    Some(_) => return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "expected `,` or `]` after array value")),
                    None => return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "unterminated JSON array")),
                },
                Some(JetJsonReadFrame::ObjectKeyOrEnd { first }) => {
                    if self.fill()? == Some(b'}') {
                        if !first { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "expected an object key after `,`")); }
                        self.take()?; self.frames.pop(); return Ok(Some(jet_std::DataEvent::ObjectEnd));
                    }
                    if !first && self.fill()?.is_none() { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Truncated, self.offset, self.line, self.column, "unterminated JSON object")); }
                    if self.fill()? != Some(b'"') { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Syntax, self.offset, self.line, self.column, "expected a quoted object key")); }
                    let key = match self.read_string() { Ok(v) => v, Err(e) => return self.fail(e) };
                    *self.frames.last_mut().unwrap() = JetJsonReadFrame::ObjectColonValue { key: key.clone() };
                    return Ok(Some(jet_std::DataEvent::Key(key)));
                }
                Some(JetJsonReadFrame::ObjectColonValue { key }) => {
                    if let Err(e) = self.expect_byte(b':', "`:` after object key") { return self.fail(e); }
                    *self.frames.last_mut().unwrap() = JetJsonReadFrame::ObjectCommaOrEnd { key };
                    return self.parse_value().map(Some).or_else(|e| self.fail(e));
                }
                Some(JetJsonReadFrame::ObjectCommaOrEnd { .. }) => match self.fill()? {
                    Some(b',') => { self.take()?; *self.frames.last_mut().unwrap() = JetJsonReadFrame::ObjectKeyOrEnd { first: false }; }
                    Some(b'}') => { self.take()?; self.frames.pop(); return Ok(Some(jet_std::DataEvent::ObjectEnd)); }
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
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), jet_std::EncodingError> {
        if self.limits.max_total_bytes.is_some_and(|max| self.total.saturating_add(bytes.len() as i64) > max) {
            return Err(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_total_bytes {} exceeded", self.limits.max_total_bytes.unwrap_or(0))));
        }
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
    fn write_event(&mut self, event: jet_std::DataEvent) -> Result<(), jet_std::EncodingError> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        match self.write_event_inner(event) {
            Ok(()) => Ok(()),
            Err(error) => self.fail(error),
        }
    }
    fn write_event_inner(&mut self, event: jet_std::DataEvent) -> Result<(), jet_std::EncodingError> {
        if self.finished { return Err(self.state_error("write called after finish")); }
        let result = match event {
            jet_std::DataEvent::Key(key) => {
                let first = match self.frames.last().copied() { Some(JetJsonWriteFrame::ObjectKey { first }) => first, _ => return self.fail(self.state_error("Key is only valid while an object expects a key")) };
                if !first { self.write_bytes(b",")?; }
                self.write_quoted(&key)?; self.write_bytes(b":")?;
                *self.frames.last_mut().unwrap() = JetJsonWriteFrame::ObjectValue; Ok(())
            }
            jet_std::DataEvent::ArrayEnd => match self.frames.last().copied() { Some(JetJsonWriteFrame::Array { .. }) => { self.write_bytes(b"]")?; self.frames.pop(); Ok(()) }, _ => Err(self.state_error("ArrayEnd does not match an open array")) },
            jet_std::DataEvent::ObjectEnd => match self.frames.last().copied() { Some(JetJsonWriteFrame::ObjectKey { .. }) => { self.write_bytes(b"}")?; self.frames.pop(); Ok(()) }, Some(JetJsonWriteFrame::ObjectValue) => Err(self.state_error("object key has no value")), _ => Err(self.state_error("ObjectEnd does not match an open object")) },
            jet_std::DataEvent::Bytes(_) => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.total, 1, self.total + 1, "JSON cannot encode Bytes; encode bytes as Text explicitly")),
            jet_std::DataEvent::Float(value) if !value.is_finite() => Err(jet_encoding_error(jet_std::EncodingErrorKind::Unsupported, self.total, 1, self.total + 1, "JSON cannot encode a non-finite Float")),
            value => {
                self.before_value()?;
                match value {
                    jet_std::DataEvent::Null => self.write_bytes(b"null"),
                    jet_std::DataEvent::Bool(v) => self.write_bytes(if v { b"true" } else { b"false" }),
                    jet_std::DataEvent::Int(v) => self.write_bytes(v.to_string().as_bytes()),
                    jet_std::DataEvent::Float(v) => self.write_bytes(v.to_string().as_bytes()),
                    jet_std::DataEvent::Text(v) => self.write_quoted(&v),
                    jet_std::DataEvent::ArrayStart => { if self.frames.len() as i64 >= self.limits.max_depth { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_depth {} exceeded", self.limits.max_depth))); } self.write_bytes(b"[")?; self.frames.push(JetJsonWriteFrame::Array { first: true }); Ok(()) },
                    jet_std::DataEvent::ObjectStart => { if self.frames.len() as i64 >= self.limits.max_depth { return self.fail(jet_encoding_error(jet_std::EncodingErrorKind::Limit, self.total, 1, self.total + 1, format!("max_depth {} exceeded", self.limits.max_depth))); } self.write_bytes(b"{")?; self.frames.push(JetJsonWriteFrame::ObjectKey { first: true }); Ok(()) },
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
        if !self.root_written || !self.frames.is_empty() { return self.fail(self.state_error("finish requires one structurally complete JSON root")); }
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

    fn charge(&mut self, retained: &mut usize, amount: usize) -> Result<(), jet_std::EncodingError> {
        let limit = self.json.limits.max_item_bytes as usize;
        *retained = retained.checked_add(amount).ok_or_else(|| {
            jet_encoding_error(
                jet_std::EncodingErrorKind::Limit,
                self.json.offset,
                self.json.line,
                self.json.column,
                format!("max_item_bytes {} exceeded", self.json.limits.max_item_bytes),
            )
        })?;
        if *retained > limit {
            return Err(jet_encoding_error(
                jet_std::EncodingErrorKind::Limit,
                self.json.offset,
                self.json.line,
                self.json.column,
                format!("max_item_bytes {} exceeded", self.json.limits.max_item_bytes),
            ));
        }
        Ok(())
    }

    fn push_value(
        &mut self,
        root: &mut Option<jet_std::DataTree>,
        frames: &mut Vec<JetJsonlFoldFrame>,
        value: jet_std::DataTree,
    ) -> Result<(), jet_std::EncodingError> {
        match frames.last_mut() {
            Some(JetJsonlFoldFrame::Array(items)) => items.push(value),
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
        let mut retained = 0usize;
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
                    self.charge(&mut retained, 1)?;
                    self.push_value(&mut root, &mut frames, jet_std::DataTree::Null)
                }
                jet_std::DataEvent::Bool(value) => {
                    self.charge(&mut retained, 1)?;
                    self.push_value(&mut root, &mut frames, jet_std::DataTree::Bool(value))
                }
                jet_std::DataEvent::Int(value) => {
                    self.charge(&mut retained, 8)?;
                    self.push_value(&mut root, &mut frames, jet_std::DataTree::Int(value))
                }
                jet_std::DataEvent::Float(value) => {
                    self.charge(&mut retained, 8)?;
                    self.push_value(&mut root, &mut frames, jet_std::DataTree::Float(value))
                }
                jet_std::DataEvent::Text(value) => {
                    self.charge(&mut retained, value.len())?;
                    self.push_value(&mut root, &mut frames, jet_std::DataTree::Text(value))
                }
                jet_std::DataEvent::Bytes(_) => Err(jet_encoding_error(
                    jet_std::EncodingErrorKind::State,
                    self.json.offset,
                    self.json.line,
                    self.json.column,
                    "JSON tokenizer produced Bytes",
                )),
                jet_std::DataEvent::ArrayStart => {
                    self.charge(&mut retained, 1)?;
                    frames.push(JetJsonlFoldFrame::Array(Vec::new()));
                    Ok(())
                }
                jet_std::DataEvent::ObjectStart => {
                    self.charge(&mut retained, 1)?;
                    frames.push(JetJsonlFoldFrame::Object { entries: Vec::new(), key: None });
                    Ok(())
                }
                jet_std::DataEvent::Key(value) => {
                    self.charge(&mut retained, value.len())?;
                    match frames.last_mut() {
                        Some(JetJsonlFoldFrame::Object { key, .. }) if key.is_none() => {
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
        jet_std::DataTree::Int(_) | jet_std::DataTree::Float(_) => 8,
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
        if let Err(mut error) = jet_jsonl_tree_size(&value, 0, "$", &self.json.limits) {
            error.byte_offset = self.json.total;
            error.line = Some(self.record_index + 1);
            error.column = Some(1);
            return self.fail(error);
        }
        if let Err(error) = jet_jsonl_write_tree(&mut self.json, &value) {
            return self.fail(error);
        }
        if !self.json.frames.is_empty() {
            return self.fail(self.json.state_error("JSONL record writer left an open container"));
        }
        if let Err(error) = self.json.write_bytes(b"\n") {
            return self.fail(error);
        }
        self.json.root_written = false;
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
