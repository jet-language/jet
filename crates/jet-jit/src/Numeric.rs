//! D-BIGINT1: `BigInt` host shims for the Cranelift JIT. `BigInt` values are
//! opaque i64 handles into the shared `JetArena` heap (`rt.heap`), exactly
//! like `String`/list handles (`Collections.rs`) — the JIT never inlines the
//! limb representation into generated CLIF, it always calls back into Rust.

use super::Concurrency;

/// Record an invalid `BigInt(...)` literal as a trap (mirrors AOT's
/// `JetBigInt::from_str(...).expect(...)` panic, but as a JIT-safe trap
/// instead of a Rust panic unwinding through a JIT frame — I1).
fn trap_bigint(msg: &str) {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap(msg);
    });
}

extern "C" fn jet_jit_bigint_from_int(n: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_bigint_from_int(n))
}

extern "C" fn jet_jit_bigint_from_str(str_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let s = rt.heap.clone_string(str_id).unwrap_or_default();
        match rt.heap.alloc_bigint_from_str(&s) {
            Ok(id) => id,
            Err(_) => {
                trap_bigint("invalid BigInt string");
                0
            }
        }
    })
}

extern "C" fn jet_jit_bigint_add(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .bigint_add(a, b)
            .expect("jit bigint add: bad handle")
    })
}

extern "C" fn jet_jit_bigint_sub(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .bigint_sub(a, b)
            .expect("jit bigint sub: bad handle")
    })
}

extern "C" fn jet_jit_bigint_mul(a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .bigint_mul(a, b)
            .expect("jit bigint mul: bad handle")
    })
}

extern "C" fn jet_jit_bigint_eq(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .bigint_eq(a, b)
            .expect("jit bigint eq: bad handle") as i8
    })
}

extern "C" fn jet_jit_bigint_neg(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.bigint_neg(a).expect("jit bigint neg: bad handle"))
}

extern "C" fn jet_jit_bigint_to_string(a: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt
            .heap
            .bigint_to_string(a)
            .expect("jit bigint to_string: bad handle");
        rt.heap.alloc_string(text)
    })
}

pub(crate) struct NumericHostFns {
    pub bigint_from_int: cranelift_module::FuncId,
    pub bigint_from_str: cranelift_module::FuncId,
    pub bigint_add: cranelift_module::FuncId,
    pub bigint_sub: cranelift_module::FuncId,
    pub bigint_mul: cranelift_module::FuncId,
    pub bigint_eq: cranelift_module::FuncId,
    pub bigint_neg: cranelift_module::FuncId,
    pub bigint_to_string: cranelift_module::FuncId,
}

pub(crate) fn register_numeric_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_bigint_from_int", jet_jit_bigint_from_int as *const u8);
    builder.symbol("jet_jit_bigint_from_str", jet_jit_bigint_from_str as *const u8);
    builder.symbol("jet_jit_bigint_add", jet_jit_bigint_add as *const u8);
    builder.symbol("jet_jit_bigint_sub", jet_jit_bigint_sub as *const u8);
    builder.symbol("jet_jit_bigint_mul", jet_jit_bigint_mul as *const u8);
    builder.symbol("jet_jit_bigint_eq", jet_jit_bigint_eq as *const u8);
    builder.symbol("jet_jit_bigint_neg", jet_jit_bigint_neg as *const u8);
    builder.symbol(
        "jet_jit_bigint_to_string",
        jet_jit_bigint_to_string as *const u8,
    );
}

pub(crate) fn declare_numeric_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<NumericHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut sig_unary = Signature::new(cc);
    sig_unary.params.push(AbiParam::new(types::I64));
    sig_unary.returns.push(AbiParam::new(types::I64));
    let mut sig_binary = Signature::new(cc);
    sig_binary.params.push(AbiParam::new(types::I64));
    sig_binary.params.push(AbiParam::new(types::I64));
    sig_binary.returns.push(AbiParam::new(types::I64));
    let mut sig_compare = Signature::new(cc);
    sig_compare.params.push(AbiParam::new(types::I64));
    sig_compare.params.push(AbiParam::new(types::I64));
    sig_compare.returns.push(AbiParam::new(types::I8));

    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

    Ok(NumericHostFns {
        bigint_from_int: import("jet_jit_bigint_from_int", &sig_unary)?,
        bigint_from_str: import("jet_jit_bigint_from_str", &sig_unary)?,
        bigint_add: import("jet_jit_bigint_add", &sig_binary)?,
        bigint_sub: import("jet_jit_bigint_sub", &sig_binary)?,
        bigint_mul: import("jet_jit_bigint_mul", &sig_binary)?,
        bigint_eq: import("jet_jit_bigint_eq", &sig_compare)?,
        bigint_neg: import("jet_jit_bigint_neg", &sig_unary)?,
        bigint_to_string: import("jet_jit_bigint_to_string", &sig_unary)?,
    })
}
