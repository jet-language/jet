use jet_foundation::{
    AST::{Item, ProgramBundle},
    Diagnostics::{Diagnostic, Span},
};

/// A resident Cranelift gap discovered before or during JIT lowering/compile.
#[derive(Debug, Clone)]
pub struct JitGap {
    pub function: String,
    pub reason: String,
}

impl JitGap {
    pub fn new(function: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            function: function.into(),
            reason: reason.into(),
        }
    }
}

pub fn entry_run_name(bundle: &ProgramBundle) -> String {
    bundle
        .modules
        .get(bundle.entry)
        .and_then(|module| {
            module.items.iter().find_map(|item| {
                if let Item::Func(f) = item {
                    if f.name == "run" {
                        Some(f.name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "run".to_string())
}

pub fn entry_run_span(bundle: &ProgramBundle) -> Option<Span> {
    bundle.modules.get(bundle.entry).and_then(|module| {
        module.items.iter().find_map(|item| {
            if let Item::Func(f) = item {
                if f.name == "run" {
                    Some(f.name_span)
                } else {
                    None
                }
            } else {
                None
            }
        })
    })
}

/// Retired E2211 detector — kept so tests can assert the code is never emitted
/// after D-LENS-RUN2=A / #778 (silent interpreter deopt).
pub fn is_e2211(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.code == "E2211")
}
