// One Component Model value wire for AOT, JIT, and interpreter adapters.

#[derive(Debug, Clone)]
pub enum PluginValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    List(Vec<PluginValue>),
    Record(Vec<(String, PluginValue)>),
    Tuple(Vec<PluginValue>),
    Option(Option<Box<PluginValue>>),
    ResultOk(Option<Box<PluginValue>>),
    ResultErr(Option<Box<PluginValue>>),
    Variant(String, Option<Box<PluginValue>>),
    Enum(String),
    Flags(Vec<String>),
}

use std::fmt::Write as _;

pub const PLUGIN_MAX_PARAMS: usize = 1024;
const PLUGIN_MAX_VALUE_DEPTH: usize = 64;

fn plugin_write_tagged(out: &mut String, tag: char, payload: &str) {
    let _ = write!(out, "{tag}{}:", payload.len());
    out.push_str(payload);
}

fn plugin_write_value(out: &mut String, value: &PluginValue) {
    match value {
        PluginValue::Int(value) => plugin_write_tagged(out, 'I', &value.to_string()),
        PluginValue::Float(value) => plugin_write_tagged(out, 'F', &value.to_string()),
        PluginValue::Bool(value) => plugin_write_tagged(out, 'B', if *value { "true" } else { "false" }),
        PluginValue::Text(value) => plugin_write_tagged(out, 'T', value),
        PluginValue::List(values) | PluginValue::Tuple(values) => {
            let tag = if matches!(value, PluginValue::List(_)) { 'L' } else { 'Q' };
            let _ = write!(out, "{tag}{}:", values.len());
            for value in values { plugin_write_value(out, value); }
        }
        PluginValue::Record(values) => {
            let _ = write!(out, "R{}:", values.len());
            for (name, value) in values {
                plugin_write_tagged(out, 'F', name);
                plugin_write_value(out, value);
            }
        }
        PluginValue::Option(None) => out.push('N'),
        PluginValue::Option(Some(value)) => {
            out.push('P');
            plugin_write_value(out, value);
        }
        PluginValue::ResultOk(payload) | PluginValue::ResultErr(payload) => {
            out.push(if matches!(value, PluginValue::ResultOk(_)) { 'K' } else { 'X' });
            match payload {
                Some(value) => { out.push('P'); plugin_write_value(out, value); }
                None => out.push('N'),
            }
        }
        PluginValue::Variant(name, value) => {
            plugin_write_tagged(out, 'V', name);
            match value {
                Some(value) => { out.push('P'); plugin_write_value(out, value); }
                None => out.push('N'),
            }
        }
        PluginValue::Enum(name) => plugin_write_tagged(out, 'E', name),
        PluginValue::Flags(names) => {
            let _ = write!(out, "G{}:", names.len());
            for name in names { plugin_write_tagged(out, 'F', name); }
        }
    }
}

pub fn plugin_encode_value(value: &PluginValue) -> String {
    let mut out = String::new();
    plugin_write_value(&mut out, value);
    out
}

pub fn plugin_encode_params(values: &[PluginValue]) -> String {
    let mut out = String::new();
    let _ = write!(out, "{}:", values.len());
    for value in values { plugin_write_value(&mut out, value); }
    out
}

pub fn plugin_decode_result(wire: &str) -> Result<PluginValue, String> {
    if let Some(message) = wire.strip_prefix("E:") {
        return Err(message.to_owned());
    }
    let body = wire.strip_prefix("O:")
        .ok_or_else(|| "plugin returned malformed component value".to_owned())?;
    plugin_decode_value(body.as_bytes())
        .map_err(|_| "plugin returned malformed component value".to_owned())
}

fn plugin_read_number(bytes: &[u8], pos: &mut usize) -> Option<usize> {
    let start = *pos;
    while let Some(byte) = bytes.get(*pos) {
        if *byte == b':' {
            let value = std::str::from_utf8(bytes.get(start..*pos)?)
                .ok()?
                .parse::<usize>()
                .ok()?;
            *pos += 1;
            return Some(value);
        }
        if !byte.is_ascii_digit() {
            return None;
        }
        *pos += 1;
    }
    None
}

fn plugin_read_payload<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<&'a str> {
    let len = plugin_read_number(bytes, pos)?;
    let end = (*pos).checked_add(len)?;
    let payload = std::str::from_utf8(bytes.get(*pos..end)?).ok()?;
    *pos = end;
    Some(payload)
}

fn plugin_read_value(
    bytes: &[u8],
    pos: &mut usize,
    depth: usize,
) -> Option<PluginValue> {
    if depth > PLUGIN_MAX_VALUE_DEPTH {
        return None;
    }
    let tag = *bytes.get(*pos)? as char;
    *pos += 1;
    match tag {
        'I' => Some(PluginValue::Int(plugin_read_payload(bytes, pos)?.parse().ok()?)),
        'F' => Some(PluginValue::Float(plugin_read_payload(bytes, pos)?.parse().ok()?)),
        'B' => Some(PluginValue::Bool(match plugin_read_payload(bytes, pos)? {
            "true" => true,
            "false" => false,
            _ => return None,
        })),
        'T' => Some(PluginValue::Text(plugin_read_payload(bytes, pos)?.to_owned())),
        'L' | 'Q' => {
            let count = plugin_read_number(bytes, pos)?;
            if count > bytes.len() - *pos {
                return None;
            }
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(plugin_read_value(bytes, pos, depth + 1)?);
            }
            if tag == 'L' {
                Some(PluginValue::List(values))
            } else {
                Some(PluginValue::Tuple(values))
            }
        }
        'R' => {
            let count = plugin_read_number(bytes, pos)?;
            if count > (bytes.len() - *pos) / 4 {
                return None;
            }
            let mut fields = Vec::with_capacity(count);
            for _ in 0..count {
                if *bytes.get(*pos)? as char != 'F' {
                    return None;
                }
                *pos += 1;
                let name = plugin_read_payload(bytes, pos)?.to_owned();
                let value = plugin_read_value(bytes, pos, depth + 1)?;
                fields.push((name, value));
            }
            Some(PluginValue::Record(fields))
        }
        'N' => Some(PluginValue::Option(None)),
        'P' => Some(PluginValue::Option(Some(Box::new(plugin_read_value(
            bytes,
            pos,
            depth + 1,
        )?)))),
        'K' | 'X' => {
            let marker = *bytes.get(*pos)? as char;
            *pos += 1;
            let value = match marker {
                'N' => None,
                'P' => Some(Box::new(plugin_read_value(bytes, pos, depth + 1)?)),
                _ => return None,
            };
            if tag == 'K' {
                Some(PluginValue::ResultOk(value))
            } else {
                Some(PluginValue::ResultErr(value))
            }
        }
        'V' => {
            let name = plugin_read_payload(bytes, pos)?.to_owned();
            let marker = *bytes.get(*pos)? as char;
            *pos += 1;
            let value = match marker {
                'N' => None,
                'P' => Some(Box::new(plugin_read_value(bytes, pos, depth + 1)?)),
                _ => return None,
            };
            Some(PluginValue::Variant(name, value))
        }
        'E' => Some(PluginValue::Enum(plugin_read_payload(bytes, pos)?.to_owned())),
        'G' => {
            let count = plugin_read_number(bytes, pos)?;
            if count > (bytes.len() - *pos) / 3 {
                return None;
            }
            let mut names = Vec::with_capacity(count);
            for _ in 0..count {
                if *bytes.get(*pos)? as char != 'F' {
                    return None;
                }
                *pos += 1;
                names.push(plugin_read_payload(bytes, pos)?.to_owned());
            }
            Some(PluginValue::Flags(names))
        }
        _ => None,
    }
}

pub fn plugin_decode_value(bytes: &[u8]) -> Result<PluginValue, PluginParamDecodeError> {
    let mut pos = 0;
    let value = plugin_read_value(bytes, &mut pos, 0).ok_or(PluginParamDecodeError::Malformed)?;
    if pos == bytes.len() {
        Ok(value)
    } else {
        Err(PluginParamDecodeError::Malformed)
    }
}

#[derive(Debug)]
pub enum PluginParamDecodeError {
    Malformed,
    TooMany { count: usize },
}

impl std::fmt::Display for PluginParamDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => f.write_str("malformed plugin call argument frame"),
            Self::TooMany { count } => write!(
                f,
                "plugin call argument frame contains {count} parameters; maximum is {PLUGIN_MAX_PARAMS}"
            ),
        }
    }
}

/// Decode a count-prefixed recursive component-value list:
/// `"<count>:<value>…"`. Every byte belongs to one declared value.
pub fn plugin_decode_params(wire: &str) -> Result<Vec<PluginValue>, PluginParamDecodeError> {
    let bytes = wire.as_bytes();
    let mut pos = 0;
    let count = plugin_read_number(bytes, &mut pos).ok_or(PluginParamDecodeError::Malformed)?;
    if count > PLUGIN_MAX_PARAMS {
        return Err(PluginParamDecodeError::TooMany { count });
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(plugin_read_value(bytes, &mut pos, 0).ok_or(PluginParamDecodeError::Malformed)?);
    }
    if pos != bytes.len() {
        return Err(PluginParamDecodeError::Malformed);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_list_is_one_parameter() {
        let count = PLUGIN_MAX_PARAMS * 2;
        let argument = PluginValue::List(
            (0..count).map(|value| PluginValue::Int(value as i64)).collect(),
        );
        let decoded = plugin_decode_params(&plugin_encode_params(&[argument])).unwrap();
        let [PluginValue::List(values)] = decoded.as_slice() else {
            panic!("the component list did not survive argument decoding");
        };
        assert_eq!(values.len(), count);
        assert!(values.iter().enumerate().all(|(index, value)| {
            matches!(value, PluginValue::Int(value) if *value == index as i64)
        }));
    }

    #[test]
    fn collection_counts_cannot_allocate_beyond_the_frame() {
        for frame in ["L18446744073709551615:", "R999999:", "G999999:"] {
            assert!(matches!(
                plugin_decode_value(frame.as_bytes()),
                Err(PluginParamDecodeError::Malformed)
            ));
        }
    }
}
