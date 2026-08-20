use crate::AST::{walk_imports, ProgramBundle};
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use std::collections::BTreeSet;

const HTTP_SERVER: &str = "core.http.server";

/// D-WASISRV1=A: reject a reachable Core server surface before codegen when
/// the selected target has no socket runtime that the Prelude can use.
pub fn check_target_surface(bundle: &ProgramBundle, target: &str) -> Vec<Diagnostic> {
    if http_server_target_supported(target) {
        return Vec::new();
    }

    let mut seen = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for module in &bundle.modules {
        for (_, import) in walk_imports(module) {
            for binding in import.walk_bindings() {
                if binding.path() != HTTP_SERVER {
                    continue;
                }
                let span = binding.items_span.unwrap_or(binding.module_alias_span);
                let key = (module.display.clone(), span.start, span.end);
                if seen.insert(key) {
                    diagnostics.push(Diagnostic::from_row(
                        "E3304",
                        &[("module", HTTP_SERVER), ("target", target)],
                        Some(span),
                    ));
                }
            }
        }
    }
    diagnostics
}

fn http_server_target_supported(target: &str) -> bool {
    target == Syntax::BUILD_TARGET_WASI_SERVER
        || [
            "linux",
            "android",
            "apple",
            "darwin",
            "windows",
            "freebsd",
            "openbsd",
            "netbsd",
        ]
        .iter()
        .any(|os| target.contains(os))
}
