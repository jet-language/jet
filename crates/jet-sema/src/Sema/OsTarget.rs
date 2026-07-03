//! D-OSTARGET1=A (ratified 2026-07-01, c134): native OS platform gating.
//! Mirrors `WebPartition.rs`'s shape — one structural check, one signature
//! check — for the second, mutually-exclusive `#Target(Os.*)` axis of the
//! same `#Target(...)` marker family.

use crate::AST::{Func, Item, ProgramBundle, Type};
use crate::Diagnostics::Diagnostic;
use crate::Syntax::{self, OsTarget as Os};
use std::collections::HashMap;

/// Walk the bundle: flag a `#Target(Os.*)`-gated impl whose enclosing file/
/// module also carries a web-bucket ceiling (`#Target(Wasm)`/`#Target(Js)`)
/// — a structural conflict between the two mutually-exclusive axes
/// (E-OSTARGET-MIXED-AXIS) — and flag a function/method that isn't itself
/// gated to match but takes or returns a value of a gated type
/// (E-OSTARGET-UNMATCHED-CALL): reachable from any build, it would call a
/// method the gated `impl` supplies — a Rust compile error (unresolved
/// method) on every OS but the gated one, since codegen strips that `impl`
/// entirely there (`Codegen/Imports.rs::emit_program_items`). Catching it in
/// sema turns that would-be rustc ICE (I2) into a Jet-level diagnostic.
///
/// Signature-level, not a full call-graph walk: mirrors how
/// `WebPartition::check_abi_export` also only inspects param/return types,
/// never call bodies. The existing effect call-graph (`fx_edges`) only
/// records bare-name calls (`CheckerInfer/calls.rs`), never method calls, so
/// it can't see a caller reaching a gated `impl`'s methods either way.
pub fn check_os_target(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut gate_of_type: HashMap<String, Os> = HashMap::new();

    for module in &bundle.modules {
        for item in &module.items {
            let Item::Impl(i) = item else { continue };
            let Some(os) = i.os_target else { continue };
            gate_of_type.insert(i.type_name.clone(), os);
            if let Some(bucket) = module.web_target_ceiling {
                let label = match &i.trait_name {
                    Some(t) => format!("{}.{}", i.type_name, t),
                    None => i.type_name.clone(),
                };
                diags.push(Syntax::os_target_mixed_axis(
                    &label,
                    os,
                    bucket.name(),
                    Some(i.type_span),
                ));
            }
        }
    }

    if gate_of_type.is_empty() {
        return diags;
    }

    for module in &bundle.modules {
        check_items_signatures(&module.items, &gate_of_type, &mut diags);
    }

    diags
}

fn check_items_signatures(
    items: &[Item],
    gate_of_type: &HashMap<String, Os>,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items {
        match item {
            Item::Func(f) => check_func_sig(f, None, gate_of_type, diags),
            Item::Impl(i) => {
                for m in &i.methods {
                    check_func_sig(m, i.os_target, gate_of_type, diags);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    check_func_sig(m, None, gate_of_type, diags);
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    check_func_sig(m, None, gate_of_type, diags);
                }
            }
            _ => {}
        }
    }
}

fn check_func_sig(
    f: &Func,
    own_gate: Option<Os>,
    gate_of_type: &HashMap<String, Os>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut flag = |type_name: &str| {
        // `self`'s placeholder type is `Type::Named("")` (S27) — never a real
        // gated type name, so a method's own receiver never self-triggers.
        if type_name.is_empty() {
            return;
        }
        let Some(&os) = gate_of_type.get(type_name) else {
            return;
        };
        if own_gate == Some(os) {
            return;
        }
        diags.push(Syntax::os_target_unmatched_call(
            &f.name,
            type_name,
            os,
            Some(f.name_span),
        ));
    };
    for p in &f.params {
        if let Type::Named(n) = &p.ty {
            flag(n);
        }
    }
    if let Some(Type::Named(n)) = &f.return_type {
        flag(n);
    }
}
