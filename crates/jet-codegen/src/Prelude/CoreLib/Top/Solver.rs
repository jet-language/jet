// D-SOLVER-LIB1=A: one finite-solver kernel for every execution tier.

pub(crate) fn jet_solver_new(seed: i64) -> jet_std::Solver {
    jet_std::Solver {
        seed,
        checked: 0,
        failures: 0,
    }
}

pub(crate) fn jet_solver_require(solver: &mut jet_std::Solver, ok: bool) {
    solver.checked += 1;
    if !ok {
        solver.failures += 1;
    }
}

pub(crate) fn jet_solver_failure_count(solver: &jet_std::Solver) -> i64 {
    solver.failures
}

pub(crate) fn jet_solver_status(solver: &jet_std::Solver) -> String {
    if solver.failures == 0 {
        "ok".to_string()
    } else {
        "failed".to_string()
    }
}
