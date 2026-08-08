//! D-SOLVER-LIB1: resident-JIT host implementation of the finite Solver.
//! Handles index runtime-owned state; checked TIR fixes every operation/type.

use super::Concurrency;

#[derive(Default)]
pub(crate) struct SolverState {
    pub(crate) checked: i64,
    pub(crate) failures: i64,
}

extern "C" fn jet_jit_solver_new(_seed: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.solvers.push(SolverState::default());
        rt.solvers.len() as i64
    })
}

extern "C" fn jet_jit_solver_require(handle: i64, ok: i8) {
    Concurrency::with_runtime_mut(|rt| {
        let solver = rt
            .solvers
            .get_mut(handle.saturating_sub(1) as usize)
            .expect("jit solver require: bad handle");
        solver.checked += 1;
        if ok == 0 {
            solver.failures += 1;
        }
    });
}

extern "C" fn jet_jit_solver_failure_count(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.solvers
            .get(handle.saturating_sub(1) as usize)
            .expect("jit solver failure_count: bad handle")
            .failures
    })
}

extern "C" fn jet_jit_solver_status(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let status = if rt
            .solvers
            .get(handle.saturating_sub(1) as usize)
            .expect("jit solver status: bad handle")
            .failures
            == 0
        {
            "ok"
        } else {
            "failed"
        };
        rt.heap.alloc_string(status.to_string())
    })
}

host_fns! {
    struct SolverHostFns;
    register: register_solver_symbols;
    declare: declare_solver_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut require = Signature::new(cc);
        require.params.push(AbiParam::new(types::I64));
        require.params.push(AbiParam::new(types::I8));


    }
    new: "jet_jit_solver_new" => jet_jit_solver_new: unary;
    require: "jet_jit_solver_require" => jet_jit_solver_require: require;
    failure_count: "jet_jit_solver_failure_count" => jet_jit_solver_failure_count: unary;
    status: "jet_jit_solver_status" => jet_jit_solver_status: unary;
}





