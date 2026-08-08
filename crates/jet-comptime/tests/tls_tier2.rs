use std::collections::{HashMap, HashSet};
use std::path::Path;

use jet_comptime::AST::{Expr, Stmt};
use jet_comptime::Comptime::{
    CtValue, DevSink, ReplAuthorizer, ReplEffectRequest, evaluate_owned_with_imports_opts,
    run_repl_step,
};
use jet_comptime::Diagnostics::{Diagnostic, Span};

fn tls_call_diag(impure_depth: usize, allow_impure: bool) -> Diagnostic {
    let expr = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("tls".to_string(), Span::new(0, 3))),
        method: "client".to_string(),
        method_span: Span::new(4, 10),
        owner_type_args: Vec::new(),
        type_args: Vec::new(),
        args: Vec::new(),
        recv_type: None,
        resolved_ret: None,
        checked_widen: false,
    };
    let funcs = HashMap::new();
    let extern_names = HashSet::new();
    let globals: HashMap<String, CtValue> = HashMap::new();
    let mut core_imports = HashMap::new();
    core_imports.insert("tls".to_string(), "core.tls".to_string());

    evaluate_owned_with_imports_opts(
        &expr,
        &funcs,
        &extern_names,
        Path::new("."),
        &globals,
        &core_imports,
        allow_impure,
        impure_depth,
    )
    .expect_err("core.tls.client must not execute at comptime")
}

#[test]
fn core_tls_follows_the_whole_tier2_comptime_gate() {
    let pure = tls_call_diag(0, false);
    assert_eq!(pure.code, "E3410");
    assert_ne!(pure.code, "E0956");

    let gated = tls_call_diag(1, false);
    assert_eq!(gated.code, "E3411");
    assert_ne!(gated.code, "E0956");

    let allowed = tls_call_diag(1, true);
    assert_eq!(allowed.code, "E3412");
    assert_ne!(allowed.code, "E0956");
    assert_eq!(
        allowed.what,
        "`core.tls.client()` is not available at comptime"
    );
}

#[derive(Default)]
struct RecordingAuthorizer {
    requests: Vec<ReplEffectRequest>,
}

impl ReplAuthorizer for RecordingAuthorizer {
    fn preflight(&mut self, request: &ReplEffectRequest, span: Span) -> Result<(), Diagnostic> {
        self.requests.push(request.clone());
        Err(Diagnostic::error(
            "E1803",
            "test stopped after effect classification".to_string(),
            "the host operation must not run in this test".to_string(),
            "inspect the recorded request".to_string(),
            Some(span),
        ))
    }

    fn authorize(&mut self, _: &ReplEffectRequest, _: Span) -> Result<(), Diagnostic> {
        unreachable!("preflight stops the test")
    }

    fn fs_read(&mut self, _: &str) -> std::io::Result<Vec<u8>> {
        unreachable!("TLS classification performs no filesystem I/O")
    }

    fn fs_write(&mut self, _: &str, _: &[u8], _: bool) -> std::io::Result<()> {
        unreachable!("TLS classification performs no filesystem I/O")
    }

    fn fs_exists(&mut self, _: &str) -> std::io::Result<bool> {
        unreachable!("TLS classification performs no filesystem I/O")
    }

    fn fs_is_dir(&mut self, _: &str) -> std::io::Result<bool> {
        unreachable!("TLS classification performs no filesystem I/O")
    }

    fn fs_create_dir(&mut self, _: &str) -> std::io::Result<()> {
        unreachable!("TLS classification performs no filesystem I/O")
    }

    fn fs_remove(&mut self, _: &str) -> std::io::Result<()> {
        unreachable!("TLS classification performs no filesystem I/O")
    }

    fn verified_root(&mut self) -> std::io::Result<std::fs::File> {
        unreachable!("TLS classification does not verify a process root")
    }
}

#[test]
fn core_tls_repl_requests_use_the_net_effect() {
    let span = Span::new(0, 10);
    let tls_call = Expr::MethodCall {
        receiver: Box::new(Expr::Ident("tls".to_string(), Span::new(0, 3))),
        method: "client".to_string(),
        method_span: Span::new(4, 10),
        owner_type_args: Vec::new(),
        type_args: Vec::new(),
        args: Vec::new(),
        recv_type: None,
        resolved_ret: None,
        checked_widen: false,
    };
    let stmts = vec![Stmt::Grant {
        caps: vec![("Net".to_string(), span)],
        caps_span: span,
        binding: "caps".to_string(),
        binding_span: span,
        body: vec![Stmt::Expr(tls_call)],
        span,
    }];
    let funcs = HashMap::new();
    let mut scope = HashMap::new();
    let mut sink = DevSink::default();
    let mut core_imports = HashMap::new();
    core_imports.insert("tls".to_string(), "core.tls".to_string());
    let mut authorizer = RecordingAuthorizer::default();

    let error = run_repl_step(
        &stmts,
        &funcs,
        Path::new("."),
        &mut sink,
        &mut scope,
        100,
        true,
        &core_imports,
        &HashMap::new(),
        &HashMap::new(),
        &mut authorizer,
    )
    .expect_err("recording preflight stops execution");
    assert_eq!(error.code, "E1803");
    assert_eq!(authorizer.requests.len(), 1);
    assert_eq!(authorizer.requests[0].root, "Net");
    assert_eq!(authorizer.requests[0].operation, "client");
    assert_eq!(authorizer.requests[0].resource, "<network resource>");
}
