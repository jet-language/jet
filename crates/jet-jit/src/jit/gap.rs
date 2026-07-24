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

/// E2211 — D-LENS-RUN1 / card #728 exact product copy.
pub fn e2211_diagnostic(gap: &JitGap, bundle: &ProgramBundle) -> Diagnostic {
    Diagnostic::error(
        "E2211",
        "Jet JIT has a compiler gap for this checked program.".to_string(),
        format!(
            "jet run uses the JIT lens and does not hide a JIT compiler gap by running AOT. JIT gap in {}: {}.",
            gap.function, gap.reason
        ),
        "run `jet build <file>` and the binary for now, then report E2211 with the detail below."
            .to_string(),
        entry_run_span(bundle),
    )
}

pub fn is_e2211(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.code == "E2211")
}
