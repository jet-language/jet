// D-EXEC1: the C-ABI export surface of the one compiled runtime.
//
// The Jet-native backend (Compiler/JetBackend, contract in
// Docs/research/jet-backend-design-2026-10-01.md, "Calls into the compiled
// runtime") calls the runtime by symbol with the System V convention. Every
// export below is a carrier adapter over the Prelude function AOT already
// calls: no export decides meaning of its own (I9).
//
// Carrier rules, one machine word each:
// - `Int` is the exact-Int carrier word (inline, or a tagged big-integer
//   owner). Kernels that take native `i64` are adapted the way generated
//   Rust adapts them (`native_int_input` / `native_int_result`).
// - `String` is an owned `Box<String>` handle. `jet_rt_string_drop` and
//   `jet_rt_string_builder_finish` consume their argument; every other
//   String parameter is borrowed and stays owned by the caller.
// - `Bool` parameters are a word holding 0 or 1; `Bool` results are returned
//   in the low byte.
// - A cursor handle is an owned `Box<JetLoopRangeCursor>`.
//
// Generated code has no unwind tables, so no unwind may leave an export. A
// stop renders and exits the process exactly as the AOT entry boundary does.
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi {
    use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

    type JetCString = *mut String;

    /// Run one export body; a caught unwind ends the process through the AOT
    /// entry boundary's report carriers instead of entering generated frames.
    #[inline(always)]
    fn guard<T>(run: impl FnOnce() -> T) -> T {
        match catch_unwind(AssertUnwindSafe(run)) {
            Ok(value) => value,
            Err(payload) => stop(payload),
        }
    }

    #[cold]
    fn stop(payload: Box<dyn std::any::Any + Send>) -> ! {
        // The boundary renders a runtime report and exits; any other payload
        // is a host fault, which ends like a panic leaving an AOT `main` (the
        // panic hook has already printed it).
        let _ = catch_unwind(AssertUnwindSafe(move || {
            super::jet_runtime_boundary(move || -> () { resume_unwind(payload) })
        }));
        std::process::exit(101)
    }

    fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
        if len == 0 {
            return &[];
        }
        // SAFETY: the caller passes `len` initialized bytes at `ptr` (static
        // data emitted by the backend) that outlive this call.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }

    fn text(ptr: *const u8, len: usize) -> std::borrow::Cow<'static, str> {
        String::from_utf8_lossy(bytes(ptr, len))
    }

    fn handle(value: String) -> JetCString {
        Box::into_raw(Box::new(value))
    }

    fn view<'a>(value: JetCString) -> &'a str {
        // SAFETY: `value` is a live handle from `handle` that the caller owns
        // for the duration of this call.
        unsafe { (*value).as_str() }
    }

    fn builder<'a>(value: JetCString) -> &'a mut String {
        // SAFETY: `value` is a live builder from `jet_rt_string_builder_new`;
        // the caller owns it exclusively for the duration of this call.
        unsafe { &mut *value }
    }

    fn take(value: JetCString) -> String {
        // SAFETY: the caller transfers its owned handle from `handle`.
        *unsafe { Box::from_raw(value) }
    }

    fn line_of(line: i64) -> u32 {
        u32::try_from(line).unwrap_or(0)
    }

    /// `native_int_input`: an exact Int a native kernel receives as `i64`.
    fn native(value: i64) -> i64 {
        crate::jet_std::jet_int_to_i64(value).unwrap_or_else(|| {
            super::jet_arithmetic_stop("<core.prelude>", 0, "native Int argument exceeds host range")
        })
    }

    // Process entry.

    #[no_mangle]
    pub extern "C" fn jet_rt_main(entry: extern "C" fn()) -> i32 {
        guard(|| {
            super::jet_std_env_init();
            super::jet_runtime_boundary(|| entry());
            0
        })
    }

    // Memory for boxes owned by generated code.

    #[no_mangle]
    pub extern "C" fn jet_rt_alloc(size: usize, align: usize) -> *mut u8 {
        guard(|| {
            let layout = std::alloc::Layout::from_size_align(size.max(1), align)
                .unwrap_or_else(|_| super::jet_panic("<core.prelude>", 0, "invalid box layout"));
            // SAFETY: the layout has a non-zero size.
            let ptr = unsafe { std::alloc::alloc(layout) };
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            ptr
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_free(ptr: *mut u8, size: usize, align: usize) {
        guard(|| {
            if ptr.is_null() {
                return;
            }
            let layout = std::alloc::Layout::from_size_align(size.max(1), align)
                .unwrap_or_else(|_| super::jet_panic("<core.prelude>", 0, "invalid box layout"));
            // SAFETY: `ptr` came from `jet_rt_alloc` with this size and align.
            unsafe { std::alloc::dealloc(ptr, layout) }
        })
    }

    // Strings.

    #[no_mangle]
    pub extern "C" fn jet_rt_string_from_static(ptr: *const u8, len: usize) -> JetCString {
        guard(|| handle(text(ptr, len).into_owned()))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_string_clone(value: JetCString) -> JetCString {
        guard(|| handle(view(value).to_owned()))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_string_drop(value: JetCString) {
        guard(|| {
            if !value.is_null() {
                drop(take(value));
            }
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_string_eq(left: JetCString, right: JetCString) -> bool {
        guard(|| view(left) == view(right))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_string_compare(left: JetCString, right: JetCString) -> i64 {
        guard(|| match view(left).as_bytes().cmp(view(right).as_bytes()) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_char_len(value: JetCString) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(super::jet_char_len(unsafe { &*value })))
    }

    #[no_mangle]
    pub extern "C" fn jet_string_is_empty(value: JetCString) -> bool {
        guard(|| super::jet_string_is_empty(unsafe { &*value }))
    }

    #[no_mangle]
    pub extern "C" fn jet_string_contains(value: JetCString, needle: JetCString) -> bool {
        guard(|| super::jet_string_contains(view(value), view(needle)))
    }

    #[no_mangle]
    pub extern "C" fn jet_string_starts_with(value: JetCString, needle: JetCString) -> bool {
        guard(|| super::jet_string_starts_with(unsafe { &*value }, unsafe { &*needle }))
    }

    #[no_mangle]
    pub extern "C" fn jet_string_ends_with(value: JetCString, needle: JetCString) -> bool {
        guard(|| super::jet_string_ends_with(unsafe { &*value }, unsafe { &*needle }))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_string_builder_new() -> JetCString {
        guard(|| handle(String::new()))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_string_builder_push_static(target: JetCString, ptr: *const u8, len: usize) {
        guard(|| builder(target).push_str(&text(ptr, len)))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_string_builder_push(target: JetCString, value: JetCString) {
        guard(|| builder(target).push_str(view(value)))
    }

    /// The builder box becomes the String handle; no bytes are copied.
    #[no_mangle]
    pub extern "C" fn jet_rt_string_builder_finish(target: JetCString) -> JetCString {
        target
    }

    // Display text, as generated Rust renders `print` and interpolation.

    #[no_mangle]
    pub extern "C" fn jet_rt_int_to_string(value: i64) -> JetCString {
        guard(|| handle(crate::jet_std::jet_int_to_string(value)))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_i64_to_string(value: i64) -> JetCString {
        guard(|| handle(value.to_string()))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_u64_to_string(value: u64) -> JetCString {
        guard(|| handle(value.to_string()))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_char_to_string(value: i64) -> JetCString {
        guard(|| {
            let scalar = u32::try_from(value).ok().and_then(char::from_u32);
            handle(scalar.unwrap_or(char::REPLACEMENT_CHARACTER).to_string())
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_bool_to_string(value: u8) -> JetCString {
        guard(|| handle((value != 0).to_string()))
    }

    /// A Float arrives in xmm0 as the double itself.
    #[no_mangle]
    pub extern "C" fn jet_rt_float_to_string(value: f64) -> JetCString {
        guard(|| handle(super::jet_fmt_display(&value)))
    }

    // Stops raised by generated code itself.

    #[no_mangle]
    pub extern "C" fn jet_rt_panic_overflow() {
        guard(|| {
            super::jet_arithmetic_stop(
                "<core.prelude>",
                0,
                "This arithmetic overflows the value's type (the result is outside its range)",
            )
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_panic_division_by_zero() {
        guard(|| super::jet_fixed_error_stop(super::JetFixedArithmeticError::DivideZero, "<core.prelude>", 0))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_panic_list_bounds(len: i64, index: i64) {
        guard(|| super::jet_arithmetic_stop("<core.prelude>", 0, &super::jet_list_bounds_message(len, native(index))))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_unreachable(ptr: *const u8, len: usize) {
        let reason = text(ptr, len).into_owned();
        // Generated Rust spells MIR `Never` as `unreachable!(reason)`.
        guard(move || -> () { unreachable!("{}", reason) })
    }

    /// A Prelude `jet_panic("<core.prelude>", 0, message)` stop that generated
    /// code checks inline (e.g. an iterator loop stride, Collections.rs:1518).
    #[no_mangle]
    pub extern "C" fn jet_rt_panic_message(ptr: *const u8, len: usize) {
        let message = text(ptr, len).into_owned();
        guard(move || -> () { super::jet_panic("<core.prelude>", 0, &message) })
    }

    /// `#require_eq` String operand: Rust Debug quoting (`JetDebug for str`).
    #[no_mangle]
    pub extern "C" fn jet_rt_require_debug_string(value: JetCString) -> JetCString {
        guard(|| handle(super::JetDebug::jet_debug(view(value))))
    }

    /// `#require` / `#require_eq` / `#panic` stop (kind 0/1/2) over the Prelude
    /// functions generated Rust calls. `descriptor` is eleven read-only words:
    /// file ptr/len, line, function ptr/len, source line ptr/len, column,
    /// caret, then the pre-rendered context ptr/len that only the freestanding
    /// image reads. Kind 0 and 2 read `message`, kind 1 reads `left` and
    /// `right` (Debug text); `locals` may be null.
    #[no_mangle]
    pub extern "C" fn jet_rt_require_stop(
        kind: i64,
        descriptor: *const usize,
        message: JetCString,
        left: JetCString,
        right: JetCString,
        locals: JetCString,
    ) {
        guard(|| {
            // SAFETY: the caller passes eleven initialized read-only words that
            // outlive this call (static data emitted by the backend).
            let d = unsafe { std::slice::from_raw_parts(descriptor, 11) };
            let file = text(d[0] as *const u8, d[1]);
            let line = line_of(d[2] as i64);
            let function = text(d[3] as *const u8, d[4]);
            let source = text(d[5] as *const u8, d[6]);
            let column = line_of(d[7] as i64);
            let caret = line_of(d[8] as i64);
            let locals = if locals.is_null() { "" } else { view(locals) };
            match kind {
                0 => super::jet_require(false, view(message), &file, line, &function, &source, column, caret, locals),
                1 => super::jet_require_eq(false, view(left), view(right), &file, line, &function, &source, column, caret, locals),
                _ => super::jet_panic_rich(&file, line, &function, &source, column, caret, view(message), locals),
            }
        })
    }

    /// A fixed-width arithmetic stop (E3010) that generated code checks inline,
    /// at its Jet source location.
    #[no_mangle]
    pub extern "C" fn jet_rt_numeric_stop(file_ptr: *const u8, file_len: usize, line: i64, message_ptr: *const u8, message_len: usize) {
        guard(|| super::jet_arithmetic_stop(&text(file_ptr, file_len), line_of(line), &text(message_ptr, message_len)))
    }

    // Prelude routes: the C name is the last `::` segment of the MIR symbol.

    macro_rules! exact_int_binary {
        ($($name:ident),* $(,)?) => {$(
            #[no_mangle]
            pub extern "C" fn $name(left: i64, right: i64) -> i64 {
                guard(|| crate::jet_std::$name(left, right))
            }
        )*};
    }

    macro_rules! exact_int_located {
        ($($name:ident),* $(,)?) => {$(
            #[no_mangle]
            pub extern "C" fn $name(left: i64, right: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64 {
                guard(|| crate::jet_std::$name(left, right, &text(file_ptr, file_len), line_of(line)))
            }
        )*};
    }

    exact_int_binary!(jet_int_add, jet_int_sub, jet_int_mul, jet_int_bit_and, jet_int_bit_or, jet_int_bit_xor, jet_int_compare);
    exact_int_located!(jet_int_div, jet_int_rem, jet_int_floor_div, jet_int_mod, jet_int_pow, jet_int_shl, jet_int_shr, jet_int_div_euclid, jet_int_rem_euclid);

    #[no_mangle]
    pub extern "C" fn jet_int_owned_from_i64(value: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_owned_from_i64(value).into_raw())
    }

    #[no_mangle]
    pub extern "C" fn jet_int_owned_to_i64(value: i64) -> i64 {
        guard(|| native(value))
    }

    #[no_mangle]
    pub extern "C" fn jet_int_owned_to_f64(value: i64) -> f64 {
        guard(|| crate::jet_std::jet_int_to_f64(value))
    }

    #[no_mangle]
    pub extern "C" fn jet_term_write_stdout_line(value: JetCString, flush: u8) {
        // Generated Rust ignores the stream result of `print` the same way.
        guard(|| {
            let _ = super::jet_term_write_stdout_line(view(value), flush != 0);
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_loop_range_init(
        start: i64,
        end: i64,
        step: i64,
        has_step: u8,
        exclusive: u8,
    ) -> *mut super::JetLoopRangeCursor {
        guard(|| {
            let step = if has_step != 0 { native(step) } else { 1 };
            Box::into_raw(Box::new(super::jet_loop_range_init(
                native(start),
                native(end),
                step,
                has_step != 0,
                exclusive != 0,
            )))
        })
    }

    fn cursor<'a>(value: *mut super::JetLoopRangeCursor) -> &'a mut super::JetLoopRangeCursor {
        // SAFETY: `value` is a live cursor from `jet_loop_range_init` that the
        // caller owns for the duration of this call.
        unsafe { &mut *value }
    }

    #[no_mangle]
    pub extern "C" fn jet_loop_range_has_next(value: *mut super::JetLoopRangeCursor) -> bool {
        guard(|| super::jet_loop_range_has_next(cursor(value)))
    }

    #[no_mangle]
    pub extern "C" fn jet_loop_range_value(value: *mut super::JetLoopRangeCursor) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(super::jet_loop_range_value(cursor(value))))
    }

    #[no_mangle]
    pub extern "C" fn jet_loop_range_advance(value: *mut super::JetLoopRangeCursor) {
        guard(|| super::jet_loop_range_advance(cursor(value)))
    }

    #[no_mangle]
    pub extern "C" fn jet_loop_cursor_drop(value: *mut super::JetLoopRangeCursor) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned cursor.
            drop(unsafe { Box::from_raw(value) });
        }
    }
}
