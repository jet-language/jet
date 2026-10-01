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
