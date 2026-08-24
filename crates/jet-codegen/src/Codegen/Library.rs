//! Native `Library` export projection (D-LIB-EXPORT1=C).
//!
//! The first native boundary is deliberately the same scalar boundary already
//! used by the sandboxed plugin backend: one homogeneous `Int`, `Float`, `Bool`, or `Text`
//! signature per exported `pub fn`. Sema/driver validation is the enforcement
//! point; this module only renders wrappers and deterministic foreign text.

use crate::AST::{AccessConvention, ProgramBundle};
use jet_foundation::Names::mangle;

/// Library compatibility name for the shared embedding scalar table.
pub use super::Embedding::ExportScalar as LibraryScalar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryExport {
    pub name: String,
    pub scalar: LibraryScalar,
    pub params: usize,
    pub conventions: Vec<AccessConvention>,
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
    super::Embedding::export_shape(function)
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
    super::Embedding::export_surface(bundle)
        .into_iter()
        .map(|export| LibraryExport {
            name: export.name,
            scalar: export.scalar,
            params: export.params.len(),
            conventions: export.params,
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
    let has_text_export = exports
        .iter()
        .any(|export| export.scalar == LibraryScalar::Text);
    for export in &exports {
        let params = (0..export.params)
            .map(|index| {
                if export.scalar == LibraryScalar::Text {
                    format!("p{index}: JetText")
                } else {
                    format!("p{index}: {}", export.scalar.rust_ty())
                }
            })
            .collect::<Vec<_>>();
        let locals = export
            .conventions
            .iter()
            .enumerate()
            .map(|(index, convention)| {
                let mutable = matches!(convention, AccessConvention::Write)
                    .then_some("mut ")
                    .unwrap_or_default();
                if export.scalar == LibraryScalar::Text {
                    format!("let {mutable}p{index} = __jet_library_read_text(p{index});")
                } else if matches!(convention, AccessConvention::Write) {
                    format!("let mut p{index} = p{index};")
                } else {
                    String::new()
                }
            })
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let args = (0..export.params)
            .map(|index| match (export.scalar, export.conventions[index]) {
                (LibraryScalar::Text, AccessConvention::Read) => format!("&p{index}"),
                (_, AccessConvention::Read | AccessConvention::Move) => format!("p{index}"),
                (_, AccessConvention::Write) => format!("&mut p{index}"),
            })
            .collect::<Vec<_>>();
        let wrapper = mangle(&format!("library_export_{}", export.name));
        let symbol = c_symbol(&export.name);
        let callee = mangle(&export.name);
        let return_type = if export.scalar == LibraryScalar::Text {
            "JetText"
        } else {
            export.scalar.rust_ty()
        };
        let call = format!("{callee}({})", args.join(", "));
        let body = if export.scalar == LibraryScalar::Text {
            format!("{} __jet_library_return_text({call})", locals.join(" "))
        } else if !locals.is_empty() {
            format!("{} {call}", locals.join(" "))
        } else {
            call
        };
        wrappers.push_str(&format!(
            "#[export_name = \"{symbol}\"]\npub extern \"C\" fn {wrapper}({params}) -> {ret} {{ {body} }}\n",
            params = params.join(", "),
            ret = return_type,
            body = body,
        ));
        wrappers.push_str(&format!("// jet:library-symbol={symbol}\n"));
    }

    let mut rust = whole_program_rust.to_string();
    rust.push_str("\n// D-LIB-EXPORT1=C: generated native Library wrappers.\n");
    if has_text_export {
        rust.push_str("// JET_VETTED_UNSAFE_BEGIN: library_text_abi\n");
        rust.push_str(LIBRARY_TEXT_HELPERS);
        rust.push_str("\n// JET_VETTED_UNSAFE_END: library_text_abi\n");
    }
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
        "/* Generated by Jet — D-LIB-EXPORT1=C / D-EMBED1=E / D-EMBED2=C. */\n#ifndef {guard}\n#define {guard}\n#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n",
    );
    if exports
        .iter()
        .any(|export| export.scalar == LibraryScalar::Text)
    {
        out.push_str(
            "\ntypedef struct JetText { const uint8_t *ptr; size_t len; } JetText;\nvoid jet_text_free(JetText value);\n",
        );
    }
    out.push_str("\n#ifdef __cplusplus\nextern \"C\" {\n#endif\n");
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
    if exports
        .iter()
        .any(|export| export.scalar == LibraryScalar::Text)
    {
        out = out.replace(
            "class Library:",
            "class JetText(ctypes.Structure):\n    _fields_ = [(\"ptr\", ctypes.POINTER(ctypes.c_uint8)), (\"len\", ctypes.c_size_t)]\n\nclass Library:",
        );
        out.push_str("        self._lib.jet_text_free.argtypes = [JetText]\n");
        out.push_str("        self._lib.jet_text_free.restype = None\n");
    }
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
        if export.scalar == LibraryScalar::Text {
            out.push_str(&format!(
                "    def {name}(self, *args):\n        encoded = [ctypes.create_string_buffer(arg.encode(\"utf-8\")) for arg in args]\n        values = [JetText(ctypes.cast(arg, ctypes.POINTER(ctypes.c_uint8)), len(arg.value)) for arg in encoded]\n        result = self._lib.{symbol}(*values)\n        text = ctypes.string_at(result.ptr, result.len).decode(\"utf-8\")\n        self._lib.jet_text_free(result)\n        return text\n",
                name = export.name,
                symbol = c_symbol(&export.name),
            ));
        } else {
            out.push_str(&format!(
                "    def {name}(self, *args):\n        return self._lib.{symbol}(*args)\n",
                name = export.name,
                symbol = c_symbol(&export.name),
            ));
        }
    }
    out.push_str(&format!(
        "\ndef load(path):\n    return Library(path)\n\n# library: {name}\n"
    ));
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
            conventions: vec![AccessConvention::Read],
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
    if exports
        .iter()
        .any(|export| export.scalar == LibraryScalar::Text)
    {
        out.push_str(
            "public struct JetText { public var ptr: UnsafePointer<UInt8>?; public var len: Int }\n\n",
        );
    }
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

const LIBRARY_TEXT_HELPERS: &str = r#"
#[repr(C)]
pub struct JetText {
    pub ptr: *const u8,
    pub len: usize,
}

fn __jet_library_read_text(value: JetText) -> String {
    if value.ptr.is_null() || value.len == 0 {
        return String::new();
    }
    unsafe { String::from_utf8_lossy(std::slice::from_raw_parts(value.ptr, value.len)).into_owned() }
}

fn __jet_library_return_text(value: String) -> JetText {
    let bytes = value.into_bytes();
    if bytes.is_empty() {
        return JetText { ptr: std::ptr::null(), len: 0 };
    }
    let len = bytes.len();
    let ptr = Box::into_raw(bytes.into_boxed_slice()) as *const u8;
    JetText { ptr, len }
}

#[export_name = "jet_text_free"]
pub extern "C" fn __jet_library_text_free(value: JetText) {
    if value.ptr.is_null() || value.len == 0 {
        return;
    }
    unsafe {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            value.ptr as *mut u8,
            value.len,
        )));
    }
}
"#;
