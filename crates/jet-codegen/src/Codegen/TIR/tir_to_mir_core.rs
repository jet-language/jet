//! Canonical Core-call projection for the TIR-to-MIR boundary.
//!
//! Core-call records are resolved before this seam. Source-owned declarations
//! remain sema metadata and lower as ordinary function calls, never MIR host
//! rows. Executable registry rows retain their exact symbols and route facts.

use jet_foundation::MIR::MirCoreCall;
use jet_foundation::Syntax::{CoreCallRecord, CoreCallSymbol};

/// Project the exact checked Core-call records used by a TIR program.
///
/// The registry record, rather than a reconstructed `(module, member)` pair,
/// is the identity input so receiver rows and every resolved metadata field
/// survive unchanged.  The result has one row per `(id, key)`, in stable
/// `(id, key)` order, regardless of the input traversal order.
pub(super) fn lower_core_records(
    records: impl IntoIterator<Item = &'static CoreCallRecord>,
) -> Vec<MirCoreCall> {
    let mut rows = records
        .into_iter()
        .filter(|record| !matches!(record.symbol, CoreCallSymbol::SourceOwned))
        .map(MirCoreCall::from_record)
        .collect::<Vec<_>>();
    rows.sort_unstable_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.key.cmp(&right.key))
    });
    rows.dedup_by(|left, right| left.id == right.id && left.key == right.key);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::Syntax::CoreCallInterpreterRoute;

    #[test]
    fn source_owned_declarations_do_not_enter_executable_mir_registry() {
        static RECORDS: [CoreCallRecord; 4] = [
            CoreCallRecord::new(
                "core.time", "__sleep_ns", "jet_std_time_sleep_duration_ns", true, &[false],
            ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
            CoreCallRecord::source_owned("core.time", "sleep", &[true]),
            CoreCallRecord::receiver_with_symbol(
                &["Condition"], "notify_one", "jet_condition_notify_one", true, &[true],
            ).with_source_owned(),
            CoreCallRecord::source_owned("core.event", "scope", &[])
                .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
        ];
        let rows = lower_core_records([
            &RECORDS[3], &RECORDS[2], &RECORDS[0], &RECORDS[1], &RECORDS[0],
        ]);
        let expected = MirCoreCall::from_record(&RECORDS[0]);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.id, expected.id);
        assert_eq!(row.key, expected.key);
        assert_eq!(row.symbol, expected.symbol);
        assert_eq!(row.arity, expected.arity);
        assert_eq!(row.max_arity, expected.max_arity);
        assert_eq!(row.borrow_mask, expected.borrow_mask);
        assert_eq!(row.coverage_bits, expected.coverage_bits);
        assert_eq!(row.interpreter_route, expected.interpreter_route);
        assert_eq!(row.pure_route, expected.pure_route);
        assert!(lower_core_records([&RECORDS[1], &RECORDS[2], &RECORDS[3]]).is_empty());
    }
}
