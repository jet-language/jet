// D-EXEC1: calls the compiled runtime's C-ABI exports
// (crates/jet-codegen/src/Prelude/Core/CAbi.rs) the way the Jet-native
// backend's generated code does (Compiler/JetBackend Lower/Require.jet and
// Lower/Numeric.jet). tests/corelib_parts/runtime.rs links this file against
// the cached runtime rlib; argv[1] names the case.
extern crate jet_runtime;

/// The canonical C-layout UTF-8 carrier, independent of Rust String layout.
#[repr(C)]
struct TextCarrier { ptr: *mut u8, len: usize, cap: usize }
type Text = *mut TextCarrier;

extern "C" {
    fn jet_rt_main(entry: extern "C" fn()) -> i32;
    fn jet_rt_string_from_static(ptr: *const u8, len: usize) -> Text;
    fn jet_rt_string_drop(value: Text);
    fn jet_rt_alloc(size: usize, align: usize) -> *mut u8;
    fn jet_rt_string_clone(value: Text) -> Text;
    fn jet_rt_string_data(value: Text) -> *const u8;
    fn jet_rt_string_byte_len(value: Text) -> usize;
    fn jet_char_len(value: Text) -> i64;
    fn jet_string_is_empty(value: Text) -> bool;
    fn jet_rt_string_builder_new() -> Text;
    fn jet_rt_string_builder_push_static(value: Text, ptr: *const u8, len: usize);
    fn jet_rt_string_builder_push(value: Text, text: Text);
    fn jet_rt_string_builder_finish(value: Text) -> Text;
    fn jet_rt_int_to_string(value: i64) -> Text;
    fn jet_term_write_stdout_line(value: Text, flush: u8);
    #[cfg(jet_native_pack)]
    fn jet_rt_require_debug_string(value: Text) -> Text;
    fn jet_int_div_euclid(left: i64, right: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64;
    fn jet_int_rem_euclid(left: i64, right: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64;
    #[cfg(jet_native_pack)]
    fn jet_rt_numeric_stop(file_ptr: *const u8, file_len: usize, line: i64, message_ptr: *const u8, message_len: usize);
    #[cfg(jet_native_pack)]
    fn jet_rt_require_stop(kind: i64, descriptor: *const usize, message: Text, left: Text, right: Text, locals: Text);
}

const FILE: &str = "main.jet";
#[cfg(jet_native_pack)]
const FUNCTION: &str = "run";
#[cfg(jet_native_pack)]
const SOURCE: &str = "    #require_eq(total, 2)";

fn text(value: &str) -> Text {
    unsafe { jet_rt_string_from_static(value.as_ptr(), value.len()) }
}

fn print_owned(value: Text) {
    unsafe {
        jet_term_write_stdout_line(value, 1);
        jet_rt_string_drop(value);
    }
}

/// Require.jet's eleven-word static descriptor; the context words (the image's
/// pre-rendered box) are unused by the compiled runtime.
#[cfg(jet_native_pack)]
fn descriptor() -> [usize; 11] {
    [
        FILE.as_ptr() as usize, FILE.len(), 3,
        FUNCTION.as_ptr() as usize, FUNCTION.len(),
        SOURCE.as_ptr() as usize, SOURCE.len(),
        5, 23, 0, 0,
    ]
}

#[cfg(jet_native_pack)]
fn require_stop(kind: i64, message: Text, left: Text, right: Text, locals: Text) {
    let words = descriptor();
    unsafe { jet_rt_require_stop(kind, words.as_ptr(), message, left, right, locals) }
}

extern "C" fn values() {
    unsafe {
        print_owned(jet_rt_int_to_string(jet_int_div_euclid(-7, 2, FILE.as_ptr(), FILE.len(), 4)));
        print_owned(jet_rt_int_to_string(jet_int_rem_euclid(-7, 2, FILE.as_ptr(), FILE.len(), 4)));
        #[cfg(jet_native_pack)]
        {
            let raw = text("a\"b\n");
            print_owned(jet_rt_require_debug_string(raw));
            jet_rt_string_drop(raw);
        }
    }
}

/// Build a carrier the way a Jet runtime layer does, not through Rust String.
extern "C" fn text_carriers() {
    unsafe {
        assert_eq!(std::mem::size_of::<TextCarrier>(), 24);
        let input = "hé\0😀";
        let ptr = jet_rt_alloc(input.len(), 1);
        std::ptr::copy_nonoverlapping(input.as_ptr(), ptr, input.len());
        let carrier = jet_rt_alloc(24, 8) as Text;
        carrier.write(TextCarrier { ptr, len: input.len(), cap: input.len() });
        assert_eq!(jet_rt_string_data(carrier), ptr);
        assert_eq!(jet_rt_string_byte_len(carrier), input.len());
        assert_eq!(jet_char_len(carrier), 4);
        let clone = jet_rt_string_clone(carrier);
        jet_rt_string_drop(carrier);
        let bytes = std::slice::from_raw_parts((*clone).ptr, (*clone).len);
        assert_eq!(bytes, input.as_bytes());
        jet_rt_string_drop(clone);

        let empty = jet_rt_string_builder_new();
        assert_eq!(((*empty).ptr as usize, (*empty).len, (*empty).cap), (1, 0, 0));
        assert!(jet_string_is_empty(empty));
        let seed = input.repeat(2);
        jet_rt_string_builder_push_static(empty, seed.as_ptr(), seed.len());
        let first = (*empty).ptr;
        jet_rt_string_builder_push(empty, empty); // aliases and grows
        assert_ne!((*empty).ptr, first);
        assert_eq!((*empty).len, seed.len() * 2);
        let window = (*empty).ptr.add(input.len());
        jet_rt_string_builder_push_static(empty, window, input.len()); // aliases and grows
        let grown = (*empty).ptr;
        let window = (*empty).ptr.add(input.len());
        jet_rt_string_builder_push_static(empty, window, input.len()); // aliases, no growth
        assert_eq!((*empty).ptr, grown);
        let window = (*empty).ptr;
        let count = (*empty).len;
        jet_rt_string_builder_push_static(empty, window, count); // aliases and grows
        let finished = jet_rt_string_builder_finish(empty);
        assert_eq!(finished, empty);
        assert_eq!(std::slice::from_raw_parts((*finished).ptr, (*finished).len), input.repeat(12).as_bytes());
        jet_rt_string_drop(finished);
        jet_rt_string_drop(std::ptr::null_mut());
    }
}

extern "C" fn euclid_by_zero() {
    unsafe { jet_int_rem_euclid(1, 0, FILE.as_ptr(), FILE.len(), 6) };
}

#[cfg(jet_native_pack)]
extern "C" fn numeric_stop() {
    let message = "This addition overflows the value's type (the result is outside its range)";
    unsafe { jet_rt_numeric_stop(FILE.as_ptr(), FILE.len(), 7, message.as_ptr(), message.len()) }
}

#[cfg(jet_native_pack)]
extern "C" fn require() {
    require_stop(0, text("condition failed"), std::ptr::null_mut(), std::ptr::null_mut(), text("total = 1"));
}

#[cfg(jet_native_pack)]
extern "C" fn require_eq() {
    let null = std::ptr::null_mut();
    require_stop(1, null, text("1"), text("2"), null);
}

#[cfg(jet_native_pack)]
extern "C" fn panic() {
    let null = std::ptr::null_mut();
    require_stop(2, text("boom"), null, null, null);
}

// Minimal raw C-ABI caller for the allocator's cross-tier failure witness;
// this Rust fixture seam dies at cutover with the Rust reference tier.
extern "C" fn alloc_invalid_layout() {
    unsafe { jet_rt_alloc(1, 3) };
}

extern "C" fn alloc_failure() {
    // Aligning this valid layout stays below isize::MAX. It cannot be backed
    // by a userspace mapping, independently of overcommit or physical RAM.
    unsafe { jet_rt_alloc(isize::MAX as usize - 7, 8) };
}

fn main() {
    let case = std::env::args().nth(1).unwrap_or_default();
    let entry: extern "C" fn() = match case.as_str() {
        "alloc-invalid-layout" => alloc_invalid_layout,
        "alloc-failure" => alloc_failure,
        #[cfg(not(runtime_alloc_failure_only))]
        "values" => values,
        #[cfg(not(runtime_alloc_failure_only))]
        "text-carriers" => text_carriers,
        #[cfg(not(runtime_alloc_failure_only))]
        "euclid-by-zero" => euclid_by_zero,
        #[cfg(all(jet_native_pack, not(runtime_alloc_failure_only)))]
        "numeric-stop" => numeric_stop,
        #[cfg(all(jet_native_pack, not(runtime_alloc_failure_only)))]
        "require" => require,
        #[cfg(all(jet_native_pack, not(runtime_alloc_failure_only)))]
        "require-eq" => require_eq,
        #[cfg(all(jet_native_pack, not(runtime_alloc_failure_only)))]
        "panic" => panic,
        _ => {
            eprintln!("unknown case {case:?}");
            std::process::exit(2);
        }
    };
    std::process::exit(unsafe { jet_rt_main(entry) });
}
