// Raw native range callbacks. Scheduling and failure selection are the same
// vetted parallel chunk kernel used by generated Rust; typed storage and
// collection policy stay in the Jet backend's callback body.
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi_parallel {
    use super::jet_c_abi::guard;

    // Seed/verification path only; Jet Parallel owns the native-pack symbol.
    #[cfg(not(jet_native_pack))]
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

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_para_fold(
        len: i64,
        env: *mut u8,
        chunk: usize,
        merge: usize,
    ) -> *mut u8 {
        guard(|| {
            // SAFETY: checked helpers lend env through the joined scheduler;
            // each chunk returns owned accumulator storage. The merge helper
            // consumes both inputs and returns fresh owned storage.
            let chunk: unsafe extern "C" fn(*mut u8, i64, i64) -> *mut u8 =
                unsafe { std::mem::transmute(chunk) };
            let merge: unsafe extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8 =
                unsafe { std::mem::transmute(merge) };
            let env = env as usize;
            let len = usize::try_from(len).expect("checked native List length");
            let partials = super::jet_list_para_chunks(len, usize::MAX, |range| {
                let start = range.start;
                super::jet_para_call(start, || {
                    (start, unsafe {
                        chunk(env as *mut u8, start as i64, range.end as i64)
                    } as usize)
                })
            });
            if partials.is_empty() {
                return unsafe { chunk(env as *mut u8, 0, 0) };
            }
            super::jet_list_para_merge_tree(partials, |(index, left), (_, right)| {
                super::jet_para_call(index, || {
                    (index, unsafe {
                        merge(env as *mut u8, left as *mut u8, right as *mut u8)
                    } as usize)
                })
            })
            .unwrap_or_else(|failure| super::jet_para_raise_failure(failure))
            .1 as *mut u8
        })
    }
}
