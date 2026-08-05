//! D-SYNC1=A / D-DBPOLICY1=A / I9: `core.sync` ambient includes Prelude `Sync.rs`.

use crate::AST::{CtValue, Type};
use crate::Diagnostics::{Diagnostic, Span};
use super::Diagnostics::unsupported;

// `Sync.rs` is shared verbatim with the AOT Prelude.  The comptime tier
// supplies the same small host boundary that the emitted module gets from
// `CoreLib/JetStd`: canonical DataTree values, FieldError lists, and Codable
// traits.  The CRDT and policy algorithms themselves remain in the included
// Prelude source.
trait JetShow {
    fn jet_show(&self) -> String;
}

#[allow(non_camel_case_types)]
trait user_Encode {
    fn jet_encode(&self) -> jet_std::DataTree;
}

#[allow(non_camel_case_types)]
trait user_Decode: Sized {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>>;
}

mod jet_std {
    include!("../../../jet-codegen/src/Prelude/CoreLib/JetStd/DataTree.rs");

    fn quote_json(s: &str) -> String {
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

    pub fn render_datatree_json(tree: &DataTree, pretty: bool, depth: usize) -> String {
        match tree {
            DataTree::Null => "null".to_string(),
            DataTree::Bool(value) => value.to_string(),
            DataTree::Int(value) => value.to_string(),
            DataTree::Float(value) => format!("{:?}", value),
            DataTree::Text(value) => quote_json(value),
            DataTree::Bytes(values) => format!(
                "[{}]",
                values.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
            ),
            DataTree::Array(values) => {
                if values.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    return format!(
                        "[{}]",
                        values
                            .iter()
                            .map(|value| render_datatree_json(value, false, depth))
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts = values
                    .iter()
                    .map(|value| format!("{}{}", pad, render_datatree_json(value, true, depth + 1)))
                    .collect::<Vec<_>>();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            DataTree::Object(fields) => {
                if fields.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    return format!(
                        "{{{}}}",
                        fields
                            .iter()
                            .map(|(key, value)| {
                                format!(
                                    "{}:{}",
                                    quote_json(key),
                                    render_datatree_json(value, false, depth)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts = fields
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{}{}: {}",
                            pad,
                            quote_json(key),
                            render_datatree_json(value, true, depth + 1)
                        )
                    })
                    .collect::<Vec<_>>();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    pub fn datatree_kind(tree: &DataTree) -> &'static str {
        match tree {
            DataTree::Null => "null",
            DataTree::Bool(_) => "Bool",
            DataTree::Int(_) => "Int",
            DataTree::Float(_) => "Float",
            DataTree::Text(_) => "Text",
            DataTree::Bytes(_) => "Bytes",
            DataTree::Array(_) => "a list",
            DataTree::Object(_) => "an object",
        }
    }
}

impl user_Encode for String {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(self.clone())
    }
}

impl user_Decode for String {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_string(tree)
    }
}

impl user_Encode for i64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Int(*self)
    }
}

impl user_Decode for i64 {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_int(tree)
    }
}

impl user_Encode for u64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Int(*self as i64)
    }
}

impl user_Decode for u64 {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_int(tree).and_then(|value| {
            u64::try_from(value).map_err(|_| jet_std::FieldError::one("expected non-negative integer"))
        })
    }
}

impl user_Encode for bool {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Bool(*self)
    }
}

impl user_Decode for bool {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_bool(tree)
    }
}

impl user_Encode for f64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Float(*self)
    }
}

impl user_Decode for f64 {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_float(tree)
    }
}

include!("../../../jet-codegen/src/Prelude/CoreLib/Top/Sync.rs");

fn text_to_ct(doc: &JetSyncText) -> CtValue {
    CtValue::Struct {
        type_name: "SyncText".to_string(),
        fields: vec![
            ("show".to_string(), CtValue::Str(jet_sync_text_show(doc))),
            (
                "atoms".to_string(),
                CtValue::List(
                    doc.atoms
                        .iter()
                        .map(|atom| CtValue::Struct {
                            type_name: "SyncTextAtom".to_string(),
                            fields: vec![
                                ("replica".to_string(), CtValue::Str(atom.replica.clone())),
                                ("counter".to_string(), CtValue::Str(atom.counter.to_string())),
                                (
                                    "after".to_string(),
                                    CtValue::Str(match &atom.after {
                                        Some((replica, counter)) => {
                                            format!("{replica}:{counter}")
                                        }
                                        None => String::new(),
                                    }),
                                ),
                                ("ch".to_string(), CtValue::Str(atom.ch.to_string())),
                                (
                                    "deleted".to_string(),
                                    CtValue::Str(atom.deleted.to_string()),
                                ),
                            ],
                        })
                        .collect(),
                ),
            ),
        ],
    }
}

fn counter_to_ct(c: &JetSyncCounter) -> CtValue {
    CtValue::Struct {
        type_name: "SyncCounter".to_string(),
        fields: vec![(
            "counts".to_string(),
            CtValue::List(
                c.counts
                    .iter()
                    .map(|(replica, positive, negative)| {
                        CtValue::Struct {
                            type_name: "SyncCounterEntry".to_string(),
                            fields: vec![
                                ("replica".to_string(), CtValue::Str(replica.clone())),
                                ("positive".to_string(), CtValue::Str(positive.to_string())),
                                ("negative".to_string(), CtValue::Str(negative.to_string())),
                            ],
                        }
                    })
                    .collect(),
            ),
        )],
    }
}

fn map_to_ct(m: &JetSyncMap) -> CtValue {
    CtValue::Struct {
        type_name: "SyncMap".to_string(),
        fields: vec![
            ("show".to_string(), CtValue::Str(jet_sync_map_show(m))),
            (
                "entries".to_string(),
                CtValue::List(
                    m.entries
                        .iter()
                        .map(|(key, value, clock, writer)| CtValue::Struct {
                            type_name: "SyncMapEntry".to_string(),
                            fields: vec![
                                ("key".to_string(), CtValue::Str(key.clone())),
                                ("value".to_string(), CtValue::Str(value.clone())),
                                ("clock".to_string(), CtValue::Str(clock.to_string())),
                                ("writer".to_string(), CtValue::Str(writer.clone())),
                            ],
                        })
                        .collect(),
                ),
            ),
        ],
    }
}

fn list_to_ct(l: &JetSyncList) -> CtValue {
    CtValue::Struct {
        type_name: "SyncList".to_string(),
        fields: vec![
            ("show".to_string(), CtValue::Str(jet_sync_list_show(l))),
            (
                "items".to_string(),
                CtValue::List(
                    l.items
                        .iter()
                        .map(|(replica, item)| CtValue::Struct {
                            type_name: "SyncListItem".to_string(),
                            fields: vec![
                                ("replica".to_string(), CtValue::Str(replica.clone())),
                                ("item".to_string(), CtValue::Str(item.clone())),
                            ],
                        })
                        .collect(),
                ),
            ),
        ],
    }
}

fn policy_to_ct(p: &JetRowPolicy) -> CtValue {
    CtValue::Struct {
        type_name: "RowPolicy".to_string(),
        fields: vec![
            ("table".to_string(), CtValue::Str(p.table.clone())),
            ("expression".to_string(), CtValue::Str(p.expression.clone())),
        ],
    }
}

fn ct_to_text(v: &CtValue, span: Span) -> Result<JetSyncText, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields }
            if type_name == "SyncText" || type_name == "JetSyncText" =>
        {
            let Some(value) = fields.iter().find(|(n, _)| n == "atoms").map(|(_, v)| v) else {
                return Err(unsupported("SyncText atoms", span));
            };
            let CtValue::List(entries) = value else {
                return Err(unsupported("SyncText atoms", span));
            };
            if entries.len() > MAX_SYNC_ATOMS {
                return Err(unsupported("SyncText atom limit", span));
            }
            let mut atoms = Vec::with_capacity(entries.len());
            let mut seen: std::collections::BTreeSet<(String, u64)> =
                std::collections::BTreeSet::new();
            for entry in entries {
                let CtValue::Struct { type_name, fields } = entry else {
                    return Err(unsupported("SyncText atom", span));
                };
                if type_name != "SyncTextAtom" {
                    return Err(unsupported("SyncText atom", span));
                }
                let field = |name: &str| {
                    fields
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, v)| v)
                        .ok_or_else(|| unsupported("SyncText atom field", span))
                };
                let text = |name: &str| match field(name)? {
                    CtValue::Str(s) => Ok(s.clone()),
                    _ => Err(unsupported("SyncText atom field", span)),
                };
                let replica = text("replica")?;
                let counter = match field("counter")? {
                    CtValue::Int(n) if *n >= 0 => u64::try_from(*n)
                        .map_err(|_| unsupported("SyncText atom counter", span))?,
                    CtValue::Str(value) => value
                        .parse::<u64>()
                        .map_err(|_| unsupported("SyncText atom counter", span))?,
                    _ => return Err(unsupported("SyncText atom counter", span)),
                };
                let after = text("after")?;
                let after = if after.is_empty() {
                    None
                } else {
                    let (replica, counter) = after
                        .rsplit_once(':')
                        .ok_or_else(|| unsupported("SyncText atom anchor", span))?;
                    let counter = counter
                        .parse::<u64>()
                        .map_err(|_| unsupported("SyncText atom anchor", span))?;
                    if !jet_sync_token_is_valid(replica) {
                        return Err(unsupported("SyncText atom anchor", span));
                    }
                    Some((replica.to_string(), counter))
                };
                let ch = text("ch")?;
                let mut chars = ch.chars();
                let (Some(ch), None) = (chars.next(), chars.next()) else {
                    return Err(unsupported("SyncText atom character", span));
                };
                let deleted = match text("deleted")?.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(unsupported("SyncText atom tombstone", span)),
                };
                if !jet_sync_token_is_valid(&replica) {
                    return Err(unsupported("SyncText atom replica", span));
                }
                if counter == 0 || !seen.insert((replica.clone(), counter)) {
                    return Err(unsupported("SyncText atom identity", span));
                }
                atoms.push(JetSyncTextAtom {
                    replica,
                    counter,
                    after,
                    ch,
                    deleted,
                });
            }
            // I9: refuse here exactly what the Prelude decode refuses, so the
            // comptime tier cannot evaluate a document AOT would reject.
            let doc = JetSyncText { atoms };
            if jet_sync_text_order(&doc).len() != doc.atoms.len() {
                return Err(unsupported("SyncText reachability", span));
            }
            Ok(doc)
        }
        _ => Err(unsupported("SyncText", span)),
    }
}

fn ct_to_counter(v: &CtValue, span: Span) -> Result<JetSyncCounter, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields }
            if type_name == "SyncCounter" || type_name == "JetSyncCounter" =>
        {
            if let Some(value) = fields.iter().find(|(n, _)| n == "counts").map(|(_, v)| v) {
                let CtValue::List(entries) = value else {
                    return Err(unsupported("SyncCounter counts", span));
                };
                if entries.len() > MAX_SYNC_REPLICAS {
                    return Err(unsupported("SyncCounter replica limit", span));
                }
                let mut counts = Vec::new();
                for entry in entries {
                    match entry {
                        CtValue::Struct {
                            type_name,
                            fields: entry_fields,
                        } if type_name == "SyncCounterEntry" => {
                            let replica = entry_fields
                                .iter()
                                .find(|(n, _)| n == "replica")
                                .and_then(|(_, v)| match v {
                                    CtValue::Str(s) => Some(s.clone()),
                                    _ => None,
                                })
                                .ok_or_else(|| unsupported("SyncCounter replica", span))?;
                            let positive = entry_fields
                                .iter()
                                .find(|(n, _)| n == "positive")
                                .and_then(|(_, v)| match v {
                                    CtValue::Int(n) if *n >= 0 => u64::try_from(*n).ok(),
                                    CtValue::Str(value) => value.parse::<u64>().ok(),
                                    _ => None,
                                })
                                .ok_or_else(|| unsupported("SyncCounter positive", span))?;
                            let negative = entry_fields
                                .iter()
                                .find(|(n, _)| n == "negative")
                                .and_then(|(_, v)| match v {
                                    CtValue::Int(n) if *n >= 0 => u64::try_from(*n).ok(),
                                    CtValue::Str(value) => value.parse::<u64>().ok(),
                                    _ => None,
                                })
                                .ok_or_else(|| unsupported("SyncCounter negative", span))?;
                            if !jet_sync_token_is_valid(&replica) {
                                return Err(unsupported("SyncCounter replica", span));
                            }
                            counts.push((replica, positive, negative));
                        }
                        CtValue::List(parts) if parts.len() == 3 => {
                            let replica = match &parts[0] {
                                CtValue::Str(s) => s.clone(),
                                _ => return Err(unsupported("SyncCounter replica", span)),
                            };
                            let positive = match &parts[1] {
                                CtValue::Int(n) if *n >= 0 => u64::try_from(*n)
                                    .map_err(|_| unsupported("SyncCounter positive", span))?,
                                _ => return Err(unsupported("SyncCounter positive", span)),
                            };
                            let negative = match &parts[2] {
                                CtValue::Int(n) if *n >= 0 => u64::try_from(*n)
                                    .map_err(|_| unsupported("SyncCounter negative", span))?,
                                _ => return Err(unsupported("SyncCounter negative", span)),
                            };
                            if !jet_sync_token_is_valid(&replica) {
                                return Err(unsupported("SyncCounter replica", span));
                            }
                            counts.push((replica, positive, negative));
                        }
                        CtValue::List(parts) if parts.len() == 2 => {
                            let replica = match &parts[0] {
                                CtValue::Str(s) => s.clone(),
                                _ => return Err(unsupported("SyncCounter replica", span)),
                            };
                            let value = match &parts[1] {
                                CtValue::Int(n) => *n,
                                _ => return Err(unsupported("SyncCounter value", span)),
                            };
                            if !jet_sync_token_is_valid(&replica) {
                                return Err(unsupported("SyncCounter replica", span));
                            }
                            counts.push((
                                replica,
                                if value >= 0 { value as u64 } else { 0 },
                                if value < 0 { value.unsigned_abs() } else { 0 },
                            ));
                        }
                        _ => return Err(unsupported("SyncCounter entry", span)),
                    }
                }
                return Ok(JetSyncCounter { counts });
            }
            // Legacy ambient shape stored only the summed value.
            let value = match fields.iter().find(|(n, _)| n == "value") {
                Some((_, CtValue::Int(n))) => *n,
                Some(_) => return Err(unsupported("SyncCounter value", span)),
                None => return Err(unsupported("SyncCounter counts", span)),
            };
            Ok(JetSyncCounter {
                counts: vec![
                    (
                        "ambient".to_string(),
                        if value >= 0 { value as u64 } else { 0 },
                        if value < 0 { value.unsigned_abs() } else { 0 },
                    ),
                ],
            })
        }
        _ => Err(unsupported("SyncCounter", span)),
    }
}

fn ct_to_map(v: &CtValue, span: Span) -> Result<JetSyncMap, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields }
            if type_name == "SyncMap" || type_name == "JetSyncMap" =>
        {
            if let Some(value) = fields.iter().find(|(n, _)| n == "entries").map(|(_, v)| v) {
                let CtValue::List(entries) = value else {
                    return Err(unsupported("SyncMap entries", span));
                };
                if entries.len() > MAX_SYNC_ENTRIES {
                    return Err(unsupported("SyncMap entry limit", span));
                }
                let mut map_entries = Vec::with_capacity(entries.len());
                for entry in entries {
                    let CtValue::Struct { type_name, fields } = entry else {
                        return Err(unsupported("SyncMap entry", span));
                    };
                    if type_name != "SyncMapEntry" {
                        return Err(unsupported("SyncMap entry", span));
                    }
                    let field = |name: &str| {
                        fields
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, v)| v)
                            .ok_or_else(|| unsupported("SyncMap entry field", span))
                    };
                    let key = match field("key")? {
                        CtValue::Str(s) => s.clone(),
                        _ => return Err(unsupported("SyncMap key", span)),
                    };
                    let value = match field("value")? {
                        CtValue::Str(s) => s.clone(),
                        _ => return Err(unsupported("SyncMap value", span)),
                    };
                    let clock = match field("clock")? {
                        CtValue::Int(n) if *n >= 0 => u64::try_from(*n)
                            .map_err(|_| unsupported("SyncMap clock", span))?,
                        CtValue::Str(value) => value
                            .parse::<u64>()
                            .map_err(|_| unsupported("SyncMap clock", span))?,
                        _ => return Err(unsupported("SyncMap clock", span)),
                    };
                    let writer = match field("writer")? {
                        CtValue::Str(s) => s.clone(),
                        _ => return Err(unsupported("SyncMap writer", span)),
                    };
                    if !jet_sync_token_is_valid(&key)
                        || !jet_sync_token_is_valid(&writer)
                        || value.len() > MAX_SYNC_TEXT
                    {
                        return Err(unsupported("SyncMap entry value", span));
                    }
                    map_entries.push((key, value, clock, writer));
                }
                return Ok(JetSyncMap { entries: map_entries });
            }
            let show = match fields.iter().find(|(n, _)| n == "show") {
                Some((_, CtValue::Str(s))) => s.clone(),
                Some(_) => return Err(unsupported("SyncMap show", span)),
                None => return Err(unsupported("SyncMap entries", span)),
            };
            let body = show
                .strip_prefix("SyncMap(")
                .and_then(|s| s.strip_suffix(')'))
                .ok_or_else(|| unsupported("SyncMap show", span))?;
            let mut map = JetSyncMap {
                entries: Vec::new(),
            };
            if !body.is_empty() {
                for part in body.split(',') {
                    let (k, val) = part
                        .split_once('=')
                        .ok_or_else(|| unsupported("SyncMap show", span))?;
                    if !jet_sync_token_is_valid(k) || val.len() > MAX_SYNC_TEXT {
                        return Err(unsupported("SyncMap show", span));
                    }
                    map.entries
                        .push((k.to_string(), val.to_string(), 1, "ambient".to_string()));
                }
            }
            if map.entries.len() > MAX_SYNC_ENTRIES {
                return Err(unsupported("SyncMap entry limit", span));
            }
            Ok(map)
        }
        _ => Err(unsupported("SyncMap", span)),
    }
}

fn ct_to_list(v: &CtValue, span: Span) -> Result<JetSyncList, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields }
            if type_name == "SyncList" || type_name == "JetSyncList" =>
        {
            if let Some(items) = fields.iter().find(|(n, _)| n == "items").map(|(_, v)| v) {
                let CtValue::List(items) = items else {
                    return Err(unsupported("SyncList items", span));
                };
                if items.len() > MAX_SYNC_ENTRIES {
                    return Err(unsupported("SyncList item limit", span));
                }
                let mut list = JetSyncList { items: Vec::with_capacity(items.len()) };
                for item in items {
                    let CtValue::Struct { type_name, fields } = item else {
                        return Err(unsupported("SyncList item", span));
                    };
                    if type_name != "SyncListItem" {
                        return Err(unsupported("SyncList item", span));
                    }
                    let field = |name: &str| {
                        fields
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, v)| v)
                            .ok_or_else(|| unsupported("SyncList item field", span))
                    };
                    let replica = match field("replica")? {
                        CtValue::Str(value) => value.clone(),
                        _ => return Err(unsupported("SyncList replica", span)),
                    };
                    let value = match field("item")? {
                        CtValue::Str(value) => value.clone(),
                        _ => return Err(unsupported("SyncList item value", span)),
                    };
                    if !jet_sync_token_is_valid(&replica) || value.len() > MAX_SYNC_TEXT {
                        return Err(unsupported("SyncList item value", span));
                    }
                    list.items.push((replica, value));
                }
                return Ok(list);
            }
            let show = match fields.iter().find(|(n, _)| n == "show") {
                Some((_, CtValue::Str(s))) => s.clone(),
                Some(_) => return Err(unsupported("SyncList show", span)),
                None => return Err(unsupported("SyncList items", span)),
            };
            let body = show
                .strip_prefix("SyncList(")
                .and_then(|s| s.strip_suffix(')'))
                .ok_or_else(|| unsupported("SyncList show", span))?;
            let mut list = JetSyncList { items: Vec::new() };
            if !body.is_empty() {
                for part in body.split('|') {
                    let (r, i) = part
                        .split_once(':')
                        .ok_or_else(|| unsupported("SyncList show", span))?;
                    if !jet_sync_token_is_valid(r) || i.len() > MAX_SYNC_TEXT {
                        return Err(unsupported("SyncList show", span));
                    }
                    list.items.push((r.to_string(), i.to_string()));
                }
            }
            if list.items.len() > MAX_SYNC_ENTRIES {
                return Err(unsupported("SyncList item limit", span));
            }
            Ok(list)
        }
        _ => Err(unsupported("SyncList", span)),
    }
}

fn ct_to_policy(v: &CtValue, span: Span) -> Result<JetRowPolicy, Diagnostic> {
    let CtValue::Struct { type_name, fields } = v else {
        return Err(unsupported("RowPolicy", span));
    };
    if type_name != "RowPolicy" && type_name != "JetRowPolicy" {
        return Err(unsupported("RowPolicy", span));
    }
    let table = fields
        .iter()
        .find(|(n, _)| n == "table")
        .and_then(|(_, v)| match v {
            CtValue::Str(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| unsupported("table", span))?;
    let expression = fields
        .iter()
        .find(|(n, _)| n == "expression")
        .and_then(|(_, v)| match v {
            CtValue::Str(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| unsupported("expression", span))?;
    jet_db_policy_new(table, expression)
        .map_err(|_| unsupported("unsupported row policy", span))
}

pub fn apply(method: &str, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let one = |i: usize| {
        args.get(i)
            .ok_or_else(|| unsupported(&format!("core.sync.{method} arg {i}"), span))
    };
    let as_str = |i: usize| match one(i)? {
        CtValue::Str(s) => Ok(s.clone()),
        _ => Err(unsupported("String", span)),
    };
    let as_int = |i: usize| match one(i)? {
        CtValue::Int(n) => Ok(*n),
        _ => Err(unsupported("Int", span)),
    };
    match method {
        "text_new" => Ok(text_to_ct(&jet_sync_text_new(as_str(0)?, as_str(1)?))),
        "text_set" => Ok(text_to_ct(&jet_sync_text_set(
            ct_to_text(one(0)?, span)?,
            as_str(1)?,
            as_str(2)?,
        ))),
        "text_merge" => Ok(text_to_ct(&jet_sync_text_merge(
            &ct_to_text(one(0)?, span)?,
            &ct_to_text(one(1)?, span)?,
        ))),
        "text_show" => Ok(CtValue::Str(jet_sync_text_show(&ct_to_text(
            one(0)?, span,
        )?))),
        "text_edit" => Ok(text_to_ct(&jet_sync_text_edit(
            ct_to_text(one(0)?, span)?,
            as_str(1)?,
            as_int(2)?,
            as_int(3)?,
            as_str(4)?,
        ))),
        "text_metadata" => Ok(CtValue::Str(jet_sync_text_metadata(&ct_to_text(
            one(0)?, span,
        )?))),
        "counter_new" => Ok(counter_to_ct(&jet_sync_counter_new(as_str(0)?, as_int(1)?))),
        "counter_inc" => Ok(counter_to_ct(&jet_sync_counter_inc(
            ct_to_counter(one(0)?, span)?,
            as_str(1)?,
            as_int(2)?,
        ))),
        "counter_merge" => Ok(counter_to_ct(&jet_sync_counter_merge(
            &ct_to_counter(one(0)?, span)?,
            &ct_to_counter(one(1)?, span)?,
        ))),
        "counter_value" => Ok(CtValue::Int(jet_sync_counter_value(&ct_to_counter(
            one(0)?, span,
        )?))),
        "map_new" => Ok(map_to_ct(&jet_sync_map_new())),
        "map_set" => Ok(map_to_ct(&jet_sync_map_set(
            ct_to_map(one(0)?, span)?,
            as_str(1)?,
            as_str(2)?,
        ))),
        "map_get" => Ok(match jet_sync_map_get(&ct_to_map(one(0)?, span)?, &as_str(1)?) {
            Some(s) => CtValue::Some(Box::new(CtValue::Str(s))),
            None => CtValue::None(Type::String),
        }),
        "map_merge" => Ok(map_to_ct(&jet_sync_map_merge(
            &ct_to_map(one(0)?, span)?,
            &ct_to_map(one(1)?, span)?,
        ))),
        "map_show" => Ok(CtValue::Str(jet_sync_map_show(&ct_to_map(one(0)?, span)?))),
        "list_new" => Ok(list_to_ct(&jet_sync_list_new())),
        "list_push" => Ok(list_to_ct(&jet_sync_list_push(
            ct_to_list(one(0)?, span)?,
            as_str(1)?,
            as_str(2)?,
        ))),
        "list_merge" => Ok(list_to_ct(&jet_sync_list_merge(
            &ct_to_list(one(0)?, span)?,
            &ct_to_list(one(1)?, span)?,
        ))),
        "list_show" => Ok(CtValue::Str(jet_sync_list_show(&ct_to_list(
            one(0)?, span,
        )?))),
        "policy_new" => Ok(match jet_db_policy_new(as_str(0)?, as_str(1)?) {
            Ok(p) => CtValue::ResOk(Box::new(policy_to_ct(&p))),
            Err(e) => CtValue::ResErr(Box::new(CtValue::Str(e))),
        }),
        "policy_allows" => Ok(CtValue::Bool(jet_db_policy_allows(
            &ct_to_policy(one(0)?, span)?,
            &as_str(1)?,
            &as_str(2)?,
        ))),
        "policy_show" => Ok(CtValue::Str(jet_db_policy_show(&ct_to_policy(
            one(0)?, span,
        )?))),
        "sync_over" => Ok(CtValue::Str(jet_app_sync_over(as_str(0)?, as_str(1)?))),
        "sync" => Ok(CtValue::Str(jet_app_sync(as_str(0)?, as_str(1)?))),
        _ => Err(unsupported(&format!("`core.sync.{method}()`"), span)),
    }
}

#[allow(dead_code)]
fn _type_anchor() -> Type {
    Type::Named("SyncText".to_string())
}
