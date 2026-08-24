//! The one typed export surface shared by native Library and sandbox outputs.
//!
//! Driver validation and both lowerers read this table. It is deliberately
//! small: top-level `pub fn` items with one homogeneous scalar shape.

use crate::AST::{AccessConvention, Func, Item, ProgramBundle, Type};

/// Scalar types admitted at both foreign-host boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportScalar {
    Int,
    Float,
    Bool,
    Text,
}

impl ExportScalar {
    pub(crate) fn rust_ty(self) -> &'static str {
        match self {
            Self::Int => "i64",
            Self::Float => "f64",
            Self::Bool => "bool",
            Self::Text => "String",
        }
    }

    pub(crate) fn wit_ty(self) -> &'static str {
        match self {
            Self::Int => "s64",
            Self::Float => "f64",
            Self::Bool => "bool",
            Self::Text => "string",
        }
    }

    pub(crate) fn c_ty(self) -> &'static str {
        match self {
            Self::Int => "int64_t",
            Self::Float => "double",
            Self::Bool => "bool",
            Self::Text => "JetText",
        }
    }

    pub(crate) fn python_ctypes_ty(self) -> &'static str {
        match self {
            Self::Int => "ctypes.c_int64",
            Self::Float => "ctypes.c_double",
            Self::Bool => "ctypes.c_bool",
            Self::Text => "JetText",
        }
    }

    pub(crate) fn swift_ty(self) -> &'static str {
        match self {
            Self::Int => "Int64",
            Self::Float => "Double",
            Self::Bool => "Bool",
            Self::Text => "JetText",
        }
    }
}

/// One row in the shared foreign export table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFunction {
    pub name: String,
    pub scalar: ExportScalar,
    pub params: Vec<AccessConvention>,
}

/// Classify one function's homogeneous foreign-boundary shape.
pub fn export_shape(function: &Func) -> Option<ExportScalar> {
    let mut shape = None;
    let note = |ty: &Type, shape: &mut Option<ExportScalar>| -> bool {
        let scalar = match ty {
            Type::Int => ExportScalar::Int,
            Type::Float => ExportScalar::Float,
            Type::Bool => ExportScalar::Bool,
            Type::String => ExportScalar::Text,
            _ => return false,
        };
        match shape {
            Some(existing) => *existing == scalar,
            None => {
                *shape = Some(scalar);
                true
            }
        }
    };
    for parameter in &function.params {
        if !note(&parameter.ty, &mut shape) {
            return None;
        }
    }
    match &function.return_type {
        Some(ty) if note(ty, &mut shape) => shape,
        _ => None,
    }
}

/// Collect the exact top-level export list consumed by both artifact paths.
pub fn export_surface(bundle: &ProgramBundle) -> Vec<ExportFunction> {
    bundle.modules[bundle.entry]
        .items
        .iter()
        .filter_map(|item| {
            let Item::Func(function) = item else {
                return None;
            };
            if !function.is_pub || !bundle.name_ledger.public(bundle.entry, &function.name) {
                return None;
            }
            Some(ExportFunction {
                name: function.name.clone(),
                scalar: export_shape(function)?,
                params: function
                    .params
                    .iter()
                    .map(|parameter| parameter.convention)
                    .collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::ExportScalar;

    #[test]
    fn scalar_lowering_table_covers_both_boundaries() {
        let rows = [
            (ExportScalar::Int, "i64", "s64", "int64_t"),
            (ExportScalar::Float, "f64", "f64", "double"),
            (ExportScalar::Bool, "bool", "bool", "bool"),
            (ExportScalar::Text, "String", "string", "JetText"),
        ];
        for (scalar, rust, wit, c) in rows {
            assert_eq!(scalar.rust_ty(), rust);
            assert_eq!(scalar.wit_ty(), wit);
            assert_eq!(scalar.c_ty(), c);
        }
    }
}
