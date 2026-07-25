//! Card #392 pass 5: typed `Decode` dispatch at comptime — the machinery
//! behind `json.decode<T>` / `json.decode_traced<T>` (and the csv/toml/yaml
//! siblings, plus `core.data.csv<T>`'s row decode). Mirrors AOT's derived
//! `user_Decode::jet_decode` / `jet_decode_traced` field-by-field walk
//! (`Codegen/Items.rs::emit_struct_serde` / `emit_migration_chain_walker`)
//! and `Sema::SchemaMigration`'s `#PublishedSchema` migration chain
//! byte-for-byte (R12 parity) — including error message text (E2410/E2412)
//! and the `DecodeError{path,reason}` / `MigrationStatus{migrated,from,steps}`
//! shapes `jet_std` defines.
//!
//! Operates directly on the `Json`-tagged `CtValue` tree `JsonInterp`/
//! `EncodingLite` already build for every codec (json/csv/toml/yaml) — its
//! variant tags (`Null`/`Bool`/`Int`/`Float`/`Text`/`Array`/`Object`) are
//! exactly AOT's `DataTree` shape (see `datatree_from_json`), so no separate
//! value type is needed at this tier.

use std::collections::BTreeSet;

use crate::AST::{CtFloat, CtKey, Field, Marker, MigrationDecl, MigrationOp, StructDef, Type};
use crate::Diagnostics::{Diagnostic, Span};

use super::Diagnostics::unsupported;
use super::Interpreter::Interp;
use super::JsonInterp::{json_payload, json_variant};
use super::Value::CtValue;

// ── DecodeError / MigrationStatus / DecodeResult CtValue shapes ────────────

pub(super) fn decode_error(reason: impl Into<String>) -> CtValue {
    CtValue::Struct {
        type_name: "DecodeError".to_string(),
        fields: vec![
            ("path".to_string(), CtValue::Str(String::new())),
            ("reason".to_string(), CtValue::Str(reason.into())),
        ],
    }
}

/// Mirrors `jet_std::DecodeError::under` — prefix a child error's path with
/// the field/index segment it occurred under.
pub(super) fn decode_error_under(seg: &str, e: CtValue) -> CtValue {
    match e {
        CtValue::Struct { type_name, mut fields } => {
            if let Some((_, CtValue::Str(path))) = fields.iter_mut().find(|(n, _)| n == "path") {
                *path = if path.is_empty() {
                    seg.to_string()
                } else if path.starts_with('[') {
                    format!("{}{}", seg, path)
                } else {
                    format!("{}.{}", seg, path)
                };
            }
            CtValue::Struct { type_name, fields }
        }
        other => other,
    }
}

fn migration_status(migrated: bool, from: String, steps: Vec<String>) -> CtValue {
    CtValue::Struct {
        type_name: "MigrationStatus".to_string(),
        fields: vec![
            ("migrated".to_string(), CtValue::Bool(migrated)),
            ("from".to_string(), CtValue::Str(from)),
            (
                "steps".to_string(),
                CtValue::List(steps.into_iter().map(CtValue::Str).collect()),
            ),
        ],
    }
}

fn migration_status_fresh() -> CtValue {
    migration_status(false, String::new(), Vec::new())
}

fn migration_migrated(status: &CtValue) -> bool {
    match status {
        CtValue::Struct { fields, .. } => fields
            .iter()
            .find(|(n, _)| n == "migrated")
            .map(|(_, v)| matches!(v, CtValue::Bool(true)))
            .unwrap_or(false),
        _ => false,
    }
}

pub(super) fn decode_result(value: CtValue, migration: CtValue) -> CtValue {
    CtValue::Struct {
        type_name: "DecodeResult".to_string(),
        fields: vec![("value".to_string(), value), ("migration".to_string(), migration)],
    }
}

// ── serde marker helpers (mirrors `Codegen/Items.rs`'s private helpers) ────

fn serde_marker<'a>(markers: &'a [Marker], name: &str) -> Option<&'a Marker> {
    markers.iter().find(|m| m.name == name)
}
fn serde_has(markers: &[Marker], name: &str) -> bool {
    markers.iter().any(|m| m.name == name)
}
fn cap_word(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
fn apply_rename_all(style: &str, name: &str) -> String {
    let words: Vec<&str> = name.split('_').filter(|w| !w.is_empty()).collect();
    match style {
        crate::Syntax::RENAME_ALL_CAMEL => {
            let mut it = words.iter();
            let first = it.next().copied().unwrap_or("").to_string();
            first + &it.map(|w| cap_word(w)).collect::<String>()
        }
        crate::Syntax::RENAME_ALL_PASCAL => words.iter().map(|w| cap_word(w)).collect(),
        crate::Syntax::RENAME_ALL_KEBAB => words.join("-"),
        crate::Syntax::RENAME_ALL_SCREAMING => {
            words.iter().map(|w| w.to_uppercase()).collect::<Vec<_>>().join("_")
        }
        _ => words.join("_"),
    }
}
fn container_rename_all(markers: &[Marker]) -> Option<String> {
    serde_marker(markers, crate::Syntax::ATTR_RENAME_ALL).and_then(|m| match m.args.first() {
        Some(crate::AST::Expr::Ident(n, _)) => Some(n.clone()),
        _ => None,
    })
}
fn marker_str_arg(m: &Marker) -> Option<String> {
    match m.args.first() {
        Some(crate::AST::Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
            crate::AST::StrPart::Lit(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}
fn field_wire_key(style: Option<&str>, f: &Field) -> String {
    if let Some(m) = serde_marker(&f.serde_markers, crate::Syntax::ATTR_RENAME) {
        if let Some(s) = marker_str_arg(m) {
            return s;
        }
    }
    match style {
        Some(st) => apply_rename_all(st, &f.name),
        None => f.name.clone(),
    }
}
/// Mirrors `Codegen/Items.rs::migration_wire_key` — the wire key a (possibly
/// historical, no-longer-present) field name carries.
fn migration_wire_key(style: Option<&str>, s: &StructDef, name: &str) -> String {
    if let Some(f) = s.fields.iter().find(|f| f.name == name) {
        return field_wire_key(style, f);
    }
    match style {
        Some(st) => apply_rename_all(st, name),
        None => name.to_string(),
    }
}

// ── Json-tagged CtValue tree helpers (the `DataTree` shape) ────────────────

fn variant_of(tree: &CtValue) -> Option<(&str, Option<&CtValue>)> {
    match tree {
        CtValue::Enum { type_name, variant, args } if type_name == "Json" => {
            Some((variant.as_str(), args.first().map(|(_, v)| v)))
        }
        _ => None,
    }
}
fn datatree_kind(tree: &CtValue) -> &'static str {
    match variant_of(tree) {
        Some(("Null", _)) => "null",
        Some(("Bool", _)) => "Bool",
        Some(("Int", _)) => "Int",
        Some(("Float", _)) => "Float",
        Some(("Text", _)) => "Text",
        Some(("Array", _)) => "a list",
        Some(("Object", _)) => "an object",
        _ => "value",
    }
}
fn object_pairs(tree: &CtValue) -> Option<Vec<(String, CtValue)>> {
    match json_payload(tree, "Object") {
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
fn object_get<'a>(tree: &'a CtValue, key: &str) -> Option<&'a CtValue> {
    match json_payload(tree, "Object") {
        Some(CtValue::Map(m)) => m.get(&CtKey::Str(key.to_string())),
        _ => None,
    }
}
fn rebuild_object(pairs: Vec<(String, CtValue)>) -> CtValue {
    json_variant(
        "Object",
        Some(CtValue::Map(pairs.into_iter().map(|(k, v)| (CtKey::Str(k), v)).collect())),
    )
}
fn object_key_set(tree: &CtValue) -> BTreeSet<String> {
    object_pairs(tree).map(|p| p.into_iter().map(|(k, _)| k).collect()).unwrap_or_default()
}
fn text_cell(cell: String) -> CtValue {
    json_variant("Text", Some(CtValue::Str(cell)))
}

/// Generic encode of a `CtValue` back into the `Json`-tagged tree shape —
/// mirrors the `user_Encode` blanket impls (`EncodingTraits.rs`) used by a
/// migration `add`/`change` step to write its new field's value onto the
/// wire before re-decoding. Struct encoding recurses using the type's own
/// field wire keys (so a nested-struct `add`/`change` stays consistent with
/// how that type would normally serialize).
fn encode_ct_value(v: &CtValue, structs: &std::collections::HashMap<String, &StructDef>) -> CtValue {
    match v {
        CtValue::Int(n) => json_variant("Int", Some(CtValue::Int(*n))),
        CtValue::Float(value) => json_variant(
            "Float",
            Some(CtValue::Float(CtFloat::f64(value.as_f64()))),
        ),
        CtValue::Bool(b) => json_variant("Bool", Some(CtValue::Bool(*b))),
        CtValue::Str(s) => json_variant("Text", Some(CtValue::Str(s.clone()))),
        CtValue::Char(c) => json_variant("Text", Some(CtValue::Str(c.to_string()))),
        CtValue::List(xs) => json_variant(
            "Array",
            Some(CtValue::List(xs.iter().map(|x| encode_ct_value(x, structs)).collect())),
        ),
        CtValue::Some(inner) => encode_ct_value(inner, structs),
        CtValue::None(_) => json_variant("Null", None),
        CtValue::Struct { type_name, fields } => {
            let style = structs
                .get(type_name.as_str())
                .and_then(|s| container_rename_all(&s.serde_markers));
            let entries: Vec<(String, CtValue)> = fields
                .iter()
                .map(|(name, v)| {
                    let key = structs
                        .get(type_name.as_str())
                        .and_then(|s| s.fields.iter().find(|f| &f.name == name))
                        .map(|f| field_wire_key(style.as_deref(), f))
                        .unwrap_or_else(|| name.clone());
                    (key, encode_ct_value(v, structs))
                })
                .collect();
            json_variant("Object", Some(CtValue::Map(entries.into_iter().map(|(k, v)| (CtKey::Str(k), v)).collect())))
        }
        _ => json_variant("Null", None),
    }
}

// ── decoding primitives (mirrors `EncodingTraits.rs`'s scalar `user_Decode` impls) ─

fn decode_int(tree: &CtValue) -> Result<CtValue, CtValue> {
    match variant_of(tree) {
        Some(("Int", Some(CtValue::Int(n)))) => Ok(CtValue::Int(*n)),
        Some(("Float", Some(CtValue::Float(value)))) if value.as_f64().fract() == 0.0 => {
            Ok(CtValue::Int(value.as_f64() as i64))
        }
        Some(("Text", Some(CtValue::Str(s)))) => s
            .trim()
            .parse::<i64>()
            .map(CtValue::Int)
            .map_err(|_| decode_error(format!("expected Int, found text {:?}", s))),
        _ => Err(decode_error(format!("expected Int, found {}", datatree_kind(tree)))),
    }
}
fn decode_float(tree: &CtValue) -> Result<CtValue, CtValue> {
    match variant_of(tree) {
        Some(("Float", Some(CtValue::Float(value)))) => {
            Ok(CtValue::Float(CtFloat::f64(value.as_f64())))
        }
        Some(("Int", Some(CtValue::Int(n)))) => Ok(CtValue::Float(CtFloat::f64(*n as f64))),
        Some(("Text", Some(CtValue::Str(s)))) => s
            .trim()
            .parse::<f64>()
            .map(|value| CtValue::Float(CtFloat::f64(value)))
            .map_err(|_| decode_error(format!("expected Float, found text {:?}", s))),
        _ => Err(decode_error(format!("expected Float, found {}", datatree_kind(tree)))),
    }
}
fn decode_bool(tree: &CtValue) -> Result<CtValue, CtValue> {
    match variant_of(tree) {
        Some(("Bool", Some(CtValue::Bool(b)))) => Ok(CtValue::Bool(*b)),
        Some(("Text", Some(CtValue::Str(s)))) => match s.trim() {
            "true" => Ok(CtValue::Bool(true)),
            "false" => Ok(CtValue::Bool(false)),
            _ => Err(decode_error(format!("expected Bool, found text {:?}", s))),
        },
        _ => Err(decode_error(format!("expected Bool, found {}", datatree_kind(tree)))),
    }
}
fn decode_string(tree: &CtValue) -> Result<CtValue, CtValue> {
    match variant_of(tree) {
        Some(("Text", Some(CtValue::Str(s)))) => Ok(CtValue::Str(s.clone())),
        Some(("Int", Some(CtValue::Int(n)))) => Ok(CtValue::Str(n.to_string())),
        Some(("Float", Some(CtValue::Float(f)))) => Ok(CtValue::Str(format!("{:?}", f))),
        Some(("Bool", Some(CtValue::Bool(b)))) => Ok(CtValue::Str(b.to_string())),
        _ => Err(decode_error(format!("expected Text, found {}", datatree_kind(tree)))),
    }
}
fn decode_char(tree: &CtValue) -> Result<CtValue, CtValue> {
    let CtValue::Str(s) = decode_string(tree)? else { unreachable!() };
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Ok(CtValue::Char(c)),
        _ => Err(decode_error(format!("expected a single Char, found {:?}", s))),
    }
}

/// A reasonable zero value for a type with no `#[Default(expr)]` argument —
/// mirrors what `Default::default()` would build for AOT's Rust field type.
fn zero_value(ty: &Type) -> CtValue {
    match ty {
        Type::Int => CtValue::Int(0),
        Type::Float => CtValue::Float(CtFloat::f64(0.0)),
        Type::Bool => CtValue::Bool(false),
        Type::String => CtValue::Str(String::new()),
        Type::Char => CtValue::Char('\0'),
        Type::List(_) => CtValue::List(Vec::new()),
        Type::Option(inner) => CtValue::None((**inner).clone()),
        _ => CtValue::Unit,
    }
}

impl<'a> Interp<'a> {
    /// The non-migrating decode: mirrors a type's plain `jet_decode` walk.
    /// Recurses through Option/List and nested user structs; the top-level
    /// `decode_traced<T>`/`decode<T>` entry point (`typed_decode_top`) is what
    /// additionally tries the `#PublishedSchema` migration chain on failure.
    pub(super) fn typed_decode_value(&mut self, ty: &Type, tree: &CtValue, span: Span) -> Result<CtValue, CtValue> {
        match ty {
            Type::Int => decode_int(tree),
            Type::Float => decode_float(tree),
            Type::Bool => decode_bool(tree),
            Type::String => decode_string(tree),
            Type::Char => decode_char(tree),
            Type::Option(inner) => match variant_of(tree) {
                Some(("Null", _)) => Ok(CtValue::None((**inner).clone())),
                _ => Ok(CtValue::Some(Box::new(self.typed_decode_value(inner, tree, span)?))),
            },
            Type::List(inner) => match variant_of(tree) {
                Some(("Array", Some(CtValue::List(items)))) => {
                    let mut out = Vec::with_capacity(items.len());
                    for (i, item) in items.iter().enumerate() {
                        out.push(
                            self.typed_decode_value(inner, item, span)
                                .map_err(|e| decode_error_under(&format!("[{}]", i), e))?,
                        );
                    }
                    Ok(CtValue::List(out))
                }
                _ => Err(decode_error(format!("expected a list, found {}", datatree_kind(tree)))),
            },
            Type::Named(name) => self.typed_decode_struct(name, tree, span),
            other => Err(decode_error(format!(
                "comptime can't decode `{}` yet — this type isn't a struct, primitive, `[T]`, or `T?`",
                other.name()
            ))),
        }
    }

    fn typed_decode_struct(&mut self, name: &str, tree: &CtValue, span: Span) -> Result<CtValue, CtValue> {
        let Some(sdef) = self.structs.get(name).copied() else {
            return Err(decode_error(format!(
                "comptime has no `{}` struct registered to decode into",
                name
            )));
        };
        let style = container_rename_all(&sdef.serde_markers);
        let style = style.as_deref();
        let deny = serde_has(&sdef.serde_markers, crate::Syntax::ATTR_DENY_UNKNOWN_FIELDS);
        let has_flatten = sdef
            .fields
            .iter()
            .any(|f| serde_has(&f.serde_markers, crate::Syntax::ATTR_FLATTEN));
        if deny && !has_flatten {
            if let Some(pairs) = object_pairs(tree) {
                let known: Vec<String> = sdef
                    .fields
                    .iter()
                    .filter(|f| !serde_has(&f.serde_markers, crate::Syntax::ATTR_SKIP))
                    .map(|f| field_wire_key(style, f))
                    .collect();
                for (k, _) in &pairs {
                    if !known.contains(k) {
                        return Err(decode_error(format!("E2412: unknown field `{}`", k)));
                    }
                }
            }
        }
        let mut out_fields = Vec::new();
        for f in sdef.fields.iter().filter(|f| f.computed.is_none()) {
            if serde_has(&f.serde_markers, crate::Syntax::ATTR_SKIP) {
                let v = self.field_default_value(f, span);
                out_fields.push((f.name.clone(), v));
                continue;
            }
            if serde_has(&f.serde_markers, crate::Syntax::ATTR_FLATTEN) {
                let v = self
                    .typed_decode_value(&f.ty, tree, span)
                    .map_err(|e| decode_error_under(&f.name, e))?;
                out_fields.push((f.name.clone(), v));
                continue;
            }
            let key = field_wire_key(style, f);
            let v = match object_get(tree, &key) {
                Some(cell) => self
                    .typed_decode_value(&f.ty, cell, span)
                    .map_err(|e| decode_error_under(&key, e))?,
                None => {
                    if let Type::Option(inner) = &f.ty {
                        CtValue::None((**inner).clone())
                    } else if serde_marker(&f.serde_markers, crate::Syntax::ATTR_DEFAULT).is_some() {
                        self.field_default_value(f, span)
                    } else {
                        return Err(decode_error(format!(
                            "E2410: missing required field `{}`",
                            f.name
                        )));
                    }
                }
            };
            out_fields.push((f.name.clone(), v));
        }
        Ok(CtValue::Struct { type_name: name.to_string(), fields: out_fields })
    }

    fn field_default_value(&mut self, f: &Field, _span: Span) -> CtValue {
        if let Some(m) = serde_marker(&f.serde_markers, crate::Syntax::ATTR_DEFAULT) {
            // Card #131: prefer the value sema already baked onto the marker, so
            // this decode tier and AOT codegen use the byte-identical default
            // (R12). Fall back to a live eval only if sema's pass didn't run.
            if let Some(v) = &m.ct {
                return v.clone();
            }
            if let Some(expr) = m.args.first() {
                let mut empty_scope = std::collections::HashMap::new();
                if let Ok(v) = self.eval(expr, &mut empty_scope) {
                    return v;
                }
            }
        }
        zero_value(&f.ty)
    }

    /// `TypeName -> [v1..vK] wire-key shapes` — mirrors
    /// `Codegen/Items.rs::migration_shapes`.
    fn migration_shapes(&self, style: Option<&str>, s: &StructDef, blocks: &[&MigrationDecl]) -> Vec<Vec<String>> {
        let mut shape: BTreeSet<String> = s
            .fields
            .iter()
            .filter(|f| !serde_has(&f.serde_markers, crate::Syntax::ATTR_SKIP))
            .map(|f| field_wire_key(style, f))
            .collect();
        let mut shapes: Vec<Vec<String>> = Vec::with_capacity(blocks.len());
        for block in blocks.iter().rev() {
            for op in &block.ops {
                match op {
                    MigrationOp::Add { field, .. } => {
                        shape.remove(&migration_wire_key(style, s, field));
                    }
                    MigrationOp::Remove { field, .. } => {
                        shape.insert(migration_wire_key(style, s, field));
                    }
                    MigrationOp::Rename { from, to, .. } => {
                        shape.remove(&migration_wire_key(style, s, to));
                        shape.insert(migration_wire_key(style, s, from));
                    }
                    MigrationOp::Change { .. } => {}
                }
            }
            shapes.push(shape.iter().cloned().collect());
        }
        shapes.reverse();
        shapes
    }

    /// One migration block's ops, rewriting `pairs` from shape v<i> to v<i+1>
    /// — mirrors `Codegen/Items.rs::emit_migration_step_fns`.
    fn apply_migration_step(
        &mut self,
        s: &StructDef,
        style: Option<&str>,
        block: &MigrationDecl,
        pairs: &mut Vec<(String, CtValue)>,
        span: Span,
    ) -> Result<(), CtValue> {
        for op in &block.ops {
            match op {
                MigrationOp::Rename { from, to, .. } => {
                    let from_key = migration_wire_key(style, s, from);
                    let to_key = migration_wire_key(style, s, to);
                    for p in pairs.iter_mut() {
                        if p.0 == from_key {
                            p.0 = to_key.clone();
                        }
                    }
                }
                MigrationOp::Remove { field, .. } => {
                    let key = migration_wire_key(style, s, field);
                    pairs.retain(|p| p.0 != key);
                }
                MigrationOp::Add { field, default_fn, .. } => {
                    let key = migration_wire_key(style, s, field);
                    let Some(fn_name) = default_fn.as_deref() else {
                        return Err(decode_error(format!(
                            "migration `add {}` has no lowered default function",
                            field
                        )));
                    };
                    let Some(func) = self.funcs.get(fn_name).copied() else {
                        return Err(decode_error(format!(
                            "migration `add {}`'s default function `{}` isn't registered",
                            field, fn_name
                        )));
                    };
                    let value = self
                        .call_func(fn_name, func, std::collections::HashMap::new())
                        .map_err(|d| decode_error(d.what.clone()))?;
                    let encoded = encode_ct_value(&value, self.structs);
                    pairs.push((key, encoded));
                }
                MigrationOp::Change { field, from_ty, conv_fn, .. } => {
                    let key = migration_wire_key(style, s, field);
                    let Some(fn_name) = conv_fn.as_deref() else {
                        return Err(decode_error(format!(
                            "migration `change {}` has no lowered converter function",
                            field
                        )));
                    };
                    let Some(func) = self.funcs.get(fn_name).copied() else {
                        return Err(decode_error(format!(
                            "migration `change {}`'s converter function `{}` isn't registered",
                            field, fn_name
                        )));
                    };
                    let Some(pos) = pairs.iter().position(|p| p.0 == key) else {
                        return Err(decode_error(format!("E2410: missing required field `{}`", field)));
                    };
                    let old_value = self
                        .typed_decode_value(from_ty, &pairs[pos].1, span)
                        .map_err(|e| decode_error_under(&key, e))?;
                    let mut frame = std::collections::HashMap::new();
                    if let Some(param) = func.params.first() {
                        frame.insert(param.name.clone(), old_value);
                    }
                    let new_value = self.call_func(fn_name, func, frame).map_err(|d| decode_error(d.what.clone()))?;
                    pairs[pos].1 = encode_ct_value(&new_value, self.structs);
                }
            }
        }
        Ok(())
    }

    /// `decode_traced<T>`'s full entry point — mirrors `jet_decode_traced`'s
    /// default (try `jet_decode`, report fresh) plus, for a
    /// `#PublishedSchema` type with `migration { }` blocks, the runtime
    /// chain-walker (`emit_migration_chain_walker`): on failure, detect which
    /// historical shape the data's key set matches (newest first) and walk
    /// the step functions forward.
    pub(super) fn typed_decode_top(&mut self, ty: &Type, tree: &CtValue, span: Span) -> Result<(CtValue, CtValue), CtValue> {
        match self.typed_decode_value(ty, tree, span) {
            Ok(v) => Ok((v, migration_status_fresh())),
            Err(e) => {
                let Type::Named(name) = ty else { return Err(e) };
                let Some(sdef) = self.structs.get(name.as_str()).copied() else { return Err(e) };
                let published = sdef.is_published_schema
                    || sdef.derives.iter().any(|(t, _)| t == crate::Syntax::ATTR_PUBLISHED_SCHEMA);
                if !published || !sdef.type_params.is_empty() {
                    return Err(e);
                }
                let Some(blocks) = self.migrations.get(name.as_str()).cloned() else { return Err(e) };
                if blocks.is_empty() {
                    return Err(e);
                }
                let style = container_rename_all(&sdef.serde_markers);
                let shapes = self.migration_shapes(style.as_deref(), sdef, &blocks);
                let keys = object_key_set(tree);
                for j in (0..shapes.len()).rev() {
                    let shape = &shapes[j];
                    let matches = if shape.is_empty() {
                        keys.is_empty()
                    } else {
                        shape.len() == keys.len() && shape.iter().all(|k| keys.contains(k))
                    };
                    if !matches {
                        continue;
                    }
                    let Some(mut pairs) = object_pairs(tree) else { return Err(e) };
                    let mut steps = Vec::new();
                    for i in j..shapes.len() {
                        self.apply_migration_step(sdef, style.as_deref(), blocks[i], &mut pairs, span)?;
                        steps.push(format!("v{}->v{}", i + 1, i + 2));
                    }
                    let new_tree = rebuild_object(pairs);
                    let value = self.typed_decode_value(ty, &new_tree, span)?;
                    return Ok((value, migration_status(true, format!("v{}", j + 1), steps)));
                }
                Err(e)
            }
        }
    }

    /// The `json.decode<T>` / `json.decode_traced<T>` / csv/toml/yaml sibling
    /// entry point — parses `text` with the codec's own parser, then runs
    /// `typed_decode_top`. `method` is `"decode"` (drop the migration status,
    /// same chain applied silently — matches `jet_enc_*_decode`) or
    /// `"decode_traced"` (wrap in `DecodeResult`).
    pub(super) fn eval_typed_decode(
        &mut self,
        module: &str,
        method: &str,
        text: &str,
        ty: &Type,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        if module == "core.encoding.csv" {
            return self.eval_typed_csv_decode(method, text, ty, span);
        }
        let parsed: Result<CtValue, CtValue> = match module {
            "core.encoding.json" => super::JsonInterp::parse_json(text)
                .map_err(|e| json_parse_err_to_decode("JSON", super::JsonInterp::json_error_value(e))),
            "core.encoding.toml" => super::EncodingLite::toml_parse(text).map_err(|e| json_parse_err_to_decode("TOML", e)),
            "core.encoding.yaml" => super::EncodingLite::yaml_parse(text).map_err(|e| json_parse_err_to_decode("YAML", e)),
            "core.encoding.xml" => {
                match super::EncodingLite::xml_parse(text) {
                    Ok(document) => super::EncodingLite::xml_project_for_decode(&document),
                    Err(e) => Err(super::EncodingLite::xml_error_value(e)),
                }
            }
            _ => {
                return Err(unsupported(
                    &format!("`{}.{}()` at comptime", module, method),
                    span,
                ))
            }
        };
        let tree = match parsed {
            Ok(t) => t,
            Err(e) => return Ok(CtValue::ResErr(Box::new(e))),
        };
        match self.typed_decode_top(ty, &tree, span) {
            Ok((value, migration)) => {
                if method == "decode_traced" {
                    Ok(CtValue::ResOk(Box::new(decode_result(value, migration))))
                } else {
                    Ok(CtValue::ResOk(Box::new(value)))
                }
            }
            Err(e) => Ok(CtValue::ResErr(Box::new(e))),
        }
    }

    /// `core.data.csv<T>` / `core.encoding.csv.decode(_traced)<T>` — header row
    /// maps columns to fields by name; each data row becomes an Object of Text
    /// cells, then decodes to `T`. Mirrors `jet_enc_csv_decode(_traced)`.
    pub(super) fn eval_typed_csv_decode(
        &mut self,
        method: &str,
        text: &str,
        ty: &Type,
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        let rows = match super::EncodingLite::csv_parse(text) {
            Ok(r) => r,
            Err(e) => return Ok(CtValue::ResErr(Box::new(decode_error(e)))),
        };
        let mut it = rows.into_iter();
        let Some(header) = it.next() else {
            let value = CtValue::List(Vec::new());
            return Ok(CtValue::ResOk(Box::new(if method == "decode_traced" {
                decode_result(value, migration_status_fresh())
            } else {
                value
            })));
        };
        let mut values = Vec::new();
        let mut migration = migration_status_fresh();
        for (i, row) in it.enumerate() {
            let entries: Vec<(CtKey, CtValue)> = header
                .iter()
                .enumerate()
                .map(|(c, name)| {
                    let cell = row.get(c).cloned().unwrap_or_default();
                    (CtKey::Str(name.clone()), text_cell(cell))
                })
                .collect();
            let tree = json_variant("Object", Some(CtValue::Map(entries.into_iter().collect())));
            match self.typed_decode_top(ty, &tree, span) {
                Ok((v, m)) => {
                    if migration_migrated(&m) && !migration_migrated(&migration) {
                        migration = m;
                    }
                    values.push(v);
                }
                Err(e) => {
                    return Ok(CtValue::ResErr(Box::new(decode_error_under(&format!("row {}", i + 1), e))));
                }
            }
        }
        let value = CtValue::List(values);
        Ok(CtValue::ResOk(Box::new(if method == "decode_traced" {
            decode_result(value, migration)
        } else {
            value
        })))
    }
}

fn json_parse_err_to_decode(codec: &str, e: CtValue) -> CtValue {
    let (line, message) = match e {
        CtValue::Struct { fields, .. } => {
            let line = fields
                .iter()
                .find(|(n, _)| n == "line")
                .and_then(|(_, v)| match v {
                    CtValue::Int(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            let message = fields
                .iter()
                .find(|(n, _)| n == "message")
                .and_then(|(_, v)| match v {
                    CtValue::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            (line, message)
        }
        _ => (0, String::new()),
    };
    decode_error(format!("invalid {} (line {}): {}", codec, line, message))
}
