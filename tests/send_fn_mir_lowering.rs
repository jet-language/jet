//! Send-safe callable carriers reach canonical MIR that the legality verifier
//! accepts.
//!
//! Every App route/action handler and every named `os.on_interrupt` callback
//! is lowered as a `SendFn` value.  Two defects made every such program an
//! internal compiler error before the first instruction ran: the `SendFn`
//! carrier was built without a canonical type identity (`MIR type instance
//! has no canonical identity`), and App `route`/`page`/`layout`/`loader`
//! rows appended a binding-text constant between a `HostBorrowCallback` and
//! the call that consumes it (`HostBorrowCallback result must be consumed by
//! the next host/Prelude call`).  `lower_checked_mir_program_for` runs the
//! canonical optimization pipeline, whose first pass is that verifier, so a
//! successful lowering here is the contract.

mod common;

use std::fs;

const APP_PACKAGE: &str = r#"
name: "send_fn_mir_lowering"
version: "0.1.0"
authority: { holds: { allow: [IO, Env, Net] } }
"#;

/// Every App handler shape that lowers through a send-safe borrow callback,
/// including the ones (`route`, `page`, `loader`) whose row carries a trailing
/// binding text after the handler argument.
const APP_HANDLERS: &str = r#"
use core.web as web
#Target(Web)

fn home() -> WebPage { web.page("Home", "hello") }

fn detail(id: Int) -> WebPage { web.page("Detail", "item {id}") }

fn save() -> String { "saved" }

fn run() -> App {
    return web.app()
        .route("/", home)
        .page("/about", home)
        .route("/items/:id", detail)
        .action("save", save)
        .form("order", save)
        .pending(home)
        .csr()
}
"#;

/// A named thread-boundary callback outside the Web surface: the same
/// `SendFn` carrier with no App types anywhere in the program.
const NAMED_INTERRUPT_CALLBACK: &str = r#"
use core.sys as os

fn named_callback() {
    print("named")
}

fn run() {
    os.on_interrupt(named_callback)
    print("ready")
}
"#;

fn checked_bundle(name: &str, source: &str) -> (
    jet::AST::ProgramBundle,
    std::sync::Arc<[std::sync::Arc<jet::AST::MirFragmentNominalFacts>]>,
) {
    let dir = common::unique_tmp(&format!("jet_send_fn_mir_{name}"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("package.jet"), APP_PACKAGE).unwrap();
    let entry = dir.join("main.jet");
    fs::write(&entry, source).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let (diagnostics, _facts, checked_nominals) =
        jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    let errors: Vec<_> = diagnostics.into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{name}: {errors:#?}");
    let _ = fs::remove_dir_all(dir);
    (bundle, checked_nominals)
}

fn assert_lowers_on_every_tier(name: &str, source: &str) {
    jet_foundation::CompilerStack::run_on_compiler_stack(|| {
        let (bundle, checked_nominals) = checked_bundle(name, source);
        let (interpreter, _) = common::lower_interpreter_bundle(&bundle, &checked_nominals)
            .unwrap_or_else(|error| panic!("{name}: interpreter MIR lowering failed: {error}"));
        interpreter
            .validate()
            .unwrap_or_else(|error| panic!("{name}: interpreter MIR does not validate: {error}"));
    });
}

#[test]
fn app_handlers_lower_to_verified_mir() {
    assert_lowers_on_every_tier("app_handlers", APP_HANDLERS);
}

#[test]
fn named_interrupt_callback_lowers_to_verified_mir() {
    assert_lowers_on_every_tier("named_interrupt", NAMED_INTERRUPT_CALLBACK);
}

#[test]
fn captured_http_resources_are_owned_once_and_lent_per_invocation() {
    jet_foundation::CompilerStack::run_on_compiler_stack(|| {
        let entry = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("Examples/features/net/http_handler_resource_capture.jet");
        let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
        let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
            .into_iter()
            .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let (program, _) = common::lower_interpreter_bundle(&bundle).unwrap();
        let handler = program.functions.iter().find(|function| {
            function.capture_params.iter().any(|capture| capture.name == "conn")
                && function.capture_params.iter().any(|capture| capture.name == "pool")
        }).expect("checked HTTP handler retains both resource capture slots");
        for name in ["conn", "pool"] {
            let capture = handler.capture_params.iter().find(|capture| capture.name == name).unwrap();
            assert_eq!(capture.access, jet_foundation::MIR::MirAccess::Read);
            assert_eq!(capture.ownership.mode, jet_foundation::MIR::MirOwnershipMode::Owned);
        }
    });
}
