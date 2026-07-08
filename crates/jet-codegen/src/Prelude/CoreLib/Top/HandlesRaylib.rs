// ── View<T> (D-DYNARRAY1) ────────────────────────────────────────────────────
// `list.view(a..b)` is a zero-copy window: unlike every bridge type below,
// `View<T>` has no owning Rust struct here — it lowers straight to a plain
// borrowed slice `&[T]` (`Context::rust_type`'s `View` arm, crates/jet-codegen/
// src/Codegen/Context.rs), and its constructor/method helpers
// (`jet_view_new`/`jet_view_fold`/`jet_view_map`) live in Core.rs next to
// `jet_slice_vec`/`jet_list_fold` — the same bare (non-`jet_std::`-namespaced)
// family every other list method belongs to, since `.view(...)` dispatches
// through the ordinary list-method machinery, not the handle-type dispatch
// the structs below use. Ownership (the window cannot outlive its list) is
// proved by sema's E2305, not by a Rust lifetime parameter on a wrapper type.

// ── Streaming file handles (E2-M7, D-IO2) ────────────────────────────────────
// FileReader / FileWriter are RAII: Drop closes (and flushes) them
// on every exit path — including `?` early returns and panics.
struct JetFileReader {
    inner: std::io::BufReader<std::fs::File>,
    path: String,
}
struct JetFileWriter {
    inner: std::io::BufWriter<std::fs::File>,
    path: String,
}

// ── core.db connection handle (D-DBDRIVER1) ──────────────────────────────────
// The real SQLite connection lives in the FFI bridge crate's thread-local
// handle map (`rusqlite::Connection` can't cross into this always-compiled
// prelude — I6). `JetDbConnection` is a thin, `Copy` handle wrapper so
// `.query`/`.execute`/`.begin`/`.commit`/`.rollback`/`.close` dispatch by
// receiver TYPE (`DbConnection`), the same mechanism `FileReader`/`FileWriter`
// use, instead of exposing the bare `u64` to Jet code.
#[derive(Clone, Copy, Debug)]
struct JetDbConnection {
    handle: u64,
}

// ── core.plugin sandboxed WASM handle (D-DEP-WASM1=A / D-PLUGIN1=B, c81) ─────
// The real wasmtime `Store`/`Instance` live in the FFI bridge crate's
// thread-local handle map (wasmtime types can't cross into this
// always-compiled prelude — I6). `JetPlugin` is a thin, `Copy` handle wrapper,
// same shape as `JetDbConnection`, so `.call`/`.call_int` dispatch by receiver
// TYPE (`Plugin`) instead of exposing the bare `u64` to Jet code.
#[derive(Clone, Copy, Debug)]
struct JetPlugin {
    handle: u64,
}

// jet:raylib-begin
// -- core.raylib bridge (D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A) -----------------
// Display remains explicit: without JET_RAYLIB_DISPLAY=1 the bridge is a
// deterministic headless no-op. With the flag set, Jet dynamically loads the
// native raylib shared library and calls the real C API without adding a
// compile-time link requirement to every CI run.
#[derive(Clone, Debug)]
struct RaylibWindow {
    width: i64,
    height: i64,
    title: String,
    native: bool,
}

#[derive(Clone, Copy, Debug)]
struct RaylibColor {
    r: i64,
    g: i64,
    b: i64,
    a: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct JetRaylibCColor {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

type JetRaylibInitWindow = unsafe extern "C" fn(i32, i32, *const std::os::raw::c_char);
type JetRaylibWindowShouldClose = unsafe extern "C" fn() -> bool;
type JetRaylibBeginDrawing = unsafe extern "C" fn();
type JetRaylibClearBackground = unsafe extern "C" fn(JetRaylibCColor);
type JetRaylibDrawRectangle = unsafe extern "C" fn(i32, i32, i32, i32, JetRaylibCColor);
type JetRaylibDrawText =
    unsafe extern "C" fn(*const std::os::raw::c_char, i32, i32, i32, JetRaylibCColor);
type JetRaylibEndDrawing = unsafe extern "C" fn();
type JetRaylibCloseWindow = unsafe extern "C" fn();
type JetRaylibIsKeyDown = unsafe extern "C" fn(i32) -> bool;
type JetRaylibSetTargetFps = unsafe extern "C" fn(i32);

#[derive(Clone, Copy)]
struct JetRaylibApi {
    init_window: JetRaylibInitWindow,
    window_should_close: JetRaylibWindowShouldClose,
    begin_drawing: JetRaylibBeginDrawing,
    clear_background: JetRaylibClearBackground,
    draw_rectangle: JetRaylibDrawRectangle,
    draw_text: JetRaylibDrawText,
    end_drawing: JetRaylibEndDrawing,
    close_window: JetRaylibCloseWindow,
    is_key_down: JetRaylibIsKeyDown,
    set_target_fps: JetRaylibSetTargetFps,
}

static JET_RAYLIB_WINDOW_OPEN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn jet_raylib_display_enabled() -> bool {
    std::env::var("JET_RAYLIB_DISPLAY").as_deref() == Ok("1")
}

fn jet_raylib_clamp_u8(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

fn jet_raylib_c_color(color: &RaylibColor) -> JetRaylibCColor {
    JetRaylibCColor {
        r: jet_raylib_clamp_u8(color.r),
        g: jet_raylib_clamp_u8(color.g),
        b: jet_raylib_clamp_u8(color.b),
        a: jet_raylib_clamp_u8(color.a),
    }
}

fn jet_raylib_cstring(s: &String) -> std::ffi::CString {
    let filtered: Vec<u8> = s.as_bytes().iter().copied().filter(|b| *b != 0).collect();
    std::ffi::CString::new(filtered).unwrap_or_else(|_| std::ffi::CString::new("").unwrap())
}

#[cfg(unix)]
mod jet_raylib_dyn {
    use super::*;
    use std::os::raw::{c_char, c_int, c_void};
    use std::sync::OnceLock;

    #[cfg(target_os = "linux")]
    #[link(name = "dl")]
    unsafe extern "C" {}

    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    const RTLD_NOW: c_int = 2;
    static API: OnceLock<Option<JetRaylibApi>> = OnceLock::new();

    pub(super) fn api() -> Option<&'static JetRaylibApi> {
        API.get_or_init(load).as_ref()
    }

    fn load() -> Option<JetRaylibApi> {
        // SAFETY: the loader only reads process-global dynamic-linker state.
        let handle = unsafe {
            #[cfg(target_os = "macos")]
            {
                dlopen(b"libraylib.dylib\0".as_ptr().cast(), RTLD_NOW)
            }
            #[cfg(not(target_os = "macos"))]
            {
                let first = dlopen(b"libraylib.so\0".as_ptr().cast(), RTLD_NOW);
                if first.is_null() {
                    dlopen(b"libraylib.so.5\0".as_ptr().cast(), RTLD_NOW)
                } else {
                    first
                }
            }
        };
        if handle.is_null() {
            return None;
        }
        Some(JetRaylibApi {
            init_window: symbol(handle, b"InitWindow\0")?,
            window_should_close: symbol(handle, b"WindowShouldClose\0")?,
            begin_drawing: symbol(handle, b"BeginDrawing\0")?,
            clear_background: symbol(handle, b"ClearBackground\0")?,
            draw_rectangle: symbol(handle, b"DrawRectangle\0")?,
            draw_text: symbol(handle, b"DrawText\0")?,
            end_drawing: symbol(handle, b"EndDrawing\0")?,
            close_window: symbol(handle, b"CloseWindow\0")?,
            is_key_down: symbol(handle, b"IsKeyDown\0")?,
            set_target_fps: symbol(handle, b"SetTargetFPS\0")?,
        })
    }

    fn symbol<T: Copy>(handle: *mut c_void, name: &[u8]) -> Option<T> {
        // SAFETY: names are NUL-terminated raylib symbols and T matches each
        // requested C function signature at the call site above.
        let ptr = unsafe { dlsym(handle, name.as_ptr().cast()) };
        if ptr.is_null() {
            None
        } else {
            // SAFETY: C function pointers and data pointers have the platform ABI
            // representation used by dlsym on supported Unix targets.
            Some(unsafe { std::mem::transmute_copy(&ptr) })
        }
    }
}

#[cfg(unix)]
fn jet_raylib_api() -> Option<&'static JetRaylibApi> {
    jet_raylib_dyn::api()
}

#[cfg(not(unix))]
fn jet_raylib_api() -> Option<&'static JetRaylibApi> {
    None
}

fn jet_raylib_window_open(width: i64, height: i64, title: &String) -> RaylibWindow {
    let mut native = false;
    if jet_raylib_display_enabled() {
        if let Some(api) = jet_raylib_api() {
            let title_c = jet_raylib_cstring(title);
            // SAFETY: raylib is loaded, the title pointer is valid for the call,
            // and all C interaction is confined to this vetted bridge.
            unsafe { (api.init_window)(width as i32, height as i32, title_c.as_ptr()) };
            JET_RAYLIB_WINDOW_OPEN.store(true, std::sync::atomic::Ordering::SeqCst);
            native = true;
        }
    }
    RaylibWindow {
        width,
        height,
        title: title.clone(),
        native,
    }
}

fn jet_raylib_window_should_close(window: &RaylibWindow) -> bool {
    if window.native {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: the function pointer was loaded from raylib and takes no args.
            return unsafe { (api.window_should_close)() };
        }
    }
    true
}

fn jet_raylib_window_ready(window: &RaylibWindow) -> bool {
    window.native
}

fn jet_raylib_begin_drawing(window: &RaylibWindow) {
    if window.native {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: the raylib window was opened by this bridge.
            unsafe { (api.begin_drawing)() };
        }
    }
}

fn jet_raylib_clear_background(color: &RaylibColor) {
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: color is a repr(C) mirror of raylib Color.
            unsafe { (api.clear_background)(jet_raylib_c_color(color)) };
        }
    }
}

fn jet_raylib_draw_text(text: &String, x: i64, y: i64, size: i64, color: &RaylibColor) {
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            let text_c = jet_raylib_cstring(text);
            // SAFETY: the text pointer is valid for the call, color matches C ABI,
            // and raylib owns the active drawing context.
            unsafe {
                (api.draw_text)(
                    text_c.as_ptr(),
                    x as i32,
                    y as i32,
                    size as i32,
                    jet_raylib_c_color(color),
                )
            };
        }
    }
}

fn jet_raylib_draw_rectangle(x: i64, y: i64, width: i64, height: i64, color: &RaylibColor) {
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: color is a repr(C) mirror of raylib Color.
            unsafe {
                (api.draw_rectangle)(
                    x as i32,
                    y as i32,
                    width as i32,
                    height as i32,
                    jet_raylib_c_color(color),
                )
            };
        }
    }
}

fn jet_raylib_end_drawing() {
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: the raylib window/drawing context is bridge-owned.
            unsafe { (api.end_drawing)() };
        }
    }
}

fn jet_raylib_close_window(window: &RaylibWindow) {
    if window.native {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: the window was opened by this bridge.
            unsafe { (api.close_window)() };
            JET_RAYLIB_WINDOW_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

fn jet_raylib_color(r: i64, g: i64, b: i64, a: i64) -> RaylibColor {
    RaylibColor { r, g, b, a }
}

fn jet_raylib_key_code(name: &String) -> i32 {
    match name.as_str() {
        "Space" | "space" => 32,
        "Enter" | "enter" => 257,
        "Escape" | "escape" | "Esc" | "esc" => 256,
        "Right" | "right" => 262,
        "Left" | "left" => 263,
        "Down" | "down" => 264,
        "Up" | "up" => 265,
        "A" | "a" => 65,
        "D" | "d" => 68,
        "S" | "s" => 83,
        "W" | "w" => 87,
        _ => -1,
    }
}

fn jet_raylib_key_down(name: &String) -> bool {
    let key = jet_raylib_key_code(name);
    if key < 0 {
        return false;
    }
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: key code is a plain raylib KeyboardKey integer.
            return unsafe { (api.is_key_down)(key) };
        }
    }
    false
}

fn jet_raylib_set_target_fps(fps: i64) {
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            let fps = fps.clamp(1, 240) as i32;
            // SAFETY: the raylib window was opened by this bridge.
            unsafe { (api.set_target_fps)(fps) };
        }
    }
}
// jet:raylib-end

