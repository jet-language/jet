//! The one typed export surface shared by native Library and sandbox outputs.
//!
//! Driver validation and both lowerers read this table. It is deliberately
//! small: marked top-level functions with one homogeneous scalar shape.

use crate::AST::{AccessConvention, Func, ProgramBundle};

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
    crate::Sema::guest_export_signature(function)
        .and_then(|guest| guest.scalar)
        .map(|scalar| match scalar {
            crate::Sema::GuestScalar::Int => ExportScalar::Int,
            crate::Sema::GuestScalar::Float => ExportScalar::Float,
            crate::Sema::GuestScalar::Bool => ExportScalar::Bool,
            crate::Sema::GuestScalar::Text => ExportScalar::Text,
        })
}

/// Collect the exact top-level export list consumed by both artifact paths.
pub fn export_surface(bundle: &ProgramBundle) -> Vec<ExportFunction> {
    crate::Sema::guest_export_surface(bundle)
        .into_iter()
        .filter_map(|guest| {
            Some(ExportFunction {
                name: guest.name,
                scalar: match guest.scalar? {
                    crate::Sema::GuestScalar::Int => ExportScalar::Int,
                    crate::Sema::GuestScalar::Float => ExportScalar::Float,
                    crate::Sema::GuestScalar::Bool => ExportScalar::Bool,
                    crate::Sema::GuestScalar::Text => ExportScalar::Text,
                },
                params: guest
                    .params
                    .into_iter()
                    .map(|(convention, _)| convention)
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
