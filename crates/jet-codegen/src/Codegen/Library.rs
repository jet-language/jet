//! Native `Library` export projection (D-LIB-EXPORT1=C).
//!
//! The first native boundary is deliberately the same scalar boundary already
//! used by the sandboxed plugin backend: one homogeneous `Int` or `Float`
//! signature per exported `pub fn`. Sema/driver validation is the enforcement
//! point; this module only renders wrappers and deterministic foreign text.

use crate::AST::{Item, ProgramBundle};
use crate::jet_name_format;
use jet_foundation::Names::mangle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryScalar {
    Int,
    Float,
}

impl LibraryScalar {
    fn rust_ty(self) -> &'static str {
        match self {
            Self::Int => "i64",
            Self::Float => "f64",
        }
    }

    fn c_ty(self) -> &'static str {
        match self {
            Self::Int => "int64_t",
            Self::Float => "double",
        }
    }

    fn python_ctypes_ty(self) -> &'static str {
        match self {
            Self::Int => "ctypes.c_int64",
            Self::Float => "ctypes.c_double",
        }
    }

    fn swift_ty(self) -> &'static str {
        match self {
            Self::Int => "Int64",
            Self::Float => "Double",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryExport {
    pub name: String,
    pub scalar: LibraryScalar,
    pub params: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryArtifacts {
    /// Complete generated Rust, including the C ABI wrappers.
    pub rust: String,
    /// The generated C header. It is always emitted for a native Library.
    pub header: String,
    /// Named foreign binding source, in stable language order.
    pub bindings: Vec<(String, String)>,
    pub exports: Vec<LibraryExport>,
}

/// D-LIB-EXPORT1=C: the native library currently shares the plugin's checked
/// scalar shape. Keep this wrapper so the driver does not reimplement a type
/// policy in a second place.
pub fn library_export_shape(function: &crate::AST::Func) -> Option<LibraryScalar> {
    let shape = crate::Codegen::plugin_export_shape(function)?;
    Some(match shape {
        crate::Codegen::PluginScalar::Int => LibraryScalar::Int,
        crate::Codegen::PluginScalar::Float => LibraryScalar::Float,
    })
}

fn c_symbol(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        out.insert(0, '_');
    }
    out
}

fn collect_exports(bundle: &ProgramBundle) -> Vec<LibraryExport> {
    bundle.modules[bundle.entry]
        .items
        .iter()
        .filter_map(|item| {
            let Item::Func(function) = item else { return None };
            if !function.is_pub {
                return None;
            }
            let scalar = library_export_shape(function)?;
            Some(LibraryExport {
                name: function.name.clone(),
                scalar,
                params: function.params.len(),
            })
        })
        .collect()
}

/// Render the native wrappers and the requested foreign projections.
pub fn emit_library(
    bundle: &ProgramBundle,
    whole_program_rust: &str,
    name: &str,
    requested_bindings: &[String],
) -> LibraryArtifacts {
    let exports = collect_exports(bundle);
    let mut wrappers = String::new();
    for export in &exports {
        let params = (0..export.params)
            .map(|index| format!("p{index}: {}", export.scalar.rust_ty()))
            .collect::<Vec<_>>();
        let args = (0..export.params)
            .map(|index| format!("p{index}"))
            .collect::<Vec<_>>();
        let wrapper = mangle(&format!("library_export_{}", export.name));
        let symbol = c_symbol(&export.name);
        let callee = mangle(&export.name);
        wrappers.push_str(&format!(
            "#[export_name = \"{symbol}\"]\npub extern \"C\" fn {wrapper}({params}) -> {ret} {{ {callee}({args}) }}\n",
            params = params.join(", "),
            ret = export.scalar.rust_ty(),
            args = args.join(", "),
        ));
        wrappers.push_str(&format!("// jet:library-symbol={symbol}\n"));
    }

    let mut rust = whole_program_rust.to_string();
    rust.push_str("\n// D-LIB-EXPORT1=C: generated native Library wrappers.\n");
    rust.push_str(&wrappers);

    let header = render_c_header(name, &exports);
    let mut bindings = Vec::new();
    for language in requested_bindings {
        let source = match language.as_str() {
            "c" => header.clone(),
            "python" => render_python(name, &exports),
            "swift" => render_swift(&exports),
            _ => continue,
        };
        bindings.push((language.clone(), source));
    }

    LibraryArtifacts {
        rust,
        header,
        bindings,
        exports,
    }
}

fn render_c_header(name: &str, exports: &[LibraryExport]) -> String {
    let guard = format!("{}_H", c_symbol(name).to_ascii_uppercase());
    let mut out = format!(
        "/* Generated by Jet — D-LIB-EXPORT1=C. */\n#ifndef {guard}\n#define {guard}\n#include <stdint.h>\n\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n",
    );
    for export in exports {
        let params = (0..export.params)
            .map(|index| format!("{} p{index}", export.scalar.c_ty()))
            .collect::<Vec<_>>();
        out.push_str(&format!(
            "{} {}({});\n",
            export.scalar.c_ty(),
            c_symbol(&export.name),
            if params.is_empty() {
                "void".to_string()
            } else {
                params.join(", ")
            },
        ));
    }
    out.push_str("\n#ifdef __cplusplus\n}\n#endif\n#endif\n");
    out
}

fn render_python(name: &str, exports: &[LibraryExport]) -> String {
    let mut out = String::from(
        "# Generated by Jet — D-LIB-EXPORT1=C.\nimport ctypes\n\nclass Library:\n    def __init__(self, path):\n        self._lib = ctypes.CDLL(path)\n",
    );
    for export in exports {
        out.push_str(&format!(
            "        self._lib.{symbol}.argtypes = [{types}]\n        self._lib.{symbol}.restype = {ret}\n",
            symbol = c_symbol(&export.name),
            types = (0..export.params)
                .map(|_| export.scalar.python_ctypes_ty())
                .collect::<Vec<_>>()
                .join(", "),
            ret = export.scalar.python_ctypes_ty(),
        ));
        out.push_str(&format!(
            "    def {name}(self, *args):\n        return self._lib.{symbol}(*args)\n",
            name = export.name,
            symbol = c_symbol(&export.name),
        ));
    }
    out.push_str(&format!("\ndef load(path):\n    return Library(path)\n\n# library: {name}\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_foreign_projections_use_the_native_symbol() {
        let exports = vec![LibraryExport {
            name: "on_tick".to_string(),
            scalar: LibraryScalar::Int,
            params: 1,
        }];
        let header = render_c_header("flightlog", &exports);
        let python = render_python("flightlog", &exports);
        assert!(header.contains("int64_t on_tick(int64_t p0);"));
        assert!(python.contains("self._lib.on_tick"));
        assert_eq!(c_symbol("on_tick"), "on_tick");
    }
}

fn render_swift(exports: &[LibraryExport]) -> String {
    let mut out = String::from("// Generated by Jet — D-LIB-EXPORT1=C.\nimport Foundation\n\n");
    for export in exports {
        let params = (0..export.params)
            .map(|index| format!("_ p{index}: {}", export.scalar.swift_ty()))
            .collect::<Vec<_>>();
        let args = (0..export.params)
            .map(|index| format!("p{index}"))
            .collect::<Vec<_>>();
        out.push_str(&jet_name_format!(
            "@_silgen_name(\"{symbol}\") private func {name_prefix}{name}({params}) -> {ret}\npublic func {name}({params}) -> {ret} {{ {name_prefix}{name}({args}) }}\n\n",
            symbol = c_symbol(&export.name),
            name = export.name,
            params = params.join(", "),
            ret = export.scalar.swift_ty(),
            args = args.join(", "),
        ));
    }
    out
}
