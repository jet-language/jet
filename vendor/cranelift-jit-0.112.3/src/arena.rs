//! A bounded, shared x86-64 JIT arena. PC-relative GOT and PLT references are
//! signed 32-bit displacements, so all code and data owned by a module must
//! remain within one 2 GiB address window.

use std::io;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

pub(crate) const ARENA_SIZE: usize = 1usize << 31;
const CODE_SIZE: usize = 1536 * 1024 * 1024;
const READONLY_SIZE: usize = 256 * 1024 * 1024;
const WRITABLE_SIZE: usize = 256 * 1024 * 1024;
const MIN_CHUNK_SIZE: usize = 64 * 1024;

const _: () = assert!(CODE_SIZE + READONLY_SIZE + WRITABLE_SIZE == ARENA_SIZE);

#[derive(Clone, Copy)]
pub(crate) enum Section {
    Code,
    Readonly,
    Writable,
}

struct ArenaMap {
    ptr: *mut u8,
    len: usize,
}

// The mapping is immutable metadata; the arena only mutates its disjoint
// section offsets atomically and writes committed pages through returned
// pointers.
unsafe impl Send for ArenaMap {}
unsafe impl Sync for ArenaMap {}

impl Drop for ArenaMap {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::munmap(self.ptr.cast(), self.len);
        }
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};
            VirtualFree(self.ptr.cast(), 0, MEM_RELEASE);
        }
    }
}

pub(crate) struct Arena {
    mapping: OnceLock<Result<ArenaMap, String>>,
    code_used: AtomicUsize,
    readonly_used: AtomicUsize,
    writable_used: AtomicUsize,
}

impl Arena {
    pub(crate) fn new() -> Self {
        Self {
            mapping: OnceLock::new(),
            code_used: AtomicUsize::new(0),
            readonly_used: AtomicUsize::new(0),
            writable_used: AtomicUsize::new(0),
        }
    }

    /// Allocate a page-aligned protection block in the selected sub-arena.
    /// Blocks are kept separate so later finalization never writes into a
    /// page that has already been made executable or readonly.
    pub(crate) fn allocate(
        &self,
        section: Section,
        size: usize,
        alignment: usize,
    ) -> io::Result<(*mut u8, usize)> {
        let page_size = region::page::size();
        let block_size =
            checked_align_up(size.max(MIN_CHUNK_SIZE), page_size).ok_or_else(out_of_memory)?;
        let (used, section_start, section_size) = match section {
            Section::Code => (&self.code_used, 0, CODE_SIZE),
            Section::Readonly => (&self.readonly_used, CODE_SIZE, READONLY_SIZE),
            Section::Writable => (
                &self.writable_used,
                CODE_SIZE + READONLY_SIZE,
                WRITABLE_SIZE,
            ),
        };
        if block_size > section_size {
            return Err(out_of_memory());
        }

        let mapping = self
            .mapping
            .get_or_init(|| reserve_arena().map_err(|error| error.to_string()))
            .as_ref()
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error.clone()))?;
        debug_assert_eq!(mapping.len, ARENA_SIZE);
        let section_base = (mapping.ptr as usize)
            .checked_add(section_start)
            .ok_or_else(out_of_memory)?;
        let used_before = used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                reserve_extent(
                    current,
                    block_size,
                    alignment,
                    section_size,
                    section_base,
                )
                .map(|(_, end)| end)
            })
            .map_err(|_| out_of_memory())?;
        let (offset, _) =
            checked_reservation(used_before, block_size, alignment, section_size, section_base)?;
        let mapping_offset = section_start.checked_add(offset).ok_or_else(out_of_memory)?;
        let ptr = unsafe { mapping.ptr.add(mapping_offset) };
        commit_pages(ptr, block_size)?;
        Ok((ptr, block_size))
    }
}

fn out_of_memory() -> io::Error {
    io::Error::new(
        io::ErrorKind::OutOfMemory,
        "Cranelift x86-64 JIT arena exhausted",
    )
}

fn checked_align_up(value: usize, alignment: usize) -> Option<usize> {
    if alignment == 0 {
        return None;
    }
    let remainder = value % alignment;
    if remainder == 0 {
        Some(value)
    } else {
        value.checked_add(alignment - remainder)
    }
}

fn reserve_extent(
    used: usize,
    size: usize,
    alignment: usize,
    capacity: usize,
    base: usize,
) -> Option<(usize, usize)> {
    if alignment == 0 {
        return None;
    }
    let address = base.checked_add(used)?;
    let padding = (alignment - address % alignment) % alignment;
    let start = used.checked_add(padding)?;
    let end = start.checked_add(size)?;
    (end <= capacity).then_some((start, end))
}

fn checked_reservation(
    used: usize,
    size: usize,
    alignment: usize,
    capacity: usize,
    base: usize,
) -> io::Result<(usize, usize)> {
    reserve_extent(used, size, alignment, capacity, base).ok_or_else(out_of_memory)
}

fn reserve_arena() -> io::Result<ArenaMap> {
    #[cfg(unix)]
    {
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                ARENA_SIZE,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(ArenaMap {
            ptr: ptr.cast(),
            len: ARENA_SIZE,
        })
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Memory::{
            VirtualAlloc, MEM_RESERVE, PAGE_NOACCESS,
        };
        let ptr = unsafe {
            VirtualAlloc(
                ptr::null_mut(),
                ARENA_SIZE,
                MEM_RESERVE,
                PAGE_NOACCESS,
            )
        };
        if ptr.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(ArenaMap {
            ptr: ptr.cast(),
            len: ARENA_SIZE,
        })
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "x86-64 JIT arenas are unsupported on this operating system",
        ))
    }
}

fn commit_pages(ptr: *mut u8, len: usize) -> io::Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_COMMIT, PAGE_READWRITE};
        let committed = unsafe { VirtualAlloc(ptr.cast(), len, MEM_COMMIT, PAGE_READWRITE) };
        if committed.is_null() {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(unix)]
    if unsafe { libc::mprotect(ptr.cast(), len, libc::PROT_READ | libc::PROT_WRITE) } < 0 {
        return Err(io::Error::last_os_error());
    }
    #[cfg(not(any(unix, windows)))]
    let _ = (ptr, len);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        checked_reservation, reserve_extent, ARENA_SIZE, CODE_SIZE, READONLY_SIZE, WRITABLE_SIZE,
    };

    #[test]
    fn arena_subregions_cover_a_signed_rel32_window() {
        assert_eq!(CODE_SIZE + READONLY_SIZE + WRITABLE_SIZE, ARENA_SIZE);
        assert_eq!(ARENA_SIZE - 1, i32::MAX as usize);
        assert_eq!(reserve_extent(0, 4096, 4096, 4096, 0), Some((0, 4096)));
    }

    #[test]
    fn arena_exhaustion_returns_an_allocation_error() {
        let error = checked_reservation(0, 4096, 4096, 4095, 0).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::OutOfMemory);
        assert_eq!(reserve_extent(usize::MAX, 1, 4096, usize::MAX, 0), None);
        assert_eq!(reserve_extent(0, 4096, 8192, 8192, 4096), Some((4096, 8192)));
    }
}

