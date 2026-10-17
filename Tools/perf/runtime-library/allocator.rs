// Matched comparison arm only; never linked into a Jet-native runtime.
use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn jet_rt_alloc(size: i64, align: i64) -> *mut u8 {
    let layout = Layout::from_size_align(size as usize, align as usize).unwrap();
    let pointer = unsafe { alloc(layout) };
    if pointer.is_null() {
        handle_alloc_error(layout);
    }
    pointer
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn jet_rt_free(pointer: *mut u8, size: i64, align: i64) {
    let layout = Layout::from_size_align(size as usize, align as usize).unwrap();
    unsafe { dealloc(pointer, layout) };
}
