// Raw native range callbacks. Scheduling and failure selection are the same
// vetted parallel chunk kernel used by generated Rust; typed storage and
// collection policy stay in the Jet backend's callback body.
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi_parallel {
    use super::jet_c_abi::guard;

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_para_walk(
        len: i64,
        limit: i64,
        env: *mut u8,
        invoke: usize,
    ) {
        guard(|| {
            // SAFETY: the checked native caller supplies this exact callback
            // ABI and lends env until all scoped workers have joined. Each
            // range has exclusive output slots; inputs/captures are Sync.
            let callback: unsafe extern "C" fn(*mut u8, i64, i64) =
                unsafe { std::mem::transmute(invoke) };
            let env = env as usize;
            let len = usize::try_from(len).expect("checked native List length");
            // Exact-Int carrier marshalling matches jet_list_para_map's input.
            let limit = crate::jet_std::jet_int_to_i64(limit)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(usize::MAX)
                .max(1);
            super::jet_list_para_chunks(len, limit, |range| {
                super::jet_para_call(range.start, || unsafe {
                    callback(env as *mut u8, range.start as i64, range.end as i64);
                })
            });
        });
    }
}
