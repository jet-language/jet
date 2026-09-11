// D-FREESTAND-ALLOC1=A: the canonical Alloc closure for a typed fixed heap.
//
// Collections use alloc's selected target GlobalAlloc backend. Fixed targets
// use the checked linker heap below; Provider targets receive a separate
// target adapter from Codegen. `JetFixed` is the source-level inline
// allocator; its `new` constructor obtains a dedicated region from that
// selected backend and its `try_alloc` path returns the canonical fallible
// allocation report.

use alloc::alloc::{alloc, dealloc, Layout};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout as CoreLayout};
use core::cell::RefCell;
use core::fmt;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

extern crate alloc;

#[cfg(target_os = "none")]
extern "C" {
    static __jet_heap_start: u8;
    static __jet_heap_end: u8;
}

#[repr(C)]
struct JetHeapHeader {
    base: usize,
    span: usize,
    size: usize,
    next: usize,
    magic: usize,
    state: usize,
}

const HEAP_HEADER_MAGIC: usize = usize::MAX - 0x4A45_5448;
const HEAP_ALLOCATED: usize = 1;
const HEAP_FREE: usize = 2;

pub struct JetHeapAllocator {
    next: AtomicUsize,
    end: AtomicUsize,
    free_head: AtomicUsize,
    lock: core::sync::atomic::AtomicBool,
}

impl JetHeapAllocator {
    pub const fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            end: AtomicUsize::new(0),
            free_head: AtomicUsize::new(0),
            lock: core::sync::atomic::AtomicBool::new(false),
        }
    }

    #[inline]
    fn initialize(&self) -> Option<(usize, usize)> {
        let current = self.next.load(Ordering::Acquire);
        let end = self.end.load(Ordering::Acquire);
        if current != 0 {
            return (end > current).then_some((current, end));
        }
        #[cfg(target_os = "none")]
        let (raw_start, limit) = unsafe {
            (
                core::ptr::addr_of!(__jet_heap_start) as usize,
                core::ptr::addr_of!(__jet_heap_end) as usize,
            )
        };
        #[cfg(not(target_os = "none"))]
        let (raw_start, limit) = (0usize, 0usize);
        let align = core::mem::align_of::<JetHeapHeader>();
        let Some(start) = Self::align_up(raw_start, align) else {
            return None;
        };
        if start == 0 || limit <= start {
            return None;
        }
        let _ = self
            .next
            .compare_exchange(0, start, Ordering::AcqRel, Ordering::Acquire);
        let _ = self
            .end
            .compare_exchange(0, limit, Ordering::AcqRel, Ordering::Acquire);
        let current = self.next.load(Ordering::Acquire);
        let end = self.end.load(Ordering::Acquire);
        (end > current).then_some((current, end))
    }

    #[inline]
    fn acquire(&self) {
        while self
            .lock
            .compare_exchange(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    #[inline]
    fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }

    #[inline]
    fn align_up(value: usize, align: usize) -> Option<usize> {
        let mask = align.checked_sub(1)?;
        value.checked_add(mask).map(|value| value & !mask)
    }

    #[inline]
    fn minimum_free_span() -> usize {
        core::mem::size_of::<JetHeapHeader>().saturating_add(1)
    }

    /// Return the allocation header, user address, and consumed block end.
    /// The block end is header-aligned whenever a trailing free block can be
    /// retained; otherwise the whole remaining span is consumed.
    #[inline]
    fn fit(
        base: usize,
        span: usize,
        size: usize,
        align: usize,
    ) -> Option<(usize, usize, usize)> {
        let header_bytes = core::mem::size_of::<JetHeapHeader>();
        let block_end = base.checked_add(span)?;
        let user = Self::align_up(base.checked_add(header_bytes)?, align)?;
        let header = user.checked_sub(header_bytes)?;
        if header < base {
            return None;
        }
        let payload_end = user.checked_add(size)?;
        if payload_end > block_end {
            return None;
        }
        let aligned_end = Self::align_up(
            payload_end,
            core::mem::align_of::<JetHeapHeader>(),
        )?;
        let consumed_end = if aligned_end <= block_end {
            aligned_end
        } else {
            block_end
        };
        Some((header, user, consumed_end))
    }

    #[inline]
    unsafe fn write_alloc_header(
        header: usize,
        base: usize,
        span: usize,
        size: usize,
    ) {
        (header as *mut JetHeapHeader).write(JetHeapHeader {
            base,
            span,
            size,
            next: 0,
            magic: HEAP_HEADER_MAGIC,
            state: HEAP_ALLOCATED,
        });
    }

    #[inline]
    unsafe fn write_free_header(node: usize, span: usize, next: usize) {
        (node as *mut JetHeapHeader).write(JetHeapHeader {
            base: node,
            span,
            size: 0,
            next,
            magic: HEAP_HEADER_MAGIC,
            state: HEAP_FREE,
        });
    }

    /// Take the first free block that fits, retaining both usable fragments.
    ///
    /// Free nodes stay sorted by address. A leading fragment too small for a
    /// future header and payload is absorbed by the allocation; the same rule
    /// applies to the trailing fragment. This prevents unusable splinters
    /// while preserving first-fit reuse and deterministic coalescing.
    #[inline]
    unsafe fn take_free(
        &self,
        size: usize,
        align: usize,
    ) -> Option<*mut u8> {
        let mut previous = 0usize;
        let mut node = self.free_head.load(Ordering::Acquire);
        while node != 0 {
            let current = &*(node as *const JetHeapHeader);
            if current.magic != HEAP_HEADER_MAGIC
                || current.state != HEAP_FREE
                || current.base != node
            {
                return None;
            }
            let next = current.next;
            let Some((header, user, consumed_end)) =
                Self::fit(current.base, current.span, size, align)
            else {
                previous = node;
                node = next;
                continue;
            };
            let block_end = match current.base.checked_add(current.span) {
                Some(end) => end,
                None => return None,
            };
            let leading_span = header.saturating_sub(current.base);
            let keep_leading = leading_span >= Self::minimum_free_span();
            let allocation_base = if keep_leading {
                header
            } else {
                current.base
            };
            let trailing_span = block_end.saturating_sub(consumed_end);
            let keep_trailing = trailing_span >= Self::minimum_free_span();
            let replacement = if keep_leading {
                current.base
            } else if keep_trailing {
                consumed_end
            } else {
                next
            };

            if previous == 0 {
                self.free_head.store(replacement, Ordering::Release);
            } else {
                (*(previous as *mut JetHeapHeader)).next = replacement;
            }
            if keep_trailing {
                Self::write_free_header(consumed_end, trailing_span, next);
            }
            if keep_leading {
                let leading_next = if keep_trailing {
                    consumed_end
                } else {
                    next
                };
                Self::write_free_header(current.base, leading_span, leading_next);
            }

            let allocation_end = if keep_trailing {
                consumed_end
            } else {
                block_end
            };
            let allocation_span = allocation_end.saturating_sub(allocation_base);
            Self::write_alloc_header(header, allocation_base, allocation_span, size);
            return Some(user as *mut u8);
        }
        None
    }

    #[inline]
    unsafe fn alloc_locked(&self, layout: CoreLayout) -> *mut u8 {
        let size = layout.size().max(1);
        let align = layout
            .align()
            .max(core::mem::align_of::<JetHeapHeader>());
        if let Some(ptr) = self.take_free(size, align) {
            return ptr;
        }
        let Some((current, end)) = self.initialize() else {
            return core::ptr::null_mut();
        };
        let Some((header, user, consumed_end)) =
            Self::fit(current, end.saturating_sub(current), size, align)
        else {
            return core::ptr::null_mut();
        };
        let allocation_span = consumed_end.saturating_sub(current);
        Self::write_alloc_header(header, current, allocation_span, size);
        self.next.store(consumed_end, Ordering::Release);
        user as *mut u8
    }

    /// Insert a deallocated block into the sorted free list and merge both
    /// adjacent neighbours. All callers hold the allocator lock.
    #[inline]
    unsafe fn insert_free_locked(&self, base: usize, span: usize) {
        let mut previous = 0usize;
        let mut node = self.free_head.load(Ordering::Acquire);
        while node != 0 && node < base {
            let current = &*(node as *const JetHeapHeader);
            if current.magic != HEAP_HEADER_MAGIC || current.state != HEAP_FREE {
                return;
            }
            previous = node;
            node = current.next;
        }
        if node == base {
            return;
        }

        let merge_previous = if previous == 0 {
            false
        } else {
            let previous_record = &*(previous as *const JetHeapHeader);
            if previous_record.magic != HEAP_HEADER_MAGIC
                || previous_record.state != HEAP_FREE
            {
                return;
            }
            match previous.checked_add(previous_record.span) {
                Some(end) => end == base,
                None => return,
            }
        };

        let owner = if merge_previous {
            let previous_record = &mut *(previous as *mut JetHeapHeader);
            let Some(merged_span) = previous_record.span.checked_add(span) else {
                return;
            };
            previous_record.span = merged_span;
            previous_record.next = node;
            previous
        } else {
            Self::write_free_header(base, span, node);
            if previous == 0 {
                self.free_head.store(base, Ordering::Release);
            } else {
                (*(previous as *mut JetHeapHeader)).next = base;
            }
            base
        };

        let owner_end = match owner.checked_add(
            (*(owner as *const JetHeapHeader)).span,
        ) {
            Some(end) => end,
            None => return,
        };
        if node != 0 {
            let next_node = &*(node as *const JetHeapHeader);
            if next_node.magic != HEAP_HEADER_MAGIC || next_node.state != HEAP_FREE {
                return;
            }
            if owner_end == next_node.base {
                let merged_span = match (*(owner as *const JetHeapHeader))
                    .span
                    .checked_add(next_node.span)
                {
                    Some(merged) => merged,
                    None => return,
                };
                (*(owner as *mut JetHeapHeader)).span = merged_span;
                (*(owner as *mut JetHeapHeader)).next = next_node.next;
            }
        }
    }
}

/// Global allocator adapter for a checked target allocator provider.
///
/// The provider owns the storage. This adapter only forwards the Rust layout
/// facts and preserves a null result as allocation failure; default
/// `GlobalAlloc` `alloc_zeroed`/`realloc` behavior remains the single fallback.
pub struct JetTargetAllocator {
    alloc: unsafe fn(usize, usize) -> *mut u8,
    dealloc: unsafe fn(*mut u8, usize, usize),
}

impl JetTargetAllocator {
    pub const fn from_callbacks(
        alloc: unsafe fn(usize, usize) -> *mut u8,
        dealloc: unsafe fn(*mut u8, usize, usize),
    ) -> Self {
        Self { alloc, dealloc }
    }
}

unsafe impl GlobalAlloc for JetTargetAllocator {
    unsafe fn alloc(&self, layout: CoreLayout) -> *mut u8 {
        unsafe { (self.alloc)(layout.size().max(1), layout.align()) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: CoreLayout) {
        if ptr.is_null() {
            return;
        }
        unsafe { (self.dealloc)(ptr, layout.size().max(1), layout.align()) };
    }
}
unsafe impl GlobalAlloc for JetHeapAllocator {
    unsafe fn alloc(&self, layout: CoreLayout) -> *mut u8 {
        self.acquire();
        let ptr = self.alloc_locked(layout);
        self.release();
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: CoreLayout) {
        if ptr.is_null() {
            return;
        }
        let Some(header) = (ptr as usize).checked_sub(
            core::mem::size_of::<JetHeapHeader>(),
        ) else {
            return;
        };
        self.acquire();
        let valid = {
            let record = &*(header as *const JetHeapHeader);
            let heap_end = self.end.load(Ordering::Acquire);
            let block_end = record.base.checked_add(record.span);
            record.magic == HEAP_HEADER_MAGIC
                && record.state == HEAP_ALLOCATED
                && record.base != 0
                && record.span >= core::mem::size_of::<JetHeapHeader>()
                && header >= record.base
                && block_end.is_some_and(|end| end <= heap_end)
                && header
                    .checked_add(core::mem::size_of::<JetHeapHeader>())
                    .is_some_and(|end| block_end.is_some_and(|block_end| end <= block_end))
                && header
                    .checked_add(core::mem::size_of::<JetHeapHeader>())
                    == Some(ptr as usize)
        };
        if valid {
            let record = &mut *(header as *mut JetHeapHeader);
            let base = record.base;
            let span = record.span;
            // Leave a tombstone at the user header so an immediate duplicate
            // deallocation is harmless. The canonical free header lives at
            // the block base for sorted coalescing.
            record.state = HEAP_FREE;
            self.insert_free_locked(base, span);
        }
        self.release();
    }

    unsafe fn alloc_zeroed(&self, layout: CoreLayout) -> *mut u8 {
        let ptr = self.alloc(layout);
        if !ptr.is_null() {
            core::ptr::write_bytes(ptr, 0, layout.size().max(1));
        }
        ptr
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: CoreLayout,
        new_size: usize,
    ) -> *mut u8 {
        if ptr.is_null() {
            let new_layout = match CoreLayout::from_size_align(
                new_size.max(1),
                layout.align(),
            ) {
                Ok(layout) => layout,
                Err(_) => return core::ptr::null_mut(),
            };
            return self.alloc(new_layout);
        }
        let Some(header) = (ptr as usize).checked_sub(
            core::mem::size_of::<JetHeapHeader>(),
        ) else {
            return core::ptr::null_mut();
        };
        self.acquire();
        let old_size = {
            let record = &mut *(header as *mut JetHeapHeader);
            let heap_end = self.end.load(Ordering::Acquire);
            let block_end = record.base.checked_add(record.span);
            if record.magic != HEAP_HEADER_MAGIC
                || record.state != HEAP_ALLOCATED
                || record.base == 0
                || record.span < core::mem::size_of::<JetHeapHeader>()
                || header < record.base
                || !block_end.is_some_and(|end| end <= heap_end)
                || !header
                    .checked_add(core::mem::size_of::<JetHeapHeader>())
                    .is_some_and(|end| block_end.is_some_and(|block_end| end <= block_end))
                || header
                    .checked_add(core::mem::size_of::<JetHeapHeader>())
                    != Some(ptr as usize)
            {
                None
            } else {
                let old_size = record.size.max(1);
                if new_size <= old_size {
                    record.size = new_size.max(1);
                    Some(old_size)
                } else {
                    Some(old_size)
                }
            }
        };
        self.release();
        let Some(old_size) = old_size else {
            return core::ptr::null_mut();
        };
        if new_size <= old_size {
            return ptr;
        }
        let new_layout = match CoreLayout::from_size_align(
            new_size.max(1),
            layout.align(),
        ) {
            Ok(layout) => layout,
            Err(_) => return core::ptr::null_mut(),
        };
        let replacement = self.alloc(new_layout);
        if replacement.is_null() {
            return core::ptr::null_mut();
        }
        core::ptr::copy_nonoverlapping(ptr, replacement, old_size.min(new_size));
        self.dealloc(ptr, layout);
        replacement
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocError {
    pub requested_bytes: i64,
    pub allocator: String,
}

#[inline]
pub fn jet_alloc_error(requested_bytes: usize, allocator: &str) -> AllocError {
    AllocError {
        requested_bytes: requested_bytes.min(i64::MAX as usize) as i64,
        allocator: allocator.to_string(),
    }
}

impl JetPortableError for AllocError {
    fn jet_error_bytes(&self) -> &[u8] {
        self.allocator.as_bytes()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetErr {
    message: String,
    code: JetOutcome<String, JetAbsent>,
    cause: JetOutcome<Box<JetErr>, JetAbsent>,
}

#[inline]
pub fn jet_err(
    message: String,
    code: JetOutcome<String, JetAbsent>,
    cause: JetOutcome<JetErr, JetAbsent>,
) -> JetErr {
    JetErr {
        message,
        code,
        cause: cause.map(Box::new),
    }
}

#[inline]
pub fn jet_err_from_message(message: String) -> JetErr {
    jet_err(message, Err(JetAbsent), Err(JetAbsent))
}

#[inline]
pub fn jet_err_message(error: &JetErr) -> String {
    error.message.clone()
}

#[inline]
pub fn jet_err_code(error: &JetErr) -> JetOutcome<String, JetAbsent> {
    error.code.clone()
}

#[inline]
pub fn jet_err_cause(error: &JetErr) -> JetOutcome<JetErr, JetAbsent> {
    error
        .cause
        .as_ref()
        .map(|cause| (**cause).clone())
        .map_err(|_| JetAbsent)
}

impl JetPortableError for JetErr {
    fn jet_error_bytes(&self) -> &[u8] {
        self.message.as_bytes()
    }
}
impl JetPortableError for String {
    fn jet_error_bytes(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl JetPortableError for &String {
    fn jet_error_bytes(&self) -> &[u8] {
        self.as_bytes()
    }
}


pub trait JetShow {
    fn jet_show(&self) -> String;
}

pub trait JetDisplay {
    fn jet_display(&self) -> String;
}

pub trait JetDebug {
    fn jet_debug(&self) -> String;
}

impl<T: fmt::Display> JetShow for T {
    fn jet_show(&self) -> String {
        self.to_string()
    }
}

impl<T: fmt::Display> JetDisplay for T {
    fn jet_display(&self) -> String {
        self.to_string()
    }
}

impl<T: fmt::Debug> JetDebug for T {
    fn jet_debug(&self) -> String {
        format!("{:?}", self)
    }
}

impl<K: fmt::Debug + Ord, V: fmt::Debug> JetShow for BTreeMap<K, V> {
    fn jet_show(&self) -> String {
        format!("{:?}", self)
    }
}

impl<K: fmt::Debug + Ord, V: fmt::Debug> JetDisplay for BTreeMap<K, V> {
    fn jet_display(&self) -> String {
        format!("{:?}", self)
    }
}

impl<K: fmt::Debug + Ord, V: fmt::Debug> JetDebug for BTreeMap<K, V> {
    fn jet_debug(&self) -> String {
        format!("{:?}", self)
    }
}

pub type JetMap<K, V> = BTreeMap<K, V>;

#[inline(always)]
pub fn jet_outcome_of<T>(value: Option<T>) -> JetOutcome<T, JetAbsent> {
    value.ok_or(JetAbsent)
}

/// A fixed, source-level allocator over an inline or linker-backed byte span.
/// Backing acquisition stays in this portable adapter; placement, metadata, and
/// reverse destruction are delegated to the one generated `jet_fixed_kernel`.
pub mod jet_mem {
    use super::*;
    use super::jet_fixed_kernel::FixedState;
    pub struct JetFixed {
        state: RefCell<FixedState>,
        base: NonNull<u8>,
        capacity: usize,
        owned: bool,
    }

    impl JetFixed {
        pub fn new(size: usize) -> Self {
            let capacity = size.max(1);
            let layout = Layout::from_size_align(capacity, 1)
                .unwrap_or_else(|_| jet_panic("<core.mem>", 0, "invalid Fixed allocator size"));
            let ptr = unsafe { alloc(layout) };
            let Some(ptr) = NonNull::new(ptr) else {
                return jet_panic(
                    "<core.mem>",
                    0,
                    "Fixed allocator exhausted its inline backing buffer",
                );
            };
            Self::from_raw(ptr.as_ptr(), capacity, true)
        }

        pub fn over(bytes: &mut [u8]) -> Self {
            if bytes.is_empty() {
                return jet_panic("<core.mem>", 0, "Fixed allocator needs backing bytes");
            }
            Self::from_raw(bytes.as_mut_ptr(), bytes.len(), false)
        }

        fn from_raw(ptr: *mut u8, capacity: usize, owned: bool) -> Self {
            let Some(base) = NonNull::new(ptr) else {
                return jet_panic("<core.mem>", 0, "Fixed allocator needs backing bytes");
            };
            // SAFETY: constructors establish a non-empty, non-null backing
            // span; FixedState rechecks its address arithmetic.
            let state = unsafe { FixedState::from_raw(base.as_ptr(), capacity) };
            Self {
                state: RefCell::new(state),
                base,
                capacity,
                owned,
            }
        }

        pub fn alloc<T: 'static>(&self, value: T) -> &mut T {
            let ptr = {
                let mut state = self.state.borrow_mut();
                match state.try_alloc(value) {
                    Ok(ptr) => ptr,
                    Err(value) => {
                        core::mem::drop(value);
                        jet_panic(
                            "<core.mem>",
                            0,
                            "Fixed allocator exhausted its inline backing buffer",
                        )
                    }
                }
            };
            unsafe { &mut *ptr }
        }

        pub fn try_alloc<T: 'static>(&self, value: T) -> Result<&mut T, AllocError> {
            if super::jet_fault_should_fail_allocation() {
                return Err(super::jet_alloc_error(
                    core::mem::size_of::<T>().max(1),
                    "Fixed",
                ));
            }
            let size = core::mem::size_of::<T>().max(1);
            let ptr = {
                let mut state = self.state.borrow_mut();
                state
                    .try_alloc(value)
                    .map_err(|_| super::jet_alloc_error(size, "Fixed"))?
            };
            // SAFETY: the shared kernel placed `T` at this address and keeps
            // it alive until reset/close.
            Ok(unsafe { &mut *ptr })
        }

        pub fn reset(&mut self) {
            // SAFETY: FixedState owns every live payload and walks its own
            // metadata links in reverse allocation order.
            unsafe {
                self.state.get_mut().reset(|_, _| {});
            }
        }

        pub const fn capacity(&self) -> usize {
            self.capacity
        }

        pub fn used(&self) -> usize {
            self.state.borrow().used()
        }
    }

    impl Drop for JetFixed {
        fn drop(&mut self) {
            self.reset();
            if self.owned {
                if let Ok(layout) = Layout::from_size_align(self.capacity, 1) {
                    // The shared state has already dropped every initialized
                    // payload before this backing release.
                    unsafe { dealloc(self.base.as_ptr(), layout) };
                }
            }
        }
    }
}
