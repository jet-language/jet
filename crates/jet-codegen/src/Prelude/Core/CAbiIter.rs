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
        pub(crate) unsafe fn moved(source: *const u8, meta: Arc<Meta>) -> Self {
            let mut value = Self::zeroed(meta);
            ptr::copy_nonoverlapping(source, value.slot.data_mut(), value.meta.payload_size());
            value
        }
        unsafe fn cloned(source: *const u8, meta: Arc<Meta>) -> Self {
            let mut value = Self::zeroed(meta);
            assert_ne!(value.meta.clone, 0, "native clone not reachable for this checked element");
            let clone: unsafe extern "C" fn(*const u8, *mut u8) = std::mem::transmute(value.meta.clone);
            clone(source, value.slot.data_mut());
            value
        }
        pub(crate) fn word(&self) -> u64 { unsafe { ptr::read_unaligned(self.slot.data().cast()) } }
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
        fn into_word(mut self) -> u64 {
            let word = self.word();
            self.owned = false;
            word
        }
        fn into_list(mut self) -> NativeList {
            assert_eq!(self.meta.tag(), 6, "native list descriptor");
            let child = self.meta.first();
            let header = unsafe { ptr::read_unaligned(self.slot.data().cast::<Header>()) };
            self.owned = false;
            NativeList { header, at: 0, meta: child }
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
            unsafe { equal(self.slot.data(), other.slot.data()) != 0 }
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
            unsafe { compare(self.slot.data(), other.slot.data()).cmp(&0) }
        }
    }
    impl JetShow for NativeValue {
        fn jet_show(&self) -> String {
            assert_ne!(self.meta.show, 0, "native Show callback");
            let show: unsafe extern "C" fn(*const u8) -> JetCString = unsafe { std::mem::transmute(self.meta.show) };
            *unsafe { Box::from_raw(show(self.slot.data())) }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(crate) struct Header { pub(crate) len: usize, pub(crate) cap: usize, pub(crate) data: *mut u8 }
    pub(crate) unsafe fn allocate(size: usize) -> *mut u8 {
        if size == 0 { return ptr::null_mut(); }
        let layout = std::alloc::Layout::from_size_align_unchecked(size, 8);
        let raw = std::alloc::alloc(layout);
        if raw.is_null() { std::alloc::handle_alloc_error(layout); }
        raw
    }
    pub(crate) unsafe fn free(raw: *mut u8, size: usize) {
        if !raw.is_null() { std::alloc::dealloc(raw, std::alloc::Layout::from_size_align_unchecked(size.max(1), 8)); }
    }
    // Values.jet lower_result_box_size reserves at least one payload word,
    // including when both checked payloads are zero-sized.
    pub(crate) fn result_box_size(ok: &Meta, err: &Meta) -> usize {
        8 + ((ok.payload_size().max(err.payload_size()).max(8) + 7) & !7)
    }
    struct NativeList { header: Header, at: usize, meta: Arc<Meta> }
    impl Iterator for NativeList {
        type Item = NativeValue;
        fn next(&mut self) -> Option<Self::Item> {
            if self.at == self.header.len { return None; }
            let source = unsafe { self.header.data.add(self.at * self.meta.stride) };
            self.at += 1;
            Some(unsafe { NativeValue::moved(source, self.meta.clone()) })
        }
        fn size_hint(&self) -> (usize, Option<usize>) {
            let left = self.header.len - self.at; (left, Some(left))
        }
    }
    impl Drop for NativeList {
        fn drop(&mut self) {
            if self.meta.drop != 0 {
                let drop: unsafe extern "C" fn(*mut u8) = unsafe { std::mem::transmute(self.meta.drop) };
                for at in self.at..self.header.len { unsafe { drop(self.header.data.add(at * self.meta.stride)) }; }
            }
            unsafe { free(self.header.data, self.header.cap * self.meta.stride) };
        }
    }
    unsafe fn list_from(raw: *mut Header, meta: Arc<Meta>, by_value: u8) -> NativeList {
        let mut header = ptr::read_unaligned(raw);
        if by_value != 0 {
            ptr::write_unaligned(raw, Header { len: 0, cap: 0, data: ptr::null_mut() });
        } else {
            let copied = allocate(header.len * meta.stride);
            for at in 0..header.len {
                let value = NativeValue::cloned(header.data.add(at * meta.stride), meta.clone());
                value.put(copied.add(at * meta.stride));
            }
            header = Header { len: header.len, cap: header.len, data: copied };
        }
        NativeList { header, at: 0, meta }
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
    type NativeIter = JetIter<NativeValue>;
    fn handle(iter: NativeIter) -> *mut NativeIter {
        let iter = if jet_native_comptime_active() {
            JetIter(Box::new(iter.0.inspect(|_| jet_native_comptime_work(1))))
        } else { iter };
        Box::into_raw(Box::new(iter))
    }
    unsafe fn take(raw: *mut NativeIter) -> NativeIter { *Box::from_raw(raw) }
    fn enrich(iter: NativeIter, meta: Arc<Meta>) -> NativeIter {
        JetIter(Box::new(iter.0.map(move |mut value| { value.meta = meta.clone(); value })))
    }
    fn option(value: JetOutcome<NativeValue, JetAbsent>) -> *mut u8 {
        match value {
            Ok(value) => { let raw = unsafe { allocate(value.meta.stride.max(8)) }; value.put(raw); raw }
            Err(_) => ptr::null_mut(),
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_from_list(source: *mut Header, meta: *const NativeMeta, by_value: u8) -> *mut NativeIter {
        guard(|| handle(JetIter(Box::new(list_from(source, Meta::copy(meta), by_value)))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_drop(iter: *mut NativeIter) {
        guard(|| { if !iter.is_null() { drop(Box::from_raw(iter)); } })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_to_list(iter: *mut NativeIter, target: *mut Header) {
        guard(|| list_into(jet_iter_to_list(take(iter)), target))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_first(iter: *mut NativeIter) -> *mut u8 {
        guard(|| option(jet_iter_first(take(iter))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_next(iter: *mut NativeIter) -> *mut u8 {
        guard(|| option(jet_iter_next(&mut *iter)))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_len(iter: *mut NativeIter) -> i64 {
        guard(|| jet_iter_len(take(iter)))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_is_empty(iter: *mut NativeIter) -> u8 {
        guard(|| u8::from(jet_iter_is_empty(take(iter))))
    }
    macro_rules! counted {
        ($($name:ident => $kernel:ident),* $(,)?) => {$(
            #[no_mangle]
            pub unsafe extern "C" fn $name(iter: *mut NativeIter, count: i64) -> *mut NativeIter {
                guard(|| handle($kernel(take(iter), count)))
            }
        )*};
    }
    counted! {
        jet_rt_native_iter_take => jet_iter_take,
        jet_rt_native_iter_skip => jet_iter_skip,
        jet_rt_native_iter_step_by => jet_iter_step_by,
        jet_rt_native_iter_drop_last => jet_iter_drop_last,
        jet_rt_native_iter_repeat => jet_iter_repeat,
        jet_rt_native_iter_cycle => jet_iter_cycle,
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_shuffle(iter: *mut NativeIter) -> *mut NativeIter {
        guard(|| handle(jet_iter_shuffle(take(iter))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_dedup(iter: *mut NativeIter, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| handle(jet_iter_dedup(enrich(take(iter), Meta::copy(meta)))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_is_sorted(iter: *mut NativeIter, meta: *const NativeMeta) -> u8 {
        guard(|| u8::from(jet_iter_is_sorted(enrich(take(iter), Meta::copy(meta)))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_join(iter: *mut NativeIter, separator: JetCString, meta: *const NativeMeta) -> JetCString {
        guard(|| Box::into_raw(Box::new(jet_iter_join(enrich(take(iter), Meta::copy(meta)), &*separator))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_compare(iter: *mut NativeIter, other: *mut Header, meta: *const NativeMeta, by_value: u8) -> i64 {
        guard(|| {
            let meta = Meta::copy(meta);
            let other = jet_iter_to_list(JetIter(Box::new(list_from(other, meta.clone(), by_value))));
            jet_iter_compare(enrich(take(iter), meta), other)
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_last_index_of(iter: *mut NativeIter, needle: *const u8, meta: *const NativeMeta, by_value: u8) -> *mut u8 {
        guard(|| {
            let meta = Meta::copy(meta);
            let needle = if by_value != 0 { NativeValue::moved(needle, meta.clone()) } else { NativeValue::cloned(needle, meta.clone()) };
            match jet_iter_last_index_of(enrich(take(iter), meta), needle) {
                Ok(index) => {
                    let raw = allocate(8);
                    ptr::write_unaligned(raw.cast::<i64>(), crate::jet_std::jet_int_owned_from_i64(index).into_raw());
                    raw
                }
                Err(_) => ptr::null_mut(),
            }
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_intersperse(iter: *mut NativeIter, separator: *const u8, meta: *const NativeMeta, by_value: u8) -> *mut NativeIter {
        guard(|| {
            let meta = Meta::copy(meta);
            let separator = if by_value != 0 { NativeValue::moved(separator, meta.clone()) } else { NativeValue::cloned(separator, meta.clone()) };
            handle(jet_iter_intersperse(take(iter), separator))
        })
    }
    macro_rules! nested {
        ($($name:ident => $kernel:ident),* $(,)?) => {$(
            #[no_mangle]
            pub unsafe extern "C" fn $name(iter: *mut NativeIter, count: i64, meta: *const NativeMeta) -> *mut NativeIter {
                guard(|| {
                    let meta = Meta::copy(meta);
                    let result = $kernel(take(iter), count);
                    handle(JetIter(Box::new(result.0.map(move |values| list_value(values, meta.clone())))))
                })
            }
        )*};
    }
    nested! { jet_rt_native_iter_chunks => jet_iter_chunks, jet_rt_native_iter_windows => jet_iter_windows }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_flatten(iter: *mut NativeIter, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let meta = Meta::copy(meta);
            let source = JetIter(Box::new(take(iter).0.map(|value| value.into_list().collect::<Vec<_>>())));
            handle(enrich(jet_iter_flatten(source), meta))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_some(iter: *mut NativeIter, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let meta = Meta::copy(meta);
            let source = jet_iter_some(take(iter));
            handle(JetIter(Box::new(source.0.map(move |value| NativeValue::scalar(meta.clone(), option(value) as u64)))))
        })
    }
    macro_rules! split {
        ($($name:ident => $kernel:ident),* $(,)?) => {$(
            #[no_mangle]
            pub unsafe extern "C" fn $name(text: JetCString, separator: JetCString, meta: *const NativeMeta) -> *mut NativeIter {
                guard(|| {
                    let meta = Meta::copy(meta);
                    let source = $kernel(view(text), view(separator));
                    handle(JetIter(Box::new(source.0.map(move |text| NativeValue::scalar(meta.clone(), Box::into_raw(Box::new(text)) as u64)))))
                })
            }
        )*};
    }
    split! { jet_rt_native_iter_string_split => jet_iter_string_split, jet_rt_native_iter_string_rsplit => jet_iter_string_rsplit }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_empty(_meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| handle(jet_iter_empty()))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_indexes(count: i64, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let meta = Meta::copy(meta);
            let source = jet_iter_indexes(count);
            handle(JetIter(Box::new(source.0.map(move |index| NativeValue::scalar(meta.clone(), crate::jet_std::jet_int_owned_from_i64(index).into_raw() as u64)))))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_progress(iter: *mut NativeIter, description: JetCString, format: JetCString) -> *mut NativeIter {
        guard(|| handle(jet_std_io_progress_iter(take(iter), &*description, &*format)))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_split_at(iter: *mut NativeIter, count: i64, target: *mut u8, left: usize, right: usize) {
        guard(|| jet_iter_split_at(take(iter), count, |a, b| {
            list_into(a, target.add(left).cast()); list_into(b, target.add(right).cast());
        }))
    }
    fn signed(value: NativeValue) -> i64 {
        let width = (value.meta.kind >> 32) as u32;
        let word = value.word() as i64;
        if width == 64 { word } else { (word << (64 - width)) >> (64 - width) }
    }
    fn unsigned(value: NativeValue) -> u64 {
        let width = (value.meta.kind >> 32) as u32;
        let word = value.word();
        if width == 64 { word } else { word & (u64::MAX >> (64 - width)) }
    }
    impl JetIntAverage for NativeValue {
        fn jet_sum_as_float(items: Vec<Self>) -> f64 {
            match items.first().map(|value| value.meta.tag()) {
                None | Some(2) => <i64 as JetIntAverage>::jet_sum_as_float(items.into_iter().map(signed).collect()),
                Some(3) => <u64 as JetIntAverage>::jet_sum_as_float(items.into_iter().map(unsigned).collect()),
                Some(1) => <jet_foundation::Numeric::JetInt as JetIntAverage>::jet_sum_as_float(items.into_iter().map(|value| {
                    // SAFETY: transfer the exact-Int slot owner into its shared runtime type.
                    unsafe { jet_foundation::Numeric::JetInt::from_raw_owned(value.into_word() as i64) }
                }).collect()),
                _ => unreachable!("checked integer average element"),
            }
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_average_int(iter: *mut NativeIter) -> f64 {
        guard(|| jet_iter_average_int(take(iter)))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_average_float(iter: *mut NativeIter) -> f64 {
        guard(|| {
            let source = JetIter(Box::new(take(iter).0.map(|value| f64::from_bits(value.word()))));
            jet_iter_average_float(source)
        })
    }
    fn iter_result(result: Result<NativeIter, Failure>, carrier: Arc<Meta>) -> *mut u8 {
        if carrier.tag() == 7 {
            match result {
                Ok(iter) => {
                    let raw = unsafe { allocate(8) };
                    unsafe { ptr::write_unaligned(raw.cast::<*mut NativeIter>(), handle(iter)) };
                    raw
                }
                Err(Failure::Absent) => ptr::null_mut(),
                Err(Failure::Error(_)) => unreachable!("checked Option failure carrier"),
            }
        } else {
            let size = result_box_size(
                carrier.child0.as_deref().expect("native child descriptor"),
                carrier.child1.as_deref().expect("native error descriptor"),
            );
            let raw = unsafe { allocate(size) };
            match result {
                Ok(iter) => unsafe {
                    ptr::write_unaligned(raw.cast::<u64>(), 1);
                    ptr::write_unaligned(raw.add(8).cast::<*mut NativeIter>(), handle(iter));
                },
                Err(Failure::Error(value)) => {
                    unsafe { ptr::write_unaligned(raw.cast::<u64>(), 0) };
                    value.put(unsafe { raw.add(8) });
                }
                Err(Failure::Absent) => unreachable!("checked Result failure carrier"),
            }
            raw
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_try_collect(iter: *mut NativeIter, list_meta: *const NativeMeta, error_meta: *const NativeMeta) -> *mut u8 {
        guard(|| {
            let meta = Meta::copy(list_meta);
            let source = take(iter).0.map(NativeValue::into_result);
            let result = jet_list_try_collect(source);
            if error_meta.is_null() {
                match result {
                    Ok(values) => {
                        let raw = allocate(meta.stride.max(8));
                        list_value(values, meta).put(raw); raw
                    }
                    Err(Failure::Absent) => ptr::null_mut(),
                    Err(Failure::Error(_)) => unreachable!("checked Option collection"),
                }
            } else {
                let error = Meta::copy(error_meta);
                let size = result_box_size(&meta, &error);
                let raw = allocate(size);
                match result {
                    Ok(values) => { ptr::write_unaligned(raw.cast::<u64>(), 1); list_value(values, meta).put(raw.add(8)); }
                    Err(Failure::Error(value)) => { ptr::write_unaligned(raw.cast::<u64>(), 0); value.put(raw.add(8)); }
                    Err(Failure::Absent) => unreachable!("checked Result collection"),
                }
                raw
            }
        })
    }
    type NativeCursor = JetLoopIterCursor<Result<NativeValue, String>>;
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_cursor_init(iter: *mut NativeIter, step: i64, has_step: u8) -> *mut NativeCursor {
        guard(|| {
            let source: Box<dyn Iterator<Item = Result<NativeValue, String>>> = Box::new(take(iter).0.map(Ok));
            let cursor = jet_loop_iter_init_typed(source, step, has_step != 0)
                .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, message));
            Box::into_raw(Box::new(cursor))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_cursor_test(cursor: *mut NativeCursor) -> u8 {
        guard(|| u8::from(jet_loop_iter_typed_has_next(&*cursor)
            .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, &message))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_cursor_value(cursor: *mut NativeCursor, target: *mut u8, by_value: u8) {
        guard(|| {
            if by_value != 0 {
                jet_loop_iter_typed_value(&mut *cursor)
                    .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, &message)).put(target);
            } else {
                let value = jet_loop_iter_typed_value_ref(&*cursor)
                    .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, &message));
                ptr::copy_nonoverlapping(value.slot.data(), target, value.meta.payload_size());
            }
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_cursor_advance(cursor: *mut NativeCursor) {
        guard(|| jet_loop_iter_typed_advance(&mut *cursor)
            .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, &message)))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_cursor_drop(cursor: *mut NativeCursor) {
        guard(|| { if !cursor.is_null() { drop(Box::from_raw(cursor)); } })
    }
    struct Callback { env: *mut u8, invoke: usize, release: usize, output: Arc<Meta> }
    impl Callback {
        unsafe fn new(env: *mut u8, invoke: usize, release: usize, output: *const NativeMeta) -> Self {
            Self { env, invoke, release, output: Meta::copy(output) }
        }
        fn unary(&mut self, value: &NativeValue) -> NativeValue {
            let mut result = NativeValue::zeroed(self.output.clone());
            let invoke: unsafe extern "C" fn(*mut u8, *const u8, *mut u8) -> u64 = unsafe { std::mem::transmute(self.invoke) };
            let moved = unsafe { invoke(self.env, value.slot.data(), result.slot.data_mut()) };
            assert_eq!(moved, 0, "borrowed native callback consumes only its own typed copies");
            result
        }
        fn binary(&mut self, a: &NativeValue, b: &NativeValue) -> NativeValue {
            let mut result = NativeValue::zeroed(self.output.clone());
            let invoke: unsafe extern "C" fn(*mut u8, *const u8, *const u8, *mut u8) -> u64 = unsafe { std::mem::transmute(self.invoke) };
            let moved = unsafe { invoke(self.env, a.slot.data(), b.slot.data(), result.slot.data_mut()) };
            assert_eq!(moved, 0, "borrowed native callback consumes only its own typed copies");
            result
        }
        fn owned(&mut self, mut a: NativeValue, mut b: NativeValue) -> NativeValue {
            let mut result = NativeValue::zeroed(self.output.clone());
            let invoke: unsafe extern "C" fn(*mut u8, *const u8, *const u8, *mut u8) -> u64 = unsafe { std::mem::transmute(self.invoke) };
            let moved = unsafe { invoke(self.env, a.slot.data_mut(), b.slot.data_mut(), result.slot.data_mut()) };
            if moved & 1 != 0 { a.owned = false; }
            if moved & 2 != 0 { b.owned = false; }
            result
        }
    }
    impl Drop for Callback {
        fn drop(&mut self) {
            let release: unsafe extern "C" fn(*mut u8) = unsafe { std::mem::transmute(self.release) };
            unsafe { release(self.env) };
        }
    }
    fn boolean(value: NativeValue) -> bool { unsafe { ptr::read(value.slot.data()) != 0 } }
    macro_rules! mapped {
        ($($name:ident => $kernel:ident),* $(,)?) => {$(
            #[no_mangle]
            pub unsafe extern "C" fn $name(iter: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut NativeIter {
                guard(|| {
                    let mut callback = Callback::new(env, invoke, release, meta);
                    handle($kernel(take(iter), move |value| callback.unary(value)))
                })
            }
        )*};
    }
    mapped! {
        jet_rt_native_iter_map => jet_iter_map,
        jet_rt_native_iter_map_mut => jet_iter_map_mut,
        jet_rt_native_iter_dedup_by => jet_iter_dedup_by,
    }
    macro_rules! predicate {
        ($($name:ident => $kernel:ident),* $(,)?) => {$(
            #[no_mangle]
            pub unsafe extern "C" fn $name(iter: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut NativeIter {
                guard(|| {
                    let mut callback = Callback::new(env, invoke, release, meta);
                    handle($kernel(take(iter), move |value| boolean(callback.unary(value))))
                })
            }
        )*};
    }
    predicate! {
        jet_rt_native_iter_filter => jet_iter_filter,
        jet_rt_native_iter_take_while => jet_iter_take_while,
        jet_rt_native_iter_skip_while => jet_iter_skip_while,
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_flat_map(iter: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let mut callback = Callback::new(env, invoke, release, meta);
            handle(jet_iter_flat_map(take(iter), move |value| callback.unary(value).into_list().collect::<Vec<_>>()))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_filter_map(iter: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let mut callback = Callback::new(env, invoke, release, meta);
            handle(jet_iter_filter_map(take(iter), move |value| callback.unary(value).into_result()))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_try_map(iter: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut u8 {
        guard(|| {
            let mut callback = Callback::new(env, invoke, release, meta);
            let carrier = callback.output.clone();
            iter_result(jet_iter_try_map(take(iter), move |value| callback.unary(value).into_result()), carrier)
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_try_filter(iter: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut u8 {
        guard(|| {
            let mut callback = Callback::new(env, invoke, release, meta);
            let carrier = callback.output.clone();
            iter_result(jet_iter_try_filter(take(iter), move |value| callback.unary(value).into_result().map(boolean)), carrier)
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_is_sorted_by(iter: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> u8 {
        guard(|| {
            let mut callback = Callback::new(env, invoke, release, meta);
            u8::from(jet_iter_is_sorted_by(take(iter), move |value| callback.unary(value)))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_chunk_while(iter: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta, list_meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let mut callback = Callback::new(env, invoke, release, meta);
            let meta = Meta::copy(list_meta);
            let source = jet_iter_chunk_while(take(iter), move |a, b| boolean(callback.binary(a, b)));
            handle(JetIter(Box::new(source.0.map(move |values| list_value(values, meta.clone())))))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_scan(iter: *mut NativeIter, seed: *const u8, seed_meta: *const NativeMeta, seed_by_value: u8, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let seed_meta = Meta::copy(seed_meta);
            let seed = if seed_by_value != 0 { NativeValue::moved(seed, seed_meta) } else { NativeValue::cloned(seed, seed_meta) };
            let mut callback = Callback::new(env, invoke, release, meta);
            handle(jet_iter_scan(take(iter), seed, move |state, value| callback.binary(state, value)))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_zip(a: *mut NativeIter, b: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let mut callback = Callback::new(env, invoke, release, meta);
            handle(jet_iter_zip(take(a), take(b), move |a, b| callback.owned(a, b)))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_zip_pad(a: *mut NativeIter, b: *mut NativeIter, fill_a: *const u8, fill_b: *const u8, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta, a_meta: *const NativeMeta, b_meta: *const NativeMeta, a_by_value: u8, b_by_value: u8) -> *mut NativeIter {
        guard(|| {
            let a_meta = Meta::copy(a_meta); let b_meta = Meta::copy(b_meta);
            let fill_a = if a_by_value != 0 { NativeValue::moved(fill_a, a_meta) } else { NativeValue::cloned(fill_a, a_meta) };
            let fill_b = if b_by_value != 0 { NativeValue::moved(fill_b, b_meta) } else { NativeValue::cloned(fill_b, b_meta) };
            let mut callback = Callback::new(env, invoke, release, meta);
            handle(jet_iter_zip_pad(take(a), take(b), fill_a, fill_b, move |a, b| callback.owned(a, b)))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_zip_strict(a: *mut NativeIter, b: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta, policy: JetCString, file: JetCString, line: u64, function: JetCString, source_line: JetCString, col: u64, caret: u64) -> *mut NativeIter {
        guard(|| {
            let mut callback = Callback::new(env, invoke, release, meta);
            handle(jet_iter_zip_strict(take(a), take(b), move |a, b| callback.owned(a, b),
                view(policy).to_owned(), view(file).to_owned(), line as u32,
                view(function).to_owned(), view(source_line).to_owned(), col as u32, caret as u32))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_native_iter_enumerate(iter: *mut NativeIter, env: *mut u8, invoke: usize, release: usize, meta: *const NativeMeta) -> *mut NativeIter {
        guard(|| {
            let callback = Callback::new(env, invoke, release, meta);
            handle(jet_iter_enumerate(take(iter), move |index, value| {
                let word = crate::jet_std::jet_int_owned_from_i64(index).into_raw();
                let mut result = NativeValue::zeroed(callback.output.clone());
                let invoke: unsafe extern "C" fn(*mut u8, *const u8, *const u8, *mut u8) -> u64 = std::mem::transmute(callback.invoke);
                let moved = invoke(callback.env, (&word as *const i64).cast(), value.slot.data(), result.slot.data_mut());
                assert_eq!(moved, 0, "enumerate callback inputs are borrowed");
                drop(jet_foundation::Numeric::JetInt::from_raw_owned(word));
                result
            }))
        })
    }
}
