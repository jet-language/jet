//! D-REACT1 / D-EVENT1 / D-PENDING1: resident-JIT reactive + event host.
//! Signal/derived/effect use the canonical observer algorithm via `include!`
//! of the reactive core extracted from ReactiveEventWatch.rs (build.rs).
//! Opaque i64 handles + JIT fn-ptr adapters — no third reactive graph.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use jet_codegen::scheduler::{
    jet_ctx_deadline_ms, jet_ctx_push_deadline, jet_scheduler_blocking_wait_enter,
    jet_scheduler_blocking_wait_leave, jet_scheduler_is_cancel_unwind,
    jet_scheduler_is_deadline_unwind, jet_scheduler_propagate_deadline,
    jet_scheduler_shielded, jet_scheduler_spawn_blocking_with_control_at, jet_scheduler_yield,
    jet_std_time_now, jet_task_join_deadline_check, JetSchedulerJoin, JetTaskControl,
    JetTaskFailure, JetTypedDeadlineBoundary, ParkSlot
};
use jet_codegen::task_group::jet_task_deadline_if_expired;
use std::sync::Arc;
use jet_foundation::Devtools::jet_devtools_publish_event;

/// No evaluator run carries a web devtools policy.
fn jet_web_runtime_devtools_enabled() -> bool {
    false
}

mod loadable_kernel {
    include!("../../jet-codegen/src/Prelude/Core/Loadable.rs");
}

include!("../../jet-foundation/src/LiveLifecycle.rs");
include!("../../jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs");
include!("../../jet-codegen/src/Prelude/CoreLib/Top/CoreSlice7LiveRaw.rs");

/// Canonical reactive core (JetSignal / JetDerived / jet_reactive_effect*).
#[allow(unused_imports)]
pub(crate) mod reactive_rt {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/reactive_rt.rs"));
}

fn with_async_runtime<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    f()
}

use reactive_rt as jet_std;
include!("../../jet-codegen/src/Prelude/CoreLib/Top/WebQuery.rs");

pub(crate) fn jet_jit_app_transact_invalidate_text(write_set: String) -> i64 {
    jet_app_transact_invalidate(write_set)
}

fn jet_deadline_check(wait_kind: &str) {
    if jet_scheduler_shielded() {
        return;
    }
    let remaining = jet_ctx_deadline_ms().map(|deadline| {
        deadline.saturating_sub(jet_std_time_now())
    });
    if let Some(deadline) = jet_task_deadline_if_expired(remaining, wait_kind) {
        jet_scheduler_propagate_deadline(deadline.render());
    }
}

// ── Events (thin adapters over canonical JetEvent*) ──────────────────────────

// ── Async event host: canonical scheduler-backed Event lifecycle ─────────────

fn _arc_keepalive() {
    let _: Arc<()> = Arc::new(());
}
