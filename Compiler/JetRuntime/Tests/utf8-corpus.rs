// Rust seed/verification oracle only; dies at cutover. The tested export and
// text ownership functions come exclusively from the fresh Jet runtime pack.
// Reference: Prelude/Core/StringBytes.rs jet_string_validate_utf8 is exactly
// std::str::from_utf8(bytes).map(|_| ()).map_err(|e| e.to_string()).
unsafe extern "C" {
    fn jet_rt_alloc_init();
    fn jet_string_validate_utf8(ptr: i64, len: i64, error_out: i64) -> i64;
    fn jet_rt_string_data(value: i64) -> i64;
    fn jet_rt_string_byte_len(value: i64) -> i64;
    fn jet_rt_string_drop(value: i64);
}

fn check(bytes: &[u8]) {
    let slots: Vec<u64> = bytes.iter().map(|&byte| u64::from(byte)).collect();
    let mut error = 0_i64;
    let tag = unsafe {
        jet_string_validate_utf8(slots.as_ptr() as i64, slots.len() as i64,
            (&mut error as *mut i64) as i64)
    };
    match std::str::from_utf8(bytes) {
        Ok(_) => {
            assert_eq!(tag, 1, "valid bytes {bytes:?}");
            assert_eq!(error, 0, "success wrote an error payload for {bytes:?}");
        }
        Err(expected) => {
            assert_eq!(tag, 0, "invalid bytes {bytes:?}");
            assert_ne!(error, 0, "missing error text for {bytes:?}");
            let actual = unsafe {
                let ptr = jet_rt_string_data(error) as *const u8;
                let len = jet_rt_string_byte_len(error) as usize;
                std::str::from_utf8(std::slice::from_raw_parts(ptr, len))
                    .expect("error text must be UTF-8").to_owned()
            };
            unsafe { jet_rt_string_drop(error) };
            assert_eq!(actual, expected.to_string(), "error bytes {bytes:?}");
        }
    }
}

fn main() {
    unsafe { jet_rt_alloc_init() };
    check(&[]);
    for first in 0..=255 {
        check(&[first]);
        for second in 0..=255 { check(&[first, second]); }
    }
    for bytes in [
        &[224, 160, 128][..], &[237, 159, 191], &[239, 191, 191],
        &[240, 144, 128, 128], &[244, 143, 191, 191],
        &[224, 159, 191], &[237, 160, 128], &[240, 143, 191, 191],
        &[244, 144, 128, 128], &[245, 128, 128, 128],
        &[224, 160], &[240, 144, 128],
        &[65, 194, 32], &[65, 224, 160, 32],
        &[65, 240, 144, 128, 32],
    ] {
        check(bytes);
        let mut prefixed = vec![65; 17];
        prefixed.extend_from_slice(bytes);
        check(&prefixed);
    }
    println!("UTF-8 Jet/native acceptance and exact errors: passed");
}
