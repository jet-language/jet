// D-TESTFAULT1=A / D-EFFTREE1: one Prelude-owned fault schedule over the
// existing effect tree. AOT and Cranelift adapters only report typed operation
// paths here; they do not choose roots, ordinals, policy, or error meaning.
//
// The generated harness returns ordinary `Result` values. The surrounding test
// harness wraps any error in the existing `JetTestFailure` report envelope
// (D-REPORT-TEST1); this part never invents a second report format.

const JET_FAULT_MAX_ATTEMPTS: usize = 4096;

/// A schedule target is either one canonical effect-tree path or the shared
/// fallible-allocation rail. Allocation is deliberately a channel, not a
/// synthetic public effect selector.
#[derive(Clone, Debug, PartialEq, Eq)]
enum JetFaultTarget {
    EffectPath(String),
    Allocation,
}

#[derive(Default)]
struct JetFaultState {
    targets: Vec<JetFaultTarget>,
    counts: Vec<usize>,
    active: Option<usize>,
    fail_nth: usize,
    injected: bool,
}

thread_local! {
    static JET_FAULT_STATE: std::cell::RefCell<JetFaultState> =
        std::cell::RefCell::new(JetFaultState::default());
}

fn jet_fault_clear() {
    JET_FAULT_STATE.with(|state| {
        *state.borrow_mut() = JetFaultState::default();
    });
}

fn jet_fault_state_is_clear() -> bool {
    JET_FAULT_STATE.with(|state| {
        let state = state.borrow();
        state.targets.is_empty()
            && state.counts.is_empty()
            && state.active.is_none()
            && state.fail_nth == 0
            && !state.injected
    })
}

/// D-EFFTREE1's one ancestor rule: a root covers every dotted path beneath it;
/// a leaf covers itself and deeper descendants, never a sibling.
fn jet_fault_effect_covers(bound: &str, observed: &str) -> bool {
    observed == bound
        || observed
            .strip_prefix(bound)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

fn jet_fault_record(state: &mut JetFaultState, target_index: usize, inject: bool) -> bool {
    let fail_nth = state.fail_nth;
    let injected = {
        let Some(count) = state.counts.get_mut(target_index) else {
            return false;
        };
        *count = count.saturating_add(1);
        inject && fail_nth != 0 && *count == fail_nth
    };
    if injected {
        state.injected = true;
    }
    injected
}

/// Called by Core operation adapters. An inactive schedule answers false.
/// Operation paths are already canonicalized by sema and supplied by the
/// shared AOT/JIT Prelude adapter code.
pub(crate) fn jet_fault_should_fail(operation: &str) -> bool {
    JET_FAULT_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let active = state.active;
        let mut injected = false;
        // The clean pass and every injected pass observe every configured
        // target. Only the selected target may inject; this captures recovery
        // calls under other roots for the next deterministic schedule round.
        for target_index in 0..state.targets.len() {
            let matches = state.targets.get(target_index).is_some_and(|target| {
                matches!(target, JetFaultTarget::EffectPath(selector)
                    if jet_fault_effect_covers(selector, operation))
            });
            if matches {
                injected |= jet_fault_record(&mut state, target_index, active == Some(target_index));
            }
        }
        injected
    })
}

/// Called by every shared fallible-allocation rail. This is the final typed
/// target in the same schedule as effect paths, so OOM recovery gets the same
/// fail-nth loop without adding a fake `Alloc` effect root.
pub(crate) fn jet_fault_should_fail_allocation() -> bool {
    JET_FAULT_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(target_index) = state
            .targets
            .iter()
            .position(|target| matches!(target, JetFaultTarget::Allocation))
        else {
            return false;
        };
        let inject = state.active == Some(target_index);
        jet_fault_record(&mut state, target_index, inject)
    })
}

/// Stable identity for a fault plan. It is deliberately independent of time,
/// process identity, hash-map order, or thread scheduling, so a capped run is
/// reproducible from the source selector list alone.
fn jet_fault_seed(selectors: &[&str]) -> u64 {
    let mut seed = 0xcbf29ce484222325u64;
    for selector in selectors {
        for byte in selector.as_bytes() {
            seed ^= u64::from(*byte);
            seed = seed.wrapping_mul(0x100000001b3);
        }
        seed ^= 0xff;
        seed = seed.wrapping_mul(0x100000001b3);
    }
    seed.max(1)
}

struct JetFaultScope;

impl Drop for JetFaultScope {
    fn drop(&mut self) {
        // Runs can leave through a normal error, a panic, or a runtime stop.
        // One Drop path clears the scheduler in all three cases before the
        // next deterministic attempt starts.
        jet_fault_clear();
    }
}

struct JetFaultRun {
    result: Result<(), String>,
    injected: bool,
    counts: Vec<usize>,
    scheduler_clean: bool,
}

fn jet_fault_run_once_with_cleanup<F: FnMut() -> Result<(), String>>(
    selectors: &[&str],
    active: Option<usize>,
    fail_nth: usize,
    body: &mut F,
) -> Result<JetFaultRun, String> {
    let mut targets = selectors
        .iter()
        .map(|selector| JetFaultTarget::EffectPath((*selector).to_string()))
        .collect::<Vec<_>>();
    // The shared allocation rail is part of every non-empty fault plan. It is
    // not exposed as a user-written selector and therefore cannot drift from
    // the ratified effect-root surface.
    targets.push(JetFaultTarget::Allocation);
    let target_count = targets.len();
    if active.is_some_and(|index| index >= target_count) {
        return Err("fault schedule selected an invalid target".to_string());
    }

    JET_FAULT_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.targets = targets;
        state.counts = vec![0; target_count];
        state.active = active;
        state.fail_nth = fail_nth;
        state.injected = false;
    });

    // This guard owns scheduler cleanup. The body itself still runs its normal
    // Jet defer/sentry cleanup on every return and on every caught unwind.
    let scope = JetFaultScope;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
    let (injected, counts) = JET_FAULT_STATE.with(|state| {
        let state = state.borrow();
        (state.injected, state.counts.clone())
    });
    drop(scope);
    let scheduler_clean = jet_fault_state_is_clear();
    if !scheduler_clean {
        return Err("fault scheduler cleanup did not complete".to_string());
    }
    let result = match result {
        Ok(result) => result,
        Err(_) => return Err("test panicked during fault injection".to_string()),
    };
    Ok(JetFaultRun {
        result,
        injected,
        counts,
        scheduler_clean,
    })
}

// Kept as the small internal probe used by allocator-family checks. The
// production loop uses the richer result above so cleanup remains an explicit
// criterion instead of an assertion hidden in a test helper.
fn jet_fault_run_once<F: FnMut() -> Result<(), String>>(
    selectors: &[&str],
    active: Option<usize>,
    fail_nth: usize,
    body: &mut F,
) -> Result<(Result<(), String>, bool, Vec<usize>), String> {
    let run = jet_fault_run_once_with_cleanup(selectors, active, fail_nth, body)?;
    Ok((run.result, run.injected, run.counts))
}

/// Run the clean case, then fail each reachable effect/allocation target at
/// each observed ordinal. An injected iteration must return through the
/// ordinary error rail without a panic. Newly revealed sites extend the same
/// bounded deterministic schedule; a runaway body becomes one ordinary test
/// failure instead of an unbounded test process.
pub(crate) fn jet_fault_test_loop<F: FnMut() -> Result<(), String>>(
    selectors: &[&str],
    mut body: F,
) -> Result<(), String> {
    if selectors.is_empty() {
        return body();
    }
    let seed = jet_fault_seed(selectors);
    let target_count = selectors.len() + 1; // effect paths + allocation rail
    let clean = jet_fault_run_once_with_cleanup(selectors, None, 0, &mut body)?;
    if !clean.scheduler_clean {
        return Err("fault scheduler cleanup did not complete".to_string());
    }
    if let Err(error) = clean.result {
        return Err(error);
    }
    let mut max_counts = clean.counts;
    let mut next_fail_nth = vec![1; target_count];
    let mut attempts = 0usize;

    loop {
        let mut discovered = false;
        let mut selector_index = 0usize;
        while selector_index < target_count {
            let mut fail_nth = next_fail_nth[selector_index];
            while fail_nth <= max_counts[selector_index] {
                if attempts >= JET_FAULT_MAX_ATTEMPTS {
                    return Err(format!(
                        "fault schedule exceeded {} attempts (seed {})",
                        JET_FAULT_MAX_ATTEMPTS, seed
                    ));
                }
                attempts += 1;
                let run = jet_fault_run_once_with_cleanup(
                    selectors,
                    Some(selector_index),
                    fail_nth,
                    &mut body,
                )?;
                if !run.scheduler_clean {
                    return Err("fault scheduler cleanup did not complete".to_string());
                }
                if !run.injected {
                    if let Err(error) = run.result {
                        return Err(error);
                    }
                }
                if run.injected {
                    for (index, count) in run.counts.iter().enumerate() {
                        if *count > max_counts[index] {
                            max_counts[index] = *count;
                            discovered = true;
                        }
                    }
                }
                fail_nth = fail_nth.saturating_add(1);
                next_fail_nth[selector_index] = fail_nth;
            }
            selector_index += 1;
        }
        if !discovered
            && next_fail_nth
                .iter()
                .zip(&max_counts)
                .all(|(next, max)| *next > *max)
        {
            break;
        }
    }
    Ok(())
}
