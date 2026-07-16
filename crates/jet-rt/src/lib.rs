#![deny(warnings)]

/// Compiler/runtime-only traced heap. Jet source reaches this through codegen.
#[doc(hidden)]
pub mod __gc;

pub fn display_f32(v: f32) -> String {
    format!("{v:?}")
}

pub fn display_f64(v: f64) -> String {
    format!("{v:?}")
}

pub fn string_len_chars(s: &str) -> i64 {
    s.chars().count() as i64
}

pub fn string_trim(s: &str) -> String {
    s.trim().to_string()
}

pub fn string_to_upper(s: &str) -> String {
    s.to_uppercase()
}

pub fn string_to_lower(s: &str) -> String {
    s.to_lowercase()
}

pub fn string_replace(s: &str, from: &str, to: &str) -> String {
    s.replace(from, to)
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetVal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    String(String),
    List(Vec<JetVal>),
    Record(Vec<JetVal>),
    // D-BIGINT1: JIT-tier `BigInt` handle. Reuses `CtBigInt` (jet-foundation)
    // limb-for-limb so a JIT-computed BigInt prints byte-identical to the AOT
    // `JetBigInt` (CommonTypes.rs) and comptime `CtBigInt` paths (R12 parity).
    BigInt(jet_foundation::Numeric::CtBigInt),
}

#[derive(Clone, Debug, Default)]
pub struct JetArena {
    values: Vec<JetVal>,
}

impl JetArena {
    pub fn alloc_empty_string(&mut self) -> i64 {
        self.alloc_string(String::new())
    }

    pub fn alloc_string(&mut self, text: impl Into<String>) -> i64 {
        let id = self.values.len() as i64;
        self.values.push(JetVal::String(text.into()));
        id
    }

    pub fn get_string(&self, id: i64) -> Option<&str> {
        match self.values.get(id as usize) {
            Some(JetVal::String(value)) => Some(value.as_str()),
            _ => None,
        }
    }

    pub fn get_string_mut(&mut self, id: i64) -> Option<&mut String> {
        match self.values.get_mut(id as usize) {
            Some(JetVal::String(value)) => Some(value),
            _ => None,
        }
    }

    pub fn clone_string(&self, id: i64) -> Option<String> {
        self.get_string(id).map(str::to_string)
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    pub fn alloc_int_list(&mut self, values: Vec<i64>) -> i64 {
        let id = self.values.len() as i64;
        self.values
            .push(JetVal::List(values.into_iter().map(JetVal::Int).collect()));
        id
    }

    pub fn alloc_empty_list(&mut self) -> i64 {
        let id = self.values.len() as i64;
        self.values.push(JetVal::List(Vec::new()));
        id
    }

    pub fn list_push_int(&mut self, list: i64, value: i64) -> Option<()> {
        match self.values.get_mut(list as usize) {
            Some(JetVal::List(values)) => {
                values.push(JetVal::Int(value));
                Some(())
            }
            _ => None,
        }
    }

    pub fn list_push_float(&mut self, list: i64, value: f64) -> Option<()> {
        match self.values.get_mut(list as usize) {
            Some(JetVal::List(values)) => {
                values.push(JetVal::Float(value));
                Some(())
            }
            _ => None,
        }
    }

    pub fn list_len(&self, list: i64) -> Option<i64> {
        match self.values.get(list as usize) {
            Some(JetVal::List(values)) => Some(values.len() as i64),
            _ => None,
        }
    }

    pub fn list_get_int(&self, list: i64, index: i64) -> Option<i64> {
        if index < 0 {
            return None;
        }
        match self.values.get(list as usize) {
            Some(JetVal::List(values)) => match values.get(index as usize) {
                Some(JetVal::Int(value)) => Some(*value),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn list_get_float(&self, list: i64, index: i64) -> Option<f64> {
        if index < 0 {
            return None;
        }
        match self.values.get(list as usize) {
            Some(JetVal::List(values)) => match values.get(index as usize) {
                Some(JetVal::Float(value)) => Some(*value),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn list_set_int(&mut self, list: i64, index: i64, value: i64) -> Option<()> {
        if index < 0 {
            return None;
        }
        match self.values.get_mut(list as usize) {
            Some(JetVal::List(values)) => match values.get_mut(index as usize) {
                Some(slot @ JetVal::Int(_)) => {
                    *slot = JetVal::Int(value);
                    Some(())
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub fn list_set_float(&mut self, list: i64, index: i64, value: f64) -> Option<()> {
        if index < 0 {
            return None;
        }
        match self.values.get_mut(list as usize) {
            Some(JetVal::List(values)) => match values.get_mut(index as usize) {
                Some(slot @ JetVal::Float(_)) => {
                    *slot = JetVal::Float(value);
                    Some(())
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub fn list_sort_int(&mut self, list: i64) -> Option<()> {
        match self.values.get_mut(list as usize) {
            Some(JetVal::List(values)) => {
                let mut ints = Vec::with_capacity(values.len());
                for value in values.iter() {
                    let JetVal::Int(value) = value else {
                        return None;
                    };
                    ints.push(*value);
                }
                ints.sort_unstable();
                *values = ints.into_iter().map(JetVal::Int).collect();
                Some(())
            }
            _ => None,
        }
    }

    pub fn list_slice(&mut self, list: i64, start: i64, end: i64) -> Option<i64> {
        if start < 0 || end < start {
            return None;
        }
        let slice = match self.values.get(list as usize) {
            Some(JetVal::List(values)) if end <= values.len() as i64 => {
                values[start as usize..end as usize].to_vec()
            }
            _ => return None,
        };
        let id = self.values.len() as i64;
        self.values.push(JetVal::List(slice));
        Some(id)
    }

    pub fn clone_int_list(&self, list: i64) -> Option<Vec<i64>> {
        match self.values.get(list as usize) {
            Some(JetVal::List(values)) => {
                let mut out = Vec::with_capacity(values.len());
                for value in values {
                    let JetVal::Int(value) = value else {
                        return None;
                    };
                    out.push(*value);
                }
                Some(out)
            }
            _ => None,
        }
    }

    pub fn clone_list(&mut self, list: i64) -> Option<i64> {
        let values = match self.values.get(list as usize) {
            Some(JetVal::List(values)) => values.clone(),
            _ => return None,
        };
        let id = self.values.len() as i64;
        self.values.push(JetVal::List(values));
        Some(id)
    }

    pub fn alloc_record(&mut self, fields: usize) -> i64 {
        let id = self.values.len() as i64;
        self.values
            .push(JetVal::Record(vec![JetVal::Int(0); fields]));
        id
    }

    fn record_set(&mut self, record: i64, index: i64, value: JetVal) -> Option<()> {
        if index < 0 {
            return None;
        }
        match self.values.get_mut(record as usize) {
            Some(JetVal::Record(fields)) => {
                let slot = fields.get_mut(index as usize)?;
                *slot = value;
                Some(())
            }
            _ => None,
        }
    }

    fn record_get(&self, record: i64, index: i64) -> Option<&JetVal> {
        if index < 0 {
            return None;
        }
        match self.values.get(record as usize) {
            Some(JetVal::Record(fields)) => fields.get(index as usize),
            _ => None,
        }
    }

    pub fn record_set_int(&mut self, record: i64, index: i64, value: i64) -> Option<()> {
        self.record_set(record, index, JetVal::Int(value))
    }

    pub fn record_set_float(&mut self, record: i64, index: i64, value: f64) -> Option<()> {
        self.record_set(record, index, JetVal::Float(value))
    }

    pub fn record_set_bool(&mut self, record: i64, index: i64, value: bool) -> Option<()> {
        self.record_set(record, index, JetVal::Bool(value))
    }

    pub fn record_set_char(&mut self, record: i64, index: i64, value: char) -> Option<()> {
        self.record_set(record, index, JetVal::Char(value))
    }

    pub fn record_set_string(&mut self, record: i64, index: i64, value_handle: i64) -> Option<()> {
        let value = self.clone_string(value_handle)?;
        self.record_set(record, index, JetVal::String(value))
    }

    pub fn record_get_int(&self, record: i64, index: i64) -> Option<i64> {
        match self.record_get(record, index) {
            Some(JetVal::Int(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn record_get_float(&self, record: i64, index: i64) -> Option<f64> {
        match self.record_get(record, index) {
            Some(JetVal::Float(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn record_get_bool(&self, record: i64, index: i64) -> Option<bool> {
        match self.record_get(record, index) {
            Some(JetVal::Bool(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn record_get_char(&self, record: i64, index: i64) -> Option<char> {
        match self.record_get(record, index) {
            Some(JetVal::Char(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn record_get_string(&mut self, record: i64, index: i64) -> Option<i64> {
        let value = match self.record_get(record, index)? {
            JetVal::String(value) => value.clone(),
            _ => return None,
        };
        Some(self.alloc_string(value))
    }

    // ── D-BIGINT1: JIT-tier `BigInt` handles ────────────────────────────────

    pub fn alloc_bigint_from_int(&mut self, n: i64) -> i64 {
        let id = self.values.len() as i64;
        self.values
            .push(JetVal::BigInt(jet_foundation::Numeric::CtBigInt::from_int(n)));
        id
    }

    /// `Err` on a malformed literal (mirrors AOT's `JetBigInt::from_str(...).expect(...)`
    /// panic path — the caller traps instead of unwinding a Rust panic through the
    /// JIT frame, I1).
    pub fn alloc_bigint_from_str(&mut self, s: &str) -> Result<i64, String> {
        let v = jet_foundation::Numeric::CtBigInt::from_str(s)?;
        let id = self.values.len() as i64;
        self.values.push(JetVal::BigInt(v));
        Ok(id)
    }

    fn get_bigint(&self, id: i64) -> Option<&jet_foundation::Numeric::CtBigInt> {
        match self.values.get(id as usize) {
            Some(JetVal::BigInt(v)) => Some(v),
            _ => None,
        }
    }

    pub fn bigint_add(&mut self, a: i64, b: i64) -> Option<i64> {
        let result = self.get_bigint(a)?.add(self.get_bigint(b)?);
        let id = self.values.len() as i64;
        self.values.push(JetVal::BigInt(result));
        Some(id)
    }

    pub fn bigint_sub(&mut self, a: i64, b: i64) -> Option<i64> {
        let result = self.get_bigint(a)?.sub(self.get_bigint(b)?);
        let id = self.values.len() as i64;
        self.values.push(JetVal::BigInt(result));
        Some(id)
    }

    pub fn bigint_mul(&mut self, a: i64, b: i64) -> Option<i64> {
        let result = self.get_bigint(a)?.mul(self.get_bigint(b)?);
        let id = self.values.len() as i64;
        self.values.push(JetVal::BigInt(result));
        Some(id)
    }

    pub fn bigint_neg(&mut self, a: i64) -> Option<i64> {
        let result = self.get_bigint(a)?.neg();
        let id = self.values.len() as i64;
        self.values.push(JetVal::BigInt(result));
        Some(id)
    }

    pub fn bigint_to_string(&self, a: i64) -> Option<String> {
        Some(self.get_bigint(a)?.to_string_rep())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_matches_aot_float_prelude() {
        assert_eq!(display_f32(1.0), "1.0");
        assert_eq!(display_f64(-5.0), "-5.0");
        assert_eq!(display_f64(2.5), "2.5");
    }

    #[test]
    fn string_helpers_match_prelude_semantics() {
        assert_eq!(string_len_chars("aé日"), 3);
        assert_eq!(string_trim("  jet\n"), "jet");
        assert_eq!(string_to_upper("Jet"), "JET");
        assert_eq!(string_to_lower("Jet"), "jet");
        assert_eq!(string_replace("one two one", "one", "1"), "1 two 1");
    }

    #[test]
    fn arena_allocates_string_handles() {
        let mut arena = JetArena::default();
        let first = arena.alloc_empty_string();
        let second = arena.alloc_string("jet");

        arena.get_string_mut(first).unwrap().push_str("dev");

        assert_eq!(first, 0);
        assert_eq!(second, 1);
        assert_eq!(arena.get_string(first), Some("dev"));
        assert_eq!(arena.clone_string(second), Some("jet".to_string()));

        arena.clear();
        assert_eq!(arena.get_string(first), None);
    }

    #[test]
    fn arena_allocates_int_lists() {
        let mut arena = JetArena::default();
        let list = arena.alloc_int_list(vec![3, 1, 2]);

        arena.list_push_int(list, 4).unwrap();
        arena.list_sort_int(list).unwrap();
        assert_eq!(arena.list_len(list), Some(4));
        assert_eq!(arena.list_get_int(list, 2), Some(3));
        arena.list_set_int(list, 2, 9).unwrap();
        assert_eq!(arena.clone_int_list(list), Some(vec![1, 2, 9, 4]));

        let slice = arena.list_slice(list, 1, 3).unwrap();
        assert_eq!(arena.clone_int_list(slice), Some(vec![2, 9]));
    }

    #[test]
    fn arena_allocates_float_lists() {
        let mut arena = JetArena::default();
        let list = arena.alloc_empty_list();

        arena.list_push_float(list, 1.5).unwrap();
        arena.list_push_float(list, 2.5).unwrap();
        assert_eq!(arena.list_get_float(list, 1), Some(2.5));
        arena.list_set_float(list, 1, 4.5).unwrap();
        assert_eq!(arena.list_get_float(list, 1), Some(4.5));

        let slice = arena.list_slice(list, 0, 1).unwrap();
        assert_eq!(arena.list_get_float(slice, 0), Some(1.5));
    }

    #[test]
    fn arena_allocates_records() {
        let mut arena = JetArena::default();
        let name = arena.alloc_string("jet");
        let record = arena.alloc_record(4);

        arena.record_set_string(record, 0, name).unwrap();
        arena.record_set_float(record, 1, 2.5).unwrap();
        arena.record_set_bool(record, 2, true).unwrap();
        arena.record_set_char(record, 3, 'J').unwrap();

        let cloned_name = arena.record_get_string(record, 0).unwrap();
        assert_eq!(arena.get_string(cloned_name), Some("jet"));
        assert_eq!(arena.record_get_float(record, 1), Some(2.5));
        assert_eq!(arena.record_get_bool(record, 2), Some(true));
        assert_eq!(arena.record_get_char(record, 3), Some('J'));
    }
}
