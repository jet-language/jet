//! Canonical Core-call projection for the TIR-to-MIR boundary.
//!
//! Core-call records are resolved before this seam.  This module only copies
//! the registry row into the canonical MIR carrier; it does not rediscover
//! symbols, routes, coverage, or backend policy.

use jet_foundation::MIR::MirCoreCall;
use jet_foundation::Syntax::CoreCallRecord;



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
