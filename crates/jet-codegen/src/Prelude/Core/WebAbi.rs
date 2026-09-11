// D-FAIL-EDGE1: preserve the Prelude error report across the Wasm host boundary.
thread_local! {
    static JET_WASM_ERROR: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

fn jet_wasm_store_error(error: &JetErr) {
    let report = jet_error_report(error);
    JET_WASM_ERROR.with(|slot| {
        *slot.borrow_mut() = Some(format!(
            "{{\"tag\":\"Err\",\"error\":{}}}",
            report.to_json(),
        ));
    });
}

fn jet_wasm_store_absent() {
    JET_WASM_ERROR.with(|slot| {
        *slot.borrow_mut() = Some("{\"tag\":\"Err\",\"error\":{\"tag\":\"Absent\",\"values\":[]}}".to_string());
    });
}

#[no_mangle]
pub extern "C" fn jet_wasm_error_len() -> u32 {
    JET_WASM_ERROR.with(|slot| slot.borrow().as_ref().map_or(0, |json| json.len() as u32))
}

#[no_mangle]
pub extern "C" fn jet_wasm_error_ptr() -> u32 {
    JET_WASM_ERROR.with(|slot| slot.borrow().as_ref().map_or(0, |json| json.as_ptr() as u32))
}

#[no_mangle]
pub extern "C" fn jet_wasm_error_status() -> i32 {
    JET_WASM_ERROR.with(|slot| i32::from(slot.borrow().is_some()))
}

#[no_mangle]
pub extern "C" fn jet_wasm_error_clear() {
    JET_WASM_ERROR.with(|slot| *slot.borrow_mut() = None);
}

// D-JSBIND1: only registered allocations may cross the Wasm ownership boundary.
// Scalar Int and integer collections keep exact decimal text on the existing JS rail.
const JET_ABI_STRING_KIND: u8 = 1;
const JET_ABI_LIST_I64_KIND: u8 = 2;
const JET_ABI_LIST_INT_KIND: u8 = 3;
const JET_ABI_LIST_STRING_KIND: u8 = 4;
const JET_ABI_MAP_STRING_INT_KIND: u8 = 5;

static JET_ABI_ALLOCATIONS: std::sync::Mutex<Vec<(u8, u32, u32)>> =
    std::sync::Mutex::new(Vec::new());

fn jet_abi_register(kind: u8, ptr: u32, byte_len: u32) {
    assert!(ptr != 0 && byte_len != 0, "invalid Wasm ABI allocation");
    let mut allocations = JET_ABI_ALLOCATIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(
        !allocations.iter().any(|&(known_kind, known_ptr, _)| known_kind == kind && known_ptr == ptr),
        "duplicate Wasm ABI allocation"
    );
    allocations.push((kind, ptr, byte_len));
}

fn jet_abi_take(kind: u8, ptr: u32, byte_len: u32) -> bool {
    let mut allocations = JET_ABI_ALLOCATIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(index) = allocations.iter().position(|&(known_kind, known_ptr, known_len)| {
        known_kind == kind && known_ptr == ptr && known_len == byte_len
    }) else {
        return false;
    };
    allocations.swap_remove(index);
    true
}

fn jet_abi_require(kind: u8, ptr: u32, byte_len: u32) {
    assert!(
        ptr != 0 && byte_len != 0 && jet_abi_take(kind, ptr, byte_len),
        "untrusted or already-consumed Wasm ABI allocation"
    );
}

fn jet_abi_string_ret(s: String) -> u64 {
    if s.is_empty() { return 0; }
    let boxed = s.into_bytes().into_boxed_slice();
    let len = boxed.len() as u32;
    let ptr = Box::into_raw(boxed) as *mut u8 as u32;
    jet_abi_register(JET_ABI_STRING_KIND, ptr, len);
    ((ptr as u64) << 32) | (len as u64)
}

fn jet_abi_string_arg(packed: u64) -> String {
    let ptr = (packed >> 32) as u32;
    let len = (packed & 0xffff_ffff) as u32;
    if len == 0 {
        assert_eq!(ptr, 0, "non-null empty Wasm string allocation");
        return String::new();
    }
    jet_abi_require(JET_ABI_STRING_KIND, ptr, len);
    unsafe {
        let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize));
        String::from_utf8(boxed.into_vec()).expect("JS TextEncoder UTF-8")
    }
}

#[no_mangle]
pub extern "C" fn jet_abi_string_alloc(len: u32) -> u32 {
    if len == 0 { return 0; }
    let boxed = vec![0u8; len as usize].into_boxed_slice();
    let ptr = Box::into_raw(boxed) as *mut u8 as u32;
    jet_abi_register(JET_ABI_STRING_KIND, ptr, len);
    ptr
}

#[no_mangle]
pub extern "C" fn jet_abi_string_free(ptr: u32, len: u32) {
    if ptr == 0 { assert_eq!(len, 0, "null Wasm string allocation with length"); return; }
    jet_abi_require(JET_ABI_STRING_KIND, ptr, len);
    unsafe {
        let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize));
    }
}


fn jet_abi_list_i64_ret(v: Vec<i64>) -> u64 {
    if v.is_empty() { return 0; }
    let boxed = v.into_boxed_slice();
    let len = boxed.len() as u32;
    let byte_len = len.checked_mul(std::mem::size_of::<i64>() as u32).expect("list-i64 byte length overflow");
    let ptr = Box::into_raw(boxed) as *mut i64 as u32;
    jet_abi_register(JET_ABI_LIST_I64_KIND, ptr, byte_len);
    ((ptr as u64) << 32) | (len as u64)
}

fn jet_abi_list_i64_arg(packed: u64) -> Vec<i64> {
    let ptr = (packed >> 32) as u32;
    let len = (packed & 0xffff_ffff) as u32;
    if len == 0 {
        assert_eq!(ptr, 0, "non-null empty Wasm list-i64 allocation");
        return Vec::new();
    }
    let byte_len = len.checked_mul(std::mem::size_of::<i64>() as u32).expect("list-i64 byte length overflow");
    jet_abi_require(JET_ABI_LIST_I64_KIND, ptr, byte_len);
    unsafe {
        let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut i64, len as usize));
        boxed.into_vec()
    }
}

#[no_mangle]
pub extern "C" fn jet_abi_list_i64_alloc(len: u32) -> u32 {
    if len == 0 { return 0; }
    let boxed = vec![0i64; len as usize].into_boxed_slice();
    let ptr = Box::into_raw(boxed) as *mut i64 as u32;
    let byte_len = len.checked_mul(std::mem::size_of::<i64>() as u32).expect("list-i64 byte length overflow");
    jet_abi_register(JET_ABI_LIST_I64_KIND, ptr, byte_len);
    ptr
}

#[no_mangle]
pub extern "C" fn jet_abi_list_i64_free(ptr: u32, len: u32) {
    if ptr == 0 { assert_eq!(len, 0, "null Wasm list-i64 allocation with length"); return; }
    let byte_len = len.checked_mul(std::mem::size_of::<i64>() as u32).expect("list-i64 byte length overflow");
    jet_abi_require(JET_ABI_LIST_I64_KIND, ptr, byte_len);
    unsafe {
        let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut i64, len as usize));
    }
}


fn jet_abi_list_int_ret(v: Vec<jet_foundation::Numeric::JetInt>) -> u64 {
    if v.is_empty() { return 0; }
    let mut buf = Vec::new();
    buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
    for value in v {
        let bytes = value.to_string().into_bytes();
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(&bytes);
    }
    let boxed = buf.into_boxed_slice();
    let len = boxed.len() as u32;
    let ptr = Box::into_raw(boxed) as *mut u8 as u32;
    jet_abi_register(JET_ABI_LIST_INT_KIND, ptr, len);
    ((ptr as u64) << 32) | len as u64
}

fn jet_abi_list_int_arg(packed: u64) -> Vec<jet_foundation::Numeric::JetInt> {
    let ptr = (packed >> 32) as u32;
    let len = (packed & 0xffff_ffff) as u32;
    if len == 0 {
        assert_eq!(ptr, 0, "non-null empty Wasm list-int allocation");
        return Vec::new();
    }
    jet_abi_require(JET_ABI_LIST_INT_KIND, ptr, len);
    unsafe {
        let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize));
        let bytes = boxed.into_vec();
        assert!(bytes.len() >= 4, "list-int header");
        let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        assert!(count <= (bytes.len() - 4) / 4, "list-int count");
        let mut offset = 4usize;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            assert!(offset + 4 <= bytes.len(), "list-int length");
            let end = offset + 4;
            let value_len = u32::from_le_bytes(bytes[offset..end].try_into().unwrap()) as usize;
            offset = end;
            let end = offset.checked_add(value_len).expect("list-int value overflow");
            assert!(end <= bytes.len(), "list-int value");
            let text = std::str::from_utf8(&bytes[offset..end]).expect("JS UTF-8");
            values.push(jet_std::jet_int_owned_parse(text).expect("JS exact integer"));
            offset = end;
        }
        assert_eq!(offset, bytes.len(), "list-int trailing bytes");
        values
    }
}

#[no_mangle]
pub extern "C" fn jet_abi_list_int_alloc(byte_len: u32) -> u32 {
    if byte_len == 0 { return 0; }
    let boxed = vec![0u8; byte_len as usize].into_boxed_slice();
    let ptr = Box::into_raw(boxed) as *mut u8 as u32;
    jet_abi_register(JET_ABI_LIST_INT_KIND, ptr, byte_len);
    ptr
}

#[no_mangle]
pub extern "C" fn jet_abi_list_int_free(ptr: u32, byte_len: u32) {
    if ptr == 0 { assert_eq!(byte_len, 0, "null Wasm list-int allocation with length"); return; }
    jet_abi_require(JET_ABI_LIST_INT_KIND, ptr, byte_len);
    unsafe { let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, byte_len as usize)); }
}


fn jet_abi_list_string_ret(v: Vec<String>) -> u64 {
    if v.is_empty() {
        return 0;
    }
    let mut buf: Vec<u8> = Vec::new();
    let count = v.len() as u32;
    buf.extend_from_slice(&count.to_le_bytes());
    for s in &v {
        let bytes = s.as_bytes();
        let len = bytes.len() as u32;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(bytes);
    }
    let boxed = buf.into_boxed_slice();
    let byte_len = boxed.len() as u32;
    let ptr = Box::into_raw(boxed) as *mut u8 as u32;
    jet_abi_register(JET_ABI_LIST_STRING_KIND, ptr, byte_len);
    ((ptr as u64) << 32) | (byte_len as u64)
}

fn jet_abi_list_string_arg(packed: u64) -> Vec<String> {
    let ptr = (packed >> 32) as u32;
    let byte_len = (packed & 0xffff_ffff) as u32;
    if byte_len == 0 {
        assert_eq!(ptr, 0, "non-null empty Wasm list-string allocation");
        return Vec::new();
    }
    jet_abi_require(JET_ABI_LIST_STRING_KIND, ptr, byte_len);
    unsafe {
        let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, byte_len as usize));
        let buf = boxed.into_vec();
        let mut i = 0usize;
        assert!(buf.len() >= 4, "list-string header");
        let count = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
        assert!(count <= (buf.len() - 4) / 4, "list-string count");
        i = 4;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            assert!(i + 4 <= buf.len(), "list-string len");
            let len = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
            i += 4;
            let end = i.checked_add(len).expect("list-string byte length overflow");
            assert!(end <= buf.len(), "list-string bytes");
            out.push(String::from_utf8(buf[i..end].to_vec()).expect("JS UTF-8"));
            i = end;
        }
        assert_eq!(i, buf.len(), "list-string trailing bytes");
        out
    }
}

#[no_mangle]
pub extern "C" fn jet_abi_list_string_alloc(byte_len: u32) -> u32 {
    if byte_len == 0 { return 0; }
    let boxed = vec![0u8; byte_len as usize].into_boxed_slice();
    let ptr = Box::into_raw(boxed) as *mut u8 as u32;
    jet_abi_register(JET_ABI_LIST_STRING_KIND, ptr, byte_len);
    ptr
}

#[no_mangle]
pub extern "C" fn jet_abi_list_string_free(ptr: u32, byte_len: u32) {
    if ptr == 0 { assert_eq!(byte_len, 0, "null Wasm list-string allocation with length"); return; }
    jet_abi_require(JET_ABI_LIST_STRING_KIND, ptr, byte_len);
    unsafe {
        let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, byte_len as usize));
    }
}


fn jet_abi_map_string_int_ret(m: JetMap<String, jet_foundation::Numeric::JetInt>) -> u64 {
    if m.is_empty() {
        return 0;
    }
    let mut buf: Vec<u8> = Vec::new();
    let count = m.len() as u32;
    buf.extend_from_slice(&count.to_le_bytes());
    for (k, v) in &*m {
        let bytes = k.as_bytes();
        let len = bytes.len() as u32;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(bytes);
        let value = v.to_string();
        let value_bytes = value.as_bytes();
        buf.extend_from_slice(&(value_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(value_bytes);
    }
    let boxed = buf.into_boxed_slice();
    let byte_len = boxed.len() as u32;
    let ptr = Box::into_raw(boxed) as *mut u8 as u32;
    jet_abi_register(JET_ABI_MAP_STRING_INT_KIND, ptr, byte_len);
    ((ptr as u64) << 32) | (byte_len as u64)
}

fn jet_abi_map_string_int_arg(packed: u64) -> JetMap<String, jet_foundation::Numeric::JetInt> {
    let ptr = (packed >> 32) as u32;
    let byte_len = (packed & 0xffff_ffff) as u32;
    if byte_len == 0 {
        assert_eq!(ptr, 0, "non-null empty Wasm map-string-int allocation");
        return JetMap::new();
    }
    jet_abi_require(JET_ABI_MAP_STRING_INT_KIND, ptr, byte_len);
    unsafe {
        let boxed = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, byte_len as usize));
        let buf = boxed.into_vec();
        let mut i = 0usize;
        assert!(buf.len() >= 4, "map-string-int header");
        let count = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
        assert!(count <= (buf.len() - 4) / 8, "map-string-int count");
        i = 4;
        let mut out = JetMap::new();
        for _ in 0..count {
            let len_end = i.checked_add(4).expect("map-string-int key len overflow");
            assert!(len_end <= buf.len(), "map-string-int key len");
            let len = u32::from_le_bytes(buf[i..len_end].try_into().unwrap()) as usize;
            i = len_end;
            let key_end = i.checked_add(len).expect("map-string-int key overflow");
            let value_len_end = key_end.checked_add(4).expect("map-string-int value length overflow");
            assert!(value_len_end <= buf.len(), "map-string-int value length");
            let value_len = u32::from_le_bytes(buf[key_end..value_len_end].try_into().unwrap()) as usize;
            let value_end = value_len_end.checked_add(value_len).expect("map-string-int value overflow");
            assert!(value_end <= buf.len(), "map-string-int entry");
            let key = String::from_utf8(buf[i..key_end].to_vec()).expect("JS UTF-8");
            let text = std::str::from_utf8(&buf[value_len_end..value_end]).expect("JS UTF-8");
            let val = jet_std::jet_int_owned_parse(text).expect("JS exact integer");
            i = value_end;
            out.insert(key, val);
        }
        assert_eq!(i, buf.len(), "map-string-int trailing bytes");
        out
    }
}

#[no_mangle]
pub extern "C" fn jet_abi_map_string_int_alloc(byte_len: u32) -> u32 {
    if byte_len == 0 { return 0; }
    let boxed = vec![0u8; byte_len as usize].into_boxed_slice();
    let ptr = Box::into_raw(boxed) as *mut u8 as u32;
    jet_abi_register(JET_ABI_MAP_STRING_INT_KIND, ptr, byte_len);
    ptr
}

#[no_mangle]
pub extern "C" fn jet_abi_map_string_int_free(ptr: u32, byte_len: u32) {
    if ptr == 0 { assert_eq!(byte_len, 0, "null Wasm map-string-int allocation with length"); return; }
    jet_abi_require(JET_ABI_MAP_STRING_INT_KIND, ptr, byte_len);
    unsafe {
        let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, byte_len as usize));
    }
}


fn jet_abi_int_ret(value: jet_foundation::Numeric::JetInt) -> u64 {
    jet_abi_string_ret(value.to_string())
}

fn jet_abi_int_arg(packed: u64) -> jet_foundation::Numeric::JetInt {
    jet_std::jet_int_owned_parse(&jet_abi_string_arg(packed)).expect("JS exact integer")
}
