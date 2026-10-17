// The canonical native descriptor ABI and its image-bound metadata copy.
// Payloads are never stored here; the caller retains the originating image.
#[doc(hidden)]
#[allow(dead_code)]
pub mod jet_native_meta {
    use std::sync::Arc;

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

    pub struct Meta {
        pub(crate) stride: usize,
        pub(crate) clone: usize,
        pub(crate) drop: usize,
        pub(crate) show: usize,
        pub(crate) equal: usize,
        pub(crate) compare: usize,
        pub(crate) kind: u64,
        pub(crate) child0: Option<Arc<Meta>>,
        pub(crate) child1: Option<Arc<Meta>>,
    }

    impl Meta {
        /// Copy only descriptors from a checked, pinned native image.
        ///
        /// # Safety
        /// `raw` and its children must be valid canonical descriptors; all callback
        /// code images must outlive every user of the returned metadata.
        pub unsafe fn copy(raw: *const NativeMeta) -> Arc<Self> {
            let row = &*raw;
            Arc::new(Self {
                stride: row.stride,
                clone: row.clone,
                drop: row.drop,
                show: row.show,
                equal: row.equal,
                compare: row.compare,
                kind: row.kind,
                child0: (!row.child0.is_null()).then(|| Self::copy(row.child0)),
                child1: (!row.child1.is_null()).then(|| Self::copy(row.child1)),
            })
        }

        pub fn stride(&self) -> usize { self.stride }
        pub fn kind(&self) -> u64 { self.kind }
        pub fn tag(&self) -> u32 { self.kind as u32 }
        pub fn payload_size(&self) -> usize {
            match self.tag() { 11 => 0, 9 => 1, _ => self.stride }
        }
        pub(crate) fn first(&self) -> Arc<Meta> {
            self.child0.as_ref().expect("native child descriptor").clone()
        }
        pub(crate) fn second(&self) -> Arc<Meta> {
            self.child1.as_ref().expect("native error descriptor").clone()
        }
    }

    #[cfg(target_pointer_width = "64")]
    const _: () = {
        assert!(std::mem::size_of::<NativeMeta>() == 72);
        assert!(std::mem::align_of::<NativeMeta>() == 8);
        assert!(std::mem::offset_of!(NativeMeta, stride) == 0);
        assert!(std::mem::offset_of!(NativeMeta, clone) == 8);
        assert!(std::mem::offset_of!(NativeMeta, drop) == 16);
        assert!(std::mem::offset_of!(NativeMeta, show) == 24);
        assert!(std::mem::offset_of!(NativeMeta, equal) == 32);
        assert!(std::mem::offset_of!(NativeMeta, compare) == 40);
        assert!(std::mem::offset_of!(NativeMeta, kind) == 48);
        assert!(std::mem::offset_of!(NativeMeta, child0) == 56);
        assert!(std::mem::offset_of!(NativeMeta, child1) == 64);
    };
}
