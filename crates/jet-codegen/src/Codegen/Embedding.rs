//! The typed export shapes shared by native Library and sandbox outputs.
//!
//! Native Library exports come from the explicit `#Export(c)` surface; sandbox
//! exports come from top-level public functions. Both use the same scalar ABI
//! table while keeping their selection rules separate.


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

fn export_scalar(scalar: crate::Sema::GuestScalar) -> ExportScalar {
    match scalar {
        crate::Sema::GuestScalar::Int => ExportScalar::Int,
        crate::Sema::GuestScalar::Float => ExportScalar::Float,
        crate::Sema::GuestScalar::Bool => ExportScalar::Bool,
        crate::Sema::GuestScalar::Text => ExportScalar::Text,
    }
}

fn export_function(guest: crate::Sema::GuestFunction) -> Option<ExportFunction> {
    Some(ExportFunction {
        name: guest.name,
        scalar: export_scalar(guest.scalar?),
        params: guest
            .params
            .into_iter()
            .map(|(convention, _)| convention)
            .collect(),
    })
}

/// Classify one explicitly marked native Library export's homogeneous shape.
pub fn export_shape(function: &Func) -> Option<ExportScalar> {
    crate::Sema::guest_export_signature(function)
        .and_then(|guest| guest.scalar)
        .map(export_scalar)
}

/// Classify one sandbox's top-level public export shape.
pub fn sandbox_export_shape(function: &Func) -> Option<ExportScalar> {
    crate::Sema::sandbox_export_signature(function)
        .and_then(|guest| guest.scalar)
        .map(export_scalar)
}

/// Collect the exact top-level `#Export(c)` list for native Library artifacts.
pub fn export_surface(bundle: &ProgramBundle) -> Vec<ExportFunction> {
    crate::Sema::guest_export_surface(bundle)
        .into_iter()
        .filter_map(export_function)
        .collect()
}

/// Collect the exact top-level public list for sandbox artifacts.
pub fn sandbox_export_surface(bundle: &ProgramBundle) -> Vec<ExportFunction> {
    crate::Sema::sandbox_export_surface(bundle)
        .into_iter()
        .filter_map(export_function)
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
