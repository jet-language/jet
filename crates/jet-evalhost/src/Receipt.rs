//! Interpreter and resident-host adapters for the shared receipt section kernel.
//!
//! The kernel owns canonicalization, limits, identity, and publication. This
//! module only turns a tier value into the kernel's encoded boundary and wires
//! the interpreter ambient callback.

use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};

pub(crate) mod kernel {
    pub(crate) use crate::Encoding::json_rt as jet_std;
    use jet_codegen::development_receipt::ensure_receipt_directory;

    #[allow(non_camel_case_types)]
    pub(crate) trait __jet_Encode {
        fn jet_encode(&self) -> jet_std::Dyn;
    }

    fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
        crate::Crypto::runtime::jet_crypto_email_sha256_impl(data)
    }

    include!("../../jet-codegen/src/Prelude/Core/Receipt.rs");
}

enum RuntimeWord {
    Bits(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    String(String)
}

fn bits(word: RuntimeWord, owner: &str) -> Result<i64, String> {
    match word {
        RuntimeWord::Bits(value) => Ok(value),
        RuntimeWord::Float(_)
        | RuntimeWord::Bool(_)
        | RuntimeWord::Char(_)
        | RuntimeWord::String(_) => {
            Err(format!("receipt value `{owner}` has an incompatible runtime carrier"))
        }
    }
}

fn receipt_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the interpreter ambient receipt adapter rejected the attachment".to_string(),
        "report this as a compiler bug".to_string(),
        Some(span),
    )
}

fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    // Only the registered raw leaf receives the payload and compiler metadata.
    if module != "core.prelude" || method != "__receipt_attach" {
        return None;
    }
    let [value, CtValue::Str(section_name), CtValue::Str(type_name), CtValue::Str(schema_digest)] =
        args.as_slice()
    else {
        return Some(Err(receipt_diag(
            "receipt.attach received malformed payload or metadata",
            span,
        )));
    };
    let payload = match jet_codegen::Comptime::render_dyn_for_tir(value) {
        Ok(payload) => payload,
        Err(error) => return Some(Err(receipt_diag(error.reason, span))),
    };
    kernel::jet_receipt_attach_encoded(
        &payload,
        section_name,
        type_name,
        schema_digest,
    );
    Some(Ok(CtValue::Unit))
}

pub(crate) fn register_interpreter_ambient(context: &mut crate::InterpreterAmbientContext) {
    context.register_core_call(ambient_core_call);
}

