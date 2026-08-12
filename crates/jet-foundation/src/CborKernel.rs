//! One whole-value CBOR kernel for AOT, JIT, comptime, and interpreter adapters.

use crate::CborBudget::CborAllocationBudget;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub max_depth: i64,
    pub max_items: i64,
    pub max_bytes: i64,
    pub require_canonical: bool,
}

impl Options {
    pub fn safe() -> Self {
        Self {
            max_depth: 256,
            max_items: 1_000_000,
            max_bytes: 1_073_741_824,
            require_canonical: false,
        }
    }

    pub fn from_fields(
        max_depth: Option<i64>,
        max_items: Option<i64>,
        max_bytes: Option<i64>,
        require_canonical: Option<bool>,
    ) -> Result<Self, Error> {
        let safe = Self::safe();
        let options = Self {
            max_depth: max_depth.unwrap_or(safe.max_depth),
            max_items: max_items.unwrap_or(safe.max_items),
            max_bytes: max_bytes.unwrap_or(safe.max_bytes),
            require_canonical: require_canonical.unwrap_or(safe.require_canonical),
        };
        options.validate().map(|()| options)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if !(1..=4096).contains(&self.max_depth) {
            return Err(Error::new(
                ErrorKind::Limit,
                0,
                "$",
                "max_depth must be in 1..4096",
            ));
        }
        if !(1..=1_000_000_000).contains(&self.max_items) {
            return Err(Error::new(
                ErrorKind::Limit,
                0,
                "$",
                "max_items must be in 1..1000000000",
            ));
        }
        if !(0..=1_073_741_824).contains(&self.max_bytes) {
            return Err(Error::new(
                ErrorKind::Limit,
                0,
                "$",
                "max_bytes must be in 0..1073741824",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Syntax,
    Truncated,
    Unsupported,
    Limit,
    TypeMismatch,
    TrailingData,
    NonCanonical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub byte_offset: usize,
    pub path: String,
    pub reason: String,
}

impl Error {
    fn new(
        kind: ErrorKind,
        byte_offset: usize,
        path: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            byte_offset,
            path: path.into(),
            reason: reason.into(),
        }
    }
}

fn push_len(out: &mut Vec<u8>, major: u8, n: u64) {
    if n < 24 {
        out.push((major << 5) | n as u8);
    } else if n <= u8::MAX as u64 {
        out.extend_from_slice(&[(major << 5) | 24, n as u8]);
    } else if n <= u16::MAX as u64 {
        out.push((major << 5) | 25);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else if n <= u32::MAX as u64 {
        out.push((major << 5) | 26);
        out.extend_from_slice(&(n as u32).to_be_bytes());
    } else {
        out.push((major << 5) | 27);
        out.extend_from_slice(&n.to_be_bytes());
    }
}

fn negative_magnitude(value: i64) -> u64 {
    value
        .checked_neg()
        .map(|positive| positive as u64 - 1)
        .unwrap_or(i64::MAX as u64)
}

fn f32_to_half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 255) as i32;
    let fraction = bits & 0x7fffff;
    if exponent == 255 {
        return sign | 0x7c00 | if fraction == 0 { 0 } else { 0x0200 };
    }
    let half_exponent = exponent - 127 + 15;
    if half_exponent >= 31 {
        return sign | 0x7c00;
    }
    if half_exponent <= 0 {
        if half_exponent < -10 {
            return sign;
        }
        let mantissa = fraction | 0x800000;
        let shift = (14 - half_exponent) as u32;
        let mut rounded = mantissa >> shift;
        let remainder = mantissa & ((1u32 << shift) - 1);
        let halfway = 1u32 << (shift - 1);
        if remainder > halfway || (remainder == halfway && rounded & 1 != 0) {
            rounded += 1;
        }
        return sign | rounded as u16;
    }
    let mut rounded = fraction >> 13;
    let remainder = fraction & 0x1fff;
    if remainder > 0x1000 || (remainder == 0x1000 && rounded & 1 != 0) {
        rounded += 1;
    }
    if rounded == 0x0400 {
        return sign | (((half_exponent + 1) as u16) << 10);
    }
    sign | ((half_exponent as u16) << 10) | rounded as u16
}

fn half_to_f64(bits: u16) -> f64 {
    let sign = ((bits >> 15) as u64) << 63;
    let exponent = (bits >> 10) & 31;
    let fraction = bits & 1023;
    if exponent == 0 {
        if fraction == 0 {
            return f64::from_bits(sign);
        }
        let mut mantissa = fraction as u64;
        let mut power = -14i32;
        while mantissa & 1024 == 0 {
            mantissa <<= 1;
            power -= 1;
        }
        mantissa &= 1023;
        f64::from_bits(sign | (((power + 1023) as u64) << 52) | (mantissa << 42))
    } else if exponent == 31 {
        f64::from_bits(sign | (0x7ffu64 << 52) | ((fraction as u64) << 42))
    } else {
        f64::from_bits(
            sign | (((exponent as i32 - 15 + 1023) as u64) << 52) | ((fraction as u64) << 42),
        )
    }
}

fn half_exact(value: f64) -> Option<u16> {
    if value.is_nan() {
        return Some(0x7e00);
    }
    let narrowed = value as f32;
    if (narrowed as f64).to_bits() != value.to_bits() {
        return None;
    }
    let bits = f32_to_half_bits(narrowed);
    (half_to_f64(bits).to_bits() == value.to_bits()).then_some(bits)
}

fn push_preferred_float(out: &mut Vec<u8>, value: f64) {
    if let Some(bits) = half_exact(value) {
        out.push(0xf9);
        out.extend_from_slice(&bits.to_be_bytes());
    } else if ((value as f32) as f64).to_bits() == value.to_bits() {
        out.push(0xfa);
        out.extend_from_slice(&(value as f32).to_bits().to_be_bytes());
    } else {
        out.push(0xfb);
        out.extend_from_slice(&value.to_bits().to_be_bytes());
    }
}

fn encode_value(value: &Value, out: &mut Vec<u8>, canonical: bool) -> Result<(), Error> {
    match value {
        Value::Null => out.push(0xf6),
        Value::Bool(false) => out.push(0xf4),
        Value::Bool(true) => out.push(0xf5),
        Value::Int(value) if *value >= 0 => push_len(out, 0, *value as u64),
        Value::Int(value) => push_len(out, 1, negative_magnitude(*value)),
        Value::Float(value) => push_preferred_float(out, *value),
        Value::Text(value) => {
            push_len(out, 3, value.len() as u64);
            out.extend_from_slice(value.as_bytes());
        }
        Value::Bytes(value) => {
            push_len(out, 2, value.len() as u64);
            out.extend_from_slice(value);
        }
        Value::Array(values) => {
            push_len(out, 4, values.len() as u64);
            for value in values {
                encode_value(value, out, canonical)?;
            }
        }
        Value::Object(entries) => {
            for (index, (key, _)) in entries.iter().enumerate() {
                if entries[..index].iter().any(|(old, _)| old == key) {
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        0,
                        "$",
                        "duplicate encoded CBOR map key",
                    ));
                }
            }
            let mut encoded = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                let mut key_bytes = Vec::new();
                encode_value(&Value::Text(key.clone()), &mut key_bytes, canonical)?;
                let mut value_bytes = Vec::new();
                encode_value(value, &mut value_bytes, canonical)?;
                encoded.push((key_bytes, value_bytes));
            }
            if canonical {
                encoded.sort_by(|left, right| left.0.cmp(&right.0));
            }
            push_len(out, 5, encoded.len() as u64);
            for (key, value) in encoded {
                out.extend_from_slice(&key);
                out.extend_from_slice(&value);
            }
        }
    }
    Ok(())
}

pub fn encode(value: &Value, canonical: bool) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    encode_value(value, &mut out, canonical)?;
    Ok(out)
}

struct ErrorLength(usize);

impl std::fmt::Write for ErrorLength {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0 = self.0.saturating_add(value.len());
        Ok(())
    }
}

fn error_with_budget(
    budget: &mut CborAllocationBudget,
    kind: ErrorKind,
    byte_offset: usize,
    path: &str,
    arguments: std::fmt::Arguments<'_>,
) -> Error {
    let mut length = ErrorLength(0);
    let _ = std::fmt::Write::write_fmt(&mut length, arguments);
    let requested = path.len().checked_add(length.0);
    if let Some(requested) = requested {
        if budget.reserve(requested, 1, "CBOR error").is_err() {
            budget.reserve_terminal_error(requested);
        }
    }
    let mut reason = String::with_capacity(length.0);
    let _ = std::fmt::Write::write_fmt(&mut reason, arguments);
    Error {
        kind,
        byte_offset,
        path: path.to_string(),
        reason,
    }
}

macro_rules! cbor_error {
    ($budget:expr, $kind:expr, $offset:expr, $path:expr, $reason:expr $(,)?) => {
        error_with_budget($budget, $kind, $offset, $path, format_args!("{}", $reason))
    };
}

fn read_len(
    input: &[u8],
    cursor: &mut usize,
    additional: u8,
    start: usize,
    canonical: bool,
    budget: &mut CborAllocationBudget,
    path: &str,
) -> Result<u64, Error> {
    let need = match additional {
        n @ 0..=23 => return Ok(n as u64),
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => {
            return Err(cbor_error!(
                budget,
                ErrorKind::Unsupported,
                start,
                path,
                "indefinite/reserved CBOR length is unsupported by whole-value decoding",
            ))
        }
    };
    if *cursor + need > input.len() {
        return Err(cbor_error!(
            budget,
            ErrorKind::Truncated,
            input.len(),
            path,
            "CBOR length argument is truncated",
        ));
    }
    let mut value = 0u64;
    for _ in 0..need {
        value = (value << 8) | input[*cursor] as u64;
        *cursor += 1;
    }
    if canonical
        && ((additional == 24 && value < 24)
            || (additional == 25 && value <= u8::MAX as u64)
            || (additional == 26 && value <= u16::MAX as u64)
            || (additional == 27 && value <= u32::MAX as u64))
    {
        return Err(cbor_error!(
            budget,
            ErrorKind::NonCanonical,
            start,
            path,
            "CBOR argument does not use its shortest form",
        ));
    }
    Ok(value)
}

fn budget_error(
    budget: &mut CborAllocationBudget,
    error: crate::CborBudget::CborAllocationError,
    offset: usize,
    path: &str,
) -> Error {
    let requested = path.len().checked_add(error.reason_len());
    if let Some(requested) = requested {
        if budget.reserve(requested, 1, "CBOR error").is_err() {
            budget.reserve_terminal_error(requested);
        }
    }
    let mut reason = String::with_capacity(error.reason_len());
    error.write_reason(&mut reason);
    Error {
        kind: ErrorKind::Limit,
        byte_offset: offset,
        path: path.to_string(),
        reason,
    }
}

fn reserve_vec<T>(
    budget: &mut CborAllocationBudget,
    values: &mut Vec<T>,
    additional: usize,
    unit: usize,
    offset: usize,
    path: &str,
    what: &'static str,
) -> Result<(), Error> {
    budget
        .reserve_vec(values, additional, unit, what)
        .map_err(|error| budget_error(budget, error, offset, path))
}

fn index_path(
    path: &str,
    index: usize,
    budget: &mut CborAllocationBudget,
    offset: usize,
) -> Result<(String, usize), Error> {
    let mut value = index;
    let mut digit_count = 1usize;
    while value >= 10 {
        value /= 10;
        digit_count += 1;
    }
    let capacity = path
        .len()
        .checked_add(digit_count)
        .and_then(|value| value.checked_add(2))
        .ok_or_else(|| {
            cbor_error!(
                budget,
                ErrorKind::Limit,
                offset,
                path,
                "CBOR path allocation exceeds target capacity",
            )
        })?;
    let charged = budget
        .reserve(capacity, 1, "CBOR path")
        .map_err(|error| budget_error(budget, error, offset, path))?;
    let mut output = String::with_capacity(capacity);
    output.push_str(path);
    output.push('[');
    use std::fmt::Write as _;
    let _ = write!(output, "{index}");
    output.push(']');
    Ok((output, charged))
}

fn key_path(
    path: &str,
    key: &str,
    budget: &mut CborAllocationBudget,
    offset: usize,
) -> Result<(String, usize), Error> {
    let escaped = key
        .chars()
        .map(|value| {
            value
                .escape_debug()
                .map(|part| part.len_utf8())
                .sum::<usize>()
        })
        .sum::<usize>();
    let capacity = path
        .len()
        .checked_add(escaped)
        .and_then(|value| value.checked_add(4))
        .ok_or_else(|| {
            cbor_error!(
                budget,
                ErrorKind::Limit,
                offset,
                path,
                "CBOR path allocation exceeds target capacity",
            )
        })?;
    let charged = budget
        .reserve(capacity, 1, "CBOR path")
        .map_err(|error| budget_error(budget, error, offset, path))?;
    let mut output = String::with_capacity(capacity);
    output.push_str(path);
    output.push('[');
    output.push('"');
    for value in key.chars() {
        output.extend(value.escape_debug());
    }
    output.push('"');
    output.push(']');
    Ok((output, charged))
}

fn count_item(
    items: &mut i64,
    options: &Options,
    budget: &mut CborAllocationBudget,
    offset: usize,
    path: &str,
) -> Result<(), Error> {
    *items = items.checked_add(1).ok_or_else(|| {
        cbor_error!(
            budget,
            ErrorKind::Limit,
            offset,
            path,
            "max_items counter overflow"
        )
    })?;
    if *items > options.max_items {
        return Err(cbor_error!(
            budget,
            ErrorKind::Limit,
            offset,
            path,
            format_args!("max_items {} exceeded", options.max_items),
        ));
    }
    Ok(())
}

fn indefinite(
    options: &Options,
    budget: &mut CborAllocationBudget,
    offset: usize,
    path: &str,
) -> Result<(), Error> {
    if options.require_canonical {
        Err(cbor_error!(
            budget,
            ErrorKind::NonCanonical,
            offset,
            path,
            "indefinite-length CBOR is not Core deterministic",
        ))
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_indefinite_string(
    input: &[u8],
    cursor: &mut usize,
    options: &Options,
    budget: &mut CborAllocationBudget,
    depth: i64,
    items: &mut i64,
    path: &str,
    major: u8,
    start: usize,
    allow_bytes: bool,
) -> Result<Value, Error> {
    indefinite(options, budget, start, path)?;
    if depth + 1 > options.max_depth {
        return Err(cbor_error!(
            budget,
            ErrorKind::Limit,
            start,
            path,
            format_args!("max_depth {} exceeded", options.max_depth),
        ));
    }
    if major == 2 && !allow_bytes {
        return Err(cbor_error!(
            budget,
            ErrorKind::Unsupported,
            start,
            path,
            "CBOR byte strings are outside core.encoding.Data; use decode<[U8]>",
        ));
    }
    let mut bytes = Vec::new();
    loop {
        if *cursor >= input.len() {
            return Err(cbor_error!(
                budget,
                ErrorKind::Truncated,
                input.len(),
                path,
                "indefinite CBOR string ended before its break",
            ));
        }
        if input[*cursor] == 0xff {
            *cursor += 1;
            break;
        }
        let chunk_start = *cursor;
        let head = input[*cursor];
        *cursor += 1;
        let chunk_major = head >> 5;
        let chunk_additional = head & 31;
        count_item(items, options, budget, chunk_start, path)?;
        if chunk_major != major || chunk_additional == 31 {
            return Err(cbor_error!(
                budget,
                ErrorKind::Syntax,
                chunk_start,
                path,
                "indefinite CBOR string contains a wrong or indefinite chunk",
            ));
        }
        let length = usize::try_from(read_len(
            input,
            cursor,
            chunk_additional,
            chunk_start,
            false,
            budget,
            path,
        )?)
        .map_err(|_| {
            cbor_error!(
                budget,
                ErrorKind::Limit,
                chunk_start,
                path,
                "CBOR string chunk length exceeds target capacity",
            )
        })?;
        if length > input.len() - *cursor {
            return Err(cbor_error!(
                budget,
                ErrorKind::Truncated,
                input.len(),
                path,
                "CBOR byte/text string chunk is truncated",
            ));
        }
        if major == 3 && std::str::from_utf8(&input[*cursor..*cursor + length]).is_err() {
            return Err(cbor_error!(
                budget,
                ErrorKind::Syntax,
                chunk_start,
                path,
                "CBOR text chunk is not UTF-8",
            ));
        }
        reserve_vec(
            budget,
            &mut bytes,
            length,
            1,
            chunk_start,
            path,
            if major == 2 {
                "CBOR byte string"
            } else {
                "CBOR text string"
            },
        )?;
        bytes.extend_from_slice(&input[*cursor..*cursor + length]);
        *cursor += length;
    }
    if major == 2 {
        Ok(Value::Bytes(bytes))
    } else {
        String::from_utf8(bytes).map(Value::Text).map_err(|_| {
            cbor_error!(
                budget,
                ErrorKind::Syntax,
                start,
                path,
                "CBOR text is not UTF-8"
            )
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_value(
    input: &[u8],
    cursor: &mut usize,
    options: &Options,
    budget: &mut CborAllocationBudget,
    depth: i64,
    items: &mut i64,
    path: &str,
    allow_bytes: bool,
) -> Result<Value, Error> {
    if *cursor >= input.len() {
        return Err(cbor_error!(
            budget,
            ErrorKind::Truncated,
            input.len(),
            path,
            "CBOR value is missing",
        ));
    }
    let start = *cursor;
    let head = input[*cursor];
    *cursor += 1;
    count_item(items, options, budget, start, path)?;
    let major = head >> 5;
    let additional = head & 31;
    match major {
        0 => i64::try_from(read_len(
            input,
            cursor,
            additional,
            start,
            options.require_canonical,
            budget,
            path,
        )?)
        .map(Value::Int)
        .map_err(|_| {
            cbor_error!(
                budget,
                ErrorKind::Unsupported,
                start,
                path,
                "CBOR integer is outside Jet Int",
            )
        }),
        1 => i64::try_from(read_len(
            input,
            cursor,
            additional,
            start,
            options.require_canonical,
            budget,
            path,
        )?)
        .ok()
        .and_then(|value| value.checked_neg()?.checked_sub(1))
        .map(Value::Int)
        .ok_or_else(|| {
            cbor_error!(
                budget,
                ErrorKind::Unsupported,
                start,
                path,
                "CBOR integer is outside Jet Int",
            )
        }),
        2 | 3 => {
            if additional == 31 {
                return decode_indefinite_string(
                    input,
                    cursor,
                    options,
                    budget,
                    depth,
                    items,
                    path,
                    major,
                    start,
                    allow_bytes,
                );
            }
            let length = usize::try_from(read_len(
                input,
                cursor,
                additional,
                start,
                options.require_canonical,
                budget,
                path,
            )?)
            .map_err(|_| {
                cbor_error!(
                    budget,
                    ErrorKind::Limit,
                    start,
                    path,
                    "CBOR string length exceeds target capacity",
                )
            })?;
            if length > input.len() - *cursor {
                return Err(cbor_error!(
                    budget,
                    ErrorKind::Truncated,
                    input.len(),
                    path,
                    "CBOR byte/text string is truncated",
                ));
            }
            if major == 2 && !allow_bytes {
                return Err(cbor_error!(
                    budget,
                    ErrorKind::Unsupported,
                    start,
                    path,
                    "CBOR byte strings are outside core.encoding.Data; use decode<[U8]>",
                ));
            }
            let mut bytes = Vec::new();
            reserve_vec(
                budget,
                &mut bytes,
                length,
                1,
                start,
                path,
                if major == 2 {
                    "CBOR byte string"
                } else {
                    "CBOR text string"
                },
            )?;
            bytes.extend_from_slice(&input[*cursor..*cursor + length]);
            *cursor += length;
            if major == 2 {
                Ok(Value::Bytes(bytes))
            } else {
                String::from_utf8(bytes).map(Value::Text).map_err(|_| {
                    cbor_error!(
                        budget,
                        ErrorKind::Syntax,
                        start,
                        path,
                        "CBOR text is not UTF-8"
                    )
                })
            }
        }
        4 => {
            if depth + 1 > options.max_depth {
                return Err(cbor_error!(
                    budget,
                    ErrorKind::Limit,
                    start,
                    path,
                    format_args!("max_depth {} exceeded", options.max_depth),
                ));
            }
            let mut values = Vec::new();
            if additional == 31 {
                indefinite(options, budget, start, path)?;
                let mut index = 0usize;
                loop {
                    if *cursor >= input.len() {
                        return Err(cbor_error!(
                            budget,
                            ErrorKind::Truncated,
                            input.len(),
                            path,
                            "indefinite CBOR array ended before its break",
                        ));
                    }
                    if input[*cursor] == 0xff {
                        *cursor += 1;
                        break;
                    }
                    let (child_path, charged) = index_path(path, index, budget, *cursor)?;
                    if *items >= options.max_items {
                        let error = cbor_error!(
                            budget,
                            ErrorKind::Limit,
                            *cursor,
                            &child_path,
                            format_args!("max_items {} exceeded", options.max_items),
                        );
                        budget.release(charged);
                        return Err(error);
                    }
                    reserve_vec(
                        budget,
                        &mut values,
                        1,
                        crate::CborBudget::DATA_TREE_SLOT_BYTES,
                        *cursor,
                        &child_path,
                        "CBOR array",
                    )?;
                    let child = decode_value(
                        input,
                        cursor,
                        options,
                        budget,
                        depth + 1,
                        items,
                        &child_path,
                        allow_bytes,
                    );
                    budget.release(charged);
                    values.push(child?);
                    index += 1;
                }
            } else {
                let length = usize::try_from(read_len(
                    input,
                    cursor,
                    additional,
                    start,
                    options.require_canonical,
                    budget,
                    path,
                )?)
                .map_err(|_| {
                    cbor_error!(
                        budget,
                        ErrorKind::Limit,
                        start,
                        path,
                        "CBOR array length exceeds target capacity",
                    )
                })?;
                reserve_vec(
                    budget,
                    &mut values,
                    length,
                    crate::CborBudget::DATA_TREE_SLOT_BYTES,
                    start,
                    path,
                    "CBOR array",
                )?;
                for index in 0..length {
                    let (child_path, charged) = index_path(path, index, budget, start)?;
                    let child = decode_value(
                        input,
                        cursor,
                        options,
                        budget,
                        depth + 1,
                        items,
                        &child_path,
                        allow_bytes,
                    );
                    budget.release(charged);
                    values.push(child?);
                }
            }
            Ok(Value::Array(values))
        }
        5 => {
            if depth + 1 > options.max_depth {
                return Err(cbor_error!(
                    budget,
                    ErrorKind::Limit,
                    start,
                    path,
                    format_args!("max_depth {} exceeded", options.max_depth),
                ));
            }
            let mut entries = Vec::new();
            let length = if additional == 31 {
                indefinite(options, budget, start, path)?;
                None
            } else {
                Some(
                    usize::try_from(read_len(
                        input,
                        cursor,
                        additional,
                        start,
                        options.require_canonical,
                        budget,
                        path,
                    )?)
                    .map_err(|_| {
                        cbor_error!(
                            budget,
                            ErrorKind::Limit,
                            start,
                            path,
                            "CBOR map length exceeds target capacity",
                        )
                    })?,
                )
            };
            if let Some(length) = length {
                reserve_vec(
                    budget,
                    &mut entries,
                    length,
                    crate::CborBudget::MAP_ENTRY_SLOT_BYTES,
                    start,
                    path,
                    "CBOR map",
                )?;
            }
            let mut prior_key: Option<(usize, usize)> = None;
            let mut index = 0usize;
            loop {
                if let Some(length) = length {
                    if index == length {
                        break;
                    }
                } else {
                    if *cursor >= input.len() {
                        return Err(cbor_error!(
                            budget,
                            ErrorKind::Truncated,
                            input.len(),
                            path,
                            "indefinite CBOR map ended before its break",
                        ));
                    }
                    if input[*cursor] == 0xff {
                        *cursor += 1;
                        break;
                    }
                    if *items >= options.max_items {
                        return Err(cbor_error!(
                            budget,
                            ErrorKind::Limit,
                            *cursor,
                            path,
                            format_args!("max_items {} exceeded", options.max_items),
                        ));
                    }
                    reserve_vec(
                        budget,
                        &mut entries,
                        1,
                        crate::CborBudget::MAP_ENTRY_SLOT_BYTES,
                        *cursor,
                        path,
                        "CBOR map",
                    )?;
                }
                let key_start = *cursor;
                let key_value = decode_value(
                    input,
                    cursor,
                    options,
                    budget,
                    depth + 1,
                    items,
                    path,
                    false,
                )?;
                let key = match key_value {
                    Value::Text(value) => value,
                    _ => {
                        return Err(cbor_error!(
                            budget,
                            ErrorKind::Unsupported,
                            key_start,
                            path,
                            "CBOR map key must be text",
                        ))
                    }
                };
                let key_end = *cursor;
                if options.require_canonical
                    && prior_key.is_some_and(|(old_start, old_end)| {
                        input[old_start..old_end] >= input[key_start..key_end]
                    })
                {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::NonCanonical,
                        key_start,
                        path,
                        "CBOR map keys are not in Core deterministic bytewise order",
                    ));
                }
                if entries.iter().any(|(old, _)| old == &key) {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::Unsupported,
                        key_start,
                        path,
                        "duplicate CBOR text map key",
                    ));
                }
                prior_key = Some((key_start, key_end));
                if length.is_none() && *cursor >= input.len() {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::Truncated,
                        input.len(),
                        path,
                        "indefinite CBOR map ended before its value",
                    ));
                }
                if length.is_none() && input.get(*cursor) == Some(&0xff) {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::Syntax,
                        *cursor,
                        path,
                        "indefinite CBOR map break appears where a value is required",
                    ));
                }
                let (child_path, charged) = key_path(path, &key, budget, key_start)?;
                let value = decode_value(
                    input,
                    cursor,
                    options,
                    budget,
                    depth + 1,
                    items,
                    &child_path,
                    allow_bytes,
                );
                budget.release(charged);
                entries.push((key, value?));
                index += 1;
            }
            Ok(Value::Object(entries))
        }
        6 => Err(cbor_error!(
            budget,
            ErrorKind::Unsupported,
            start,
            path,
            "CBOR tags are unsupported",
        )),
        7 => match additional {
            20 => Ok(Value::Bool(false)),
            21 => Ok(Value::Bool(true)),
            22 => Ok(Value::Null),
            25 => {
                if *cursor + 2 > input.len() {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::Truncated,
                        input.len(),
                        path,
                        "CBOR Float16 is truncated",
                    ));
                }
                let bits = u16::from_be_bytes([input[*cursor], input[*cursor + 1]]);
                *cursor += 2;
                let value = half_to_f64(bits);
                if options.require_canonical && value.is_nan() && bits != 0x7e00 {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::NonCanonical,
                        start,
                        path,
                        "CBOR NaN is not the canonical 0xf97e00 encoding",
                    ));
                }
                Ok(Value::Float(value))
            }
            26 => {
                if *cursor + 4 > input.len() {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::Truncated,
                        input.len(),
                        path,
                        "CBOR Float32 is truncated",
                    ));
                }
                let mut bytes = [0u8; 4];
                bytes.copy_from_slice(&input[*cursor..*cursor + 4]);
                *cursor += 4;
                let value = f32::from_be_bytes(bytes) as f64;
                if options.require_canonical && (value.is_nan() || half_exact(value).is_some()) {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::NonCanonical,
                        start,
                        path,
                        "CBOR Float does not use its preferred shortest encoding",
                    ));
                }
                Ok(Value::Float(value))
            }
            27 => {
                if *cursor + 8 > input.len() {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::Truncated,
                        input.len(),
                        path,
                        "CBOR Float64 is truncated",
                    ));
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&input[*cursor..*cursor + 8]);
                *cursor += 8;
                let value = f64::from_be_bytes(bytes);
                if options.require_canonical
                    && (value.is_nan()
                        || half_exact(value).is_some()
                        || ((value as f32) as f64).to_bits() == value.to_bits())
                {
                    return Err(cbor_error!(
                        budget,
                        ErrorKind::NonCanonical,
                        start,
                        path,
                        "CBOR Float does not use its preferred shortest encoding",
                    ));
                }
                Ok(Value::Float(value))
            }
            31 => Err(cbor_error!(
                budget,
                ErrorKind::Syntax,
                start,
                path,
                "CBOR break outside an indefinite container",
            )),
            _ => Err(cbor_error!(
                budget,
                ErrorKind::Unsupported,
                start,
                path,
                format_args!("unsupported CBOR simple value {additional}"),
            )),
        },
        _ => Err(cbor_error!(
            budget,
            ErrorKind::Unsupported,
            start,
            path,
            format_args!("unsupported CBOR major type {major}"),
        )),
    }
}

pub fn decode(bytes: &[u8], options: &Options, allow_bytes: bool) -> Result<Value, Error> {
    options.validate()?;
    let mut budget = CborAllocationBudget::new(options.max_bytes as usize);
    if bytes.len() as i64 > options.max_bytes {
        return Err(cbor_error!(
            &mut budget,
            ErrorKind::Limit,
            0,
            "$",
            format_args!("input exceeds max_bytes {}", options.max_bytes),
        ));
    }
    let mut cursor = 0usize;
    let mut items = 0i64;
    let value = decode_value(
        bytes,
        &mut cursor,
        options,
        &mut budget,
        0,
        &mut items,
        "$",
        allow_bytes,
    )?;
    if cursor != bytes.len() {
        return Err(cbor_error!(
            &mut budget,
            ErrorKind::TrailingData,
            cursor,
            "$",
            "trailing CBOR data after root value",
        ));
    }
    Ok(value)
}
