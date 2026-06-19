use super::*;
use crate::AST::Type;
pub(crate) fn emit_c_module(cm: &crate::AST::CModule, out: &mut String) {
    if cm.functions.is_empty() {
        return;
    }
    out.push_str("#[allow(non_snake_case)]\nextern \"C\" {\n");
    for ef in &cm.functions {
        let params: Vec<String> = ef
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| format!("a{i}: {}", c_abi_rust_type(&p.ty)))
            .collect();
        let ret = ef
            .return_type
            .as_ref()
            .map(|t| format!(" -> {}", c_abi_rust_type(t)))
            .unwrap_or_default();
        out.push_str(&format!(
            "    fn {}({}){};\n",
            ef.rust_path,
            params.join(", "),
            ret
        ));
    }
    out.push_str("}\n\n");

    for ef in &cm.functions {
        let mut sig_params = Vec::new();
        let mut conv_lines = Vec::new();
        let mut call_args = Vec::new();
        for (i, p) in ef.params.iter().enumerate() {
            sig_params.push(format!("a{i}: {}", c_wrapper_param_type(&p.ty)));
            match &p.ty {
                Type::String => {
                    conv_lines.push(format!(
                        "    let c{i} = std::ffi::CString::new(a{i}.as_str()).unwrap_or_default();"
                    ));
                    call_args.push(format!("c{i}.as_ptr()"));
                }
                Type::Char => call_args.push(format!("(*a{i} as u32)")),
                _ => call_args.push(format!("a{i}")),
            }
        }
        let ret = ef
            .return_type
            .as_ref()
            .map(|t| format!(" -> {}", c_wrapper_ret_type(t)))
            .unwrap_or_default();
        let call = format!("{}({})", ef.rust_path, call_args.join(", "));
        let body = match &ef.return_type {
            None => format!("    unsafe {{ {}; }}", call),
            Some(Type::String) => format!(
                "    let p = unsafe {{ {} }};\n    if p.is_null() {{ return String::new(); }}\n    unsafe {{ std::ffi::CStr::from_ptr(p) }}.to_string_lossy().into_owned()",
                call
            ),
            Some(Type::Char) => format!(
                "    let v = unsafe {{ {} }};\n    char::from_u32(v).unwrap_or('\\u{{0}}')",
                call
            ),
            Some(_) => format!("    unsafe {{ {} }}", call),
        };
        out.push_str(&format!(
            "pub fn {}({}){} {{\n{}\n}}\n\n",
            mangle(&ef.name),
            sig_params.join(", "),
            ret,
            body
        ));
    }
}

/// The C-ABI Rust type used in the `extern "C"` declaration.
fn c_abi_rust_type(ty: &Type) -> String {
    match ty {
        Type::Int => "std::os::raw::c_longlong".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Char => "u32".to_string(),
        Type::String => "*const std::os::raw::c_char".to_string(),
        // Sema (E3203) rejects anything else at the C boundary.
        other => format!("/* unsupported: {} */ ()", other.name()),
    }
}

/// The Rust type the safe wrapper accepts, matching cross-module call sites
/// (Read convention: scalars by value, `String`/`Char` by shared reference).
fn c_wrapper_param_type(ty: &Type) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Char => "&char".to_string(),
        Type::String => "&String".to_string(),
        other => format!("/* unsupported: {} */ ()", other.name()),
    }
}

fn c_wrapper_ret_type(ty: &Type) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Char => "char".to_string(),
        Type::String => "String".to_string(),
        other => format!("/* unsupported: {} */ ()", other.name()),
    }
}
