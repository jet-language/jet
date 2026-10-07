// Native slot marshaling for the existing lazy iterator kernels. This module
// owns only representation/ownership conversion; argument checks, laziness,
// short-circuiting and failure policy remain in Collections.rs/LoopCursor.rs.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod jet_c_abi_iter {
    use super::*;
    use super::jet_c_abi::{guard, JetCString, view};
    use std::cmp::Ordering;
    use std::ptr;
    use std::sync::Arc;

    // SAFETY boundary: descriptors and slots originate in checked native
    // lowering. Descriptors are copied recursively during the call; callbacks
    // belong to the loaded code image, which must outlive all returned owners.
    #[repr(C)]
    pub struct NativeMeta {
        stride: usize,
        clone: usize,
        drop: usize,
        show: usize,
        equal: usize,
        compare: usize,
        kind: u64,
        child0: *const NativeMeta,
        child1: *const NativeMeta,
    }
    pub(crate) struct Meta {
        stride: usize,
        clone: usize,
        drop: usize,
        show: usize,
        equal: usize,
        compare: usize,
        kind: u64,
        child0: Option<Arc<Meta>>,
        child1: Option<Arc<Meta>>,
    }
    impl Meta {
        pub(crate) unsafe fn copy(raw: *const NativeMeta) -> Arc<Self> {
            let row = &*raw;
            Arc::new(Self {
                stride: row.stride, clone: row.clone, drop: row.drop,
                show: row.show, equal: row.equal, compare: row.compare,
                kind: row.kind,
                child0: (!row.child0.is_null()).then(|| Self::copy(row.child0)),
                child1: (!row.child1.is_null()).then(|| Self::copy(row.child1)),
            })
        }
        pub(crate) fn tag(&self) -> u32 { self.kind as u32 }
        pub(crate) fn payload_size(&self) -> usize { match self.tag() { 11 => 0, 9 => 1, _ => self.stride } }
        pub(crate) fn first(&self) -> Arc<Meta> { self.child0.as_ref().expect("native child descriptor").clone() }
        pub(crate) fn second(&self) -> Arc<Meta> { self.child1.as_ref().expect("native error descriptor").clone() }
    }

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
            if self.owned && self.meta.drop != 0 {
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
}
