// Native carrier adapters over the existing checked-text kernels. Literal
// handles are borrowed; holes are owned and consumed. SQL stays a bound query,
// Sh stays argv, and URL holes are applied by the URL parser's opaque-hole law.
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi_typed_text {
    use super::jet_c_abi::{guard, handle, JetCString};
    type SQL = crate::jet_std::SQL;
    type DBValue = crate::jet_std::DBValue;
    #[repr(C)]
    pub struct Literal { data: *const u8, len: usize }

    unsafe fn slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
        if len == 0 { &[] } else {
            // SAFETY: the native caller lends len initialized carrier slots.
            unsafe { std::slice::from_raw_parts(ptr, len) }
        }
    }
    unsafe fn literals<'a>(ptr: *const Literal, len: usize) -> Vec<&'a str> {
        // SAFETY: lowering supplies valid UTF-8 literal bytes live for the call.
        unsafe { slice(ptr, len) }.iter().map(|value| unsafe { std::str::from_utf8_unchecked(slice(value.data, value.len)) }).collect()
    }
    unsafe fn take_texts(ptr: *const JetCString, len: usize) -> Vec<String> {
        // SAFETY: each hole is an owned, unique String box transferred by caller.
        unsafe { slice(ptr, len) }.iter().map(|value| *unsafe { Box::from_raw(*value) }).collect()
    }
    unsafe fn write_text_list(out: *mut usize, texts: Vec<String>) {
        let mut values: Vec<JetCString> = texts.into_iter().map(handle).collect();
        // SAFETY: caller provides three aligned words of exclusive list storage.
        unsafe { out.write(values.len()); out.add(1).write(values.capacity()); out.add(2).write(values.as_mut_ptr() as usize); }
        std::mem::forget(values);
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_typed_sh(lit: *const Literal, lit_len: usize, holes: *const JetCString, hole_len: usize, out: *mut usize) {
        guard(|| {
            let literals = unsafe { literals(lit, lit_len) };
            let argv = super::jet_typed_sh_interpolate(&literals, unsafe { take_texts(holes, hole_len) });
            unsafe { write_text_list(out, argv) };
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_typed_url(lit: *const Literal, lit_len: usize, holes: *const JetCString, hole_len: usize) -> *mut crate::jet_std::JetURL {
        guard(|| {
            let literals = unsafe { literals(lit, lit_len) };
            Box::into_raw(Box::new(crate::jet_std::jet_typed_url_literal(&literals, unsafe { take_texts(holes, hole_len) })))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_typed_sql(lit: *const Literal, lit_len: usize, holes: *const *mut DBValue, hole_len: usize) -> *mut SQL {
        guard(|| {
            let literals = unsafe { literals(lit, lit_len) };
            // SAFETY: the caller transfers each uniquely owned DBValue box.
            let holes = unsafe { slice(holes, hole_len) }.iter().map(|value| *unsafe { Box::from_raw(*value) }).collect();
            Box::into_raw(Box::new(super::jet_typed_sql_interpolate(&literals, holes)))
        })
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_typed_sql_drop(value: *mut SQL) {
        if !value.is_null() { guard(|| drop(unsafe { Box::from_raw(value) })) }
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_typed_sql_clone(value: *const SQL) -> *mut SQL {
        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_typed_sql_template(value: *const SQL) -> JetCString {
        guard(|| handle(super::jet_typed_sql_template(unsafe { &*value })))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_typed_sql_params(value: *const SQL, out: *mut usize) {
        guard(|| {
            let mut values: Vec<*mut DBValue> = super::jet_typed_sql_params(unsafe { &*value }).into_iter().map(|value| Box::into_raw(Box::new(value))).collect();
            // SAFETY: caller provides the native [len, cap, data] header.
            unsafe { out.write(values.len()); out.add(1).write(values.capacity()); out.add(2).write(values.as_mut_ptr() as usize); }
            std::mem::forget(values);
        })
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_typed_sql_int(value: i64) -> *mut DBValue {
        guard(|| Box::into_raw(Box::new(DBValue::Int(value))))
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_typed_sql_exact_int(value: i64) -> *mut DBValue {
        guard(|| Box::into_raw(Box::new(DBValue::Int(crate::jet_std::jet_int_to_i64(value).expect("SQL Int exceeds i64 wire range")))))
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_typed_sql_float(value: f64) -> *mut DBValue {
        guard(|| Box::into_raw(Box::new(DBValue::Float(value))))
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_typed_sql_bool(value: u8) -> *mut DBValue {
        guard(|| Box::into_raw(Box::new(DBValue::Bool(value != 0))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_typed_sql_text(value: JetCString) -> *mut DBValue {
        guard(|| Box::into_raw(Box::new(DBValue::Text(*unsafe { Box::from_raw(value) }))))
    }
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_typed_sql_blob(data: *const u64, len: usize) -> *mut DBValue {
        guard(|| Box::into_raw(Box::new(DBValue::Blob(unsafe { slice(data, len) }.iter().map(|value| *value as u8).collect()))))
    }
}
