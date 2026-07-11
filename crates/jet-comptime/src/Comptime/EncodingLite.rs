//! Card #392 pass 4 gap fix: `core.encoding.{csv,toml,yaml,xml,cbor,jsonl}`
//! + `core.encoding.json.{canonical,events}`, ported verbatim into the
//! comptime/REPL tier-0 interpreter so they match AOT byte-for-byte (R12
//! parity). Sources (std-only, I6, no changes to logic — only the target
//! value type changes from `jet_std::DataTree` to the `Json`-tagged
//! `CtValue` shape `JsonInterp.rs` already established for `core.encoding.json`):
//!
//! - csv: `jet_ring_csv_parse`/`jet_ring_csv_render`,
//!   `crates/jet-codegen/src/Prelude/CoreLib/Top/RingCsvLogTimeCrypto.rs`.
//! - toml: `pub mod toml` (`parse_to_tree`/`render`),
//!   `crates/jet-codegen/src/Prelude/CoreLib/JetStd/Toml.rs`.
//! - yaml: `pub mod yaml` (`parse_to_tree`/`render`),
//!   `crates/jet-codegen/src/Prelude/CoreLib/JetStd/Yaml.rs`.
//! - xml/cbor: `jet_std_xml_parse`/`jet_std_xml_render`/`jet_std_cbor_encode`/
//!   `jet_std_cbor_decode`, `crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingCodecs.rs`.
//! - jsonl: `jet_std_jsonl_parse`/`jet_std_jsonl_render`,
//!   `crates/jet-codegen/src/Prelude/CoreLib/Top/MathRandomTime.rs`.
//! - json canonical/events: `jet_std_json_render_canonical`/`jet_std_json_events`,
//!   same file.
//!
//! sema (`fixed_sigs.rs`) types `core.encoding.{toml,yaml,xml,cbor}`'s
//! parsed value as the same `json` type as `core.encoding.json` — AOT backs
//! every one of these with the single `jet_std::DataTree`. Comptime mirrors
//! that with the same `Json`-tagged `CtValue::Enum` (`JsonInterp::json_variant`/
//! `json_payload`) every accessor method (`.field`/`.at`/...) already reads,
//! so no new value machinery is needed — just new parse/render walkers that
//! build/consume that shape instead of `DataTree`.

use std::collections::BTreeMap;

use crate::AST::{CtKey, CtValue};

use super::JsonInterp::{json_payload, json_variant};

// ── core.encoding.csv ───────────────────────────────────────────────────────

pub(super) fn csv_parse(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        match csv_parse_row(line) {
            Ok(row) => rows.push(row),
            Err(msg) => return Err(format!("E2701: CSV row {} — {}", line_no + 1, msg)),
        }
    }
    Ok(rows)
}

fn csv_parse_row(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        let field = if chars.peek() == Some(&'"') {
            chars.next();
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('"') => {
                        if chars.peek() == Some(&'"') {
                            chars.next();
                            s.push('"');
                        } else {
                            break;
                        }
                    }
                    Some(c) => s.push(c),
                    None => break,
                }
            }
            s
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                s.push(c);
                chars.next();
            }
            s
        };
        fields.push(field);
        match chars.next() {
            Some(',') => {}
            None => break,
            Some(c) => return Err(format!("unexpected character {:?} after field", c)),
        }
    }
    Ok(fields)
}

pub(super) fn csv_render(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|field| {
                    if field.contains(',') || field.contains('"') || field.contains('\n') {
                        format!("\"{}\"", field.replace('"', "\"\""))
                    } else {
                        field.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── shared: JsonError-shaped CtValue::Struct (line/message) ────────────────
// Mirrors `JsonInterp::json_error_value` — toml/yaml `parse` share the same
// sema-declared `JsonError` return type (`fixed_sigs.rs`'s `json_error_ty()`
// is reused for `core.encoding.{toml,yaml}` exactly like `core.encoding.json`).
fn json_error_struct(line: i64, message: String) -> CtValue {
    CtValue::Struct {
        type_name: "JsonError".to_string(),
        fields: vec![
            ("line".to_string(), CtValue::Int(line)),
            ("message".to_string(), CtValue::Str(message)),
        ],
    }
}

// ── core.encoding.toml ──────────────────────────────────────────────────────
// Ported from `Toml.rs`'s `pub mod toml`, target type swapped from
// `DataTree` to the `Json`-tagged `CtValue`.

#[derive(Clone, Debug, PartialEq)]
enum TomlValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Datetime(String),
    Array(Vec<TomlValue>),
    InlineTable(Vec<(String, TomlValue)>),
}

#[derive(Clone, Debug, PartialEq)]
enum TomlItem {
    Header { path: Vec<String>, array: bool },
    KeyVal { path: Vec<String>, value: TomlValue },
}

pub(super) fn toml_parse(raw: &str) -> Result<CtValue, CtValue> {
    let mut p = TomlParser {
        chars: raw.chars().collect(),
        pos: 0,
        line: 1,
    };
    let mut items = Vec::new();
    loop {
        p.skip_between_statements();
        if p.peek().is_none() {
            break;
        }
        match p.statement().map_err(|e| json_error_struct(e.line as i64, e.message))? {
            Some(item) => items.push(item),
            None => {}
        }
    }
    Ok(toml_assemble(items))
}

// A tagged `Json` `Object` node whose payload is a sorted `Map` — mirrors
// `json_variant("Object", CtValue::Map(...))` used throughout `JsonInterp`.
fn json_object(entries: Vec<(String, CtValue)>) -> CtValue {
    let map: BTreeMap<CtKey, CtValue> =
        entries.into_iter().map(|(k, v)| (CtKey::Str(k), v)).collect();
    json_variant("Object", Some(CtValue::Map(map)))
}
fn json_array(items: Vec<CtValue>) -> CtValue {
    json_variant("Array", Some(CtValue::List(items)))
}
fn json_object_entries(v: &CtValue) -> Option<Vec<(String, CtValue)>> {
    match json_payload(v, "Object") {
        Some(CtValue::Map(m)) => Some(
            m.iter()
                .map(|(k, v)| {
                    (
                        match k {
                            CtKey::Str(s) => s.clone(),
                            other => format!("{:?}", other),
                        },
                        v.clone(),
                    )
                })
                .collect(),
        ),
        _ => None,
    }
}
fn json_array_items(v: &CtValue) -> Option<Vec<CtValue>> {
    match json_payload(v, "Array") {
        Some(CtValue::List(xs)) => Some(xs.clone()),
        _ => None,
    }
}

fn toml_assemble(items: Vec<TomlItem>) -> CtValue {
    let mut root = json_object(Vec::new());
    let mut current: Vec<String> = Vec::new();
    for item in items {
        match item {
            TomlItem::Header { path, array } => {
                if array {
                    toml_push_array_table(&mut root, &path);
                } else {
                    let _ = toml_table_at(&mut root, &path);
                }
                current = path;
            }
            TomlItem::KeyVal { path, value } => {
                toml_set_key(&mut root, &current, &path, toml_value_to_json(value));
            }
        }
    }
    root
}

// Ensure a table exists at `path` (creating empty tables along the way, but
// never clobbering a table that already has keys) — used for a bare
// `[table]` header with no keys under it yet. A value-typed
// re-implementation of the AOT version's `&mut DataTree` walk (which
// mutates in place via live aliasing that doesn't fit an immutable-by-value
// `CtValue` tree directly); behaviorally identical output (`descend into
// the last element of an existing array-of-tables` included).
fn toml_table_at(root: &mut CtValue, path: &[String]) {
    *root = toml_ensure_table(root, path);
}
fn toml_ensure_table(node: &CtValue, path: &[String]) -> CtValue {
    let Some((seg, rest)) = path.split_first() else {
        return node.clone();
    };
    let mut entries = json_object_entries(node).unwrap_or_default();
    match entries.iter().position(|(k, _)| k == seg) {
        Some(idx) => {
            if let Some(mut items) = json_array_items(&entries[idx].1) {
                let last_idx = items.len().saturating_sub(1);
                if let Some(last) = items.get(last_idx).cloned() {
                    items[last_idx] = toml_ensure_table(&last, rest);
                }
                entries[idx].1 = json_array(items);
            } else {
                entries[idx].1 = toml_ensure_table(&entries[idx].1, rest);
            }
        }
        None => entries.push((seg.clone(), toml_ensure_table(&json_object(Vec::new()), rest))),
    }
    json_object(entries)
}

fn toml_set_key(root: &mut CtValue, current: &[String], key_path: &[String], value: CtValue) {
    let mut full: Vec<String> = current.to_vec();
    full.extend_from_slice(&key_path[..key_path.len() - 1]);
    let fk = key_path[key_path.len() - 1].clone();
    *root = toml_set_at_path(root, &full, &fk, value);
}

// Rebuild `root` with `value` set at `path.field`, creating tables along the
// way. Recursive-by-value (immutable `CtValue`) rewrite of the AOT
// `&mut DataTree` walk, which needs live mutable aliasing that doesn't fit a
// value-typed `CtValue` tree as directly — behaviorally identical output.
fn toml_set_at_path(root: &CtValue, path: &[String], field: &str, value: CtValue) -> CtValue {
    if path.is_empty() {
        let mut entries = json_object_entries(root).unwrap_or_default();
        match entries.iter_mut().find(|(k, _)| k == field) {
            Some(slot) => slot.1 = value,
            None => entries.push((field.to_string(), value)),
        }
        return json_object(entries);
    }
    let (seg, rest) = (path[0].clone(), &path[1..]);
    let mut entries = json_object_entries(root).unwrap_or_default();
    match entries.iter().position(|(k, _)| k == &seg) {
        Some(idx) => {
            let is_arr = json_array_items(&entries[idx].1).is_some();
            if is_arr {
                let mut items = json_array_items(&entries[idx].1).unwrap();
                let last_idx = items.len().saturating_sub(1);
                if let Some(last) = items.get(last_idx).cloned() {
                    items[last_idx] = toml_set_at_path(&last, rest, field, value);
                }
                entries[idx].1 = json_array(items);
            } else {
                let updated = toml_set_at_path(&entries[idx].1, rest, field, value);
                entries[idx].1 = updated;
            }
        }
        None => {
            let child = toml_set_at_path(&json_object(Vec::new()), rest, field, value);
            entries.push((seg, child));
        }
    }
    json_object(entries)
}

fn toml_push_array_table(root: &mut CtValue, path: &[String]) {
    let (parent_path, last) = path.split_at(path.len() - 1);
    let last = &last[0];
    *root = toml_push_array_at(root, parent_path, last);
}
fn toml_push_array_at(root: &CtValue, path: &[String], field: &str) -> CtValue {
    if path.is_empty() {
        let mut entries = json_object_entries(root).unwrap_or_default();
        match entries.iter().position(|(k, _)| k == field) {
            Some(idx) => {
                let mut items = json_array_items(&entries[idx].1).unwrap_or_default();
                items.push(json_object(Vec::new()));
                entries[idx].1 = json_array(items);
            }
            None => entries.push((field.to_string(), json_array(vec![json_object(Vec::new())]))),
        }
        return json_object(entries);
    }
    let (seg, rest) = (path[0].clone(), &path[1..]);
    let mut entries = json_object_entries(root).unwrap_or_default();
    match entries.iter().position(|(k, _)| k == &seg) {
        Some(idx) => {
            let is_arr = json_array_items(&entries[idx].1).is_some();
            if is_arr {
                let mut items = json_array_items(&entries[idx].1).unwrap();
                let last_idx = items.len().saturating_sub(1);
                if let Some(last) = items.get(last_idx).cloned() {
                    items[last_idx] = toml_push_array_at(&last, rest, field);
                }
                entries[idx].1 = json_array(items);
            } else {
                entries[idx].1 = toml_push_array_at(&entries[idx].1, rest, field);
            }
        }
        None => {
            let child = toml_push_array_at(&json_object(Vec::new()), rest, field);
            entries.push((seg, child));
        }
    }
    json_object(entries)
}

fn toml_value_to_json(v: TomlValue) -> CtValue {
    match v {
        TomlValue::String(s) => json_variant("Text", Some(CtValue::Str(s))),
        TomlValue::Integer(n) => json_variant("Int", Some(CtValue::Int(n))),
        TomlValue::Float(f) => json_variant("Float", Some(CtValue::Float(f))),
        TomlValue::Boolean(b) => json_variant("Bool", Some(CtValue::Bool(b))),
        TomlValue::Datetime(s) => json_variant("Text", Some(CtValue::Str(s))),
        TomlValue::Array(xs) => json_array(xs.into_iter().map(toml_value_to_json).collect()),
        TomlValue::InlineTable(es) => {
            json_object(es.into_iter().map(|(k, v)| (k, toml_value_to_json(v))).collect())
        }
    }
}

// ── TOML render: `Json`-tagged CtValue → TOML text ─────────────────────────

pub(super) fn toml_render(v: &CtValue) -> String {
    let mut out = String::new();
    toml_render_table(v, &[], &mut out);
    out.trim_end().to_string()
}

fn toml_is_table(v: &CtValue) -> bool {
    json_object_entries(v).is_some()
}
fn toml_is_array_of_tables(v: &CtValue) -> bool {
    match json_array_items(v) {
        Some(arr) => !arr.is_empty() && arr.iter().all(toml_is_table),
        None => false,
    }
}
fn toml_render_table(t: &CtValue, path: &[String], out: &mut String) {
    let Some(entries) = json_object_entries(t) else {
        return;
    };
    for (k, v) in &entries {
        if !toml_is_table(v) && !toml_is_array_of_tables(v) {
            out.push_str(&format!("{} = {}\n", k, toml_render_value(v)));
        }
    }
    for (k, v) in &entries {
        if toml_is_table(v) {
            let mut p = path.to_vec();
            p.push(k.clone());
            out.push_str(&format!("\n[{}]\n", p.join(".")));
            toml_render_table(v, &p, out);
        } else if toml_is_array_of_tables(v) {
            let mut p = path.to_vec();
            p.push(k.clone());
            if let Some(arr) = json_array_items(v) {
                for elem in &arr {
                    out.push_str(&format!("\n[[{}]]\n", p.join(".")));
                    toml_render_table(elem, &p, out);
                }
            }
        }
    }
}
fn toml_render_value(v: &CtValue) -> String {
    match v {
        _ if json_payload(v, "Null").is_some() || matches!(v, CtValue::Unit) => "\"\"".to_string(),
        _ => match v {
            CtValue::Enum { variant, args, .. } => match (variant.as_str(), args.first()) {
                ("Null", _) => "\"\"".to_string(),
                ("Bool", Some((_, CtValue::Bool(b)))) => b.to_string(),
                ("Int", Some((_, CtValue::Int(n)))) => n.to_string(),
                ("Float", Some((_, CtValue::Float(f)))) => format!("{:?}", f),
                ("Text", Some((_, CtValue::Str(s)))) => quote_json_local(s),
                ("Array", Some((_, CtValue::List(items)))) => {
                    let parts: Vec<String> = items.iter().map(toml_render_value).collect();
                    format!("[{}]", parts.join(", "))
                }
                ("Object", Some((_, CtValue::Map(m)))) => {
                    let parts: Vec<String> = m
                        .iter()
                        .map(|(k, val)| {
                            format!(
                                "{} = {}",
                                match k {
                                    CtKey::Str(s) => s.clone(),
                                    other => format!("{:?}", other),
                                },
                                toml_render_value(val)
                            )
                        })
                        .collect();
                    format!("{{ {} }}", parts.join(", "))
                }
                _ => "\"\"".to_string(),
            },
            _ => "\"\"".to_string(),
        },
    }
}

struct TomlParser {
    chars: Vec<char>,
    pos: usize,
    line: usize,
}
struct TomlParseError {
    line: usize,
    message: String,
}
impl TomlParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }
    fn err(&self, message: impl Into<String>) -> TomlParseError {
        TomlParseError {
            line: self.line,
            message: message.into(),
        }
    }
    fn skip_inline_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }
    fn skip_between_statements(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.bump();
                }
                Some('#') => self.skip_comment(),
                _ => break,
            }
        }
    }
    fn skip_comment(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.pos += 1;
        }
    }
    fn finish_line(&mut self) -> Result<(), TomlParseError> {
        self.skip_inline_ws();
        if self.peek() == Some('#') {
            self.skip_comment();
        }
        match self.peek() {
            None | Some('\n') | Some('\r') => Ok(()),
            Some(c) => Err(self.err(format!("unexpected `{c}` after value"))),
        }
    }
    fn statement(&mut self) -> Result<Option<TomlItem>, TomlParseError> {
        match self.peek() {
            Some('[') => self.header().map(Some),
            _ => self.key_value().map(Some),
        }
    }
    fn header(&mut self) -> Result<TomlItem, TomlParseError> {
        self.bump();
        let array = self.peek() == Some('[');
        if array {
            self.bump();
        }
        self.skip_inline_ws();
        let path = self.key_path()?;
        self.skip_inline_ws();
        if self.peek() != Some(']') {
            return Err(self.err("expected `]` to close a table header"));
        }
        self.bump();
        if array {
            if self.peek() != Some(']') {
                return Err(self.err("expected `]]` to close an array-of-tables header"));
            }
            self.bump();
        }
        if path.is_empty() {
            return Err(self.err("a table header must name a table"));
        }
        self.finish_line()?;
        Ok(TomlItem::Header { path, array })
    }
    fn key_value(&mut self) -> Result<TomlItem, TomlParseError> {
        let path = self.key_path()?;
        if path.is_empty() {
            return Err(self.err("expected a key"));
        }
        self.skip_inline_ws();
        if self.peek() != Some('=') {
            return Err(self.err(format!("expected `=` after key `{}`", path.join("."))));
        }
        self.bump();
        self.skip_inline_ws();
        let value = self.value()?;
        self.finish_line()?;
        Ok(TomlItem::KeyVal { path, value })
    }
    fn key_path(&mut self) -> Result<Vec<String>, TomlParseError> {
        let mut path = Vec::new();
        loop {
            self.skip_inline_ws();
            path.push(self.simple_key()?);
            self.skip_inline_ws();
            if self.peek() == Some('.') {
                self.bump();
            } else {
                break;
            }
        }
        Ok(path)
    }
    fn simple_key(&mut self) -> Result<String, TomlParseError> {
        match self.peek() {
            Some('"') => self.basic_string(),
            Some('\'') => self.literal_string(),
            Some(c) if toml_is_bare_key_char(c) => {
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    if toml_is_bare_key_char(c) {
                        s.push(c);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                Ok(s)
            }
            Some(c) => Err(self.err(format!("`{c}` is not a valid key character"))),
            None => Err(self.err("expected a key")),
        }
    }
    fn value(&mut self) -> Result<TomlValue, TomlParseError> {
        match self.peek() {
            Some('"') => Ok(TomlValue::String(self.basic_string()?)),
            Some('\'') => Ok(TomlValue::String(self.literal_string()?)),
            Some('[') => self.array(),
            Some('{') => self.inline_table(),
            Some('t') | Some('f') => self.boolean(),
            Some('+') | Some('-') | Some('0'..='9') | Some('i') | Some('n') => {
                self.number_or_datetime()
            }
            Some(c) => Err(self.err(format!("`{c}` does not start a valid value"))),
            None => Err(self.err("expected a value")),
        }
    }
    fn boolean(&mut self) -> Result<TomlValue, TomlParseError> {
        if self.try_keyword("true") {
            Ok(TomlValue::Boolean(true))
        } else if self.try_keyword("false") {
            Ok(TomlValue::Boolean(false))
        } else {
            Err(self.err("expected `true` or `false`"))
        }
    }
    fn try_keyword(&mut self, kw: &str) -> bool {
        let chars: Vec<char> = kw.chars().collect();
        for (i, c) in chars.iter().enumerate() {
            if self.peek_at(i) != Some(*c) {
                return false;
            }
        }
        if let Some(after) = self.peek_at(chars.len()) {
            if toml_is_bare_key_char(after) || after == '.' {
                return false;
            }
        }
        for _ in 0..chars.len() {
            self.bump();
        }
        true
    }
    fn basic_string(&mut self) -> Result<String, TomlParseError> {
        if self.peek() == Some('"') && self.peek_at(1) == Some('"') && self.peek_at(2) == Some('"')
        {
            return self.multiline_basic_string();
        }
        self.bump();
        let mut out = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => return Err(self.err("unterminated string")),
                Some('"') => return Ok(out),
                Some('\\') => out.push(self.string_escape()?),
                Some(c) if (c as u32) < 0x20 => {
                    return Err(self.err("control character in string"))
                }
                Some(c) => out.push(c),
            }
        }
    }
    fn multiline_basic_string(&mut self) -> Result<String, TomlParseError> {
        self.bump();
        self.bump();
        self.bump();
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
        let mut out = String::new();
        loop {
            if self.peek() == Some('"') && self.peek_at(1) == Some('"') && self.peek_at(2) == Some('"')
            {
                self.bump();
                self.bump();
                self.bump();
                return Ok(out);
            }
            match self.bump() {
                None => return Err(self.err("unterminated multi-line string")),
                Some('\\') => {
                    if matches!(self.peek(), Some('\n') | Some('\r') | Some(' ') | Some('\t')) {
                        let mut sawline = false;
                        let save = self.pos;
                        let saveline = self.line;
                        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\r') | Some('\n')) {
                            if self.peek() == Some('\n') {
                                sawline = true;
                            }
                            self.bump();
                        }
                        if !sawline {
                            self.pos = save;
                            self.line = saveline;
                            out.push(self.string_escape()?);
                        }
                    } else {
                        out.push(self.string_escape()?);
                    }
                }
                Some(c) => out.push(c),
            }
        }
    }
    fn string_escape(&mut self) -> Result<char, TomlParseError> {
        match self.bump() {
            Some('"') => Ok('"'),
            Some('\\') => Ok('\\'),
            Some('b') => Ok('\u{0008}'),
            Some('f') => Ok('\u{000c}'),
            Some('n') => Ok('\n'),
            Some('r') => Ok('\r'),
            Some('t') => Ok('\t'),
            Some('u') => self.unicode_escape(4),
            Some('U') => self.unicode_escape(8),
            Some(c) => Err(self.err(format!("invalid escape `\\{c}`"))),
            None => Err(self.err("unterminated escape")),
        }
    }
    fn unicode_escape(&mut self, n: usize) -> Result<char, TomlParseError> {
        let mut v = 0u32;
        for _ in 0..n {
            let Some(c) = self.peek() else {
                return Err(self.err("truncated unicode escape"));
            };
            let Some(d) = c.to_digit(16) else {
                return Err(self.err("invalid unicode escape"));
            };
            v = v * 16 + d;
            self.pos += 1;
        }
        char::from_u32(v).ok_or_else(|| self.err("invalid unicode scalar value"))
    }
    fn literal_string(&mut self) -> Result<String, TomlParseError> {
        if self.peek() == Some('\'') && self.peek_at(1) == Some('\'') && self.peek_at(2) == Some('\'')
        {
            return self.multiline_literal_string();
        }
        self.bump();
        let mut out = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => return Err(self.err("unterminated literal string")),
                Some('\'') => return Ok(out),
                Some(c) => out.push(c),
            }
        }
    }
    fn multiline_literal_string(&mut self) -> Result<String, TomlParseError> {
        self.bump();
        self.bump();
        self.bump();
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
        let mut out = String::new();
        loop {
            if self.peek() == Some('\'') && self.peek_at(1) == Some('\'') && self.peek_at(2) == Some('\'')
            {
                self.bump();
                self.bump();
                self.bump();
                return Ok(out);
            }
            match self.bump() {
                None => return Err(self.err("unterminated multi-line literal string")),
                Some(c) => out.push(c),
            }
        }
    }
    fn array(&mut self) -> Result<TomlValue, TomlParseError> {
        self.bump();
        let mut items = Vec::new();
        loop {
            self.skip_ws_newlines_comments();
            match self.peek() {
                Some(']') => {
                    self.bump();
                    return Ok(TomlValue::Array(items));
                }
                None => return Err(self.err("unterminated array")),
                _ => {}
            }
            items.push(self.value()?);
            self.skip_ws_newlines_comments();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {
                    self.bump();
                    return Ok(TomlValue::Array(items));
                }
                Some(c) => {
                    return Err(self.err(format!("expected `,` or `]` in array, found `{c}`")))
                }
                None => return Err(self.err("unterminated array")),
            }
        }
    }
    fn skip_ws_newlines_comments(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.bump();
                }
                Some('#') => self.skip_comment(),
                _ => break,
            }
        }
    }
    fn inline_table(&mut self) -> Result<TomlValue, TomlParseError> {
        self.bump();
        let mut entries = Vec::new();
        self.skip_inline_ws();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(TomlValue::InlineTable(entries));
        }
        loop {
            self.skip_inline_ws();
            let path = self.key_path()?;
            self.skip_inline_ws();
            if self.bump() != Some('=') {
                return Err(self.err("expected `=` in inline table"));
            }
            self.skip_inline_ws();
            let value = self.value()?;
            entries.push((path.join("."), value));
            self.skip_inline_ws();
            match self.bump() {
                Some(',') => continue,
                Some('}') => return Ok(TomlValue::InlineTable(entries)),
                Some(c) => {
                    return Err(self.err(format!("expected `,` or `}}` in inline table, found `{c}`")))
                }
                None => return Err(self.err("unterminated inline table")),
            }
        }
    }
    fn number_or_datetime(&mut self) -> Result<TomlValue, TomlParseError> {
        if self.looks_like_date() || self.looks_like_time() {
            return self.datetime();
        }
        self.number()
    }
    fn looks_like_date(&self) -> bool {
        let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
        d(0) && d(1)
            && d(2)
            && d(3)
            && self.peek_at(4) == Some('-')
            && d(5)
            && d(6)
            && self.peek_at(7) == Some('-')
            && d(8)
            && d(9)
    }
    fn looks_like_time(&self) -> bool {
        let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
        d(0) && d(1) && self.peek_at(2) == Some(':') && d(3) && d(4)
    }
    fn datetime(&mut self) -> Result<TomlValue, TomlParseError> {
        let mut s = String::new();
        let is_dt = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '.' | '+');
        while let Some(c) = self.peek() {
            if is_dt(c) {
                s.push(c);
                self.pos += 1;
            } else if c == ' '
                && self.peek_at(1).map_or(false, |n| n.is_ascii_digit())
                && self.peek_at(3) == Some(':')
            {
                s.push(' ');
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(TomlValue::Datetime(s))
    }
    fn number(&mut self) -> Result<TomlValue, TomlParseError> {
        if self.try_keyword("inf") {
            return Ok(TomlValue::Float(f64::INFINITY));
        }
        if self.try_keyword("nan") {
            return Ok(TomlValue::Float(f64::NAN));
        }
        let mut tok = String::new();
        if matches!(self.peek(), Some('+') | Some('-')) {
            let sign = self.bump().unwrap();
            tok.push(sign);
            if self.try_keyword("inf") {
                return Ok(TomlValue::Float(if sign == '-' {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }));
            }
            if self.try_keyword("nan") {
                return Ok(TomlValue::Float(f64::NAN));
            }
        }
        if self.peek() == Some('0') {
            if let Some(r) = self.peek_at(1) {
                if matches!(r, 'x' | 'o' | 'b') && tok.is_empty() {
                    return self.radix_integer();
                }
            }
        }
        let mut is_float = false;
        while let Some(c) = self.peek() {
            match c {
                '0'..='9' | '_' => {
                    tok.push(c);
                    self.pos += 1;
                }
                '.' | 'e' | 'E' | '+' | '-' => {
                    is_float = true;
                    tok.push(c);
                    self.pos += 1;
                }
                _ => break,
            }
        }
        let clean: String = tok.chars().filter(|c| *c != '_').collect();
        if is_float {
            clean
                .parse::<f64>()
                .map(TomlValue::Float)
                .map_err(|_| self.err(format!("invalid number `{tok}`")))
        } else {
            clean
                .parse::<i64>()
                .map(TomlValue::Integer)
                .map_err(|_| self.err(format!("invalid number `{tok}`")))
        }
    }
    fn radix_integer(&mut self) -> Result<TomlValue, TomlParseError> {
        self.bump();
        let prefix = self.bump().unwrap();
        let radix = match prefix {
            'x' => 16,
            'o' => 8,
            'b' => 2,
            _ => 16,
        };
        let mut tok = String::new();
        while let Some(c) = self.peek() {
            if c == '_' {
                self.pos += 1;
            } else if c.is_digit(radix) {
                tok.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if tok.is_empty() {
            return Err(self.err("expected digits after numeric base prefix"));
        }
        i64::from_str_radix(&tok, radix)
            .map(TomlValue::Integer)
            .map_err(|_| self.err(format!("invalid base-{radix} integer `{tok}`")))
    }
}
fn toml_is_bare_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

// Mirrors `JsonInterp::quote_json` (private to that module) — same escape
// table AOT's `Toml.rs` reuses from `Json.rs`'s `quote_json` for its own
// string-value rendering.
fn quote_json_local(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── core.encoding.yaml ──────────────────────────────────────────────────────
// Ported from `Yaml.rs`'s `pub mod yaml`, target type swapped from
// `DataTree` to the `Json`-tagged `CtValue` (same convention as toml above).

pub(super) fn yaml_parse(raw: &str) -> Result<CtValue, CtValue> {
    let lines: Vec<String> = raw
        .split('\n')
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect();
    let mut p = YamlParser {
        lines,
        pos: 0,
        anchors: BTreeMap::new(),
    };
    p.skip_ignorable();
    while p.at_doc_marker() {
        p.pos += 1;
        p.skip_ignorable();
    }
    if p.pos >= p.lines.len() || p.at_doc_end() {
        return Ok(json_variant("Null", None));
    }
    let base = p.indent(p.pos);
    p.parse_node(base)
        .map_err(|e| json_error_struct(e.line as i64, e.message))
}

struct YamlParser {
    lines: Vec<String>,
    pos: usize,
    anchors: BTreeMap<String, CtValue>,
}
struct YamlParseError {
    line: usize,
    message: String,
}

impl YamlParser {
    fn indent(&self, i: usize) -> usize {
        self.lines[i].chars().take_while(|c| *c == ' ').count()
    }
    fn content(&self, i: usize) -> String {
        let after = &self.lines[i][self.indent(i)..];
        yaml_strip_comment(after)
    }
    fn is_ignorable(&self, i: usize) -> bool {
        let t = self.lines[i].trim();
        t.is_empty() || t.starts_with('#')
    }
    fn skip_ignorable(&mut self) {
        while self.pos < self.lines.len() && self.is_ignorable(self.pos) {
            self.pos += 1;
        }
    }
    fn at_doc_marker(&self) -> bool {
        self.pos < self.lines.len() && self.lines[self.pos].trim_start().starts_with("---")
    }
    fn at_doc_end(&self) -> bool {
        self.pos < self.lines.len() && self.lines[self.pos].trim() == "..."
    }

    fn parse_node(&mut self, min_indent: usize) -> Result<CtValue, YamlParseError> {
        self.skip_ignorable();
        if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
            return Ok(json_variant("Null", None));
        }
        let ind = self.indent(self.pos);
        if ind < min_indent {
            return Ok(json_variant("Null", None));
        }
        let content = self.content(self.pos);
        if content == "-" || content.starts_with("- ") {
            self.parse_block_seq(ind)
        } else if yaml_is_map_entry(&content) {
            self.parse_block_map(ind)
        } else {
            self.pos += 1;
            self.parse_inline_value(&content)
        }
    }

    fn parse_block_seq(&mut self, indent: usize) -> Result<CtValue, YamlParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_ignorable();
            if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                break;
            }
            let ind = self.indent(self.pos);
            if ind != indent {
                break;
            }
            let content = self.content(self.pos);
            if content != "-" && !content.starts_with("- ") {
                break;
            }
            let line = &mut self.lines[self.pos];
            let chars: Vec<char> = line.chars().collect();
            let mut rebuilt: String = chars
                .iter()
                .enumerate()
                .map(|(i, c)| if i == indent { ' ' } else { *c })
                .collect();
            if rebuilt.trim().is_empty() {
                rebuilt = String::new();
            }
            *line = rebuilt;
            let item = self.parse_node(indent + 1)?;
            items.push(item);
        }
        Ok(json_array(items))
    }

    fn parse_block_map(&mut self, indent: usize) -> Result<CtValue, YamlParseError> {
        let mut entries: Vec<(String, CtValue)> = Vec::new();
        loop {
            self.skip_ignorable();
            if self.pos >= self.lines.len() || self.at_doc_marker() || self.at_doc_end() {
                break;
            }
            let ind = self.indent(self.pos);
            if ind != indent {
                break;
            }
            let content = self.content(self.pos);
            if content.starts_with("- ") || content == "-" || !yaml_is_map_entry(&content) {
                break;
            }
            let line_no = self.pos + 1;
            let (key, rest) = yaml_split_key(&content).ok_or_else(|| YamlParseError {
                line: line_no,
                message: "expected `key: value`".into(),
            })?;
            self.pos += 1;
            let rest = rest.trim();
            let value = if rest.is_empty() {
                self.skip_ignorable();
                if self.pos < self.lines.len()
                    && self.indent(self.pos) > indent
                    && !self.at_doc_marker()
                    && !self.at_doc_end()
                {
                    self.parse_node(indent + 1)?
                } else {
                    json_variant("Null", None)
                }
            } else if rest.starts_with('|') || rest.starts_with('>') {
                self.parse_block_scalar(indent, rest)
            } else {
                self.parse_inline_value(rest)?
            };
            entries.push((key, value));
        }
        Ok(json_object(entries))
    }

    fn parse_block_scalar(&mut self, parent_indent: usize, header: &str) -> CtValue {
        let folded = header.starts_with('>');
        let chomp = if header.contains('-') {
            'S'
        } else if header.contains('+') {
            'K'
        } else {
            'C'
        };
        let mut body_lines: Vec<String> = Vec::new();
        let mut block_indent: Option<usize> = None;
        while self.pos < self.lines.len() {
            let raw = &self.lines[self.pos];
            if raw.trim().is_empty() {
                body_lines.push(String::new());
                self.pos += 1;
                continue;
            }
            let ind = self.indent(self.pos);
            if ind <= parent_indent {
                break;
            }
            let bi = *block_indent.get_or_insert(ind);
            let chars: Vec<char> = raw.chars().collect();
            let start = bi.min(chars.len());
            let dedented: String = chars[start..].iter().collect();
            body_lines.push(dedented);
            self.pos += 1;
        }
        let mut text = if folded {
            yaml_fold_lines(&body_lines)
        } else {
            body_lines.join("\n")
        };
        let trimmed = text.trim_end_matches('\n').to_string();
        text = match chomp {
            'S' => trimmed,
            'K' => text.trim_end_matches('\n').to_string() + "\n",
            _ => trimmed + "\n",
        };
        json_variant("Text", Some(CtValue::Str(text)))
    }

    fn parse_inline_value(&mut self, s: &str) -> Result<CtValue, YamlParseError> {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix('&') {
            let mut it = rest.splitn(2, char::is_whitespace);
            let name = it.next().unwrap_or("").to_string();
            let val_str = it.next().unwrap_or("").trim();
            let value = if val_str.is_empty() {
                self.parse_node(0)?
            } else {
                self.parse_inline_value(val_str)?
            };
            self.anchors.insert(name, value.clone());
            return Ok(value);
        }
        if let Some(name) = s.strip_prefix('*') {
            return Ok(self
                .anchors
                .get(name.trim())
                .cloned()
                .unwrap_or(json_variant("Null", None)));
        }
        if s.starts_with('[') || s.starts_with('{') {
            return Ok(yaml_parse_flow(s).0);
        }
        Ok(yaml_scalar_value(s))
    }
}

fn yaml_parse_flow(s: &str) -> (CtValue, usize) {
    let chars: Vec<char> = s.chars().collect();
    yaml_parse_flow_at(&chars, 0)
}
fn yaml_parse_flow_at(chars: &[char], mut i: usize) -> (CtValue, usize) {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() {
        return (json_variant("Null", None), i);
    }
    match chars[i] {
        '[' => {
            i += 1;
            let mut items = Vec::new();
            loop {
                while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
                    i += 1;
                }
                if i >= chars.len() || chars[i] == ']' {
                    i += 1;
                    break;
                }
                let (v, ni) = yaml_parse_flow_at(chars, i);
                items.push(v);
                i = ni;
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                if i < chars.len() && chars[i] == ',' {
                    i += 1;
                } else if i < chars.len() && chars[i] == ']' {
                    i += 1;
                    break;
                }
            }
            (json_array(items), i)
        }
        '{' => {
            i += 1;
            let mut entries = Vec::new();
            loop {
                while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
                    i += 1;
                }
                if i >= chars.len() || chars[i] == '}' {
                    i += 1;
                    break;
                }
                let (key, ni) = yaml_scan_flow_scalar(chars, i, true);
                i = ni;
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                if i < chars.len() && chars[i] == ':' {
                    i += 1;
                }
                let (v, nj) = yaml_parse_flow_at(chars, i);
                i = nj;
                entries.push((key.trim().to_string(), v));
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                if i < chars.len() && chars[i] == ',' {
                    i += 1;
                } else if i < chars.len() && chars[i] == '}' {
                    i += 1;
                    break;
                }
            }
            (json_object(entries), i)
        }
        _ => {
            let (raw, ni) = yaml_scan_flow_scalar(chars, i, false);
            (yaml_scalar_value(raw.trim()), ni)
        }
    }
}
fn yaml_scan_flow_scalar(chars: &[char], mut i: usize, as_key: bool) -> (String, usize) {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
        let q = chars[i];
        let mut out = String::new();
        i += 1;
        while i < chars.len() {
            if chars[i] == q {
                if q == '\'' && i + 1 < chars.len() && chars[i + 1] == '\'' {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                i += 1;
                break;
            }
            if chars[i] == '\\' && q == '"' && i + 1 < chars.len() {
                out.push(yaml_unescape(chars[i + 1]));
                i += 2;
                continue;
            }
            out.push(chars[i]);
            i += 1;
        }
        return (out, i);
    }
    let mut out = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == ',' || c == ']' || c == '}' {
            break;
        }
        if as_key && c == ':' {
            break;
        }
        out.push(c);
        i += 1;
    }
    (out, i)
}
fn yaml_fold_lines(lines: &[String]) -> String {
    let mut out = String::new();
    let mut prev_blank = true;
    for l in lines {
        if l.trim().is_empty() {
            out.push('\n');
            prev_blank = true;
        } else {
            if !prev_blank {
                out.push(' ');
            }
            out.push_str(l);
            prev_blank = false;
        }
    }
    out
}
fn yaml_unescape(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        '\\' => '\\',
        '"' => '"',
        _ => c,
    }
}
fn yaml_strip_comment(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut in_s = false;
    let mut in_d = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' if !in_d => in_s = !in_s,
            '"' if !in_s => in_d = !in_d,
            '#' if !in_s && !in_d && (i == 0 || chars[i - 1] == ' ' || chars[i - 1] == '\t') => {
                let kept: String = chars[..i].iter().collect();
                return kept.trim_end().to_string();
            }
            _ => {}
        }
        i += 1;
    }
    s.trim_end().to_string()
}
fn yaml_is_map_entry(s: &str) -> bool {
    yaml_top_level_colon(s).is_some()
}
fn yaml_top_level_colon(s: &str) -> Option<usize> {
    let chars: Vec<char> = s.chars().collect();
    let mut in_s = false;
    let mut in_d = false;
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' if !in_d => in_s = !in_s,
            '"' if !in_s => in_d = !in_d,
            '[' | '{' if !in_s && !in_d => depth += 1,
            ']' | '}' if !in_s && !in_d => depth -= 1,
            ':' if !in_s && !in_d && depth == 0 => {
                if i + 1 >= chars.len() || chars[i + 1] == ' ' {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
fn yaml_split_key(s: &str) -> Option<(String, String)> {
    let idx = yaml_top_level_colon(s)?;
    let chars: Vec<char> = s.chars().collect();
    let key_raw: String = chars[..idx].iter().collect();
    let rest: String = chars[idx + 1..].iter().collect();
    Some((yaml_unquote_key(key_raw.trim()), rest))
}
fn yaml_unquote_key(k: &str) -> String {
    if (k.starts_with('"') && k.ends_with('"') && k.len() >= 2)
        || (k.starts_with('\'') && k.ends_with('\'') && k.len() >= 2)
    {
        k[1..k.len() - 1].to_string()
    } else {
        k.to_string()
    }
}
fn yaml_scalar_value(s: &str) -> CtValue {
    let s = s.trim();
    if s.is_empty() {
        return json_variant("Null", None);
    }
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        let mut out = String::new();
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(yaml_unescape(n));
                }
            } else {
                out.push(c);
            }
        }
        return json_variant("Text", Some(CtValue::Str(out)));
    }
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        return json_variant(
            "Text",
            Some(CtValue::Str(s[1..s.len() - 1].replace("''", "'"))),
        );
    }
    match s {
        "null" | "Null" | "NULL" | "~" => return json_variant("Null", None),
        "true" | "True" | "TRUE" => return json_variant("Bool", Some(CtValue::Bool(true))),
        "false" | "False" | "FALSE" => return json_variant("Bool", Some(CtValue::Bool(false))),
        ".inf" | ".Inf" | ".INF" => {
            return json_variant("Float", Some(CtValue::Float(f64::INFINITY)))
        }
        "-.inf" | "-.Inf" => return json_variant("Float", Some(CtValue::Float(f64::NEG_INFINITY))),
        ".nan" | ".NaN" | ".NAN" => return json_variant("Float", Some(CtValue::Float(f64::NAN))),
        _ => {}
    }
    if let Ok(n) = s.parse::<i64>() {
        return json_variant("Int", Some(CtValue::Int(n)));
    }
    if (s.contains('.') || s.contains('e') || s.contains('E'))
        && s.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
    {
        if let Ok(f) = s.parse::<f64>() {
            return json_variant("Float", Some(CtValue::Float(f)));
        }
    }
    json_variant("Text", Some(CtValue::Str(s.to_string())))
}

pub(super) fn yaml_render(v: &CtValue) -> String {
    let mut out = String::new();
    yaml_render_node(v, 0, &mut out);
    let s = out.trim_end().to_string();
    if s.is_empty() {
        "{}".to_string()
    } else {
        s
    }
}
fn yaml_render_node(t: &CtValue, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    if let Some(entries) = json_object_entries(t) {
        if entries.is_empty() {
            out.push_str(&format!("{}{{}}\n", pad));
            return;
        }
        for (k, v) in &entries {
            let is_nonempty_obj = json_object_entries(v).map_or(false, |e| !e.is_empty());
            let is_nonempty_arr = json_array_items(v).map_or(false, |a| !a.is_empty());
            if is_nonempty_obj {
                out.push_str(&format!("{}{}:\n", pad, yaml_render_key(k)));
                yaml_render_node(v, indent + 2, out);
            } else if is_nonempty_arr {
                out.push_str(&format!("{}{}:\n", pad, yaml_render_key(k)));
                yaml_render_seq(&json_array_items(v).unwrap(), indent, out);
            } else {
                out.push_str(&format!("{}{}: {}\n", pad, yaml_render_key(k), yaml_render_scalar(v)));
            }
        }
        return;
    }
    if let Some(items) = json_array_items(t) {
        yaml_render_seq(&items, indent, out);
        return;
    }
    out.push_str(&format!("{}{}\n", pad, yaml_render_scalar(t)));
}
fn yaml_render_seq(items: &[CtValue], indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    for item in items {
        let is_nonempty_obj = json_object_entries(item).map_or(false, |e| !e.is_empty());
        let is_nonempty_arr = json_array_items(item).map_or(false, |a| !a.is_empty());
        if is_nonempty_obj {
            out.push_str(&format!("{}-\n", pad));
            yaml_render_node(item, indent + 2, out);
        } else if is_nonempty_arr {
            out.push_str(&format!("{}-\n", pad));
            yaml_render_seq(&json_array_items(item).unwrap(), indent + 2, out);
        } else {
            out.push_str(&format!("{}- {}\n", pad, yaml_render_scalar(item)));
        }
    }
}
fn yaml_render_key(k: &str) -> String {
    if k.is_empty() || k.contains(':') || k.contains(' ') || k.contains('#') {
        format!("{:?}", k)
    } else {
        k.to_string()
    }
}
fn yaml_render_scalar(v: &CtValue) -> String {
    if let Some(entries) = json_object_entries(v) {
        let parts: Vec<String> = entries
            .iter()
            .map(|(k, val)| format!("{}: {}", yaml_render_key(k), yaml_render_scalar(val)))
            .collect();
        return format!("{{{}}}", parts.join(", "));
    }
    if let Some(items) = json_array_items(v) {
        let parts: Vec<String> = items.iter().map(yaml_render_scalar).collect();
        return format!("[{}]", parts.join(", "));
    }
    match v {
        CtValue::Enum { variant, args, .. } => match (variant.as_str(), args.first()) {
            ("Null", _) => "null".to_string(),
            ("Bool", Some((_, CtValue::Bool(b)))) => b.to_string(),
            ("Int", Some((_, CtValue::Int(n)))) => n.to_string(),
            ("Float", Some((_, CtValue::Float(f)))) => format!("{:?}", f),
            ("Text", Some((_, CtValue::Str(s)))) => {
                if yaml_needs_quote(s) {
                    format!("{:?}", s)
                } else {
                    s.clone()
                }
            }
            _ => "null".to_string(),
        },
        _ => "null".to_string(),
    }
}
fn yaml_needs_quote(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    matches!(
        s,
        "null" | "Null" | "NULL" | "~" | "true" | "True" | "TRUE" | "false" | "False" | "FALSE"
    ) || s.parse::<i64>().is_ok()
        || s.parse::<f64>().is_ok()
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.starts_with([
            '-', '?', ':', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@',
            '`',
        ])
        || s.contains(": ")
        || s.contains(" #")
        || s.contains('\n')
}

// ── core.encoding.xml ────────────────────────────────────────────────────────
// Ported from `EncodingCodecs.rs`'s `jet_std_xml_parse`/`jet_std_xml_render`,
// target type swapped from `DataTree` to the `Json`-tagged `CtValue` (each
// XML node is an `Object` with `name`/`attrs`/`children`/`text` fields,
// exactly mirroring `jet_xml_obj`'s shape).

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
fn xml_obj(name: String, attrs: Vec<(String, String)>, children: Vec<CtValue>, text: String) -> CtValue {
    json_object(vec![
        ("name".to_string(), json_variant("Text", Some(CtValue::Str(name)))),
        (
            "attrs".to_string(),
            json_object(
                attrs
                    .into_iter()
                    .map(|(k, v)| (k, json_variant("Text", Some(CtValue::Str(v)))))
                    .collect(),
            ),
        ),
        ("children".to_string(), json_array(children)),
        ("text".to_string(), json_variant("Text", Some(CtValue::Str(text)))),
    ])
}
pub(super) fn xml_parse(text: &str) -> Result<CtValue, String> {
    struct Node {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<CtValue>,
        text: String,
    }
    fn finish(n: Node) -> CtValue {
        xml_obj(n.name, n.attrs, n.children, n.text.trim().to_string())
    }
    fn parse_tag(src: &str) -> Result<(String, Vec<(String, String)>, bool), String> {
        let mut s = src.trim().to_string();
        let self_close = s.ends_with('/');
        if self_close {
            s.pop();
        }
        let mut parts = s.split_whitespace();
        let name = parts
            .next()
            .ok_or_else(|| "empty XML tag".to_string())?
            .to_string();
        let mut attrs = Vec::new();
        let rest = &s[name.len()..];
        let mut i = 0usize;
        let bytes = rest.as_bytes();
        while i < bytes.len() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let start = i;
            while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let key = rest[start..i].trim().to_string();
            while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'=') {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'"' {
                return Err(format!("XML attribute `{key}` needs quoted value"));
            }
            i += 1;
            let val_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            if i >= bytes.len() {
                return Err(format!("XML attribute `{key}` is unterminated"));
            }
            attrs.push((key, xml_unescape(&rest[val_start..i])));
            i += 1;
        }
        Ok((name, attrs, self_close))
    }
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<CtValue> = None;
    let mut i = 0usize;
    while let Some(rel) = text[i..].find('<') {
        let start = i + rel;
        if start > i {
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&xml_unescape(&text[i..start]));
            }
        }
        let end = text[start..]
            .find('>')
            .ok_or_else(|| "unterminated XML tag".to_string())?
            + start;
        let tag = text[start + 1..end].trim();
        if tag.starts_with("!--") || tag.starts_with('?') {
            i = end + 1;
            continue;
        }
        if let Some(close) = tag.strip_prefix('/') {
            let node = stack
                .pop()
                .ok_or_else(|| format!("closing tag </{}> without opener", close.trim()))?;
            if node.name != close.trim() {
                return Err(format!(
                    "closing tag </{}> does not match <{}>",
                    close.trim(),
                    node.name
                ));
            }
            let tree = finish(node);
            if let Some(parent) = stack.last_mut() {
                parent.children.push(tree);
            } else {
                root = Some(tree);
            }
        } else {
            let (name, attrs, self_close) = parse_tag(tag)?;
            let node = Node {
                name,
                attrs,
                children: Vec::new(),
                text: String::new(),
            };
            if self_close {
                let tree = finish(node);
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(tree);
                } else {
                    root = Some(tree);
                }
            } else {
                stack.push(node);
            }
        }
        i = end + 1;
    }
    if i < text.len() {
        if let Some(top) = stack.last_mut() {
            top.text.push_str(&xml_unescape(&text[i..]));
        }
    }
    if !stack.is_empty() {
        return Err(format!("unclosed XML tag <{}>", stack.last().unwrap().name));
    }
    root.ok_or_else(|| "empty XML document".to_string())
}
pub(super) fn xml_render(d: &CtValue) -> String {
    fn field(d: &CtValue, name: &str) -> Option<CtValue> {
        json_object_entries(d)?
            .into_iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }
    fn text_of(v: &Option<CtValue>) -> Option<String> {
        match v {
            Some(v) => match json_payload(v, "Text") {
                Some(CtValue::Str(s)) => Some(s.clone()),
                _ => None,
            },
            None => None,
        }
    }
    fn render_node(d: &CtValue) -> String {
        let name = text_of(&field(d, "name")).unwrap_or_else(|| "node".to_string());
        let attrs = match field(d, "attrs") {
            Some(v) => json_object_entries(&v)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(k, v)| match json_payload(&v, "Text") {
                    Some(CtValue::Str(s)) => Some(format!(" {}=\"{}\"", k, xml_escape(s))),
                    _ => None,
                })
                .collect::<String>(),
            None => String::new(),
        };
        let text = text_of(&field(d, "text")).map(|s| xml_escape(&s)).unwrap_or_default();
        let children = match field(d, "children") {
            Some(v) => json_array_items(&v)
                .unwrap_or_default()
                .iter()
                .map(render_node)
                .collect::<String>(),
            None => String::new(),
        };
        if text.is_empty() && children.is_empty() {
            format!("<{name}{attrs}/>")
        } else {
            format!("<{name}{attrs}>{text}{children}</{name}>")
        }
    }
    render_node(d)
}

// ── core.encoding.cbor ──────────────────────────────────────────────────────
// Ported from `EncodingCodecs.rs`'s `jet_cbor_*`/`jet_std_cbor_*`, target
// type swapped from `DataTree` to the `Json`-tagged `CtValue`.

fn cbor_push_len(out: &mut Vec<u8>, major: u8, n: u64) {
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
fn cbor_encode_val(v: &CtValue, out: &mut Vec<u8>) {
    if let Some(entries) = json_object_entries(v) {
        let mut sorted = entries;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        cbor_push_len(out, 5, sorted.len() as u64);
        for (k, val) in sorted {
            cbor_encode_val(&json_variant("Text", Some(CtValue::Str(k))), out);
            cbor_encode_val(&val, out);
        }
        return;
    }
    if let Some(items) = json_array_items(v) {
        cbor_push_len(out, 4, items.len() as u64);
        for x in &items {
            cbor_encode_val(x, out);
        }
        return;
    }
    match v {
        CtValue::Enum { variant, args, .. } => match (variant.as_str(), args.first()) {
            ("Null", _) => out.push(0xf6),
            ("Bool", Some((_, CtValue::Bool(false)))) => out.push(0xf4),
            ("Bool", Some((_, CtValue::Bool(true)))) => out.push(0xf5),
            ("Int", Some((_, CtValue::Int(n)))) if *n >= 0 => cbor_push_len(out, 0, *n as u64),
            ("Int", Some((_, CtValue::Int(n)))) => cbor_push_len(out, 1, (-1 - *n) as u64),
            ("Float", Some((_, CtValue::Float(f)))) => {
                out.push(0xfb);
                out.extend_from_slice(&f.to_be_bytes());
            }
            ("Text", Some((_, CtValue::Str(s)))) => {
                cbor_push_len(out, 3, s.len() as u64);
                out.extend_from_slice(s.as_bytes());
            }
            _ => out.push(0xf6),
        },
        _ => out.push(0xf6),
    }
}
pub(super) fn cbor_encode(v: &CtValue) -> Vec<u8> {
    let mut out = Vec::new();
    cbor_encode_val(v, &mut out);
    out
}
fn cbor_read_len(input: &[u8], i: &mut usize, add: u8) -> Result<u64, String> {
    let need = match add {
        n @ 0..=23 => return Ok(n as u64),
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => return Err("unsupported CBOR indefinite/simple length".to_string()),
    };
    if *i + need > input.len() {
        return Err("truncated CBOR length".to_string());
    }
    let mut n = 0u64;
    for _ in 0..need {
        n = (n << 8) | input[*i] as u64;
        *i += 1;
    }
    Ok(n)
}
fn cbor_decode_val(input: &[u8], i: &mut usize) -> Result<CtValue, String> {
    if *i >= input.len() {
        return Err("truncated CBOR value".to_string());
    }
    let b = input[*i];
    *i += 1;
    let major = b >> 5;
    let add = b & 31;
    match major {
        0 => Ok(json_variant(
            "Int",
            Some(CtValue::Int(cbor_read_len(input, i, add)? as i64)),
        )),
        1 => Ok(json_variant(
            "Int",
            Some(CtValue::Int(-1 - cbor_read_len(input, i, add)? as i64)),
        )),
        2 | 3 => {
            let n = cbor_read_len(input, i, add)? as usize;
            if *i + n > input.len() {
                return Err("truncated CBOR bytes/text".to_string());
            }
            let bytes = input[*i..*i + n].to_vec();
            *i += n;
            if major == 2 {
                // D-SERDE2 `DataTree::Bytes` has no `Json` counterpart (the
                // dynamic `Json`/`Data` tagged enum only carries the
                // JSON-native leaf shapes) — AOT's CBOR decode never actually
                // produces this arm either except via an explicit `Bytes`
                // construction, which nothing in `core.encoding.cbor`'s own
                // encode path emits (`jet_cbor_encode_val` only ever writes
                // major type 2 for `DataTree::Bytes`, never for a `Text`).
                // Round-tripping arbitrary externally-produced CBOR bytes
                // strings is therefore represented as `Text` here, matching
                // every other leaf.
                String::from_utf8(bytes.clone())
                    .map(|s| json_variant("Text", Some(CtValue::Str(s))))
                    .or_else(|_| Ok(json_variant("Text", Some(CtValue::Str(String::from_utf8_lossy(&bytes).into_owned())))))
            } else {
                String::from_utf8(bytes)
                    .map(|s| json_variant("Text", Some(CtValue::Str(s))))
                    .map_err(|_| "CBOR text is not UTF-8".to_string())
            }
        }
        4 => {
            let n = cbor_read_len(input, i, add)? as usize;
            let mut xs = Vec::with_capacity(n);
            for _ in 0..n {
                xs.push(cbor_decode_val(input, i)?);
            }
            Ok(json_array(xs))
        }
        5 => {
            let n = cbor_read_len(input, i, add)? as usize;
            let mut es = Vec::with_capacity(n);
            for _ in 0..n {
                let k = match cbor_decode_val(input, i)? {
                    v => match json_payload(&v, "Text") {
                        Some(CtValue::Str(s)) => s.clone(),
                        _ => return Err("CBOR map key must be text".to_string()),
                    },
                };
                es.push((k, cbor_decode_val(input, i)?));
            }
            Ok(json_object(es))
        }
        7 => match add {
            20 => Ok(json_variant("Bool", Some(CtValue::Bool(false)))),
            21 => Ok(json_variant("Bool", Some(CtValue::Bool(true)))),
            22 => Ok(json_variant("Null", None)),
            27 => {
                if *i + 8 > input.len() {
                    return Err("truncated CBOR float".to_string());
                }
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&input[*i..*i + 8]);
                *i += 8;
                Ok(json_variant("Float", Some(CtValue::Float(f64::from_be_bytes(buf)))))
            }
            _ => Err("unsupported CBOR simple value".to_string()),
        },
        _ => Err("unsupported CBOR major type".to_string()),
    }
}
pub(super) fn cbor_decode(bytes: &[u8]) -> Result<CtValue, String> {
    let mut i = 0usize;
    let v = cbor_decode_val(bytes, &mut i)?;
    if i != bytes.len() {
        return Err("trailing CBOR bytes".to_string());
    }
    Ok(v)
}

// ── core.encoding.jsonl ─────────────────────────────────────────────────────
// Ported from `MathRandomTime.rs`'s `jet_std_jsonl_parse`/`jet_std_jsonl_render`.

pub(super) fn jsonl_parse(text: &str) -> Result<Vec<CtValue>, CtValue> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match super::JsonInterp::parse_json(trimmed) {
            Ok(v) => out.push(v),
            Err(e) => return Err(super::JsonInterp::json_error_value_at_line(e, idx as i64)),
        }
    }
    Ok(out)
}
pub(super) fn jsonl_render(rows: &[CtValue]) -> String {
    let mut out = rows
        .iter()
        .map(|v| super::JsonInterp::render_json_pretty(v, false, 0))
        .collect::<Vec<_>>()
        .join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

// ── core.encoding.json: canonical / events ─────────────────────────────────
// Ported from `MathRandomTime.rs`'s `jet_std_json_render_canonical`/
// `jet_std_json_events`. `render_json_pretty(v, false, 0)` already walks a
// `BTreeMap`-backed `Object` (sorted by key) so it's already canonical for
// object key order — the only remaining difference from a plain
// `to_string()` on this tier is that AOT's canonical form is a distinct,
// explicitly-sorting function (its plain `to_string` preserves original
// `Vec`-based insertion order); comptime's single `Map` representation
// makes the two calls coincide, which is a strict special case of the same
// canonical text AOT produces for `canonical()`, so reusing it here is exact
// for that call specifically.
pub(super) fn json_canonical(v: &CtValue) -> String {
    super::JsonInterp::render_json_pretty(v, false, 0)
}
pub(super) fn json_events(v: &CtValue) -> String {
    fn walk(path: String, t: &CtValue, out: &mut Vec<String>) {
        let here = if path.is_empty() { "$".to_string() } else { path };
        if let Some(entries) = json_object_entries(t) {
            out.push(format!("object_start {here}"));
            for (k, v) in &entries {
                walk(format!("{}.{}", here, k), v, out);
            }
            out.push(format!("object_end {here}"));
            return;
        }
        if let Some(items) = json_array_items(t) {
            out.push(format!("array_start {here}"));
            for (i, v) in items.iter().enumerate() {
                walk(format!("{}[{}]", here, i), v, out);
            }
            out.push(format!("array_end {here}"));
            return;
        }
        out.push(format!("value {here} {}", json_canonical(t)));
    }
    let mut out = Vec::new();
    walk(String::new(), v, &mut out);
    out.join("\n")
}
