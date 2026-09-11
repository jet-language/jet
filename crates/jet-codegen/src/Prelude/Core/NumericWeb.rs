// I9: Web only marshals numeric conversion operands and carriers.
// The conversion policy remains in NumericConversion.rs, CommonTypes.rs, and the
// Foundation unit-conversion kernel. The wasm32 exports below expose owned TLS
// buffers rather than accepting arbitrary host pointers.


#[cfg(target_arch = "wasm32")]
mod jet_numeric_web_bridge {
    use super::*;
    use std::cell::{Cell, RefCell};

    const TEXT_SLOT_COUNT: usize = 6;
    const FILE_SLOT: u32 = 0;
    const SCALE_NUM_SLOT: u32 = 1;
    const SCALE_DEN_SLOT: u32 = 2;
    const OFFSET_NUM_SLOT: u32 = 3;
    const OFFSET_DEN_SLOT: u32 = 4;
    const INT_SLOT: u32 = 5;
    const OK: i32 = 1;
    const ABSENT_OR_NONE: i32 = 0;
    const BRIDGE_ERROR: i32 = -1;

    thread_local! {
        static TEXT: RefCell<[Vec<u8>; TEXT_SLOT_COUNT]> =
            RefCell::new(std::array::from_fn(|_| Vec::new()));
        static ERROR: RefCell<Vec<u8>> = RefCell::new(Vec::new());
        static I128_RESULT: Cell<[u8; 16]> = Cell::new([0; 16]);
        static F64_RESULT: Cell<f64> = Cell::new(0.0);
        static F32_RESULT: Cell<f32> = Cell::new(0.0);
    }

    fn set_error(message: impl AsRef<str>) {
        ERROR.with(|cell| {
            let mut error = cell.borrow_mut();
            error.clear();
            error.extend_from_slice(message.as_ref().as_bytes());
        });
    }

    fn begin_result() {
        ERROR.with(|cell| cell.borrow_mut().clear());
        I128_RESULT.with(|cell| cell.set([0; 16]));
        F64_RESULT.with(|cell| cell.set(0.0));
        F32_RESULT.with(|cell| cell.set(0.0));
    }

    fn finish_result() {
        ERROR.with(|cell| {
            let mut error = cell.borrow_mut();
            error.clear();
            error.shrink_to_fit();
        });
    }

    fn store_i128(value: i128) {
        I128_RESULT.with(|cell| cell.set(value.to_le_bytes()));
    }

    fn result_ok_i128(value: i128) -> i32 {
        store_i128(value);
        OK
    }

    fn result_err(error: impl AsRef<str>) -> i32 {
        set_error(error);
        ABSENT_OR_NONE
    }

    fn bridge_error(error: impl AsRef<str>) -> i32 {
        set_error(error);
        BRIDGE_ERROR
    }

    fn text_slot(slot: u32) -> Result<usize, String> {
        let slot = usize::try_from(slot).map_err(|_| "invalid numeric Web text slot".to_string())?;
        (slot < TEXT_SLOT_COUNT)
            .then_some(slot)
            .ok_or_else(|| "invalid numeric Web text slot".to_string())
    }

    fn read_text(slot: u32, ptr: u32, len: u32, label: &str) -> Result<String, String> {
        let slot = text_slot(slot)?;
        let len = usize::try_from(len).map_err(|_| format!("numeric Web {label} length is invalid"))?;
        TEXT.with(|cell| {
            let slots = cell.borrow();
            let bytes = &slots[slot];
            if bytes.len() != len {
                return Err(format!("numeric Web {label} length does not match its allocation"));
            }
            if (len == 0 && ptr != 0)
                || (len != 0 && bytes.as_ptr() as usize != ptr as usize)
            {
                return Err(format!("numeric Web {label} pointer is not owned by the bridge"));
            }
            String::from_utf8(bytes.clone())
                .map_err(|_| format!("numeric Web {label} must be UTF-8"))
        })
    }
    fn read_file(ptr: u32, len: u32) -> Result<String, String> {
        read_text(FILE_SLOT, ptr, len, "source file")
    }

    fn read_int(ptr: u32, len: u32) -> Result<i64, String> {
        let value = read_text(INT_SLOT, ptr, len, "Int")?;
        jet_std::jet_int_from_str(&value)
            .map_err(|error| format!("numeric Web Int is invalid: {error}"))
    }

    fn rounding_mode(mode: i64) -> Result<UnitRoundingMode, String> {
        match mode {
            0 => Ok(UnitRoundingMode::TowardZero),
            1 => Ok(UnitRoundingMode::Floor),
            2 => Ok(UnitRoundingMode::Ceiling),
            3 => Ok(UnitRoundingMode::NearestEven),
            _ => Err("invalid unit rounding mode".to_string()),
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_text_alloc(slot: u32, length: u32) -> u32 {
        let Ok(slot) = text_slot(slot) else {
            return 0;
        };
        let Ok(length) = usize::try_from(length) else {
            return 0;
        };
        TEXT.with(|cell| {
            let mut slots = cell.borrow_mut();
            let bytes = &mut slots[slot];
            if !bytes.is_empty() {
                return 0;
            }
            if length == 0 {
                return 0;
            }
            bytes.resize(length, 0);
            bytes.as_mut_ptr() as usize as u32
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_text_free(slot: u32, ptr: u32) -> u32 {
        let Ok(slot) = text_slot(slot) else {
            return 0;
        };
        TEXT.with(|cell| {
            let mut slots = cell.borrow_mut();
            let bytes = &mut slots[slot];
            let owned = if bytes.is_empty() {
                ptr == 0
            } else {
                ptr as usize == bytes.as_ptr() as usize
            };
            if !owned {
                return 0;
            }
            bytes.clear();
            bytes.shrink_to_fit();
            1
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_result_clear() {
        finish_result();
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_i128_ptr() -> u32 {
        I128_RESULT.with(|cell| cell.as_ptr() as usize as u32)
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_i128_len() -> u32 {
        16
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_error_ptr() -> u32 {
        ERROR.with(|cell| cell.borrow().as_ptr() as usize as u32)
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_error_len() -> u32 {
        ERROR.with(|cell| cell.borrow().len() as u32)
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_f64_result() -> f64 {
        F64_RESULT.with(Cell::get)
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_f32_result() -> f32 {
        F32_RESULT.with(Cell::get)
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_float_to_int(value: f64, kind: i64) -> i32 {
        begin_result();
        match jet_numeric_float_to_int(value, kind) {
            Ok(value) => result_ok_i128(value),
            Err(error) => result_err(error),
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_float_narrow(value: f64) -> i32 {
        begin_result();
        match jet_numeric_float_narrow(value) {
            Ok(value) => {
                F32_RESULT.with(|cell| cell.set(value));
                OK
            }
            Err(error) => result_err(error),
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_bit_count(value: i64, operation: i64, width: i64) -> i64 {
        jet_numeric_bit_count(value, operation, width)
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_int_bit_count(
        value_ptr: u32,
        value_len: u32,
        operation: i64,
        width: i64,
    ) -> i32 {
        begin_result();
        let value = match read_int(value_ptr, value_len) {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        result_ok_i128(i128::from(jet_numeric_int_bit_count(value, operation, width)))
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_try_from_fixed(
        raw: u64,
        signed: u32,
        kind: i64,
    ) -> i32 {
        begin_result();
        match jet_numeric_try_from_fixed(raw, signed != 0, kind) {
            Ok(value) => result_ok_i128(value),
            Err(error) => result_err(error),
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_int_try_from(
        value_ptr: u32,
        value_len: u32,
        kind: i64,
    ) -> i32 {
        begin_result();
        let value = match read_int(value_ptr, value_len) {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        match jet_std::jet_int_try_from(value, kind) {
            Some(value) => result_ok_i128(value),
            None => ABSENT_OR_NONE,
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_int_try_from_checked(
        value_ptr: u32,
        value_len: u32,
        kind: i64,
    ) -> i32 {
        begin_result();
        let value = match read_int(value_ptr, value_len) {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        match jet_std::jet_int_try_from_checked(value, kind) {
            Ok(value) => result_ok_i128(value),
            Err(error) => result_err(error),
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_int_checked_fixed(
        value_ptr: u32,
        value_len: u32,
        kind: i64,
        file_ptr: u32,
        file_len: u32,
        line: u32,
    ) -> i32 {
        begin_result();
        let value = match read_int(value_ptr, value_len) {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        let file = match read_file(file_ptr, file_len) {
            Ok(file) => file,
            Err(error) => return bridge_error(error),
        };
        let value = jet_std::jet_int_checked_fixed(value, kind, &file, line);
        result_ok_i128(value)
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_checked_widen_at(
        raw: u64,
        signed: u32,
        target_f32: u32,
        file_ptr: u32,
        file_len: u32,
        line: u32,
    ) -> f64 {
        begin_result();
        let file = match read_file(file_ptr, file_len) {
            Ok(file) => file,
            Err(error) => {
                set_error(error);
                return 0.0;
            }
        };
        jet_numeric_checked_widen_at(raw, signed != 0, target_f32 != 0, &file, line)
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_int_checked_widen(
        value_ptr: u32,
        value_len: u32,
        target_f32: u32,
        file_ptr: u32,
        file_len: u32,
        line: u32,
    ) -> f64 {
        begin_result();
        let value = match read_int(value_ptr, value_len) {
            Ok(value) => value,
            Err(error) => {
                set_error(error);
                return 0.0;
            }
        };
        let file = match read_file(file_ptr, file_len) {
            Ok(file) => file,
            Err(error) => {
                set_error(error);
                return 0.0;
            }
        };
        jet_std::jet_int_checked_widen(value, target_f32 != 0, &file, line)
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_unit_conversion_exact(
        value: f64,
        scale_num_ptr: u32,
        scale_num_len: u32,
        scale_den_ptr: u32,
        scale_den_len: u32,
        offset_num_ptr: u32,
        offset_num_len: u32,
        offset_den_ptr: u32,
        offset_den_len: u32,
    ) -> i32 {
        begin_result();
        let scale_num = match read_text(SCALE_NUM_SLOT, scale_num_ptr, scale_num_len, "scale numerator") {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        let scale_den = match read_text(SCALE_DEN_SLOT, scale_den_ptr, scale_den_len, "scale denominator") {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        let offset_num = match read_text(OFFSET_NUM_SLOT, offset_num_ptr, offset_num_len, "offset numerator") {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        let offset_den = match read_text(OFFSET_DEN_SLOT, offset_den_ptr, offset_den_len, "offset denominator") {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        match jet_unit_conversion_exact(value, &scale_num, &scale_den, &offset_num, &offset_den) {
            Some(value) => {
                F64_RESULT.with(|cell| cell.set(value));
                OK
            }
            None => ABSENT_OR_NONE,
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_numeric_web_unit_conversion_rounded(
        value: f64,
        scale_num_ptr: u32,
        scale_num_len: u32,
        scale_den_ptr: u32,
        scale_den_len: u32,
        offset_num_ptr: u32,
        offset_num_len: u32,
        offset_den_ptr: u32,
        offset_den_len: u32,
        mode: i64,
        digits: i64,
    ) -> i32 {
        begin_result();
        let mode = match rounding_mode(mode) {
            Ok(mode) => mode,
            Err(error) => return bridge_error(error),
        };
        let scale_num = match read_text(SCALE_NUM_SLOT, scale_num_ptr, scale_num_len, "scale numerator") {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        let scale_den = match read_text(SCALE_DEN_SLOT, scale_den_ptr, scale_den_len, "scale denominator") {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        let offset_num = match read_text(OFFSET_NUM_SLOT, offset_num_ptr, offset_num_len, "offset numerator") {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        let offset_den = match read_text(OFFSET_DEN_SLOT, offset_den_ptr, offset_den_len, "offset denominator") {
            Ok(value) => value,
            Err(error) => return bridge_error(error),
        };
        match jet_unit_conversion_rounded(
            value,
            &scale_num,
            &scale_den,
            &offset_num,
            &offset_den,
            mode,
            digits,
        ) {
            Ok(value) => {
                F64_RESULT.with(|cell| cell.set(value));
                OK
            }
            Err(error) => result_err(error),
        }
    }
}
