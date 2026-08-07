//! Heap-handle marshalling for the JIT host shims.
//!
//! I9: a Cranelift host is an adapter, not a second implementation. It converts
//! arguments and results and calls the Prelude. These six functions are that
//! whole conversion vocabulary — every `core.*` host module shares them instead
//! of re-declaring its own copy.

use super::Concurrency;
use crate::runtime_host::alloc_jit_result;

/// Read a heap string handle. An unknown handle reads as the empty string.
pub(crate) fn clone_string(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

/// Store a string on the heap and return its handle.
pub(crate) fn alloc_string(s: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
}

/// Read a `[Int]` byte list handle as bytes.
pub(crate) fn clone_bytes(list: i64) -> Vec<u8> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(list, i).unwrap_or(0) as u8);
        }
        out
    })
}

/// Store bytes as a `[Int]` list and return its handle.
pub(crate) fn alloc_byte_list(bytes: &[u8]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for &b in bytes {
            let _ = rt.heap.list_push_int(list, b as i64);
        }
        list
    })
}

/// Carry an `Ok` payload out of a host call.
pub(crate) fn result_ok(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, true, bits))
}

/// Carry an `Err` message out of a host call.
pub(crate) fn result_err_msg(msg: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg.to_string());
        alloc_jit_result(rt, false, sid as u64)
    })
}
