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
    #![allow(non_snake_case)]
    use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

    pub(crate) type JetCString = *mut String;

    /// No Rust unwind may leave an export. Compiler workers publish a typed
    /// internal failure; ordinary calls keep the shared AOT stop boundary.
    #[inline(always)]
    pub(crate) fn guard<T>(run: impl FnOnce() -> T) -> T {
        match catch_unwind(AssertUnwindSafe(run)) {
            Ok(value) => value,
            Err(payload) if super::jet_native_comptime_active() => {
                let message = payload.downcast_ref::<String>().map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("native compiler runtime carried an unknown panic payload");
                super::jet_native_comptime_fail(4, message)
            }
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

    pub(crate) fn handle(value: String) -> JetCString {
        Box::into_raw(Box::new(value))
    }

    pub(crate) fn view<'a>(value: JetCString) -> &'a str {
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
    pub(crate) fn native(value: i64) -> i64 {
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

    // Installed by the isolated worker, never ordinary program entry.
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_begin(fuel: u64, max_depth: u64) -> bool {
        guard(|| super::jet_native_comptime_begin(fuel, max_depth))
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_step(owner: u64, start: u64, end: u64) { super::jet_native_comptime_step(owner, start, end); }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_work(units: u64) { super::jet_native_comptime_work(units); }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_enter(owner: u64, start: u64, end: u64) { super::jet_native_comptime_enter(owner, start, end); }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_leave() { super::jet_native_comptime_leave(); }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_end() { super::jet_native_comptime_end(); }

    // Entry error edge (MIRRust.rs entry_error_exit): a failing entry ends
    // the process with its report and exit status 1.

    /// Ends the process with the report of `error` (taken, from
    /// jet_rt_record_new_JetErr).
    #[no_mangle]
    pub extern "C" fn jet_rt_entry_error_exit_err(error: *mut super::JetErr) {
        let error = take_record(error);
        guard(move || -> () { super::jet_entry_error_exit_jet(error) })
    }

    /// Ends the process with the entry report of an error's Printable text.
    #[no_mangle]
    pub extern "C" fn jet_rt_entry_error_exit(text: JetCString) {
        let text = view(text).to_owned();
        guard(move || -> () { super::jet_entry_error_exit(text) })
    }

    // Core records (Compiler/JetBackend/Source/Lower/Records.jet): a
    // MIR-defined Core type crosses a route as a handle to the runtime's own
    // value. `jet_rt_record_tag_<R>` returns a payload enum's variant index;
    // a getter borrows the handle and returns an owned carrier (an Option
    // getter returns its tag and writes the payload through `some`); a
    // constructor borrows String handles and Option boxes of a String or Int
    // (null is None; the payload is the box's first word) and takes nested
    // record handles. A fieldless Core enum member crosses as its index.

    fn record<T>(value: T) -> *mut T {
        Box::into_raw(Box::new(value))
    }

    fn take_record<T>(value: *mut T) -> T {
        // SAFETY: the caller transfers its owned handle from `record`.
        *unsafe { Box::from_raw(value) }
    }

    fn option_text(boxed: *const JetCString) -> super::JetOutcome<String, super::JetAbsent> {
        // SAFETY: a non-null Option box holds a live String handle first.
        if boxed.is_null() { Err(super::JetAbsent) } else { Ok(view(unsafe { *boxed }).to_owned()) }
    }

    fn option_int(boxed: *const i64) -> super::JetOutcome<i64, super::JetAbsent> {
        // SAFETY: a non-null Option box holds an exact-Int word first.
        if boxed.is_null() { Err(super::JetAbsent) } else { Ok(native(unsafe { *boxed })) }
    }

    fn some_text(some: *mut JetCString, value: &super::JetOutcome<String, super::JetAbsent>) -> i64 {
        match value {
            // SAFETY: `some` is the caller's zeroed out slot.
            Ok(text) => { unsafe { some.write(handle(text.clone())) }; 1 }
            Err(_) => 0,
        }
    }

    fn record_index_stop() -> ! {
        super::jet_panic("<core.prelude>", 0, "record variant index out of range")
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_new_JetErr(message: JetCString, code: *const JetCString, cause: *mut super::JetErr) -> *mut super::JetErr {
        guard(|| {
            let cause = if cause.is_null() { Err(super::JetAbsent) } else { Ok(take_record(cause)) };
            record(super::jet_err(view(message).to_owned(), option_text(code), cause))
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_JetErr_message(error: *mut super::JetErr) -> JetCString {
        // SAFETY: `error` is a live handle the caller lends for this call.
        guard(|| handle(super::jet_err_message(unsafe { &*error })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_JetErr_code(error: *mut super::JetErr, some: *mut JetCString) -> i64 {
        // SAFETY: `error` is a live handle the caller lends for this call.
        guard(|| some_text(some, &super::jet_err_code(unsafe { &*error })))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_JetErr_cause(error: *mut super::JetErr, some: *mut *mut super::JetErr) -> i64 {
        // SAFETY: `error` is a live handle the caller lends; `some` is its zeroed out slot.
        guard(|| match super::jet_err_cause(unsafe { &*error }) {
            Ok(cause) => { unsafe { some.write(record(cause)) }; 1 }
            Err(_) => 0,
        })
    }

    fn io_operation(index: i64) -> crate::jet_std::IOOperation {
        use crate::jet_std::IOOperation::*;
        match index {
            0 => Read,
            1 => Write,
            2 => Flush,
            3 => Connect,
            4 => Accept,
            5 => Close,
            6 => Resolve,
            7 => Codec,
            _ => record_index_stop(),
        }
    }

    fn resource_limit(index: i64) -> crate::jet_std::ProcessResourceLimit {
        use crate::jet_std::ProcessResourceLimit::*;
        match index {
            0 => WallTime,
            1 => CpuTime,
            2 => Memory,
            3 => OpenFiles,
            4 => Output,
            _ => record_index_stop(),
        }
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_new_IOContext(operation: i64, resource: *const JetCString, os_code: *const i64, cause: *const JetCString) -> *mut crate::jet_std::IOContext {
        guard(|| {
            record(crate::jet_std::IOContext {
                operation: io_operation(operation),
                resource: option_text(resource),
                os_code: option_int(os_code),
                cause: option_text(cause),
            })
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_IOContext_operation(context: *mut crate::jet_std::IOContext) -> i64 {
        // SAFETY: `context` is a live handle the caller lends for this call.
        guard(|| unsafe { &*context }.operation as i64)
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_IOContext_resource(context: *mut crate::jet_std::IOContext, some: *mut JetCString) -> i64 {
        // SAFETY: `context` is a live handle the caller lends for this call.
        guard(|| some_text(some, &unsafe { &*context }.resource))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_IOContext_os_code(context: *mut crate::jet_std::IOContext, some: *mut i64) -> i64 {
        // SAFETY: `context` is a live handle the caller lends; `some` is its zeroed out slot.
        guard(|| match unsafe { &*context }.os_code {
            Ok(code) => { unsafe { some.write(crate::jet_std::jet_int_from_i64(code)) }; 1 }
            Err(_) => 0,
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_IOContext_cause(context: *mut crate::jet_std::IOContext, some: *mut JetCString) -> i64 {
        // SAFETY: `context` is a live handle the caller lends for this call.
        guard(|| some_text(some, &unsafe { &*context }.cause))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_tag_IOError(error: *mut crate::jet_std::IOError) -> i64 {
        use crate::jet_std::IOError::*;
        // SAFETY: `error` is a live handle the caller lends for this call.
        guard(|| match unsafe { &*error } {
            InvalidInput(_) => 0,
            NotFound(_) => 1,
            PermissionDenied(_) => 2,
            TimedOut(_) => 3,
            Cancelled(_) => 4,
            Closed(_) => 5,
            Protocol(_) => 6,
            Other(_) => 7,
            ResourceLimit(_) => 8,
        })
    }

    // The IOContext payload of each context variant: its getter (a clone)
    // and its constructor (taking the context handle).
    macro_rules! io_error_context_variants {
        ($($variant:ident: $get:ident, $new:ident;)*) => {$(
            #[no_mangle]
            pub extern "C" fn $get(error: *mut crate::jet_std::IOError) -> *mut crate::jet_std::IOContext {
                // SAFETY: `error` is a live handle the caller lends for this call.
                guard(|| match unsafe { &*error } {
                    crate::jet_std::IOError::$variant(context) => record(context.clone()),
                    _ => record_index_stop(),
                })
            }

            #[no_mangle]
            pub extern "C" fn $new(context: *mut crate::jet_std::IOContext) -> *mut crate::jet_std::IOError {
                guard(|| record(crate::jet_std::IOError::$variant(take_record(context))))
            }
        )*};
    }

    io_error_context_variants! {
        InvalidInput: jet_rt_record_get_IOError_InvalidInput, jet_rt_record_new_IOError_InvalidInput;
        NotFound: jet_rt_record_get_IOError_NotFound, jet_rt_record_new_IOError_NotFound;
        PermissionDenied: jet_rt_record_get_IOError_PermissionDenied, jet_rt_record_new_IOError_PermissionDenied;
        TimedOut: jet_rt_record_get_IOError_TimedOut, jet_rt_record_new_IOError_TimedOut;
        Cancelled: jet_rt_record_get_IOError_Cancelled, jet_rt_record_new_IOError_Cancelled;
        Closed: jet_rt_record_get_IOError_Closed, jet_rt_record_new_IOError_Closed;
        Protocol: jet_rt_record_get_IOError_Protocol, jet_rt_record_new_IOError_Protocol;
        Other: jet_rt_record_get_IOError_Other, jet_rt_record_new_IOError_Other;
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_IOError_ResourceLimit(error: *mut crate::jet_std::IOError) -> i64 {
        // SAFETY: `error` is a live handle the caller lends for this call.
        guard(|| match unsafe { &*error } {
            crate::jet_std::IOError::ResourceLimit(limit) => *limit as i64,
            _ => record_index_stop(),
        })
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_record_new_IOError_ResourceLimit(limit: i64) -> *mut crate::jet_std::IOError {
        guard(|| record(crate::jet_std::IOError::ResourceLimit(resource_limit(limit))))
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

    /// Borrow UTF-8 bytes for compiler/host marshaling. The pointer remains
    /// valid until the caller mutates or drops the owning String handle.
    #[no_mangle]
    pub extern "C" fn jet_rt_string_data(value: JetCString) -> *const u8 {
        guard(|| view(value).as_ptr())
    }

    /// Byte count paired with jet_rt_string_data, not Jet's character count.
    #[no_mangle]
    pub extern "C" fn jet_rt_string_byte_len(value: JetCString) -> usize {
        guard(|| view(value).len())
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

    /// A String slice by a range (Core.rs `JetSliceRange for String`): the
    /// receiver is borrowed, bounds are native, and out-of-range bounds stop at
    /// the Jet source location.
    #[no_mangle]
    pub extern "C" fn jet_rt_string_slice_range(
        value: JetCString,
        start: i64,
        end: i64,
        exclusive: u8,
        file_ptr: *const u8,
        file_len: usize,
        line: i64,
    ) -> JetCString {
        guard(|| {
            let sliced = super::jet_string_slice_value(view(value), start, end, exclusive != 0)
                .unwrap_or_else(|message| super::jet_panic(&text(file_ptr, file_len), line_of(line), &message));
            handle(sliced)
        })
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

    /// A Float's Debug text (`JetDebug for f64`, which differs from Display).
    #[no_mangle]
    pub extern "C" fn jet_rt_float_debug(value: f64) -> JetCString {
        guard(|| handle(super::JetDebug::jet_debug(&value)))
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

    /// The exclusive end of a List slice window (Core.rs
    /// jet_checked_range_bounds, action "slice"), stopping at the source
    /// location when the native bounds fall outside `len`.
    #[no_mangle]
    pub extern "C" fn jet_rt_slice_end(start: i64, end: i64, exclusive: u8, len: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64 {
        guard(|| {
            let range = super::JetRange { start, end, exclusive: exclusive != 0 };
            super::jet_checked_range_bounds(len, &range, "slice", &text(file_ptr, file_len), line_of(line)).end as i64
        })
    }

    /// The exclusive end of a View window (Core.rs jet_checked_view_window),
    /// stopping at the source location when the native bounds fall outside `len`.
    #[no_mangle]
    pub extern "C" fn jet_rt_view_end(start: i64, end: i64, exclusive: u8, len: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64 {
        guard(|| super::jet_checked_view_window(start, end, exclusive != 0, len, &text(file_ptr, file_len), line_of(line)).1)
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

    /// An exact Int converted to a fixed-width integer (`int_checked_fixed`,
    /// stopping at the source location when out of range). Every fixed width
    /// the native backend lowers is at most 64 bits, so the checked value is
    /// its word (two's complement for a U64 above I64.MAX).
    #[no_mangle]
    pub extern "C" fn jet_int_checked_fixed(value: i64, kind: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_checked_fixed(value, kind, &text(file_ptr, file_len), line_of(line)) as i64)
    }

    #[no_mangle]
    pub extern "C" fn jet_int_owned_from_i64(value: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_owned_from_i64(value).into_raw())
    }

    /// A copy of an exact Int carrier word (a big-integer owner gains a reference).
    #[no_mangle]
    pub extern "C" fn jet_rt_int_clone(value: i64) -> i64 {
        // SAFETY: `value` is a live exact-Int carrier word the caller owns.
        guard(|| unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(value) }.into_raw())
    }

    /// An exact Int constant beyond the inline word range, from its decimal
    /// text (MIR `BigInt` constants; the JetBackend builds them at use).
    #[no_mangle]
    pub extern "C" fn jet_rt_int_from_static(ptr: *const u8, len: usize) -> i64 {
        guard(|| crate::jet_std::jet_int_from_str(&text(ptr, len)).unwrap_or_else(|message| panic!("{message}")))
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
