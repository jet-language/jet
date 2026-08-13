//! D-METAREFLECT1: the registered field rows shared by every reflection view.
//!
//! Comptime `T.reflect().fields` and runtime `reflect.of(x).fields` are two
//! projections of these rows.  The runtime projection erases compile-time
//! metadata, but it never rebuilds a second field list from a string fallback.

use crate::Diagnostics::Span;
use crate::AST::{Field, Marker, StructDef, Type};

/// One registered user-type field as seen by the reflection model.
#[derive(Debug, Clone)]
pub struct ReflectionField {
    pub name: String,
    pub ty: Type,
    pub markers: Vec<Marker>,
    pub is_pub: bool,
    pub span: Span,
}

impl ReflectionField {
    /// Copy the canonical field facts once from the AST registration row.
    pub fn from_field(field: &Field) -> Self {
        Self {
            name: field.name.clone(),
            ty: field.ty.clone(),
            markers: field.serde_markers.clone(),
            is_pub: field.is_pub,
            span: field.name_span,
        }
    }
}

/// Read the one registered field-row list for a struct in declaration order.
pub fn fields(definition: &StructDef) -> Vec<ReflectionField> {
    definition
        .reflection_fields()
        .map(ReflectionField::from_field)
        .collect()
}
