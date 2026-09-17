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
// prelude — I6). `JetDbConnection` is a thin, move-only handle wrapper so
// `.query`/`.execute`/`.begin`/`.commit`/`.rollback`/`.close` dispatch by
// receiver TYPE (`DBConnection`), the same mechanism `FileReader`/`FileWriter`
// use, instead of exposing the bare `u64` to Jet code.
#[derive(Debug)]
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
// The FFI bridge keeps wasmtime-specific state out of generated Jet values. A
// generated invocation expands `jet_plugin_bridge!(bridge_crate)` after the
// cached core block, where it can convert the bridge's wire result into the
// typed handle expected by CoreCall lowering. The authority argument has
// already crossed the checked boundary as its canonical wire string.
macro_rules! jet_plugin_bridge {
    ($bridge:ident) => {
        fn jet_plugin_load(path: &String, authority_wire: &String) -> JetPlugin {
            let wire = $bridge::jet_plugin_load(path, authority_wire);
            match wire
                .strip_prefix("O:")
                .and_then(|value| value.parse::<u64>().ok())
            {
                Some(handle) if handle != 0 => JetPlugin { handle },
                _ => {
                    let message = wire.strip_prefix("E:").unwrap_or("plugin load failed");
                    jet_runtime_stop("E3001", "", 0, message)
                }
            }
        }
    };
}

// jet:raylib-begin
// -- core.game.raylib bridge (D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A) -----------------
// Display remains explicit: without JET_RAYLIB_DISPLAY=1 the bridge is a
// deterministic headless adapter. With the flag set, Jet dynamically loads
// the native raylib shared library and calls the real C API without adding a
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

/// D-GAME-LOOP1=A: typed sound handle (headless-safe; native play is best-effort).
#[derive(Clone, Debug)]
struct RaylibSound {
    path: String,
}

type RaylibAtlasRegion = JetRaylibAtlasRegion;

/// Typed atlas metadata. The optional native texture is loaded only when a
/// display is active and the atlas declares a texture asset.
#[derive(Clone, Debug)]
struct RaylibTextureAtlas {
    path: String,
    name: String,
    texture_path: Option<String>,
    regions: Vec<RaylibAtlasRegion>,
    native_texture: Option<JetRaylibCTexture2D>,
}

type RaylibSpriteDrawCall = JetRaylibSpriteDrawCall;

impl JetShow for RaylibTextureAtlas {
    fn jet_show(&self) -> String {
        format!("RaylibTextureAtlas({})", self.name)
    }
}
impl JetDebug for RaylibTextureAtlas {
    fn jet_debug(&self) -> String {
        self.jet_show()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct JetRaylibCTexture2D {
    id: i32,
    width: i32,
    height: i32,
    mipmaps: i32,
    format: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct JetRaylibCColor {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct JetRaylibCRectangle {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct JetRaylibCVector2 {
    x: f32,
    y: f32,
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
type JetRaylibIsGamepadButtonDown = unsafe extern "C" fn(i32, i32) -> bool;
type JetRaylibGetGamepadAxisMovement = unsafe extern "C" fn(i32, i32) -> f32;
type JetRaylibSetTargetFps = unsafe extern "C" fn(i32);
type JetRaylibLoadTexture =
    unsafe extern "C" fn(*const std::os::raw::c_char) -> JetRaylibCTexture2D;
type JetRaylibDrawTexturePro = unsafe extern "C" fn(
    JetRaylibCTexture2D,
    JetRaylibCRectangle,
    JetRaylibCRectangle,
    JetRaylibCVector2,
    f32,
    JetRaylibCColor,
);

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
    is_gamepad_button_down: Option<JetRaylibIsGamepadButtonDown>,
    get_gamepad_axis_movement: Option<JetRaylibGetGamepadAxisMovement>,
    set_target_fps: JetRaylibSetTargetFps,
    load_texture: Option<JetRaylibLoadTexture>,
    draw_texture_pro: Option<JetRaylibDrawTexturePro>,
}

static JET_RAYLIB_WINDOW_OPEN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static JET_RAYLIB_FRAME_INDEX: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);


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
fn jet_raylib_native_color(color: &RaylibColor) -> JetDevtoolsNativeColor {
    JetDevtoolsNativeColor::new(
        jet_raylib_clamp_u8(color.r),
        jet_raylib_clamp_u8(color.g),
        jet_raylib_clamp_u8(color.b),
        jet_raylib_clamp_u8(color.a),
    )
}

fn jet_raylib_draw_native_command(command: &JetDevtoolsNativeDrawCommand) {
    if !JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let Some(api) = jet_raylib_api() else {
        return;
    };
    match command {
        JetDevtoolsNativeDrawCommand::Rectangle {
            x,
            y,
            width,
            height,
            color,
        } => {
            // SAFETY: the raylib drawing context is active and the command
            // contains plain integer/color ABI values.
            unsafe {
                (api.draw_rectangle)(
                    *x,
                    *y,
                    *width,
                    *height,
                    JetRaylibCColor {
                        r: color.red,
                        g: color.green,
                        b: color.blue,
                        a: color.alpha,
                    },
                )
            };
        }
        JetDevtoolsNativeDrawCommand::Text {
            text,
            x,
            y,
            size,
            color,
        } => {
            let text_c = jet_raylib_cstring(text);
            // SAFETY: the text pointer is valid for the call and the command
            // color matches raylib's C ABI.
            unsafe {
                (api.draw_text)(
                    text_c.as_ptr(),
                    *x,
                    *y,
                    (*size).max(1),
                    JetRaylibCColor {
                        r: color.red,
                        g: color.green,
                        b: color.blue,
                        a: color.alpha,
                    },
                )
            };
        }
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
            is_gamepad_button_down: symbol(handle, b"IsGamepadButtonDown\0"),
            get_gamepad_axis_movement: symbol(handle, b"GetGamepadAxisMovement\0"),
            set_target_fps: symbol(handle, b"SetTargetFPS\0")?,
            load_texture: symbol(handle, b"LoadTexture\0"),
            draw_texture_pro: symbol(handle, b"DrawTexturePro\0"),
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


fn jet_raylib_native_texture(path: &String) -> Option<JetRaylibCTexture2D> {
    if !JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        return None;
    }
    let api = jet_raylib_api()?;
    let load_texture = api.load_texture?;
    let path_c = jet_raylib_cstring(path);
    // SAFETY: the path pointer is valid for the call and the function pointer
    // was loaded from the active raylib shared library.
    let texture = unsafe { load_texture(path_c.as_ptr()) };
    (texture.id > 0).then_some(texture)
}

fn jet_raylib_load_texture_atlas(path: &String) -> RaylibTextureAtlas {
    let spec = jet_raylib_load_texture_atlas_spec(path);
    let native_texture = spec
        .texture_path
        .as_ref()
        .and_then(jet_raylib_native_texture);
    RaylibTextureAtlas {
        path: spec.path,
        name: spec.name,
        texture_path: spec.texture_path,
        regions: spec.regions,
        native_texture,
    }
}

fn jet_raylib_draw_sprite(
    atlas: &RaylibTextureAtlas,
    region: &String,
    x: i64,
    y: i64,
) {
    let Some(draw_call) =
        jet_raylib_sprite_draw_call(&atlas.name, &atlas.regions, region, x, y)
    else {
        return;
    };
    let source_x = draw_call.source_x;
    let source_y = draw_call.source_y;
    let width = draw_call.width;
    let height = draw_call.height;
    jet_raylib_record_draw_call(draw_call);
    let Some(texture) = atlas.native_texture else {
        return;
    };
    let Some(draw_texture_pro) = jet_raylib_api().and_then(|api| api.draw_texture_pro) else {
        return;
    };
    let source = JetRaylibCRectangle {
        x: source_x as f32,
        y: source_y as f32,
        width: width as f32,
        height: height as f32,
    };
    let destination = JetRaylibCRectangle {
        x: x as f32,
        y: y as f32,
        width: width as f32,
        height: height as f32,
    };
    // SAFETY: texture and geometry are ABI mirrors, and raylib owns the
    // active drawing context established by begin_drawing.
    unsafe {
        draw_texture_pro(
            texture,
            source,
            destination,
            JetRaylibCVector2 { x: 0.0, y: 0.0 },
            0.0,
            JetRaylibCColor {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        );
    }
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
    if native {
        let native_width = width.clamp(1, i64::from(i32::MAX)) as u32;
        let native_height = height.clamp(1, i64::from(i32::MAX)) as u32;
        jet_devtools_native_window_open(native_width, native_height);
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
            let frame_index =
                JET_RAYLIB_FRAME_INDEX.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let width = window.width.clamp(1, i64::from(i32::MAX)) as u32;
            let height = window.height.clamp(1, i64::from(i32::MAX)) as u32;
            for command in jet_devtools_native_frame_begin(frame_index, width, height) {
                jet_raylib_draw_native_command(&command);
            }
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
            jet_devtools_native_draw(JetDevtoolsNativeDrawCommand::Text {
                text: text.clone(),
                x: x as i32,
                y: y as i32,
                size: size as i32,
                color: jet_raylib_native_color(color),
            });
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
            jet_devtools_native_draw(JetDevtoolsNativeDrawCommand::Rectangle {
                x: x as i32,
                y: y as i32,
                width: width as i32,
                height: height as i32,
                color: jet_raylib_native_color(color),
            });
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
            jet_devtools_native_frame_end();
        }
    }
}

fn jet_raylib_close_window(window: &RaylibWindow) {
    if window.native {
        if let Some(api) = jet_raylib_api() {
            // SAFETY: the window was opened by this bridge.
            unsafe { (api.close_window)() };
            JET_RAYLIB_WINDOW_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
            jet_devtools_native_window_close();
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
            let pressed = unsafe { (api.is_key_down)(key) };
            if jet_devtools_native_input(JetDevtoolsNativeInput::Key {
                code: name.clone(),
                pressed,
            }) {
                return false;
            }
            return pressed;
        }
    }
    false
}

fn jet_raylib_gamepad_button_code(name: &String) -> i32 {
    jet_raylib_button_code(name).unwrap_or(-1)
}

fn jet_raylib_gamepad_axis_code(name: &String) -> i32 {
    jet_raylib_axis_code(name).unwrap_or(-1)
}

fn jet_raylib_gamepad_down(gamepad: i64, button: &String) -> bool {
    let Ok(gamepad) = i32::try_from(gamepad) else {
        return false;
    };
    let button_code = jet_raylib_gamepad_button_code(button);
    if gamepad < 0 || button_code < 0 {
        return false;
    }
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            if let Some(is_gamepad_button_down) = api.is_gamepad_button_down {
                // SAFETY: both integers are validated raylib enum values.
                let pressed = unsafe { is_gamepad_button_down(gamepad, button_code) };
                if jet_devtools_native_input(JetDevtoolsNativeInput::Gamepad {
                    gamepad: i64::from(gamepad),
                    control: button.clone(),
                    pressed,
                }) {
                    return false;
                }
                return pressed;
            }
        }
    }
    false
}

fn jet_raylib_gamepad_axis(gamepad: i64, axis: &String) -> f64 {
    let Ok(gamepad) = i32::try_from(gamepad) else {
        return 0.0;
    };
    let axis = jet_raylib_gamepad_axis_code(axis);
    if gamepad < 0 || axis < 0 {
        return 0.0;
    }
    if JET_RAYLIB_WINDOW_OPEN.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(api) = jet_raylib_api() {
            if let Some(get_gamepad_axis_movement) = api.get_gamepad_axis_movement {
                // SAFETY: both integers are validated raylib enum values.
                return unsafe { get_gamepad_axis_movement(gamepad as i32, axis) as f64 };
            }
        }
    }
    0.0
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

fn jet_raylib_load_sound(path: &String) -> RaylibSound {
    RaylibSound { path: path.clone() }
}

/// Returns whether a play was accepted (always true for a non-empty path in headless).
fn jet_raylib_play_sound(sound: &RaylibSound) -> bool {
    !sound.path.is_empty()
}
// jet:raylib-end
