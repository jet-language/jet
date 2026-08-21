use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::AST::{walk_imports, ProgramBundle};
use std::collections::BTreeSet;

const SOCKET_SURFACES: &[&str] = &[
    "core.net",
    "core.net.tls",
    "core.net.ws",
    "core.http",
    "core.http.client",
    "core.http.server",
    "core.email",
];

const WASIP2_UNSUPPORTED_SURFACES: &[&str] = &[
    "core.net.tls",
    "core.net.ws",
    "core.http.client",
    "core.email",
];

/// D-WASISRV1=A: reject a reachable Core socket surface before codegen when
/// the selected target has no socket runtime that the Prelude can use.
pub fn check_target_surface(bundle: &ProgramBundle, target: &str) -> Vec<Diagnostic> {
    let socket_target = socket_target_supported(target);
    let wasip2 = target == Syntax::BUILD_TARGET_WASI_SERVER;
    let wasip2_unsupported_surfaces = if wasip2 {
        wasip2_unsupported_surfaces(bundle)
    } else {
        BTreeSet::new()
    };
    let wasip2_unsupported = if target == Syntax::BUILD_TARGET_WASI_SERVER {
        wasip2_unsupported_operations(bundle)
    } else {
        BTreeSet::new()
    };
    if socket_target
        && wasip2_unsupported.is_empty()
        && wasip2_unsupported_surfaces.is_empty()
    {
        return Vec::new();
    }

    let mut seen = BTreeSet::<(String, usize, usize, String)>::new();
    let mut diagnostics = Vec::new();
    for module in &bundle.modules {
        for (_, import) in walk_imports(module) {
            for binding in import.walk_bindings() {
                let module_path = binding.path();
                let Some(surface) = socket_surface(&module_path) else {
                    continue;
                };
                let span = binding.items_span.unwrap_or(binding.module_alias_span);
                let key = (
                    module.display.clone(),
                    span.start,
                    span.end,
                    "E3304".to_string(),
                );
                if !socket_target && seen.insert(key) {
                    diagnostics.push(Diagnostic::from_row(
                        "E3304",
                        &[("module", surface), ("target", target)],
                        Some(span),
                    ));
                }
                if wasip2_unsupported_surfaces.contains(surface) {
                    let operation_key = (
                        module.display.clone(),
                        span.start,
                        span.end,
                        format!("E3305:{surface}"),
                    );
                    if seen.insert(operation_key) {
                        diagnostics.push(Diagnostic::from_row(
                            "E3305",
                            &[("operation", surface), ("target", target)],
                            Some(span),
                        ));
                    }
                }
                for operation in &wasip2_unsupported {
                    if operation_surface(operation) != Some(surface) {
                        continue;
                    }
                    let operation_key = (
                        module.display.clone(),
                        span.start,
                        span.end,
                        format!("E3305:{operation}"),
                    );
                    if seen.insert(operation_key) {
                        diagnostics.push(Diagnostic::from_row(
                            "E3305",
                            &[("operation", operation.as_str()), ("target", target)],
                            Some(span),
                        ));
                    }
                }
            }
        }
    }
    diagnostics
}

fn wasip2_unsupported_surfaces(bundle: &ProgramBundle) -> BTreeSet<String> {
    let mut surfaces = BTreeSet::new();
    for module in &bundle.modules {
        for (_, import) in walk_imports(module) {
            for binding in import.walk_bindings() {
                if let Some(surface) = socket_surface(&binding.path()) {
                    if WASIP2_UNSUPPORTED_SURFACES.contains(&surface) {
                        surfaces.insert(surface.to_string());
                    }
                }
            }
        }
    }
    surfaces
}

fn operation_surface(operation: &str) -> Option<&str> {
    operation.rsplit_once('.').map(|(surface, _)| surface)
}

fn socket_surface(path: &str) -> Option<&'static str> {
    if let Some(surface) = SOCKET_SURFACES.iter().find(|surface| **surface == path) {
        return Some(*surface);
    }
    if Syntax::is_known_core_module(path) {
        return None;
    }
    let module = path.rsplit_once('.')?.0;
    SOCKET_SURFACES
        .iter()
        .find(|surface| **surface == module)
        .copied()
}

fn wasip2_unsupported_operations(bundle: &ProgramBundle) -> BTreeSet<String> {
    bundle
        .used_core
        .iter()
        .filter_map(|usage| {
            let (module, call) = usage.split_once("::")?;
            let unsupported = match module {
                "core.net" => {
                    matches!(
                        call,
                        "tcp_connect_happy"
                            | "tcp_ready"
                            | "udp_ready"
                            | "tls_connect"
                            | "tls_read"
                            | "tls_write"
                            | "tls_close"
                            | "dns_a"
                            | "dns_aaaa"
                            | "dns_txt"
                            | "dns_ptr"
                            | "dns_srv"
                    ) || call.starts_with("unix_")
                }
                "core.http" => matches!(call, "get" | "post" | "request"),
                "core.http.server" => call == "tls",
                "core.net.tls" => true,
                "core.http.client" | "core.email" => true,
                _ => false,
            };
            unsupported.then(|| format!("{module}.{call}"))
        })
        .collect()
}

fn socket_target_supported(target: &str) -> bool {
    target == Syntax::BUILD_TARGET_WASI_SERVER
        || [
            "linux", "android", "apple", "darwin", "windows", "freebsd", "openbsd", "netbsd",
        ]
        .iter()
        .any(|os| target.contains(os))
}
