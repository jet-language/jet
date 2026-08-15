// D-ALLOC-PROGRAM1=A: the one hosted whole-program allocator kernel.
//
// Generated AOT programs use `JetProgramAllocator` directly as their Rust
// `#[global_allocator]`. The resident JIT and interpreter configure the same
// source through a marshalling adapter. System mode is the hidden default;
// counting mode may add a hard cap while preserving Rust's ordinary aborting
// allocation behavior. Fallible `try_` callers observe a null allocation as
// their existing typed `AllocError` value.

pub const JET_PROGRAM_ALLOCATOR_SYSTEM: u8 = 0;
pub const JET_PROGRAM_ALLOCATOR_COUNTING: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JetProgramAllocatorConfig {
    mode: u8,
    cap_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JetProgramAllocatorFacts {
    pub allocations: usize,
    pub requested_bytes: usize,
    pub live_bytes: usize,
    pub high_water_bytes: usize,
    pub cap_bytes: usize,
}

/// A system-heap wrapper whose policy is atomically replaceable between hosted
/// runs. Hidden-default allocation delegates to System with the caller's exact
/// layout and no prefix. Only selected-wrapper allocations enter the private,
/// allocation-free tracker, so blocks remain safe across policy restoration.
pub struct JetProgramAllocator {
    mode: std::sync::atomic::AtomicU8,
    cap_bytes: std::sync::atomic::AtomicUsize,
    allocations: std::sync::atomic::AtomicUsize,
    requested_bytes: std::sync::atomic::AtomicUsize,
    live_bytes: std::sync::atomic::AtomicUsize,
    high_water_bytes: std::sync::atomic::AtomicUsize,
}

/// The resident allocator instance. Native CLI binaries install a reference to
/// this value as their root `#[global_allocator]`; emitted AOT programs do the
/// same when `package.jet` selects a wrapper.
pub static JET_HOST_PROGRAM_ALLOCATOR: JetProgramAllocator = JetProgramAllocator::system();

static JET_PROGRAM_ALLOCATOR_CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct JetProgramAllocatorConfigGuard {
    previous: JetProgramAllocatorConfig,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Drop for JetProgramAllocatorConfigGuard {
    fn drop(&mut self) {
        JET_HOST_PROGRAM_ALLOCATOR.restore(self.previous);
    }
}

/// Marshal one hosted execution through the canonical allocator instance.
/// `cap_bytes = None` is the hidden system default; `Some(0)` is an uncapped
/// counting wrapper. The lock makes a process-global fact honest when tests or
/// embedding hosts attempt concurrent runs.
pub fn jet_with_host_program_allocator<R>(
    cap_bytes: Option<u64>,
    run: impl FnOnce() -> R,
) -> (R, JetProgramAllocatorFacts) {
    let lock = JET_PROGRAM_ALLOCATOR_CONFIG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let previous = match cap_bytes {
        Some(cap_bytes) => JET_HOST_PROGRAM_ALLOCATOR.configure_counting(cap_bytes),
        None => JET_HOST_PROGRAM_ALLOCATOR.configure_system(),
    };
    let guard = JetProgramAllocatorConfigGuard {
        previous,
        _lock: lock,
    };
    let output = run();
    let facts = JET_HOST_PROGRAM_ALLOCATOR.facts();
    drop(guard);
    (output, facts)
}
/// Canonical hosted preflight for fallible Prelude allocations. Engines pass
/// this function into the same `*_defaulted` collection kernel AOT uses; the
/// checked package fact has already configured the resident allocator.
pub fn jet_host_program_allocator_allows(requested: usize) -> bool {
    JET_HOST_PROGRAM_ALLOCATOR.allows(requested)
}

impl JetProgramAllocator {
    pub const fn system() -> Self {
        Self::new(JET_PROGRAM_ALLOCATOR_SYSTEM, 0)
    }

    pub const fn counting(cap_bytes: u64) -> Self {
        Self::new(
            JET_PROGRAM_ALLOCATOR_COUNTING,
            if cap_bytes > usize::MAX as u64 {
                usize::MAX
            } else {
                cap_bytes as usize
            },
        )
    }

    const fn new(mode: u8, cap_bytes: usize) -> Self {
        Self {
            mode: std::sync::atomic::AtomicU8::new(mode),
            cap_bytes: std::sync::atomic::AtomicUsize::new(cap_bytes),
            allocations: std::sync::atomic::AtomicUsize::new(0),
            requested_bytes: std::sync::atomic::AtomicUsize::new(0),
            live_bytes: std::sync::atomic::AtomicUsize::new(0),
            high_water_bytes: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn configure_system(&self) -> JetProgramAllocatorConfig {
        self.configure(JET_PROGRAM_ALLOCATOR_SYSTEM, 0)
    }

    pub fn configure_counting(&self, cap_bytes: u64) -> JetProgramAllocatorConfig {
        self.configure(
            JET_PROGRAM_ALLOCATOR_COUNTING,
            usize::try_from(cap_bytes).unwrap_or(usize::MAX),
        )
    }

    pub fn restore(&self, config: JetProgramAllocatorConfig) {
        use std::sync::atomic::Ordering;
        self.mode.store(JET_PROGRAM_ALLOCATOR_SYSTEM, Ordering::Release);
        self.cap_bytes.store(config.cap_bytes, Ordering::Release);
        self.mode.store(config.mode, Ordering::Release);
    }

    pub fn facts(&self) -> JetProgramAllocatorFacts {
        use std::sync::atomic::Ordering;
        JetProgramAllocatorFacts {
            allocations: self.allocations.load(Ordering::Acquire),
            requested_bytes: self.requested_bytes.load(Ordering::Acquire),
            live_bytes: self.live_bytes.load(Ordering::Acquire),
            high_water_bytes: self.high_water_bytes.load(Ordering::Acquire),
            cap_bytes: self.cap_bytes.load(Ordering::Acquire),
        }
    }

    fn configure(&self, mode: u8, cap_bytes: usize) -> JetProgramAllocatorConfig {
        use std::sync::atomic::Ordering;
        let previous = JetProgramAllocatorConfig {
            mode: self.mode.swap(JET_PROGRAM_ALLOCATOR_SYSTEM, Ordering::AcqRel),
            cap_bytes: self.cap_bytes.load(Ordering::Acquire),
        };
        self.cap_bytes.store(cap_bytes, Ordering::Release);
        if self.live_bytes.load(Ordering::Acquire) == 0 {
            self.allocations.store(0, Ordering::Release);
            self.requested_bytes.store(0, Ordering::Release);
            self.high_water_bytes.store(0, Ordering::Release);
        }
        self.mode.store(mode, Ordering::Release);
        previous
    }

    fn next_live_bytes(&self, live: usize, requested: usize) -> Option<usize> {
        let next = live.checked_add(requested)?;
        let cap = self
            .cap_bytes
            .load(std::sync::atomic::Ordering::Acquire);
        (cap == 0 || next <= cap).then_some(next)
    }

    fn allows(&self, requested: usize) -> bool {
        use std::sync::atomic::Ordering;
        self.mode.load(Ordering::Acquire) == JET_PROGRAM_ALLOCATOR_SYSTEM
            || self
                .next_live_bytes(self.live_bytes.load(Ordering::Acquire), requested)
                .is_some()
    }

    fn reserve(&self, requested: usize) -> Option<bool> {
        use std::sync::atomic::Ordering;
        if self.mode.load(Ordering::Acquire) == JET_PROGRAM_ALLOCATOR_SYSTEM {
            return Some(false);
        }
        let mut live = self.live_bytes.load(Ordering::Acquire);
        loop {
            let Some(next) = self.next_live_bytes(live, requested) else {
                return None;
            };
            match self.live_bytes.compare_exchange_weak(
                live,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let _ = self.allocations.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |value| Some(value.saturating_add(1)),
                    );
                    let _ = self.requested_bytes.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |value| Some(value.saturating_add(requested)),
                    );
                    self.high_water_bytes.fetch_max(next, Ordering::Relaxed);
                    return Some(true);
                }
                Err(current) => live = current,
            }
        }
    }

    fn release(&self, requested: usize) {
        use std::sync::atomic::Ordering;
        let mut live = self.live_bytes.load(Ordering::Acquire);
        loop {
            let next = live.saturating_sub(requested);
            match self.live_bytes.compare_exchange_weak(
                live,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(current) => live = current,
            }
        }
    }
}

// JET_VETTED_UNSAFE_BEGIN: program_allocator
struct JetProgramAllocationNode {
    ptr: *mut u8,
    requested_bytes: usize,
    next: *mut JetProgramAllocationNode,
}

struct JetProgramAllocationTracker {
    head: *mut JetProgramAllocationNode,
}

// SAFETY: the raw links are read or written only while the tracker mutex is
// held. Allocation/deallocation of tracker nodes bypasses the global allocator
// and goes straight to System, so bookkeeping cannot recurse.
unsafe impl Send for JetProgramAllocationTracker {}

static JET_PROGRAM_ALLOCATION_TRACKER: std::sync::Mutex<JetProgramAllocationTracker> =
    std::sync::Mutex::new(JetProgramAllocationTracker {
        head: std::ptr::null_mut(),
    });
static JET_PROGRAM_TRACKED_ALLOCATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

unsafe fn jet_track_program_allocation(ptr: *mut u8, requested_bytes: usize) -> bool {
    use std::sync::atomic::Ordering;

    let node_layout = std::alloc::Layout::new::<JetProgramAllocationNode>();
    let node = unsafe {
        std::alloc::GlobalAlloc::alloc(&std::alloc::System, node_layout)
            .cast::<JetProgramAllocationNode>()
    };
    if node.is_null() {
        return false;
    }
    let mut tracker = JET_PROGRAM_ALLOCATION_TRACKER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    unsafe {
        node.write(JetProgramAllocationNode {
            ptr,
            requested_bytes,
            next: tracker.head,
        });
    }
    tracker.head = node;
    JET_PROGRAM_TRACKED_ALLOCATIONS.fetch_add(1, Ordering::Release);
    true
}

unsafe fn jet_untrack_program_allocation(ptr: *mut u8) -> Option<usize> {
    use std::sync::atomic::Ordering;

    if JET_PROGRAM_TRACKED_ALLOCATIONS.load(Ordering::Acquire) == 0 {
        return None;
    }
    let mut tracker = JET_PROGRAM_ALLOCATION_TRACKER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut previous = std::ptr::null_mut::<JetProgramAllocationNode>();
    let mut current = tracker.head;
    while !current.is_null() {
        let node = unsafe { &*current };
        if node.ptr == ptr {
            if previous.is_null() {
                tracker.head = node.next;
            } else {
                unsafe { (*previous).next = node.next };
            }
            let requested_bytes = node.requested_bytes;
            drop(tracker);
            unsafe {
                std::alloc::GlobalAlloc::dealloc(
                    &std::alloc::System,
                    current.cast::<u8>(),
                    std::alloc::Layout::new::<JetProgramAllocationNode>(),
                );
            }
            JET_PROGRAM_TRACKED_ALLOCATIONS.fetch_sub(1, Ordering::Release);
            return Some(requested_bytes);
        }
        previous = current;
        current = node.next;
    }
    None
}

fn jet_program_allocation_is_tracked(ptr: *mut u8) -> bool {
    use std::sync::atomic::Ordering;

    if JET_PROGRAM_TRACKED_ALLOCATIONS.load(Ordering::Acquire) == 0 {
        return false;
    }
    let tracker = JET_PROGRAM_ALLOCATION_TRACKER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut current = tracker.head;
    while !current.is_null() {
        let node = unsafe { &*current };
        if node.ptr == ptr {
            return true;
        }
        current = node.next;
    }
    false
}

unsafe impl std::alloc::GlobalAlloc for JetProgramAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let requested = layout.size().max(1);
        let Some(tracked) = self.reserve(requested) else {
            return std::ptr::null_mut();
        };
        let ptr = unsafe { std::alloc::GlobalAlloc::alloc(&std::alloc::System, layout) };
        if ptr.is_null() {
            if tracked {
                self.release(requested);
            }
            return ptr;
        }
        if tracked && !unsafe { jet_track_program_allocation(ptr, requested) } {
            unsafe { std::alloc::GlobalAlloc::dealloc(&std::alloc::System, ptr, layout) };
            self.release(requested);
            return std::ptr::null_mut();
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        if ptr.is_null() {
            return;
        }
        if let Some(requested) = unsafe { jet_untrack_program_allocation(ptr) } {
            self.release(requested);
        }
        unsafe { std::alloc::GlobalAlloc::dealloc(&std::alloc::System, ptr, layout) };
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        use std::sync::atomic::Ordering;
        if self.mode.load(Ordering::Acquire) == JET_PROGRAM_ALLOCATOR_SYSTEM {
            return unsafe {
                std::alloc::GlobalAlloc::alloc_zeroed(&std::alloc::System, layout)
            };
        }
        let ptr = unsafe { self.alloc(layout) };
        if !ptr.is_null() {
            unsafe { ptr.write_bytes(0, layout.size()) };
        }
        ptr
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: std::alloc::Layout,
        new_size: usize,
    ) -> *mut u8 {
        if !jet_program_allocation_is_tracked(ptr) {
            return unsafe {
                std::alloc::GlobalAlloc::realloc(&std::alloc::System, ptr, layout, new_size)
            };
        }
        let Ok(new_layout) = std::alloc::Layout::from_size_align(new_size, layout.align()) else {
            return std::ptr::null_mut();
        };
        // Allocate first so a failed growth leaves the original block live.
        let new_ptr = unsafe { self.alloc(new_layout) };
        if new_ptr.is_null() {
            return new_ptr;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
            self.dealloc(ptr, layout);
        }
        new_ptr
    }
}

/// Zero-sized root adapter used by the resident CLI binary. All behavior stays
/// on `JET_HOST_PROGRAM_ALLOCATOR`; this type only satisfies Rust's requirement
/// that `#[global_allocator]` name a static value.
pub struct JetHostProgramAllocator;

unsafe impl std::alloc::GlobalAlloc for JetHostProgramAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        unsafe {
            std::alloc::GlobalAlloc::alloc(&JET_HOST_PROGRAM_ALLOCATOR, layout)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe {
            std::alloc::GlobalAlloc::dealloc(&JET_HOST_PROGRAM_ALLOCATOR, ptr, layout)
        }
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        unsafe {
            std::alloc::GlobalAlloc::alloc_zeroed(&JET_HOST_PROGRAM_ALLOCATOR, layout)
        }
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: std::alloc::Layout,
        new_size: usize,
    ) -> *mut u8 {
        unsafe {
            std::alloc::GlobalAlloc::realloc(
                &JET_HOST_PROGRAM_ALLOCATOR,
                ptr,
                layout,
                new_size,
            )
        }
    }
}
// JET_VETTED_UNSAFE_END: program_allocator
