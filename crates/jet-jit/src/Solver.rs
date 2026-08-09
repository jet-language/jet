//! D-SOLVER-LIB1: resident-JIT adapter for the shared finite-solver kernel.

use super::Concurrency;

mod solver_kernel {
    pub(crate) mod jet_std {
        pub(crate) struct Solver {
            pub(crate) seed: i64,
            pub(crate) checked: i64,
            pub(crate) failures: i64,
        }
    }

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Solver.rs");
}

pub(crate) use solver_kernel::jet_std::Solver as SolverState;

extern "C" fn jet_jit_solver_new(seed: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.solvers.push(solver_kernel::jet_solver_new(seed));
        rt.solvers.len() as i64
    })
}

extern "C" fn jet_jit_solver_require(handle: i64, ok: i8) {
    Concurrency::with_runtime_mut(|rt| {
        let solver = rt
            .solvers
            .get_mut(handle.saturating_sub(1) as usize)
            .expect("jit solver require: bad handle");
        solver_kernel::jet_solver_require(solver, ok != 0);
    });
}

extern "C" fn jet_jit_solver_failure_count(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let solver = rt
            .solvers
            .get(handle.saturating_sub(1) as usize)
            .expect("jit solver failure_count: bad handle");
        solver_kernel::jet_solver_failure_count(solver)
    })
}

extern "C" fn jet_jit_solver_status(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let solver = rt
            .solvers
            .get(handle.saturating_sub(1) as usize)
            .expect("jit solver status: bad handle");
        let status = solver_kernel::jet_solver_status(solver);
        rt.heap.alloc_string(status)
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



