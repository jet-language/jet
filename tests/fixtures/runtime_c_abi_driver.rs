// D-EXEC1: calls the compiled runtime's C-ABI exports
// (crates/jet-codegen/src/Prelude/Core/CAbi.rs) the way the Jet-native
// backend's generated code does (Compiler/JetBackend Lower/Require.jet and
// Lower/Numeric.jet). tests/corelib_parts/runtime.rs links this file against
// the cached runtime rlib; argv[1] names the case.
extern crate jet_runtime;

/// A `Box<String>` handle, owned or borrowed as each export documents.
type Text = *mut u8;

extern "C" {
    fn jet_rt_main(entry: extern "C" fn()) -> i32;
    fn jet_rt_string_from_static(ptr: *const u8, len: usize) -> Text;
    fn jet_rt_string_drop(value: Text);
    fn jet_rt_int_to_string(value: i64) -> Text;
    fn jet_term_write_stdout_line(value: Text, flush: u8);
    fn jet_rt_require_debug_string(value: Text) -> Text;
    fn jet_int_div_euclid(left: i64, right: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64;
    fn jet_int_rem_euclid(left: i64, right: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64;
    fn jet_rt_numeric_stop(file_ptr: *const u8, file_len: usize, line: i64, message_ptr: *const u8, message_len: usize);
    fn jet_rt_require_stop(kind: i64, descriptor: *const usize, message: Text, left: Text, right: Text, locals: Text);
}

const FILE: &str = "main.jet";
const FUNCTION: &str = "run";
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
fn descriptor() -> [usize; 11] {
    [
        FILE.as_ptr() as usize, FILE.len(), 3,
        FUNCTION.as_ptr() as usize, FUNCTION.len(),
        SOURCE.as_ptr() as usize, SOURCE.len(),
        5, 23, 0, 0,
    ]
}

fn require_stop(kind: i64, message: Text, left: Text, right: Text, locals: Text) {
    let words = descriptor();
    unsafe { jet_rt_require_stop(kind, words.as_ptr(), message, left, right, locals) }
}

extern "C" fn values() {
    unsafe {
        print_owned(jet_rt_int_to_string(jet_int_div_euclid(-7, 2, FILE.as_ptr(), FILE.len(), 4)));
        print_owned(jet_rt_int_to_string(jet_int_rem_euclid(-7, 2, FILE.as_ptr(), FILE.len(), 4)));
        let raw = text("a\"b\n");
        print_owned(jet_rt_require_debug_string(raw));
        jet_rt_string_drop(raw);
    }
}

extern "C" fn euclid_by_zero() {
    unsafe { jet_int_rem_euclid(1, 0, FILE.as_ptr(), FILE.len(), 6) };
}

extern "C" fn numeric_stop() {
    let message = "This addition overflows the value's type (the result is outside its range)";
    unsafe { jet_rt_numeric_stop(FILE.as_ptr(), FILE.len(), 7, message.as_ptr(), message.len()) }
}

extern "C" fn require() {
    require_stop(0, text("condition failed"), std::ptr::null_mut(), std::ptr::null_mut(), text("total = 1"));
}

extern "C" fn require_eq() {
    let null = std::ptr::null_mut();
    require_stop(1, null, text("1"), text("2"), null);
}

extern "C" fn panic() {
    let null = std::ptr::null_mut();
    require_stop(2, text("boom"), null, null, null);
}

fn main() {
    let case = std::env::args().nth(1).unwrap_or_default();
    let entry: extern "C" fn() = match case.as_str() {
        "values" => values,
        "euclid-by-zero" => euclid_by_zero,
        "numeric-stop" => numeric_stop,
        "require" => require,
        "require-eq" => require_eq,
        "panic" => panic,
        _ => {
            eprintln!("unknown case {case:?}");
            std::process::exit(2);
        }
    };
    std::process::exit(unsafe { jet_rt_main(entry) });
}
