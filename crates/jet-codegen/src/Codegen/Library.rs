//! Native `Library` export projection (D-LIB-EXPORT1=C).
//!
//! The native boundary is the sema-owned `#Export(c)` surface: one homogeneous
//! `Int`, `Float`, `Bool`, or `Text` signature per exported function. This
//! module only renders wrappers and deterministic foreign text.

use crate::AST::{AccessConvention, ProgramBundle};
use jet_foundation::Names::{mangle, mangle_generated};

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

/// D-ADOPT-GUEST1=A: map the sema fact to the renderer's spelling table. The
/// marker and type decision is made in sema; this function never inspects a
/// function's AST types itself.
pub fn library_export_shape(function: &crate::AST::Func) -> Option<LibraryScalar> {
    crate::Sema::guest_export_signature(function)
        .and_then(|guest| guest.scalar)
        .map(|scalar| match scalar {
            crate::Sema::GuestScalar::Int => LibraryScalar::Int,
            crate::Sema::GuestScalar::Float => LibraryScalar::Float,
            crate::Sema::GuestScalar::Bool => LibraryScalar::Bool,
            crate::Sema::GuestScalar::Text => LibraryScalar::Text,
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
    crate::Sema::guest_export_surface(bundle)
        .into_iter()
        .filter_map(|export| {
            Some(LibraryExport {
                name: export.name,
                scalar: match export.scalar? {
                    crate::Sema::GuestScalar::Int => LibraryScalar::Int,
                    crate::Sema::GuestScalar::Float => LibraryScalar::Float,
                    crate::Sema::GuestScalar::Bool => LibraryScalar::Bool,
                    crate::Sema::GuestScalar::Text => LibraryScalar::Text,
                },
                params: export.params.len(),
                conventions: export.params.into_iter().map(|(convention, _)| convention).collect(),
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
                    format!(
                        "let {mutable}p{index} = {}(p{index});",
                        mangle_generated("library_read_text")
                    )
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
        let call = format!(
            "match {callee}({}) {{ Ok(value) => value, Err(error) => jet_entry_error_exit_jet(error) }}",
            args.join(", ")
        );
        let body = if export.scalar == LibraryScalar::Text {
            format!(
                "{} {}({call})",
                locals.join(" "),
                mangle_generated("library_return_text")
            )
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
        rust.push_str(&library_text_helpers());
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
            "\ntypedef struct JetText { const uint8_t *ptr; size_t len; } JetText;\n",
        );
    }
    out.push_str("\n#ifdef __cplusplus\nextern \"C\" {\n#endif\n");
    if exports
        .iter()
        .any(|export| export.scalar == LibraryScalar::Text)
    {
        out.push_str("void jet_text_free(JetText value);\n");
    }
    for export in exports {
        let access = export
            .conventions
            .iter()
            .map(|convention| match convention {
                AccessConvention::Read => "read",
                AccessConvention::Write => "write",
                AccessConvention::Move => "move",
            })
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!("/* jet-access: {access} */\n"));
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
        "# Generated by Jet — D-LIB-EXPORT1=C.\nimport ctypes\nimport sys\n\nclass Library:\n    def __init__(self, path):\n        self._lib = ctypes.CDLL(path)\n",
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
                "    def {name}(self, *args):\n        encoded = [arg.encode(\"utf-8\") for arg in args]\n        buffers = [ctypes.create_string_buffer(arg) for arg in encoded]\n        values = [JetText(ctypes.cast(arg, ctypes.POINTER(ctypes.c_uint8)), len(raw)) for arg, raw in zip(buffers, encoded)]\n        result = self._lib.{symbol}(*values)\n        if result.len == 0:\n            return \"\"\n        address = ctypes.cast(result.ptr, ctypes.c_void_p).value\n        if address is None or result.len > sys.maxsize or result.len > sys.maxsize - address:\n            raise ValueError(\"invalid JetText pointer-length pair\")\n        try:\n            return ctypes.string_at(result.ptr, result.len).decode(\"utf-8\")\n        finally:\n            self._lib.jet_text_free(result)\n",
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

    #[test]
    fn generated_text_projections_check_pointer_length_and_utf8() {
        let exports = vec![LibraryExport {
            name: "greet".to_string(),
            scalar: LibraryScalar::Text,
            params: 1,
            conventions: vec![AccessConvention::Read],
        }];
        let python = render_python("flightlog", &exports);
        let swift = render_swift(&exports);
        assert!(python.contains("address = ctypes.cast(result.ptr, ctypes.c_void_p).value"));
        assert!(python.contains("result.len > sys.maxsize - address"));
        assert!(python.contains(".decode(\"utf-8\")"));
        assert!(swift.contains("case invalidPointerLength"));
        assert!(swift.contains("case invalidUTF8"));
        assert!(swift.contains("String(bytes: bytes, encoding: .utf8)"));
        assert!(swift.contains("mutating func release()"));
        assert!(swift.contains("jet_text_free(self)"));
        assert!(swift.contains("self.ptr = nil"));
        assert!(swift.contains("self.len = 0"));
    }
}

fn render_swift(exports: &[LibraryExport]) -> String {
    let mut out = String::from("// Generated by Jet — D-LIB-EXPORT1=C.\nimport Foundation\n\n");
    if exports
        .iter()
        .any(|export| export.scalar == LibraryScalar::Text)
    {
        out.push_str(
            "public enum JetTextError: Error { case invalidPointerLength; case invalidUTF8 }\n\npublic struct JetText {\n    public var ptr: UnsafePointer<UInt8>?\n    public var len: Int\n\n    private func checkedBuffer() throws -> UnsafeBufferPointer<UInt8> {\n        guard len >= 0 else { throw JetTextError.invalidPointerLength }\n        if len == 0 { return UnsafeBufferPointer(start: nil, count: 0) }\n        guard let ptr else { throw JetTextError.invalidPointerLength }\n        let address = UInt(bitPattern: UnsafeRawPointer(ptr))\n        guard UInt(len) <= UInt.max - address else { throw JetTextError.invalidPointerLength }\n        return UnsafeBufferPointer(start: ptr, count: len)\n    }\n\n    public func decode() throws -> String {\n        let bytes = try checkedBuffer()\n        guard let value = String(bytes: bytes, encoding: .utf8) else { throw JetTextError.invalidUTF8 }\n        return value\n    }\n\n    public mutating func release() throws {\n        _ = try checkedBuffer()\n        jet_text_free(self)\n        self.ptr = nil\n        self.len = 0\n    }\n}\n\n@_silgen_name(\"jet_text_free\") private func jet_text_free(_ value: JetText)\n\n",
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

fn library_text_helpers() -> String {
    let read_text = mangle_generated("library_read_text");
    let return_text = mangle_generated("library_return_text");
    let text_free = mangle_generated("library_text_free");
    LIBRARY_TEXT_HELPERS
        .replace("JET_LIBRARY_READ_TEXT", &read_text)
        .replace("JET_LIBRARY_RETURN_TEXT", &return_text)
        .replace("JET_LIBRARY_TEXT_FREE", &text_free)
}

const LIBRARY_TEXT_HELPERS: &str = r#"
#[repr(C)]
pub struct JetText {
    pub ptr: *const u8,
    pub len: usize,
}

fn JET_LIBRARY_READ_TEXT(value: JetText) -> String {
    if value.len == 0 {
        return String::new();
    }
    if value.ptr.is_null()
        || value.len > isize::MAX as usize
        || (value.ptr as usize).checked_add(value.len).is_none()
    {
        panic!("invalid JetText pointer-length pair");
    }
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| panic!("JetText contains invalid UTF-8"))
}

fn JET_LIBRARY_RETURN_TEXT(value: String) -> JetText {
    let bytes = value.into_bytes();
    if bytes.is_empty() {
        return JetText { ptr: std::ptr::null(), len: 0 };
    }
    let len = bytes.len();
    let ptr = Box::into_raw(bytes.into_boxed_slice()) as *const u8;
    JetText { ptr, len }
}

#[export_name = "jet_text_free"]
pub extern "C" fn JET_LIBRARY_TEXT_FREE(value: JetText) {
    if value.len == 0 {
        return;
    }
    if value.ptr.is_null()
        || value.len > isize::MAX as usize
        || (value.ptr as usize).checked_add(value.len).is_none()
    {
        panic!("invalid JetText pointer-length pair");
    }
    unsafe {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            value.ptr as *mut u8,
            value.len,
        )));
    }
}
"#;
