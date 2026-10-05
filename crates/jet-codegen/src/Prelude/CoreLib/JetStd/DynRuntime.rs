// Dyn surface operations. Engines without this surface deopt to AOT; this
// fragment uses the existing exact-Int and time payload boundaries.
#[derive(Clone, Debug)]
pub enum JetDynStep { Key(String), At(i64) }

fn dyn_stop(site: &str, detail: impl AsRef<str>) -> String {
    super::jet_dyn_stop_message(site, detail.as_ref())
}
fn dyn_expected(site: &str, expected: &str, value: &Dyn) -> String {
    dyn_stop(site, format!("expected {expected}, found {}", dyn_kind_for(value)))
}

pub fn jet_dyn_index_key(tree: &Dyn, key: &str, site: &str) -> Result<Dyn, String> {
    match tree {
        Dyn::Object(entries) => entries.iter().find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| dyn_stop(site, format!("no key {key:?}"))),
        other => Err(dyn_expected(site, "an object", other)),
    }
}
pub fn jet_dyn_index_at(tree: &Dyn, index: i64, site: &str) -> Result<Dyn, String> {
    match tree {
        Dyn::Array(items) => usize::try_from(index).ok().and_then(|i| items.get(i)).cloned()
            .ok_or_else(|| dyn_stop(site, format!("index {index} is out of range (length {})", items.len()))),
        other => Err(dyn_expected(site, "an array", other)),
    }
}
pub fn jet_dyn_get_key(tree: &Dyn, key: &str) -> super::JetOutcome<Dyn, super::JetAbsent> {
    tree.get_opt(key).cloned().ok_or(super::JetAbsent)
}
pub fn jet_dyn_get_at(tree: &Dyn, index: i64) -> super::JetOutcome<Dyn, super::JetAbsent> {
    match tree {
        Dyn::Array(items) => usize::try_from(index).ok()
            .and_then(|i| items.get(i)).cloned().ok_or(super::JetAbsent),
        _ => Err(super::JetAbsent),
    }
}
pub fn jet_dyn_has(tree: &Dyn, key: &str) -> bool { tree.get_opt(key).is_some() }
pub fn jet_dyn_keys(tree: &Dyn) -> Vec<String> {
    match tree { Dyn::Object(entries) => entries.iter().map(|(key, _)| key.clone()).collect(), _ => Vec::new() }
}
pub fn jet_dyn_len(tree: &Dyn, site: &str) -> Result<i64, String> {
    let len = match tree {
        Dyn::Array(items) => items.len(), Dyn::Object(entries) => entries.len(),
        Dyn::Text(text) => text.chars().count(), Dyn::Bytes(bytes) => bytes.len(),
        other => return Err(dyn_stop(site, format!("{} has no length", dyn_kind_for(other)))),
    };
    Ok(len as i64)
}
pub fn jet_dyn_remove(tree: &mut Dyn, key: &str) -> super::JetOutcome<Dyn, super::JetAbsent> {
    match tree {
        Dyn::Object(entries) => entries.iter().position(|(name, _)| name == key)
            .map(|i| entries.remove(i).1).ok_or(super::JetAbsent),
        _ => Err(super::JetAbsent),
    }
}
pub fn jet_dyn_set_path(tree: &mut Dyn, steps: &[JetDynStep], value: Dyn, site: &str) -> Result<(), String> {
    let Some((step, rest)) = steps.split_first() else { *tree = value; return Ok(()); };
    match step {
        JetDynStep::Key(key) => match tree {
            Dyn::Object(entries) => {
                if let Some(i) = entries.iter().position(|(name, _)| name == key) {
                    jet_dyn_set_path(&mut entries[i].1, rest, value, site)
                } else if rest.is_empty() {
                    entries.push((key.clone(), value)); Ok(())
                } else { Err(dyn_stop(site, format!("no key {key:?}"))) }
            }
            other => Err(dyn_expected(site, "an object", other)),
        },
        JetDynStep::At(index) => match tree {
            Dyn::Array(items) => {
                let len = items.len();
                let item = usize::try_from(*index).ok().and_then(|i| items.get_mut(i))
                    .ok_or_else(|| dyn_stop(site, format!("index {index} is out of range (length {len})")))?;
                jet_dyn_set_path(item, rest, value, site)
            }
            other => Err(dyn_expected(site, "an array", other)),
        },
    }
}
fn dyn_integer(tree: &Dyn) -> Option<jet_foundation::Numeric::JetInt> {
    jet_dyn_int_payload(tree)
}
pub fn jet_dyn_binary(op: &str, left: &Dyn, right: &Dyn, site: &str) -> Result<Dyn, String> {
    let left_int = dyn_integer(left);
    let right_int = dyn_integer(right);
    if let (Some(a), Some(b)) = (left_int.as_ref(), right_int.as_ref()) {
        if op == "/" {
            if jet_int_is_zero(b.to_raw()) { return Err(dyn_stop(site, "division by zero")); }
            if a.is_inline() && b.is_inline() && a.to_raw().unsigned_abs() <= (1u64 << 53)
                && b.to_raw().unsigned_abs() <= (1u64 << 53) {
                return Ok(Dyn::Float(a.to_raw() as f64 / b.to_raw() as f64));
            }
            return Ok(Dyn::Float(jet_int_value(a.to_raw()).ratio_to_f64(&jet_int_value(b.to_raw()))));
        }
        let value = match op {
            "+" => jet_int_add(a.to_raw(), b.to_raw()),
            "-" => jet_int_sub(a.to_raw(), b.to_raw()),
            "*" => jet_int_mul(a.to_raw(), b.to_raw()),
            _ => return Err(dyn_stop(site, format!("unknown Dyn operator {op}"))),
        };
        let value = jet_int_owned_from_raw_result(value);
        return Ok(Dyn::Int(value));
    }
    let a = match left { Dyn::Float(value) => Some(*value), _ => left_int.as_ref().map(jet_int_owned_to_f64) };
    let b = match right { Dyn::Float(value) => Some(*value), _ => right_int.as_ref().map(jet_int_owned_to_f64) };
    if let (Some(a), Some(b)) = (a, b) {
        let value = match op {
            "+" => a + b, "-" => a - b, "*" => a * b,
            "/" if b == 0.0 => return Err(dyn_stop(site, "division by zero")),
            "/" => a / b, _ => return Err(dyn_stop(site, format!("unknown Dyn operator {op}"))),
        };
        return Ok(Dyn::Float(value));
    }
    let verb = match op { "+" => "add", "-" => "subtract", "*" => "multiply", "/" => "divide", _ => "combine" };
    Err(dyn_stop(site, format!("cannot {verb} {} by {}", dyn_kind_for(left), dyn_kind_for(right))))
}
fn dyn_integer_float_compare(integer: i64, float: f64) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    if float.is_nan() { return None; }
    if float == f64::INFINITY { return Some(Ordering::Less); }
    if float == f64::NEG_INFINITY { return Some(Ordering::Greater); }
    if let Some(integer) = jet_int_to_i128(integer) {
        if float >= -(i128::MIN as f64) { return Some(Ordering::Less); }
        if float < i128::MIN as f64 { return Some(Ordering::Greater); }
        let ordering = integer.cmp(&(float.trunc() as i128));
        return if ordering == Ordering::Equal { float.trunc().partial_cmp(&float) } else { Some(ordering) };
    }
    let fraction = JetFraction::from_float(float).expect("finite Float");
    Some(jet_int_value(integer).mul(&fraction.denominator).compare(&fraction.numerator))
}
pub fn jet_dyn_compare(op: &str, left: &Dyn, right: &Dyn, site: &str) -> Result<bool, String> {
    if op == "==" { return Ok(left == right); }
    if op == "!=" { return Ok(left != right); }
    let left_int = dyn_integer(left);
    let right_int = dyn_integer(right);
    let ordering = match (left, right) {
        (Dyn::Text(a), Dyn::Text(b)) => Some(a.cmp(b)),
        (Dyn::Bytes(a), Dyn::Bytes(b)) => Some(a.cmp(b)),
        (Dyn::DateTime(a), Dyn::DateTime(b)) => Some(a.instant_key().cmp(&b.instant_key())),
        _ => if let (Some(a), Some(b)) = (left_int.as_ref(), right_int.as_ref()) {
            Some(a.compare(&b))
        } else if let (Some(a), Dyn::Float(b)) = (left_int.as_ref(), right) {
            dyn_integer_float_compare(a.to_raw(), *b)
        } else if let (Dyn::Float(a), Some(b)) = (left, right_int.as_ref()) {
            dyn_integer_float_compare(b.to_raw(), *a).map(std::cmp::Ordering::reverse)
        } else if let (Dyn::Float(a), Dyn::Float(b)) = (left, right) { a.partial_cmp(b) }
        else { return Err(dyn_stop(site, format!("cannot compare {} with {}", dyn_kind_for(left), dyn_kind_for(right)))); },
    };
    Ok(match op {
        "<" => ordering == Some(std::cmp::Ordering::Less),
        "<=" => matches!(ordering, Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)),
        ">" => ordering == Some(std::cmp::Ordering::Greater),
        ">=" => matches!(ordering, Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)),
        _ => return Err(dyn_stop(site, format!("unknown Dyn comparison {op}"))),
    })
}
pub fn jet_dyn_datetime(value: &super::JetZonedDateTime) -> Dyn {
    let instant = value.to_datetime();
    let offset = value.zone().offset_at_utc(instant.unix_seconds_anchor());
    if offset % 60 != 0 || offset.abs() >= 86400 {
        super::jet_runtime_stop("E3001", "", 0, "Dyn.DateTime requires a minute-aligned RFC 3339 offset");
    }
    Dyn::datetime(instant.unix_seconds_anchor(), instant.nanosecond() as u32, instant.is_leap_second(), offset as i32)
}
pub fn jet_dyn_datetime_payload(tree: &Dyn) -> Option<super::JetZonedDateTime> {
    let Dyn::DateTime(value) = tree else { return None; };
    let zone = super::JetZone::named(&value.offset_text()).expect("Dyn DateTime has a valid fixed offset");
    Some(super::JetDateTime::from_timestamp_ns_with_leap(value.unix_seconds, value.nanos, value.leap_second).in_zone(&zone))
}

impl super::__jet_Encode for super::JetZonedDateTime {
    fn jet_encode(&self) -> Dyn { jet_dyn_datetime(self) }
}
impl super::__jet_Decode for super::JetZonedDateTime {
    fn jet_decode(tree: &Dyn) -> Result<Self, Vec<FieldError>> {
        match tree {
            Dyn::DateTime(_) => Ok(jet_dyn_datetime_payload(tree).unwrap()),
            Dyn::Text(text) => {
                let tree = Dyn::parse_datetime(text).map_err(FieldError::one)?;
                Ok(jet_dyn_datetime_payload(&tree).unwrap())
            }
            other => Err(FieldError::one(format!(
                "expected ZonedDateTime, found {}", dyn_kind_for(other)))),
        }
    }
}
