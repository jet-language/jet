#[cfg(target_arch = "x86_64")]
use crate::arena::{Arena, Section};
#[cfg(target_arch = "x86_64")]
use std::sync::Arc;

use cranelift_module::{ModuleError, ModuleResult};

#[cfg(all(not(target_os = "windows"), feature = "selinux-fix"))]
use memmap2::MmapMut;

#[cfg(not(any(feature = "selinux-fix", windows)))]
use std::alloc;
use std::ffi::c_void;
use std::io;
use std::mem;
use std::ptr;
use wasmtime_jit_icache_coherence as icache_coherence;

/// A simple struct consisting of a pointer and length.
struct PtrLen {
    #[cfg(all(not(target_os = "windows"), feature = "selinux-fix"))]
    map: Option<MmapMut>,

    ptr: *mut u8,
    len: usize,
    #[cfg(all(not(target_os = "windows"), not(feature = "selinux-fix")))]
    owns: bool,
}


impl PtrLen {
    /// Create a new empty `PtrLen`.
    fn new() -> Self {
        Self {
            #[cfg(all(not(target_os = "windows"), feature = "selinux-fix"))]
            map: None,

            ptr: ptr::null_mut(),
            len: 0,
            #[cfg(all(not(target_os = "windows"), not(feature = "selinux-fix")))]
            owns: false,
        }
    }

    /// Track a block carved from the shared arena; the arena owns the mapping.
    #[cfg(target_arch = "x86_64")]
    fn from_arena(ptr: *mut u8, len: usize) -> Self {
        Self {
            #[cfg(all(not(target_os = "windows"), feature = "selinux-fix"))]
            map: None,

            ptr,
            len,
            #[cfg(all(not(target_os = "windows"), not(feature = "selinux-fix")))]
            owns: false,
        }
    }


    /// Create a new `PtrLen` pointing to at least `size` bytes of memory,
    /// suitably sized and aligned for memory protection.
    #[cfg(all(not(target_os = "windows"), feature = "selinux-fix"))]
    fn with_size(size: usize) -> io::Result<Self> {
        let alloc_size = region::page::ceil(size);
        MmapMut::map_anon(alloc_size).map(|mut mmap| {
            // The order here is important; we assign the pointer first to get
            // around compile time borrow errors.
            Self {
                ptr: mmap.as_mut_ptr(),
                map: Some(mmap),
                len: alloc_size,
            }
        })
    }

    #[cfg(all(not(target_os = "windows"), not(feature = "selinux-fix")))]
    fn with_size(size: usize) -> io::Result<Self> {
        assert_ne!(size, 0);
        let page_size = region::page::size();
        let alloc_size = region::page::ceil(size);
        let layout = alloc::Layout::from_size_align(alloc_size, page_size).unwrap();
        // Safety: We assert that the size is non-zero above.
        let ptr = unsafe { alloc::alloc(layout) };

        if !ptr.is_null() {
            Ok(Self {
                ptr,
                len: alloc_size,
                owns: true,
            })

        } else {
            Err(io::Error::from(io::ErrorKind::OutOfMemory))
        }
    }

    #[cfg(target_os = "windows")]
    fn with_size(size: usize) -> io::Result<Self> {
        use windows_sys::Win32::System::Memory::{
            VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
        };

        // VirtualAlloc always rounds up to the next multiple of the page size
        let ptr = unsafe {
            VirtualAlloc(
                ptr::null_mut(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if !ptr.is_null() {
            Ok(Self {
                ptr: ptr as *mut u8,
                len: region::page::ceil(size),
            })
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

// `MMapMut` from `cfg(feature = "selinux-fix")` already deallocates properly.
#[cfg(all(not(target_os = "windows"), not(feature = "selinux-fix")))]
impl Drop for PtrLen {
    fn drop(&mut self) {
        if self.owns && !self.ptr.is_null() {
            let page_size = region::page::size();
            let layout = alloc::Layout::from_size_align(self.len, page_size).unwrap();
            unsafe {
                if region::protect(self.ptr, self.len, region::Protection::READ_WRITE).is_ok() {
                    alloc::dealloc(self.ptr, layout);
                }
            }
        }
    }
}

// TODO: add a `Drop` impl for `cfg(target_os = "windows")`

/// Type of branch protection to apply to executable memory.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BranchProtection {
    /// No protection.
    None,
    /// Use the Branch Target Identification extension of the Arm architecture.
    BTI,
}

/// JIT memory manager. This manages pages of suitably aligned and
/// accessible memory. Memory will be leaked by default to have
/// function pointers remain valid for the remainder of the
/// program's life.
pub(crate) struct Memory {
    allocations: Vec<PtrLen>,
    already_protected: usize,
    current: PtrLen,
    position: usize,
    branch_protection: BranchProtection,
    #[cfg(target_arch = "x86_64")]
    arena: Option<(Arc<Arena>, Section)>,
}


unsafe impl Send for Memory {}

impl Memory {
    pub(crate) fn new(branch_protection: BranchProtection) -> Self {
        Self {
            allocations: Vec::new(),
            already_protected: 0,
            current: PtrLen::new(),
            position: 0,
            branch_protection,
            #[cfg(target_arch = "x86_64")]
            arena: None,
        }
    }

    #[cfg(target_arch = "x86_64")]
    pub(crate) fn new_in_arena(
        branch_protection: BranchProtection,
        arena: Arc<Arena>,
        section: Section,
    ) -> Self {
        Self {
            allocations: Vec::new(),
            already_protected: 0,
            current: PtrLen::new(),
            position: 0,
            branch_protection,
            arena: Some((arena, section)),
        }
    }


    fn finish_current(&mut self) {
        self.allocations
            .push(mem::replace(&mut self.current, PtrLen::new()));
        self.position = 0;
    }

    pub(crate) fn allocate(&mut self, size: usize, align: u64) -> io::Result<*mut u8> {
        let align = usize::try_from(align).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "JIT alignment is too large")
        })?;
        if align == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "JIT alignment must be nonzero",
            ));
        }
        let address = (self.current.ptr as usize)
            .checked_add(self.position)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::OutOfMemory, "JIT allocation size overflow")
            })?;
        let padding = (align - address % align) % align;
        let aligned_position = self.position.checked_add(padding).ok_or_else(|| {
            io::Error::new(io::ErrorKind::OutOfMemory, "JIT allocation size overflow")
        })?;
        let request_size = size.max(1);
        if let Some(end) = aligned_position.checked_add(request_size) {
            if end <= self.current.len {
                let ptr = unsafe { self.current.ptr.add(aligned_position) };
                self.position = end;
                return Ok(ptr);
            }
        }

        self.finish_current();

        #[cfg(target_arch = "x86_64")]
        if let Some((arena, section)) = &self.arena {
            let (ptr, len) = arena.allocate(*section, request_size, align)?;
            self.current = PtrLen::from_arena(ptr, len);
            self.position = request_size;
            return Ok(ptr);
        }

        self.current = PtrLen::with_size(request_size)?;
        self.position = request_size;

        Ok(self.current.ptr)
    }


    /// Set all memory allocated in this `Memory` up to now as readable and executable.
    pub(crate) fn set_readable_and_executable(&mut self) -> ModuleResult<()> {
        self.finish_current();

        // Clear all the newly allocated code from cache if the processor requires it
        //
        // Do this before marking the memory as R+X, technically we should be able to do it after
        // but there are some CPU's that have had errata about doing this with read only memory.
        for &PtrLen { ptr, len, .. } in self.non_protected_allocations_iter() {
            unsafe {
                icache_coherence::clear_cache(ptr as *const c_void, len).map_err(|error| {
                    ModuleError::Backend(anyhow::anyhow!(
                        "unable to clear JIT instruction cache: {error}"
                    ))
                })?;
            };
        }

        let set_region_readable_and_executable = |ptr, len| -> ModuleResult<()> {
            if self.branch_protection == BranchProtection::BTI {
                #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
                if std::arch::is_aarch64_feature_detected!("bti") {
                    let prot = libc::PROT_EXEC | libc::PROT_READ | /* PROT_BTI */ 0x10;

                    unsafe {
                        if libc::mprotect(ptr as *mut libc::c_void, len, prot) < 0 {
                            return Err(ModuleError::Backend(
                                anyhow::Error::new(io::Error::last_os_error())
                                    .context("unable to make memory readable+executable"),
                            ));
                        }
                    }

                    return Ok(());
                }
            }

            unsafe {
                region::protect(ptr, len, region::Protection::READ_EXECUTE).map_err(|e| {
                    ModuleError::Backend(
                        anyhow::Error::new(e).context("unable to make memory readable+executable"),
                    )
                })?;
            }
            Ok(())
        };

        for &PtrLen { ptr, len, .. } in self.non_protected_allocations_iter() {
            set_region_readable_and_executable(ptr, len)?;
        }

        // Flush any in-flight instructions from the pipeline
        icache_coherence::pipeline_flush_mt().map_err(|error| {
            ModuleError::Backend(anyhow::anyhow!(
                "unable to flush JIT instruction pipeline: {error}"
            ))
        })?;

        self.already_protected = self.allocations.len();
        Ok(())
    }

    /// Set all memory allocated in this `Memory` up to now as readonly.
    pub(crate) fn set_readonly(&mut self) -> ModuleResult<()> {
        self.finish_current();

        for &PtrLen { ptr, len, .. } in self.non_protected_allocations_iter() {
            unsafe {
                region::protect(ptr, len, region::Protection::READ).map_err(|e| {
                    ModuleError::Backend(
                        anyhow::Error::new(e).context("unable to make memory readonly"),
                    )
                })?;
            }
        }

        self.already_protected = self.allocations.len();
        Ok(())
    }

    /// Iterates non protected memory allocations that are of not zero bytes in size.
    fn non_protected_allocations_iter(&self) -> impl Iterator<Item = &PtrLen> {
        let iter = self.allocations[self.already_protected..].iter();

        return iter.filter(|&PtrLen { len, .. }| *len != 0);
    }

    /// Frees all allocated memory regions that would be leaked otherwise.
    /// Likely to invalidate existing function pointers, causing unsafety.
    pub(crate) unsafe fn free_memory(&mut self) {
        self.allocations.clear();
        self.already_protected = 0;
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        // leak memory to guarantee validity of function pointers
        mem::replace(&mut self.allocations, Vec::new())
            .into_iter()
            .for_each(mem::forget);
    }
}
