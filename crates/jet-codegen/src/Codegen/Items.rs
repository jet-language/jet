use super::*;
use crate::AST::{Expr, Marker, StrPart, StructDef, Variant};
fn serde_marker<'a>(markers: &'a [Marker], name: &str) -> Option<&'a Marker> {
    markers.iter().find(|m| m.name == name)
}
fn marker_str_arg_for(m: &Marker, label: &str) -> Option<String> {
    m.arg_labels
        .iter()
        .enumerate()
        .find_map(|(index, marker_label)| {
            marker_label
                .as_ref()
                .is_some_and(|(name, _)| name == label)
                .then(|| m.expr_arg(index))
                .flatten()
                .and_then(|expr| match expr {
                    Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
                        StrPart::Lit(s) => Some(s.clone()),
                        _ => None,
                    },
                    _ => None,
                })
        })
}
fn marker_str_arg(m: &Marker) -> Option<String> {
    if let Some(crate::AST::CtValue::Str(value)) = &m.ct {
        return Some(value.clone());
    }
    match m.expr_arg(0) {
        Some(Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
            StrPart::Lit(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}
// D-SERDE3 (= C) + D-ACRO-CASE1=A: wire-casing transform. Snake field names are
// the common path; PascalCase inputs use the mechanical acronym split.
fn apply_rename_all(style: &str, name: &str) -> String {
    match style {
        crate::Syntax::RENAME_ALL_CAMEL => crate::Syntax::to_camel_acronym(name),
        crate::Syntax::RENAME_ALL_PASCAL => crate::Syntax::to_pascal_acronym(name),
        crate::Syntax::RENAME_ALL_KEBAB => crate::Syntax::to_snake_acronym(name).replace('_', "-"),
        crate::Syntax::RENAME_ALL_SCREAMING => crate::Syntax::to_shouty_acronym(name),
        // snake (and any unrecognized — sema rejects those with E2409)
        _ => crate::Syntax::to_snake_acronym(name),
    }
}
pub(super) fn container_rename_all(markers: &[Marker]) -> Option<String> {
    serde_marker(markers, crate::Syntax::MARKER_RENAME_ALL).and_then(|m| match m.expr_arg(0) {
        Some(Expr::Ident(n, _)) => Some(n.clone()),
        _ => None,
    })
}
/// Apply the checked container wire-casing rule to a source field name.
pub(super) fn serde_wire_name(style: Option<&str>, name: &str) -> String {
    style.map_or_else(|| name.to_string(), |style| apply_rename_all(style, name))
}

/// Resolve the checked wire name for an enum variant. Per-variant `#Rename`
/// wins over the enclosing enum's `#RenameAll` rule.
pub(super) fn variant_wire_key(style: Option<&str>, variant: &Variant) -> String {
    if let Some(m) = serde_marker(&variant.serde_markers, crate::Syntax::MARKER_RENAME) {
        if let Some(s) = marker_str_arg_for(m, "json").or_else(|| marker_str_arg(m)) {
            return s;
        }
    }
    serde_wire_name(style, &variant.name)
}
/// D-MIGRATE4: the migration blocks for a struct, when the runtime chain
/// applies: `#PublishedSchema`, concrete (no type params), with at least one
/// `migration { }` block in the module. Mirrors the gate in
/// `Sema::desugar_migrations` — the two must agree on which types get runtime
/// machinery, since sema pre-lowers the converter/default functions the step
/// functions call.
pub(super) fn migration_blocks<'a>(
    cx: &'a Cx,
    s: &StructDef,
) -> Option<&'a [crate::AST::MigrationDecl]> {
    // `#PublishedSchema struct` sets the flag; the grouped
    // `#[PublishedSchema, Codable]` spelling leaves the marker in `derives`.
    let published = s.is_published_schema
        || s.derives
            .iter()
            .any(|(t, _)| t == crate::Syntax::MARKER_PUBLISHED_SCHEMA);
    if !published || !s.type_params.is_empty() {
        return None;
    }
    let blocks = cx.migrations.get(&s.name)?;
    if blocks.is_empty() {
        None
    } else {
        Some(blocks)
    }
}


