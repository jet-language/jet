use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Diagnostics::suggest_field;
use crate::Sema::{Checker, LocalInfo};
use crate::Syntax;
use crate::AST::{Expr, Type};
impl<'a> Checker<'a> {
    /// Declare one name bound by a destructuring pattern (S74).
    pub(crate) fn declare_bound(
        &mut self,
        name: &str,
        span: Span,
        ty: Type,
        mutable: bool,
        binding_sigil_span: Option<Span>,
    ) {
        let sendable = self.sendability_problem(&ty, true).is_none();
        let single_use_span = if self.type_is_single_use(&ty) {
            Some(span)
        } else {
            None
        };
        self.declare_with_sendability(
            name,
            span,
            LocalInfo {
                def_span: span,
                binding_sigil_span,
                ty,
                mutable,
                param_conv: None,
                decl_loop_depth: self.loop_depth,
                interrupt_sendable: false,
                reactive_local: false,
                reactive_shared: false,
                single_use_span,
                invalid: false,
            },
            sendable,
        );
    }

    // --- expressions ------------------------------------------------------

    pub(crate) fn require_bool(&mut self, e: &mut Expr, what: &str) {
        // Conditions consume a success Boolean, even when an enclosing value
        // expression expects its Result carrier. That lets a fallible Boolean
        // call use the same automatic propagation path as every other value.
        let saved_expected = self.expected_type.replace(Type::Bool);
        let inferred = self.infer(e);
        self.expected_type = saved_expected;
        if let Some(t) = inferred {
            if t != Type::Bool {
                self.diags.push(Diagnostic::error(
                    "E0110",
                    format!(
                        "{} must be {}, but this is {}",
                        what,
                        Type::Bool.show(),
                        t.show()
                    ),
                    "the program needs a clear yes or no here".to_string(),
                    "compare the value to something, e.g. `x > 0` or `name == \"ok\"`".to_string(),
                    Some(e.span()),
                ));
            }
        }
    }

    pub(crate) fn unknown_name(&mut self, name: &str, span: Span) {
        let mut fix = format!(
            "declare it first: `{} {} ...`",
            name,
            Syntax::SIGIL_BIND_IMMUT
        );
        if let Some(module) = unique_core_module_for_alias(name) {
            fix = format!("add `use {module} as {name}`");
        }
        // I8: one suggester. `suggest_field` is the same distance-≤2
        // nearest-candidate pick this used to inline, and it refuses a
        // candidate equal to `name` — a visible-but-unresolvable name must
        // not be echoed back as the fix for its own failed lookup (#2002).
        let candidates: Vec<String> = self
            .visible_names()
            .into_iter()
            .chain(self.consts.keys().cloned())
            .collect();
        let suggestion = suggest_field(name, &candidates);
        if let Some(cand) = suggestion.as_deref() {
            fix = format!("did you mean `{}`?", cand);
        }
        let mut diagnostic = Diagnostic::error(
            "E0107",
            format!("nothing named `{}` exists here", name),
            "a name must be declared before it's used".to_string(),
            fix,
            Some(span),
        );
        if let Some(cand) = suggestion {
            diagnostic = diagnostic.with_edit(crate::Diagnostics::TextEdit {
                span,
                new_text: cand,
            });
        }
        self.diags.push(diagnostic);
    }

}

const CORE_DIAGNOSTIC_ALIASES: &[(&str, &str)] = &[
    ("fs", "core.files"),
    ("ar", "core.archive"),
    ("gz", "core.archive.gzip"),
    ("re", "core.regex"),
];

fn unique_core_module_for_alias(alias: &str) -> Option<&'static str> {
    for &(known_alias, module) in CORE_DIAGNOSTIC_ALIASES {
        if known_alias == alias && Syntax::KNOWN_CORE_MODULES.contains(&module) {
            return Some(module);
        }
    }

    let mut matches = Syntax::KNOWN_CORE_MODULES
        .iter()
        .copied()
        .filter(|module| module.rsplit('.').next().unwrap_or(module) == alias);
    let module = matches.next()?;
    matches.next().is_none().then_some(module)
}
