// D-TESTFAULT1=A: one Prelude-owned fail-nth scheduler for effect-root tests.
// Engines only marshal calls into this state; they do not own policy or loop
// semantics.

#[derive(Default)]
struct JetFaultState {
    selectors: Vec<String>,
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

/// Called by Core operation adapters. An inactive scheduler always answers
/// false, so production programs and ordinary non-fault tests pay no policy
/// branch beyond this shared adapter.
pub(crate) fn jet_fault_should_fail(operation: &str) -> bool {
    JET_FAULT_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(active) = state.active else { return false };
        let matches = state.selectors.get(active).is_some_and(|selector| {
            operation == selector
                || operation
                    .strip_prefix(selector)
                    .is_some_and(|suffix| suffix.starts_with('.'))
        });
        if !matches {
            return false;
        }
        state.counts[active] = state.counts[active].saturating_add(1);
        if state.counts[active] == state.fail_nth {
            state.injected = true;
            true
        } else {
            false
        }
    })
}

fn jet_fault_run_once<F: FnMut() -> Result<(), String>>(
    selectors: &[&str],
    active: Option<usize>,
    fail_nth: usize,
    body: &mut F,
) -> Result<(Result<(), String>, bool, Vec<usize>), String> {
    JET_FAULT_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.selectors = selectors.iter().map(|selector| (*selector).to_string()).collect();
        state.counts = vec![0; selectors.len()];
        state.active = active;
        state.fail_nth = fail_nth;
        state.injected = false;
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
    let (injected, counts) = JET_FAULT_STATE.with(|state| {
        let state = state.borrow();
        (state.injected, state.counts.clone())
    });
    jet_fault_clear();
    match result {
        Ok(result) => Ok((result, injected, counts)),
        Err(_) => Err("test panicked during fault injection".to_string()),
    }
}

/// Run the clean case, then fail each reachable selector at each observed
/// call ordinal. A failed iteration is successful when it returns through the
/// ordinary error rail; only a panic fails the test. Counts discovered while
/// handling an earlier injected failure extend the deterministic schedule.
pub(crate) fn jet_fault_test_loop<F: FnMut() -> Result<(), String>>(
    selectors: &[&str],
    mut body: F,
) -> Result<(), String> {
    if selectors.is_empty() {
        return body();
    }
    let (clean, _, mut max_counts) = jet_fault_run_once(selectors, None, 0, &mut body)?;
    if let Err(error) = clean {
        return Err(error);
    }
    let mut next_fail_nth = vec![1; selectors.len()];
    loop {
        let mut discovered = false;
        let mut selector_index = 0;
        while selector_index < selectors.len() {
            let mut fail_nth = next_fail_nth[selector_index];
            while fail_nth <= max_counts[selector_index] {
                let (result, injected, counts) =
                    jet_fault_run_once(selectors, Some(selector_index), fail_nth, &mut body)?;
                if !injected {
                    if let Err(error) = result {
                        return Err(error);
                    }
                } else {
                    for (index, count) in counts.iter().enumerate() {
                        if *count > max_counts[index] {
                            max_counts[index] = *count;
                            discovered = true;
                        }
                    }
                }
                fail_nth += 1;
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
