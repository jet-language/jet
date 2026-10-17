// Native slot marshaling for the existing lazy iterator kernels. This module
// owns only representation/ownership conversion; argument checks, laziness,
// short-circuiting and failure policy remain in Collections.rs/LoopCursor.rs.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod jet_c_abi_iter {
    use super::*;
    use super::jet_c_abi::{guard, JetCString, view};
    use super::jet_native_meta::{Meta, NativeMeta};
    use std::cmp::Ordering;
    use std::ptr;
    use std::sync::Arc;


    // Lists and most inline records fit without a heap allocation per element.
    enum Slot { Inline([std::mem::MaybeUninit<u64>; 3]), Heap(Box<[std::mem::MaybeUninit<u64>]>) }
    impl Slot {
        fn zeroed(stride: usize) -> Self {
            if stride <= 24 { Self::Inline([std::mem::MaybeUninit::new(0); 3]) }
            else { Self::Heap(vec![std::mem::MaybeUninit::new(0); stride / 8].into_boxed_slice()) }
        }
        fn uninitialized(stride: usize) -> Self {
            if stride <= 24 { Self::Inline([std::mem::MaybeUninit::uninit(); 3]) }
            else { Self::Heap(Box::<[u64]>::new_uninit_slice(stride / 8)) }
        }
        fn data(&self) -> *const u8 {
            match self { Self::Inline(x) => x.as_ptr().cast(), Self::Heap(x) => x.as_ptr().cast() }
        }
        fn data_mut(&mut self) -> *mut u8 {
            match self { Self::Inline(x) => x.as_mut_ptr().cast(), Self::Heap(x) => x.as_mut_ptr().cast() }
        }
    }
    pub struct NativeValue { meta: Arc<Meta>, slot: Slot, owned: bool }
    impl NativeValue {
        pub(crate) fn zeroed(meta: Arc<Meta>) -> Self { Self { slot: Slot::zeroed(meta.stride), meta, owned: true } }
        pub(crate) fn uninitialized(meta: Arc<Meta>) -> Self { Self { slot: Slot::uninitialized(meta.stride), meta, owned: false } }
        /// Invoke has returned and its masked delivered-only poll was zero.
        pub(crate) fn initialized(mut self) -> Self { self.owned = true; self }
        pub(crate) unsafe fn moved(source: *const u8, meta: Arc<Meta>) -> Self {
            let mut value = Self::zeroed(meta);
            ptr::copy_nonoverlapping(source, value.slot.data_mut(), value.meta.payload_size());
            value
        }
        unsafe fn cloned(source: *const u8, meta: Arc<Meta>) -> Self {
            let mut value = Self::uninitialized(meta);
            assert_ne!(value.meta.clone, 0, "native clone not reachable for this checked element");
            let clone: unsafe extern "C" fn(*const u8, *mut u8) = std::mem::transmute(value.meta.clone);
            clone(source, value.slot.data_mut());
            if jet_scheduler_native_propagate() != 0 { jet_scheduler_resume_native_stop(); }
            value.initialized()
        }
        pub(crate) fn word(&self) -> u64 {
            match self.meta.tag() {
                11 => 0,
                2 | 3 => {
                    let bits = self.meta.kind >> 32;
                    let word = match bits {
                        1..=8 => unsafe { ptr::read_unaligned(self.slot.data()) as u64 },
                        9..=16 => unsafe { ptr::read_unaligned(self.slot.data().cast::<u16>()) as u64 },
                        17..=32 => unsafe { ptr::read_unaligned(self.slot.data().cast::<u32>()) as u64 },
                        _ => unsafe { ptr::read_unaligned(self.slot.data().cast::<u64>()) },
                    };
                    if self.meta.tag() == 2 && bits < 64 {
                        ((word << (64 - bits)) as i64 >> (64 - bits)) as u64
                    } else {
                        word
                    }
                }
                9 => unsafe { ptr::read_unaligned(self.slot.data()) as u64 },
                _ => unsafe { ptr::read_unaligned(self.slot.data().cast()) },
            }
        }
        /// The owned slot a native callee writes this value's bytes into.
        pub(crate) fn slot_mut(&mut self) -> *mut u8 { self.slot.data_mut() }
        /// The borrowed slot of this value's bytes.
        pub(crate) fn slot(&self) -> *const u8 { self.slot.data() }
        fn scalar(meta: Arc<Meta>, word: u64) -> Self {
            let mut value = Self::zeroed(meta);
            unsafe { ptr::write_unaligned(value.slot.data_mut().cast(), word) };
            value
        }
        pub(crate) fn put(mut self, target: *mut u8) {
            unsafe { ptr::copy_nonoverlapping(self.slot.data(), target, self.meta.payload_size()) };
            // Move the slot ownership, but still release its container and meta.
            self.owned = false;
        }
        // A native Result box holds 1 for Ok and 0 for Err first (Lower.jet
        // lower_result_box); the payload follows at 8.
        pub(crate) fn into_result(mut self) -> Result<Self, Failure> {
            let kind = self.meta.tag();
            let raw = self.word() as *mut u8;
            self.owned = false;
            if kind == 7 {
                if raw.is_null() { return Err(Failure::Absent); }
                let meta = self.meta.first();
                let size = meta.stride.max(8);
                let value = unsafe { Self::moved(raw, meta) };
                unsafe { free(raw, size) };
                Ok(value)
            } else {
                assert_eq!(kind, 8, "native failure carrier descriptor");
                let ok = self.meta.first(); let err = self.meta.second();
                let size = result_box_size(&ok, &err);
                let tag = unsafe { ptr::read_unaligned(raw.cast::<u64>()) };
                let meta = if tag != 0 { ok } else { err };
                let value = unsafe { Self::moved(raw.add(8), meta) };
                unsafe { free(raw, size) };
                if tag != 0 { Ok(value) } else { Err(Failure::Error(value)) }
            }
        }
    }
    impl Clone for NativeValue {
        fn clone(&self) -> Self { unsafe { Self::cloned(self.slot.data(), self.meta.clone()) } }
    }
    impl Drop for NativeValue {
        fn drop(&mut self) {
            if !self.owned { return; }
            // The consuming callback owns cleanup, including a typed Stop.
            // Never replay it while releasing the surrounding Rust container.
            self.owned = false;
            if self.meta.drop != 0 {
                let drop: unsafe extern "C" fn(*mut u8) = unsafe { std::mem::transmute(self.meta.drop) };
                unsafe { drop(self.slot.data_mut()) };
            }
        }
    }
    impl PartialEq for NativeValue {
        fn eq(&self, other: &Self) -> bool {
            assert_ne!(self.meta.equal, 0, "native equality callback");
            let equal: unsafe extern "C" fn(*const u8, *const u8) -> u8 = unsafe { std::mem::transmute(self.meta.equal) };
            let result = unsafe { equal(self.slot.data(), other.slot.data()) };
            if jet_scheduler_native_propagate() != 0 { jet_scheduler_resume_native_stop(); }
            result != 0
        }
    }
    impl Eq for NativeValue {}
    impl PartialOrd for NativeValue {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
    }
    impl Ord for NativeValue {
        fn cmp(&self, other: &Self) -> Ordering {
            assert_ne!(self.meta.compare, 0, "native ordering callback");
            let compare: unsafe extern "C" fn(*const u8, *const u8) -> i64 = unsafe { std::mem::transmute(self.meta.compare) };
            let result = unsafe { compare(self.slot.data(), other.slot.data()) };
            if jet_scheduler_native_propagate() != 0 { jet_scheduler_resume_native_stop(); }
            result.cmp(&0)
        }
    }
    impl JetShow for NativeValue {
        fn jet_show(&self) -> String {
            assert_ne!(self.meta.show, 0, "native Show callback");
            let show: unsafe extern "C" fn(*const u8) -> JetCString = unsafe { std::mem::transmute(self.meta.show) };
            let result = unsafe { show(self.slot.data()) };
            if jet_scheduler_native_propagate() != 0 { jet_scheduler_resume_native_stop(); }
            super::jet_c_abi::take(result)
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(crate) struct Header { pub(crate) len: usize, pub(crate) cap: usize, pub(crate) data: *mut u8 }
    pub(crate) unsafe fn allocate(size: usize) -> *mut u8 {
        if size == 0 { return ptr::null_mut(); }
        #[cfg(jet_native_pack)]
        { super::jet_c_abi::jet_rt_alloc(size, 8) }
        #[cfg(not(jet_native_pack))]
        {
            let layout = std::alloc::Layout::from_size_align_unchecked(size, 8);
            let raw = std::alloc::alloc(layout);
            if raw.is_null() { std::alloc::handle_alloc_error(layout); }
            raw
        }
    }
    pub(crate) unsafe fn free(raw: *mut u8, size: usize) {
        if raw.is_null() { return; }
        #[cfg(jet_native_pack)]
        { super::jet_c_abi::jet_rt_free(raw, size.max(1), 8); }
        #[cfg(not(jet_native_pack))]
        { std::alloc::dealloc(raw, std::alloc::Layout::from_size_align_unchecked(size.max(1), 8)); }
    }
    // Values.jet lower_result_box_size reserves at least one payload word,
    // including when both checked payloads are zero-sized.
    pub(crate) fn result_box_size(ok: &Meta, err: &Meta) -> usize {
        8 + ((ok.payload_size().max(err.payload_size()).max(8) + 7) & !7)
    }
    pub(crate) fn list_into(values: Vec<NativeValue>, target: *mut Header) {
        let len = values.len();
        let stride = values.first().map_or(0, |value| value.meta.stride);
        let raw = unsafe { allocate(len * stride) };
        for (at, value) in values.into_iter().enumerate() { value.put(unsafe { raw.add(at * stride) }); }
        unsafe { ptr::write_unaligned(target, Header { len, cap: len, data: raw }) };
    }
    pub(crate) fn list_value(values: Vec<NativeValue>, meta: Arc<Meta>) -> NativeValue {
        let mut value = NativeValue::zeroed(meta);
        list_into(values, value.slot.data_mut().cast()); value
    }
    pub(crate) enum Failure { Absent, Error(NativeValue) }
    pub(crate) fn option(value: JetOutcome<NativeValue, JetAbsent>) -> *mut u8 {
        match value {
            Ok(value) => { let raw = unsafe { allocate(value.meta.stride.max(8)) }; value.put(raw); raw }
            Err(_) => ptr::null_mut(),
        }
    }
    // Remaining host policy, not an iterator implementation. Jet's node owns
    // this opaque state; only these leaves dereference or free it. This seam
    // dies when the L7/S6 human-output policy moves into the Jet runtime.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_progress_state_new(description: JetCString, format: JetCString, total: i64) -> *mut JetOutputProgressIterState {
        guard(|| Box::into_raw(Box::new(JetOutputProgressIterState::new(
            view(description), view(format), (total >= 0).then_some(total as usize),
        ))))
    }
    // CollectionIoSources supplies the owned lazy source or the borrowed
    // snapshot. Only the String slot representation changes at this boundary.
    fn line_source(source: Box<dyn Iterator<Item = JetLoopAny>>, meta: Arc<Meta>) -> *mut NativeIter {
        handle(JetIter(Box::new(source.map(move |value| {
            let text = value.downcast::<String>().expect("checked line source yields String");
            let raw = Box::into_raw(text);
            unsafe { NativeValue::moved((&raw as *const *mut String).cast(), meta.clone()) }
        }))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_file_lines(source: *mut JetFileReader, by_value: u8, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let source = if by_value != 0 {
                (*Box::from_raw(source)).jet_loop_source(JetLoopSourceKind::LinesFile, true)
            } else {
                (&mut *source).jet_loop_source(JetLoopSourceKind::LinesFile, false)
            };
            line_source(source, Meta::copy(meta))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_stdin_lines(source: *mut JetStdinReader, by_value: u8, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let source = if by_value != 0 {
                (*Box::from_raw(source)).jet_loop_source(JetLoopSourceKind::LinesStdin, true)
            } else {
                (&mut *source).jet_loop_source(JetLoopSourceKind::LinesStdin, false)
            };
            line_source(source, Meta::copy(meta))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_progress_state_advance(state: *mut JetOutputProgressIterState) {
        guard(|| {
            if let Err(error) = (&mut *state).advance(jet_style_enabled(), jet_std_io_progress_emit, jet_std_io_progress_finish) {
                jet_panic("<progress>", 0, &format!("{error:?}"));
            }
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_progress_state_finish(state: *mut JetOutputProgressIterState) {
        guard(|| {
            if let Err(error) = (&mut *state).finish(jet_std_io_progress_finish) {
                jet_panic("<progress>", 0, &format!("{error:?}"));
            }
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_progress_state_drop(state: *mut JetOutputProgressIterState) {
        guard(|| {
            let mut state = Box::from_raw(state);
            if let Err(error) = state.finish(jet_std_io_progress_finish) {
                jet_panic("<progress>", 0, &format!("{error:?}"));
            }
        })
    }
    // The comptime budget remains part of the session/task layer. Charge each
    // yielded node value exactly where handle(...inspect(...)) charged it.
    #[no_mangle]
    pub extern "C" fn jet_rt_iter_charge() -> i64 {
        if jet_native_comptime_active() && super::jet_c_abi::comptime_charge(1) && !jet_scheduler_shielded() { 1 } else { 0 }
    }

    type NativeQueue = std::collections::VecDeque<NativeValue>;
    #[no_mangle]
    pub extern "C" fn jet_rt_native_queue_new(capacity: i64) -> *mut NativeQueue {
        guard(|| Box::into_raw(Box::new(NativeQueue::with_capacity(
            usize::try_from(capacity).unwrap_or_else(|_| jet_panic("<core.prelude>", 0, "invalid queue capacity")),
        ))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_queue_from(source: *mut Header, meta: *const NativeMeta, by_value: u8) -> *mut NativeQueue {
        guard(|| Box::into_raw(Box::new(jet_deque_from(list_from(source, Meta::copy(meta), by_value).collect()))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_queue_drop(queue: *mut NativeQueue) {
        guard(|| { if !queue.is_null() { drop(Box::from_raw(queue)); } })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_queue_clone(queue: *const NativeQueue) -> *mut NativeQueue {
        guard(|| Box::into_raw(Box::new((*queue).clone())))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_deque_len(queue: *const NativeQueue) -> i64 {
        guard(|| jet_deque_len(&*queue))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_deque_capacity(queue: *const NativeQueue) -> i64 {
        guard(|| jet_deque_capacity(&*queue))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_deque_is_empty(queue: *const NativeQueue) -> u8 {
        guard(|| u8::from(jet_deque_is_empty(&*queue)))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_deque_reverse(queue: *mut NativeQueue) {
        guard(|| jet_deque_reverse(&mut *queue))
    }
    macro_rules! queue_optional {
        ($($export:ident => $kernel:ident),* $(,)?) => {$(
            #[no_mangle]
            pub unsafe extern "C" fn $export(queue: *mut NativeQueue) -> *mut u8 {
                guard(|| option($kernel(&mut *queue)))
            }
        )*};
    }
    queue_optional! {
        jet_rt_native_deque_pop_front_kernel => jet_deque_pop_front_kernel,
        jet_rt_native_deque_pop_back_kernel => jet_deque_pop_back_kernel,
        jet_rt_native_deque_peek_front => jet_deque_peek_front,
        jet_rt_native_deque_peek_back => jet_deque_peek_back,
    }
    macro_rules! queue_element {
        ($($export:ident => $kernel:ident),* $(,)?) => {$(
            #[no_mangle]
            pub unsafe extern "C" fn $export(queue: *mut NativeQueue, source: *const u8, meta: *const NativeMeta, by_value: u8) {
                guard(|| {
                    let meta = Meta::copy(meta);
                    let value = if by_value != 0 { NativeValue::moved(source, meta) } else { NativeValue::cloned(source, meta) };
                    $kernel(&mut *queue, value)
                })
            }
        )*};
    }
    queue_element! {
        jet_rt_native_deque_push_front => jet_deque_push_front,
        jet_rt_native_deque_push_back => jet_deque_push_back,
        jet_rt_native_deque_delete => jet_deque_delete,
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_deque_contains(queue: *const NativeQueue, source: *const u8, meta: *const NativeMeta, _by_value: u8) -> u8 {
        guard(|| {
            let mut value = NativeValue::moved(source, Meta::copy(meta));
            value.owned = false; // a borrowed needle never consumes the caller's slot
            u8::from(jet_deque_contains(&*queue, &value))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_deque_get(queue: *const NativeQueue, index: i64) -> *mut u8 {
        guard(|| option(jet_deque_get(&*queue, index)))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_deque_split(queue: *mut NativeQueue, index: i64) -> *mut NativeQueue {
        guard(|| {
            let index = std::mem::ManuallyDrop::new(jet_foundation::Numeric::JetInt::from_raw_owned(index));
            Box::into_raw(Box::new(jet_deque_split_native(&mut *queue, jet_int_saturating_word(&index))))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_deque_to_list(queue: *const NativeQueue, out: *mut Header) {
        guard(|| list_into(jet_deque_to_list(&*queue), out))
    }
    /// Collections.rs jet_deque_join's rendering (each value shown, joined by
    /// the separator), with the element's native show glue for JetShow.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_deque_join(queue: *const NativeQueue, separator: JetCString, meta: *const NativeMeta) -> JetCString {
        guard(|| {
            let meta = Meta::copy(meta);
            let show: unsafe extern "C" fn(*const u8) -> JetCString = std::mem::transmute(meta.show);
            let parts = (*queue).iter().map(|value| {
                let result = show(value.slot());
                if jet_scheduler_native_propagate() != 0 { jet_scheduler_resume_native_stop(); }
                super::jet_c_abi::take(result)
            }).collect::<Vec<String>>();
            super::jet_c_abi::handle(parts.join(view(separator)))
        })
    }

}
