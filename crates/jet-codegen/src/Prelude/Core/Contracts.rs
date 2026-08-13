// D-FAIL-TIER1: one contract predicate and one report shape for every engine.
//
// The AOT emitter calls these functions directly.  The interpreter and
// Cranelift hosts include this same source and only marshal values into it.
// Keep policy and wording here; callers own only control flow and transport.
#[inline]
pub(crate) fn jet_contract_check(condition: bool) -> bool {
    condition
}

#[inline]
pub(crate) fn jet_contract_report(clause_kw: &str, msg: &str, file: &str, line: u32) -> String {
    format!(
        "#{} contract failed: {}\n  --> {}:{}",
        clause_kw, msg, file, line
    )
}
