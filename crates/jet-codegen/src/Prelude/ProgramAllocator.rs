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
/// runs. Every allocation carries private metadata so a block allocated under
/// one policy can be released safely after the previous policy is restored.
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

    fn reserve(&self, requested: usize) -> Option<bool> {
        use std::sync::atomic::Ordering;
        if self.mode.load(Ordering::Acquire) == JET_PROGRAM_ALLOCATOR_SYSTEM {
            return Some(false);
        }
        let cap = self.cap_bytes.load(Ordering::Acquire);
        let mut live = self.live_bytes.load(Ordering::Acquire);
        loop {
            let next = live.checked_add(requested)?;
            if cap != 0 && next > cap {
                return None;
            }
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

#[repr(C)]
struct JetProgramAllocationHeader {
    total_bytes: usize,
    alignment: usize,
    requested_bytes: usize,
    tracked: usize,
}

fn jet_program_allocation_layout(
    layout: std::alloc::Layout,
) -> Option<(std::alloc::Layout, usize)> {
    let metadata = std::mem::size_of::<JetProgramAllocationHeader>()
        .checked_add(std::mem::size_of::<usize>())?;
    let mask = layout.align().checked_sub(1)?;
    let offset = metadata.checked_add(mask)? & !mask;
    let total = offset.checked_add(layout.size().max(1))?;
    let alignment = layout
        .align()
        .max(std::mem::align_of::<JetProgramAllocationHeader>());
    Some((std::alloc::Layout::from_size_align(total, alignment).ok()?, offset))
}

// JET_VETTED_UNSAFE_BEGIN: program_allocator
unsafe impl std::alloc::GlobalAlloc for JetProgramAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let requested = layout.size().max(1);
        let Some(tracked) = self.reserve(requested) else {
            return std::ptr::null_mut();
        };
        let Some((storage_layout, offset)) = jet_program_allocation_layout(layout) else {
            if tracked {
                self.release(requested);
            }
            return std::ptr::null_mut();
        };
        // SAFETY: `storage_layout` is valid and is paired with the exact same
        // layout in `dealloc` below.
        let base =
            unsafe { std::alloc::GlobalAlloc::alloc(&std::alloc::System, storage_layout) };
        if base.is_null() {
            if tracked {
                self.release(requested);
            }
            return base;
        }
        let header = JetProgramAllocationHeader {
            total_bytes: storage_layout.size(),
            alignment: storage_layout.align(),
            requested_bytes: requested,
            tracked: usize::from(tracked),
        };
        // SAFETY: the metadata prefix was included in `offset`, the base has
        // header alignment, and the returned pointer has the caller's alignment.
        unsafe {
            base.cast::<JetProgramAllocationHeader>().write(header);
            base.add(offset - std::mem::size_of::<usize>())
                .cast::<usize>()
                .write_unaligned(offset);
            base.add(offset)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: std::alloc::Layout) {
        if ptr.is_null() {
            return;
        }
        // SAFETY: every pointer returned by this allocator has the offset word
        // immediately before it and a header at the recovered base.
        let offset = unsafe {
            ptr.sub(std::mem::size_of::<usize>())
                .cast::<usize>()
                .read_unaligned()
        };
        let base = unsafe { ptr.sub(offset) };
        let header = unsafe { base.cast::<JetProgramAllocationHeader>().read() };
        if header.tracked != 0 {
            self.release(header.requested_bytes);
        }
        let storage_layout = std::alloc::Layout::from_size_align(
            header.total_bytes,
            header.alignment,
        )
        .expect("program allocator stored an invalid layout");
        // SAFETY: `base` came from System with this exact stored layout.
        unsafe {
            std::alloc::GlobalAlloc::dealloc(&std::alloc::System, base, storage_layout)
        };
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        // SAFETY: delegation preserves the GlobalAlloc contract.
        let ptr = unsafe { self.alloc(layout) };
        if !ptr.is_null() {
            // SAFETY: `ptr` owns at least `layout.size()` writable bytes.
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
        let Ok(new_layout) = std::alloc::Layout::from_size_align(new_size, layout.align()) else {
            return std::ptr::null_mut();
        };
        // Allocate first so a failed growth leaves the original block live.
        let new_ptr = unsafe { self.alloc(new_layout) };
        if new_ptr.is_null() {
            return new_ptr;
        }
        // SAFETY: both blocks are live and non-overlapping; copy only the
        // smaller initialized extent, then release the original block.
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
