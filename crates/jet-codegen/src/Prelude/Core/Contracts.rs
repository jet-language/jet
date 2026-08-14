// D-FAIL-TIER1: one contract predicate and one report shape for every engine.
//
// The AOT emitter calls these functions directly. The interpreter and
// Cranelift hosts include this same source and only marshal values into it.
// Keep the contract message and the rich E3005 report here; callers own only
// control flow and transport.
#[inline]
pub(crate) fn jet_contract_check(condition: bool) -> bool {
    condition
}

#[inline]
pub(crate) fn jet_contract_report(
    clause_kw: &str,
    msg: &str,
    file: &str,
    line: u32,
) -> JetRuntimeDiagnostic {
    let message = format!("#{} contract failed: {}", clause_kw, msg);
    jet_render_runtime_stop("E3005", file, line, "", "", 1, 1, &message, "")
}
