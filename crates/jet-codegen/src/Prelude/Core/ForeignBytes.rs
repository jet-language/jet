// Audited private C byte carriers. Seed/verification only, dies at cutover.
// Native ownership is implemented by the Jet runtime String/bytes kernels.
pub fn jet_string_data(value: &String) -> u64 {
    value.as_ptr() as usize as u64
}

pub fn jet_string_byte_len(value: &String) -> i64 {
    value.len() as i64
}

fn jet_foreign_bytes_len(value: &i64) -> usize {
    // SAFETY: borrow the caller's live exact-Int word without taking ownership.
    let borrowed = std::mem::ManuallyDrop::new(unsafe {
        jet_foundation::Numeric::JetInt::from_raw_owned(*value)
    });
    let length = borrowed.to_native_i64()
        .expect("C byte carrier length exceeds the host range");
    usize::try_from(length).expect("C byte carrier length must be nonnegative")
}

pub fn jet_bytes_pack(value: &[u8]) -> u64 {
    jet_bytes_pack_iter(value.iter().copied())
}

pub fn jet_bytes_pack_iter(value: impl ExactSizeIterator<Item = u8>) -> u64 {
    let length = value.len();
    if length == 0 { return 1; }
    let layout = std::alloc::Layout::from_size_align(length, 1)
        .expect("C byte carrier layout exceeds the host range");
    // SAFETY: ownership passes to the audited caller until bytes_free.
    let pointer = unsafe { std::alloc::alloc(layout) };
    if pointer.is_null() { std::alloc::handle_alloc_error(layout); }
    for (index, byte) in value.enumerate() {
        // SAFETY: the checked byte iterator has exactly length elements.
        unsafe { pointer.add(index).write(byte) };
    }
    pointer as usize as u64
}

pub fn jet_bytes_free(pointer: u64, length: &i64) {
    let length = jet_foreign_bytes_len(length);
    if length == 0 { return; }
    let layout = std::alloc::Layout::from_size_align(length, 1)
        .expect("C byte carrier layout exceeds the host range");
    // SAFETY: the caller returns exactly one pack allocation, with its original
    // byte length, after all foreign uses finish.
    unsafe { std::alloc::dealloc(pointer as usize as *mut u8, layout) };
}

pub fn jet_bytes_unpack(pointer: u64, length: &i64) -> Vec<u8> {
    let length = jet_foreign_bytes_len(length);
    if length == 0 { return Vec::new(); }
    // SAFETY: the caller lends length readable bytes throughout this copy.
    // The owned result retains no foreign pointer.
    unsafe { std::slice::from_raw_parts(pointer as usize as *const u8, length).to_vec() }
}
