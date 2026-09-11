use jet_foundation::Diagnostics::Diagnostic;
use jet_foundation::MIR::MirFunctionId;

/// A Cranelift failure attached to the exact canonical MIR function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JitGap {
    pub function: MirFunctionId,
    pub function_name: String,
    pub reason: String,
}

impl JitGap {
    pub fn new(
        function: MirFunctionId,
        function_name: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            function,
            function_name: function_name.into(),
            reason: reason.into(),
        }
    }
}

/// Retained only as a diagnostic compatibility query.  The MIR adapter never
/// manufactures this code; unsupported MIR is reported as a compile error.
pub fn is_e2211(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|diagnostic| diagnostic.code == "E2211")
}
