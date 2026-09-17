// D-FOUND-PLATFORM1=A / card #2792: typed, capability-gated host services.
//
// This file is deliberately an isolated, backend-neutral kernel.  It performs
// no native calls and owns no process-global registration.  Native hosts can
// implement `JetUiHost`; the headless host below keeps the same value and
// capability vocabulary for deterministic tests.  Accessibility is an
// attachment/projection seam: the Ui.rs integration stores the value on the
// canonical `JetUiNode`, and adapters project that same node.  This module
// never builds a parallel accessibility tree.

const JET_UI_HOST_MAX_QUEUED_EVENTS: usize = 256;
const JET_FONT_SHAPING_CAPABILITY: &str = "UI.FontShaping";


/// One explicit right in the host-service capability set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JetUiCapability {
    FileDialog,
    Clipboard,
    Ime,
    DragDrop,
    Shortcuts,
    Accessibility,
}

impl JetUiCapability {
    pub const fn name(self) -> &'static str {
        match self {
            Self::FileDialog => "UI.FileDialog",
            Self::Clipboard => "UI.Clipboard",
            Self::Ime => "UI.Ime",
            Self::DragDrop => "UI.DragDrop",
            Self::Shortcuts => "UI.Shortcuts",
            Self::Accessibility => "UI.Accessibility",
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::FileDialog => 1 << 0,
            Self::Clipboard => 1 << 1,
            Self::Ime => 1 << 2,
            Self::DragDrop => 1 << 3,
            Self::Shortcuts => 1 << 4,
            Self::Accessibility => 1 << 5,
        }
    }

    const fn ordered() -> [Self; 6] {
        [
            Self::FileDialog,
            Self::Clipboard,
            Self::Ime,
            Self::DragDrop,
            Self::Shortcuts,
            Self::Accessibility,
        ]
    }
}

/// A capability fact is intentionally a value, not an ambient global.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JetUiCapabilityFact {
    pub capability: JetUiCapability,
    pub granted: bool,
}

/// The rights visible to one host instance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JetUiCapabilityFacts {
    bits: u8,
}

impl JetUiCapabilityFacts {
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn all() -> Self {
        Self { bits: (1 << 6) - 1 }
    }

    pub const fn with(self, capability: JetUiCapability) -> Self {
        Self {
            bits: self.bits | capability.bit(),
        }
    }

    pub fn grant(&mut self, capability: JetUiCapability) {
        self.bits |= capability.bit();
    }

    pub fn revoke(&mut self, capability: JetUiCapability) {
        self.bits &= !capability.bit();
    }

    pub const fn contains(self, capability: JetUiCapability) -> bool {
        self.bits & capability.bit() != 0
    }

    pub const fn fact(self, capability: JetUiCapability) -> JetUiCapabilityFact {
        JetUiCapabilityFact {
            capability,
            granted: self.contains(capability),
        }
    }

    /// Return facts in a stable service order for inspection and receipts.
    pub fn to_facts(self) -> Vec<JetUiCapabilityFact> {
        JetUiCapability::ordered()
            .into_iter()
            .map(|capability| self.fact(capability))
            .collect()
    }

    pub fn require(self, capability: JetUiCapability) -> Result<(), JetUiHostError> {
        if self.contains(capability) {
            Ok(())
        } else {
            Err(JetUiHostError::CapabilityDenied { capability })
        }
    }
}

/// Cancellation is a normal service outcome and is never represented as an
/// I/O or capability error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetUiCancellation {
    User,
    Closed,
    Headless,
    Superseded,
    Programmatic,
}

/// The canonical face name selects one checked bundled byte set. Other names
/// retain the generic default asset; hosts never inspect the system font set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetFontStyle {
    Body,
    Title,
    Monospace,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetFontFace {
    pub family: String,
    pub size: f64,
    pub style: JetFontStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetGlyphShaper {
    HarfBuzz,
    HeadlessFallback,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetGlyph {
    pub id: i64,
    pub cluster: i64,
    pub x: f64,
    pub y: f64,
    pub advance_x: f64,
    pub advance_y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetGlyphRun {
    pub glyphs: Vec<JetGlyph>,
    pub advance_x: f64,
    pub advance_y: f64,
    /// Provenance is part of the value so a fallback cannot look like
    /// HarfBuzz output to a caller or a receipt.
    pub shaper: JetGlyphShaper,
    /// Canonical bundled font bytes and the shared HarfBuzz version make
    /// native shaping deterministic across AOT, JIT, and interpreter tiers.
    pub deterministic: bool,
    pub approximate: bool,
}

/// Explicit headless fallback. It is never selected by `font.shape`; callers
/// must opt into this approximate, non-HarfBuzz value.
pub fn jet_font_shape_fallback(text: &str, face: &JetFontFace) -> JetGlyphRun {
    let mut glyphs = Vec::with_capacity(text.chars().count());
    let mut advance_x = 0.0;
    for (cluster, character) in text.char_indices() {
        let advance = if character == '\n' {
            0.0
        } else if character.is_ascii() {
            face.size * 0.6
        } else {
            face.size
        };
        glyphs.push(JetGlyph {
            id: character as u32 as i64,
            cluster: cluster as i64,
            x: advance_x,
            y: 0.0,
            advance_x: advance,
            advance_y: 0.0,
        });
        advance_x += advance;
    }
    JetGlyphRun {
        glyphs,
        advance_x,
        advance_y: 0.0,
        shaper: JetGlyphShaper::HeadlessFallback,
        deterministic: true,
        approximate: true,
    }
}
/// Native shaping uses the checked canonical font bytes supplied by the
/// generated Prelude. The dynamic seam keeps the core crate free of a Rust
/// HarfBuzz dependency while preserving one glyph result across all tiers.
// JET_VETTED_UNSAFE_BEGIN: jet_harfbuzz_unix
// AUDIT: this module is the narrow HarfBuzz C ABI bridge. Safe Rust cannot
// express the foreign function table, dlopen handle, or borrowed glyph arrays.
// The loader must keep its library open, calls must use the checked constructors
// and lengths, and drops must run in dependency order; violation can call an
// invalid symbol, read past a glyph array, or leak/double-free a native handle.

#[cfg(unix)]
mod jet_harfbuzz {
    use super::{JetFontFace, JetFontStyle, JetGlyph, JetGlyphRun, JetGlyphShaper};
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_int, c_uint, c_void};
    use std::sync::LazyLock;

    const RTLD_NOW: c_int = 2;
    const HB_MEMORY_MODE_READONLY: c_uint = 0;
    const MAX_GLYPHS: usize = 1_048_576;

    #[repr(C)]
    struct HbGlyphInfo {
        codepoint: c_uint,
        mask: c_uint,
        cluster: c_uint,
        var1: c_uint,
        var2: c_uint,
    }

    #[repr(C)]
    struct HbGlyphPosition {
        x_advance: c_int,
        y_advance: c_int,
        x_offset: c_int,
        y_offset: c_int,
        var: c_uint,
    }

    type BlobCreate = unsafe extern "C" fn(
        *const u8,
        c_uint,
        c_uint,
        *mut c_void,
        Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> *mut c_void;
    type BlobDestroy = unsafe extern "C" fn(*mut c_void);
    type FaceCreate = unsafe extern "C" fn(*mut c_void, c_uint) -> *mut c_void;
    type FaceDestroy = unsafe extern "C" fn(*mut c_void);
    type FaceGetGlyphCount = unsafe extern "C" fn(*mut c_void) -> c_uint;
    type FontCreate = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
    type FontDestroy = unsafe extern "C" fn(*mut c_void);
    type FontSetScale = unsafe extern "C" fn(*mut c_void, c_int, c_int);
    type OtFontSetFuncs = unsafe extern "C" fn(*mut c_void);
    type BufferCreate = unsafe extern "C" fn() -> *mut c_void;
    type BufferDestroy = unsafe extern "C" fn(*mut c_void);
    type BufferAddUtf8 = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, c_uint, c_int);
    type BufferGuessSegmentProperties = unsafe extern "C" fn(*mut c_void);
    type Shape = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, c_uint);
    type BufferGetLength = unsafe extern "C" fn(*mut c_void) -> c_uint;
    type BufferGetGlyphInfos =
        unsafe extern "C" fn(*mut c_void, *mut c_uint) -> *const HbGlyphInfo;
    type BufferGetGlyphPositions =
        unsafe extern "C" fn(*mut c_void, *mut c_uint) -> *const HbGlyphPosition;

    struct Api {
        _handle: *mut c_void,
        blob_create: BlobCreate,
        blob_destroy: BlobDestroy,
        face_create: FaceCreate,
        face_get_glyph_count: FaceGetGlyphCount,
        face_destroy: FaceDestroy,
        font_create: FontCreate,
        font_destroy: FontDestroy,
        font_set_scale: FontSetScale,
        ot_font_set_funcs: OtFontSetFuncs,
        buffer_create: BufferCreate,
        buffer_destroy: BufferDestroy,
        buffer_add_utf8: BufferAddUtf8,
        buffer_guess_segment_properties: BufferGuessSegmentProperties,
        shape: Shape,
        buffer_get_length: BufferGetLength,
        buffer_get_glyph_infos: BufferGetGlyphInfos,
        buffer_get_glyph_positions: BufferGetGlyphPositions,
    }

    // The handle and function pointers refer to one process-global, immutable
    // loader result. Keeping the library open is required for every pointer.
    unsafe impl Send for Api {}
    unsafe impl Sync for Api {}

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[link(name = "dl")]
    unsafe extern "C" {}

    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &'static CStr) -> Option<T> {
        let address = dlsym(handle, name.as_ptr());
        (!address.is_null()).then(|| std::mem::transmute_copy(&address))
    }

    fn load() -> Option<Api> {
        const LIBRARIES: [&[u8]; 4] = [
            b"libharfbuzz.so.0\0",
            b"libharfbuzz.so\0",
            b"libharfbuzz.0.dylib\0",
            b"libharfbuzz.dylib\0",
        ];
        for library in LIBRARIES {
            // SAFETY: the name is a static NUL-terminated string and the
            // returned handle remains open for the lifetime of the process.
            let handle = unsafe { dlopen(library.as_ptr().cast(), RTLD_NOW) };
            if handle.is_null() {
                continue;
            }
            macro_rules! load_symbol {
                ($name:literal) => {
                    unsafe { symbol(handle, CStr::from_bytes_with_nul_unchecked(concat!($name, "\0").as_bytes()))? }
                };
            }
            return Some(Api {
                _handle: handle,
                blob_create: load_symbol!("hb_blob_create"),
                blob_destroy: load_symbol!("hb_blob_destroy"),
                face_create: load_symbol!("hb_face_create"),
                face_get_glyph_count: load_symbol!("hb_face_get_glyph_count"),
                face_destroy: load_symbol!("hb_face_destroy"),
                font_create: load_symbol!("hb_font_create"),
                font_destroy: load_symbol!("hb_font_destroy"),
                font_set_scale: load_symbol!("hb_font_set_scale"),
                ot_font_set_funcs: load_symbol!("hb_ot_font_set_funcs"),
                buffer_create: load_symbol!("hb_buffer_create"),
                buffer_destroy: load_symbol!("hb_buffer_destroy"),
                buffer_add_utf8: load_symbol!("hb_buffer_add_utf8"),
                buffer_guess_segment_properties: load_symbol!("hb_buffer_guess_segment_properties"),
                shape: load_symbol!("hb_shape"),
                buffer_get_length: load_symbol!("hb_buffer_get_length"),
                buffer_get_glyph_infos: load_symbol!("hb_buffer_get_glyph_infos"),
                buffer_get_glyph_positions: load_symbol!("hb_buffer_get_glyph_positions"),
            });
        }
        None
    }

    fn api() -> Option<&'static Api> {
        static API: LazyLock<Option<Api>> = LazyLock::new(load);
        API.as_ref()
    }


    struct ShapeResources<'a> {
        api: &'a Api,
        blob: *mut c_void,
        face: *mut c_void,
        font: *mut c_void,
        buffer: *mut c_void,
    }

    impl Drop for ShapeResources<'_> {
        fn drop(&mut self) {
            // SAFETY: every pointer is created by the matching API and kept
            // alive until this guard drops in reverse dependency order.
            unsafe {
                (self.api.buffer_destroy)(self.buffer);
                (self.api.font_destroy)(self.font);
                (self.api.face_destroy)(self.face);
                (self.api.blob_destroy)(self.blob);
            }
        }
    }

    pub(super) fn shape(
        text: &str,
        face: &JetFontFace,
        font_bytes: &[u8],
    ) -> Option<JetGlyphRun> {
        if !face.size.is_finite()
            || face.size <= 0.0
            || text.len() > std::os::raw::c_int::MAX as usize
            || font_bytes.len() > c_uint::MAX as usize
        {
            return None;
        }
        let api = api()?;
        let text_len = c_uint::try_from(text.len()).ok()?;
        let font_len = c_uint::try_from(font_bytes.len()).ok()?;
        // HarfBuzz receives a borrowed, read-only blob. The bytes stay alive
        // through buffer shaping and all glyph-array reads below.
        let blob = unsafe {
            (api.blob_create)(
                font_bytes.as_ptr(),
                font_len,
                HB_MEMORY_MODE_READONLY,
                std::ptr::null_mut(),
                None,
            )
        };
        if blob.is_null() {
            return None;
        }
        let face_ptr = unsafe { (api.face_create)(blob, 0) };
        if face_ptr.is_null() {
            unsafe { (api.blob_destroy)(blob) };
            return None;
        }
        if unsafe { (api.face_get_glyph_count)(face_ptr) } == 0 {
            unsafe {
                (api.face_destroy)(face_ptr);
                (api.blob_destroy)(blob);
            }
            return None;
        }
        let font_ptr = unsafe { (api.font_create)(face_ptr) };
        if font_ptr.is_null() {
            unsafe {
                (api.face_destroy)(face_ptr);
                (api.blob_destroy)(blob);
            }
            return None;
        }
        let buffer = unsafe { (api.buffer_create)() };
        if buffer.is_null() {
            unsafe {
                (api.font_destroy)(font_ptr);
                (api.face_destroy)(face_ptr);
                (api.blob_destroy)(blob);
            }
            return None;
        }
        let resources = ShapeResources {
            api,
            blob,
            face: face_ptr,
            font: font_ptr,
            buffer,
        };
        let scale = (face.size.clamp(0.01, i32::MAX as f64 / 64.0) * 64.0).round() as c_int;
        unsafe {
            (api.ot_font_set_funcs)(resources.font);
            (api.font_set_scale)(resources.font, scale, scale);
            (api.buffer_add_utf8)(
                resources.buffer,
                text.as_ptr().cast(),
                text_len.min(c_int::MAX as c_uint) as c_int,
                0,
                -1,
            );
            (api.buffer_guess_segment_properties)(resources.buffer);
            (api.shape)(resources.font, resources.buffer, std::ptr::null(), 0);
        }

        let length = unsafe { (api.buffer_get_length)(resources.buffer) };
        let glyph_count = usize::try_from(length).ok()?;
        let max_for_input = text
            .len()
            .saturating_mul(16)
            .saturating_add(1024)
            .min(MAX_GLYPHS);
        if glyph_count > max_for_input {
            return None;
        }
        let mut info_length = length;
        let mut position_length = length;
        let (infos, positions) = unsafe {
            (
                (api.buffer_get_glyph_infos)(resources.buffer, &mut info_length),
                (api.buffer_get_glyph_positions)(resources.buffer, &mut position_length),
            )
        };
        if info_length != length
            || position_length != length
            || (length != 0 && (infos.is_null() || positions.is_null()))
        {
            return None;
        }

        let mut glyphs = Vec::with_capacity(glyph_count);
        let mut pen_x = 0.0;
        let mut pen_y = 0.0;
        for index in 0..glyph_count {
            // SAFETY: HarfBuzz returned arrays with exactly `length` entries,
            // and this loop is bounded by that checked length.
            let (info, position) = unsafe { (&*infos.add(index), &*positions.add(index)) };
            if usize::try_from(info.cluster).ok()? > text.len() {
                return None;
            }
            let x_offset = f64::from(position.x_offset) / 64.0;
            let y_offset = f64::from(position.y_offset) / 64.0;
            let x_advance = f64::from(position.x_advance) / 64.0;
            let y_advance = f64::from(position.y_advance) / 64.0;
            glyphs.push(JetGlyph {
                id: i64::from(info.codepoint),
                cluster: i64::from(info.cluster),
                x: pen_x + x_offset,
                y: pen_y + y_offset,
                advance_x: x_advance,
                advance_y: y_advance,
            });
            pen_x += x_advance;
            pen_y += y_advance;
        }
        drop(resources);
        Some(JetGlyphRun {
            glyphs,
            advance_x: pen_x,
            advance_y: pen_y,
            shaper: JetGlyphShaper::HarfBuzz,
            deterministic: true,
            approximate: false,
        })
    }
}
// JET_VETTED_UNSAFE_END: jet_harfbuzz_unix


// JET_VETTED_UNSAFE_BEGIN: jet_harfbuzz_wasm
// AUDIT: this module is the wasm host's checked HarfBuzz import table. Safe Rust
// cannot express the foreign ABI or host-owned pointers; lengths and handle
// lifetimes must match every imported call or the host can overread or release
// an invalid object.

#[cfg(target_arch = "wasm32")]
mod jet_harfbuzz {
    use super::{JetFontFace, JetGlyph, JetGlyphRun, JetGlyphShaper};
    use std::os::raw::{c_char, c_int, c_uint, c_void};

    const HB_MEMORY_MODE_READONLY: c_uint = 0;
    const MAX_GLYPHS: usize = 1_048_576;

    #[repr(C)]
    struct HbGlyphInfo {
        codepoint: c_uint,
        mask: c_uint,
        cluster: c_uint,
        var1: c_uint,
        var2: c_uint,
    }

    #[repr(C)]
    struct HbGlyphPosition {
        x_advance: c_int,
        y_advance: c_int,
        x_offset: c_int,
        y_offset: c_int,
        var: c_uint,
    }

    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        #[link_name = "jet_hb_blob_create"]
        fn blob_create(
            data: *const u8,
            length: c_uint,
            memory_mode: c_uint,
            user_data: *mut c_void,
            destroy: Option<unsafe extern "C" fn(*mut c_void)>,
        ) -> *mut c_void;
        #[link_name = "jet_hb_blob_destroy"]
        fn blob_destroy(blob: *mut c_void);
        #[link_name = "jet_hb_face_create"]
        fn face_create(blob: *mut c_void, index: c_uint) -> *mut c_void;
        #[link_name = "jet_hb_face_get_glyph_count"]
        fn face_get_glyph_count(face: *mut c_void) -> c_uint;
        #[link_name = "jet_hb_face_destroy"]
        fn face_destroy(face: *mut c_void);
        #[link_name = "jet_hb_font_create"]
        fn font_create(face: *mut c_void) -> *mut c_void;
        #[link_name = "jet_hb_font_destroy"]
        fn font_destroy(font: *mut c_void);
        #[link_name = "jet_hb_font_set_scale"]
        fn font_set_scale(font: *mut c_void, x_scale: c_int, y_scale: c_int);
        #[link_name = "jet_hb_ot_font_set_funcs"]
        fn ot_font_set_funcs(font: *mut c_void);
        #[link_name = "jet_hb_buffer_create"]
        fn buffer_create() -> *mut c_void;
        #[link_name = "jet_hb_buffer_destroy"]
        fn buffer_destroy(buffer: *mut c_void);
        #[link_name = "jet_hb_buffer_add_utf8"]
        fn buffer_add_utf8(
            buffer: *mut c_void,
            text: *const c_char,
            text_length: c_int,
            item_offset: c_uint,
            item_length: c_int,
        );
        #[link_name = "jet_hb_buffer_guess_segment_properties"]
        fn buffer_guess_segment_properties(buffer: *mut c_void);
        #[link_name = "jet_hb_shape"]
        fn hb_shape(
            font: *mut c_void,
            buffer: *mut c_void,
            features: *const c_void,
            feature_count: c_uint,
        );
        #[link_name = "jet_hb_buffer_get_length"]
        fn buffer_get_length(buffer: *mut c_void) -> c_uint;
        #[link_name = "jet_hb_buffer_get_glyph_infos"]
        fn buffer_get_glyph_infos(
            buffer: *mut c_void,
            length: *mut c_uint,
        ) -> *const HbGlyphInfo;
        #[link_name = "jet_hb_buffer_get_glyph_positions"]
        fn buffer_get_glyph_positions(
            buffer: *mut c_void,
            length: *mut c_uint,
        ) -> *const HbGlyphPosition;
    }

    struct ShapeResources {
        blob: *mut c_void,
        face: *mut c_void,
        font: *mut c_void,
        buffer: *mut c_void,
    }

    impl Drop for ShapeResources {
        fn drop(&mut self) {
            // SAFETY: every handle came from the matching imported constructor
            // and remains alive until this guard drops in dependency order.
            unsafe {
                buffer_destroy(self.buffer);
                font_destroy(self.font);
                face_destroy(self.face);
                blob_destroy(self.blob);
            }
        }
    }

    pub(super) fn shape(
        text: &str,
        face: &JetFontFace,
        font_bytes: &[u8],
    ) -> Option<JetGlyphRun> {
        if !face.size.is_finite()
            || face.size <= 0.0
            || text.len() > std::os::raw::c_int::MAX as usize
            || font_bytes.len() > c_uint::MAX as usize
        {
            return None;
        }
        let text_len = c_uint::try_from(text.len()).ok()?;
        let font_len = c_uint::try_from(font_bytes.len()).ok()?;
        // The imported blob constructor copies these bytes into its own
        // HarfBuzz memory; the app-memory slice remains alive for the call.
        let blob = unsafe {
            blob_create(
                font_bytes.as_ptr(),
                font_len,
                HB_MEMORY_MODE_READONLY,
                std::ptr::null_mut(),
                None,
            )
        };
        if blob.is_null() {
            return None;
        }
        let face_ptr = unsafe { face_create(blob, 0) };
        if face_ptr.is_null() {
            unsafe { blob_destroy(blob) };
            return None;
        }
        if unsafe { face_get_glyph_count(face_ptr) } == 0 {
            unsafe {
                face_destroy(face_ptr);
                blob_destroy(blob);
            }
            return None;
        }
        let font_ptr = unsafe { font_create(face_ptr) };
        if font_ptr.is_null() {
            unsafe {
                face_destroy(face_ptr);
                blob_destroy(blob);
            }
            return None;
        }
        let buffer = unsafe { buffer_create() };
        if buffer.is_null() {
            unsafe {
                font_destroy(font_ptr);
                face_destroy(face_ptr);
                blob_destroy(blob);
            }
            return None;
        }
        let resources = ShapeResources {
            blob,
            face: face_ptr,
            font: font_ptr,
            buffer,
        };
        let scale = (face.size.clamp(0.01, i32::MAX as f64 / 64.0) * 64.0).round() as c_int;
        unsafe {
            ot_font_set_funcs(resources.font);
            font_set_scale(resources.font, scale, scale);
            buffer_add_utf8(
                resources.buffer,
                text.as_ptr().cast(),
                text_len.min(c_int::MAX as c_uint) as c_int,
                0,
                -1,
            );
            buffer_guess_segment_properties(resources.buffer);
            hb_shape(resources.font, resources.buffer, std::ptr::null(), 0);
        }

        let length = unsafe { buffer_get_length(resources.buffer) };
        let glyph_count = usize::try_from(length).ok()?;
        let max_for_input = text
            .len()
            .saturating_mul(16)
            .saturating_add(1024)
            .min(MAX_GLYPHS);
        if glyph_count > max_for_input {
            return None;
        }
        let mut info_length = length;
        let mut position_length = length;
        let (infos, positions) = unsafe {
            (
                buffer_get_glyph_infos(resources.buffer, &mut info_length),
                buffer_get_glyph_positions(resources.buffer, &mut position_length),
            )
        };
        if info_length != length
            || position_length != length
            || (length != 0 && (infos.is_null() || positions.is_null()))
        {
            return None;
        }

        let mut glyphs = Vec::with_capacity(glyph_count);
        let mut pen_x = 0.0;
        let mut pen_y = 0.0;
        for index in 0..glyph_count {
            // SAFETY: the imported adapter copied exactly `length` records
            // into app memory before returning these pointers.
            let (info, position) = unsafe { (&*infos.add(index), &*positions.add(index)) };
            if usize::try_from(info.cluster).ok()? > text.len() {
                return None;
            }
            let x_offset = f64::from(position.x_offset) / 64.0;
            let y_offset = f64::from(position.y_offset) / 64.0;
            let x_advance = f64::from(position.x_advance) / 64.0;
            let y_advance = f64::from(position.y_advance) / 64.0;
            glyphs.push(JetGlyph {
                id: i64::from(info.codepoint),
                cluster: i64::from(info.cluster),
                x: pen_x + x_offset,
                y: pen_y + y_offset,
                advance_x: x_advance,
                advance_y: y_advance,
            });
            pen_x += x_advance;
            pen_y += y_advance;
        }
        drop(resources);
        Some(JetGlyphRun {
            glyphs,
            advance_x: pen_x,
            advance_y: pen_y,
            shaper: JetGlyphShaper::HarfBuzz,
            deterministic: true,
            approximate: false,
        })
    }
}
// JET_VETTED_UNSAFE_END: jet_harfbuzz_wasm

#[cfg(not(any(unix, target_arch = "wasm32")))]
mod jet_harfbuzz {
    pub(super) fn shape(
        _text: &str,
        _face: &JetFontFace,
        _font_bytes: &[u8],
    ) -> Option<JetGlyphRun> {
        None
    }
}

fn jet_font_bytes_are_vetted(bytes: &[u8]) -> bool {
    bytes.len() >= 4
        && matches!(
            &bytes[..4],
            b"\0\x01\0\0" | b"OTTO" | b"true" | b"typ1" | b"wOFF" | b"wOF2"
        )
}

const JET_FONT_FACE_ARABIC: &str = "Noto Sans Arabic";
const JET_FONT_FACE_SYMBOLS: &str = "Noto Sans Symbols 2";

fn jet_font_canonical_bytes(face: &JetFontFace) -> &'static [u8] {
    match face.family.as_str() {
        JET_FONT_FACE_ARABIC => JET_CANONICAL_ARABIC_FONT_BYTES,
        JET_FONT_FACE_SYMBOLS => JET_CANONICAL_SYMBOLS_FONT_BYTES,
        _ => JET_CANONICAL_FONT_BYTES,
    }
}

fn jet_font_vetted_bytes(face: &JetFontFace) -> Result<&'static [u8], JetUiHostError> {
    if !face.size.is_finite() || face.size <= 0.0 {
        return Err(JetUiHostError::InvalidRequest(
            "font size must be finite and positive".to_string(),
        ));
    }
    let bytes = jet_font_canonical_bytes(face);
    if !jet_font_bytes_are_vetted(bytes) {
        return Err(JetUiHostError::CapabilityUnavailable {
            capability: JET_FONT_SHAPING_CAPABILITY,
        });
    }
    Ok(bytes)
}

fn jet_font_bytes_match_face(face: &JetFontFace, bytes: &[u8]) -> bool {
    let canonical = jet_font_canonical_bytes(face);
    std::ptr::eq(bytes, canonical) || bytes == canonical
}

/// One shaping kernel is shared by native emit, resident JIT, and the
/// interpreter. Hosts provide only the already-vetted font bytes.
pub fn jet_font_shape_kernel(
    text: &str,
    face: &JetFontFace,
    font_bytes: &[u8],
) -> Result<JetGlyphRun, JetUiHostError> {
    if !face.size.is_finite() || face.size <= 0.0 {
        return Err(JetUiHostError::InvalidRequest(
            "font size must be finite and positive".to_string(),
        ));
    }
    if text.len() > std::os::raw::c_int::MAX as usize {
        return Err(JetUiHostError::InvalidRequest(
            "font text exceeds the supported input size".to_string(),
        ));
    }
    if !jet_font_bytes_are_vetted(font_bytes)
        || !jet_font_bytes_match_face(face, font_bytes)
    {
        return Err(JetUiHostError::CapabilityUnavailable {
            capability: JET_FONT_SHAPING_CAPABILITY,
        });
    }
    jet_harfbuzz::shape(text, face, font_bytes).ok_or(
        JetUiHostError::CapabilityUnavailable {
            capability: JET_FONT_SHAPING_CAPABILITY,
        },
    )
}

/// Native hosts do not silently fall back to character metrics. The explicit
/// fallback remains headless-only and is never a native shaping result.
pub fn jet_font_shape_native(
    text: &str,
    face: &JetFontFace,
    font_bytes: &[u8],
) -> Result<JetGlyphRun, JetUiHostError> {
    jet_font_shape_kernel(text, face, font_bytes)
}

fn jet_font_is_arabic(character: char) -> bool {
    let codepoint = character as u32;
    matches!(
        codepoint,
        0x0600..=0x06ff
            | 0x0750..=0x077f
            | 0x08a0..=0x08ff
            | 0xfb50..=0xfdff
            | 0xfe70..=0xfeff
            | 0x1ee00..=0x1eeff
    )
}

fn jet_font_is_symbol_or_emoji(character: char) -> bool {
    let codepoint = character as u32;
    matches!(
        codepoint,
        0x1f000..=0x1faff
            | 0x2300..=0x23ff
            | 0x2500..=0x27ff
            | 0x2b00..=0x2bff
    )
}

fn jet_font_is_extend(character: char) -> bool {
    let codepoint = character as u32;
    matches!(
        codepoint,
        0x0300..=0x036f
            | 0x1ab0..=0x1aff
            | 0x1dc0..=0x1dff
            | 0x20d0..=0x20ff
            | 0xfe00..=0xfe0f
            | 0xe0100..=0xe01ef
            | 0x1f3fb..=0x1f3ff
            | 0xe0020..=0xe007f
            | 0x200c..=0x200d
    )
}

/// Shape a string with deterministic bundled-font fallback.  The caller's
/// face remains the primary face; Arabic and symbol/emoji spans select the
/// checked companion face, while combining marks and emoji joiners stay with
/// their preceding span.  Each span is positioned into one logical run so
/// callers never observe a guessed metric or a second shaping contract.
pub fn jet_font_shape_with_fallback(
    text: &str,
    face: &JetFontFace,
) -> Result<JetGlyphRun, JetUiHostError> {
    if !face.size.is_finite() || face.size <= 0.0 {
        return Err(JetUiHostError::InvalidRequest(
            "font size must be finite and positive".to_string(),
        ));
    }
    if text.len() > std::os::raw::c_int::MAX as usize {
        return Err(JetUiHostError::InvalidRequest(
            "font text exceeds the supported input size".to_string(),
        ));
    }
    if face.family == JET_FONT_FACE_ARABIC || face.family == JET_FONT_FACE_SYMBOLS {
        return jet_font_shape_kernel(text, face, jet_font_vetted_bytes(face)?);
    }

    let family_for = |character: char| {
        if jet_font_is_arabic(character) {
            Some(JET_FONT_FACE_ARABIC)
        } else if jet_font_is_symbol_or_emoji(character) {
            Some(JET_FONT_FACE_SYMBOLS)
        } else {
            None
        }
    };
    let mut spans = Vec::new();
    let mut characters = text.char_indices();
    let Some((first_offset, first_character)) = characters.next() else {
        return jet_font_shape_kernel(text, face, jet_font_vetted_bytes(face)?);
    };
    let mut start = first_offset;
    let mut family = family_for(first_character);
    for (offset, character) in characters {
        let next_family = if jet_font_is_extend(character) {
            family
        } else {
            family_for(character)
        };
        if next_family != family {
            spans.push((start, offset, family));
            start = offset;
            family = next_family;
        }
    }
    spans.push((start, text.len(), family));

    let mut glyphs = Vec::new();
    let mut advance_x = 0.0;
    let mut advance_y = 0.0;
    for (start, end, family) in spans {
        let run_face = family.map_or_else(
            || face.clone(),
            |family| JetFontFace {
                family: family.to_string(),
                size: face.size,
                style: face.style,
            },
        );
        let bytes = jet_font_vetted_bytes(&run_face)?;
        let run = jet_font_shape_kernel(&text[start..end], &run_face, bytes)?;
        for mut glyph in run.glyphs {
            glyph.cluster += start as i64;
            glyph.x += advance_x;
            glyph.y += advance_y;
            glyphs.push(glyph);
        }
        advance_x += run.advance_x;
        advance_y += run.advance_y;
    }
    Ok(JetGlyphRun {
        glyphs,
        advance_x,
        advance_y,
        shaper: JetGlyphShaper::HarfBuzz,
        deterministic: true,
        approximate: false,
    })
}


pub fn jet_font_system(style: JetFontStyle) -> JetFontFace {
    let (family, size) = match style {
        JetFontStyle::Body => ("system-ui", 14.0),
        JetFontStyle::Title => ("system-ui", 24.0),
        JetFontStyle::Monospace => ("monospace", 14.0),
    };
    JetFontFace {
        family: family.to_string(),
        size,
        style,
    }
}

/// Every host operation uses the canonical two-branch outcome carrier. A
/// cancellation is a typed `UiHostError::Cancelled` payload, not a third
/// result branch, so `ResultIsOk`/`ResultValue` have one ABI everywhere.
pub type JetUiServiceResult<T> = Result<T, JetUiHostError>;

/// Typed host failure facts. Cancellation remains distinct from capability and
/// transport failures while sharing the canonical `Err` carrier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetUiHostError {
    Cancelled(JetUiCancellation),
    CapabilityUnavailable {
        capability: &'static str,
    },
    CapabilityDenied {
        capability: JetUiCapability,
    },
    InvalidRequest(String),
    HostFailure {
        service: &'static str,
        message: String,
    },
    ResourceDenied(String),
    ShortcutConflict {
        shortcut: JetUiShortcut,
        existing_action: String,
    },
    QueueFull {
        service: &'static str,
    },
}

/// A stable identity for one node in the canonical UI tree.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct JetUiNodeId(String);

impl JetUiNodeId {
    pub fn new(value: &str) -> Result<Self, JetUiHostError> {
        if value.is_empty() {
            return Err(JetUiHostError::InvalidRequest(
                "UI node identity must not be empty".to_string(),
            ));
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// UTF-8-independent text offsets used by clipboard and IME payloads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JetUiTextRange {
    pub start: usize,
    pub end: usize,
}

impl JetUiTextRange {
    pub fn new(start: usize, end: usize) -> Result<Self, JetUiHostError> {
        if start > end {
            return Err(JetUiHostError::InvalidRequest(
                "text range start must not exceed end".to_string(),
            ));
        }
        Ok(Self { start, end })
    }

    pub const fn len(self) -> usize {
        self.end - self.start
    }
}

/// The operation a resource-scoped file grant permits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JetUiFsAccess {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JetUiFsRights {
    bits: u8,
}

impl JetUiFsRights {
    pub const fn none() -> Self {
        Self { bits: 0 }
    }

    pub const fn read() -> Self {
        Self { bits: 1 }
    }

    pub const fn write() -> Self {
        Self { bits: 2 }
    }

    pub const fn read_write() -> Self {
        Self { bits: 3 }
    }

    pub const fn allows(self, access: JetUiFsAccess) -> bool {
        let bit = match access {
            JetUiFsAccess::Read => 1,
            JetUiFsAccess::Write => 2,
        };
        self.bits & bit != 0
    }

    pub const fn bits(self) -> u8 {
        self.bits
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self { bits: bits & 0b11 }
    }
}

/// A lexical, resource-scoped grant.  It deliberately does not perform file
/// I/O or resolve symlinks; a native adapter must enforce the same root at its
/// physical authorization boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiFsGrant {
    root: String,
    rights: JetUiFsRights,
}

impl JetUiFsGrant {
    pub fn new(root: &str, rights: JetUiFsRights) -> Result<Self, JetUiHostError> {
        let normalized = jet_ui_host_normalize_path(root)?;
        Ok(Self {
            root: jet_ui_host_path_text(&normalized),
            rights,
        })
    }

    pub fn root(&self) -> &str {
        self.root.as_str()
    }

    pub const fn rights(&self) -> JetUiFsRights {
        self.rights
    }

    pub const fn allows(&self, access: JetUiFsAccess) -> bool {
        self.rights.allows(access)
    }

    /// Scope an absolute path or a path relative to this grant's root.
    pub fn scope(
        &self,
        path: &str,
        access: JetUiFsAccess,
    ) -> Result<JetUiGrantedPath, JetUiHostError> {
        if !self.allows(access) {
            return Err(JetUiHostError::ResourceDenied(format!(
                "{} grant does not allow {:?}",
                self.root, access
            )));
        }

        let root = jet_ui_host_normalize_path(self.root.as_str())?;
        let requested = std::path::Path::new(path);
        let candidate = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            root.join(requested)
        };
        let candidate = jet_ui_host_normalize_path(candidate.to_string_lossy().as_ref())?;

        let within = if root.as_os_str().is_empty() {
            !candidate.is_absolute()
        } else {
            candidate == root || candidate.strip_prefix(&root).is_ok()
        };
        if !within {
            return Err(JetUiHostError::ResourceDenied(format!(
                "path `{}` escapes grant `{}`",
                path, self.root
            )));
        }

        Ok(JetUiGrantedPath {
            path: jet_ui_host_path_text(&candidate),
            grant_root: self.root.clone(),
            access,
        })
    }
}

/// A path that has already been checked against one `JetUiFsGrant`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiGrantedPath {
    path: String,
    grant_root: String,
    access: JetUiFsAccess,
}

impl JetUiGrantedPath {
    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    pub fn grant_root(&self) -> &str {
        self.grant_root.as_str()
    }

    pub const fn access(&self) -> JetUiFsAccess {
        self.access
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetUiFileDialogKind {
    Open,
    Save,
}

/// A typed filter keeps extension and media-type matching out of host code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiFileFilter {
    pub label: String,
    pub extensions: Vec<String>,
    pub mime_types: Vec<String>,
}

impl JetUiFileFilter {
    pub fn new(
        label: &str,
        extensions: Vec<String>,
        mime_types: Vec<String>,
    ) -> Result<Self, JetUiHostError> {
        if label.is_empty() && extensions.is_empty() && mime_types.is_empty() {
            return Err(JetUiHostError::InvalidRequest(
                "file filter must name an extension, media type, or label".to_string(),
            ));
        }
        if extensions.iter().any(|extension| extension.is_empty()) {
            return Err(JetUiHostError::InvalidRequest(
                "file filter extensions must not be empty".to_string(),
            ));
        }
        if mime_types.iter().any(|mime| mime.is_empty()) {
            return Err(JetUiHostError::InvalidRequest(
                "file filter media types must not be empty".to_string(),
            ));
        }
        Ok(Self {
            label: label.to_string(),
            extensions,
            mime_types,
        })
    }

    pub fn text() -> Self {
        Self {
            label: "Text".to_string(),
            extensions: vec!["txt".to_string()],
            mime_types: vec!["text/plain".to_string()],
        }
    }
}

/// A dialog request carries its resource grant rather than consulting an
/// ambient package or process permission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiFileDialogRequest {
    pub kind: JetUiFileDialogKind,
    pub title: String,
    pub grant: JetUiFsGrant,
    pub initial_directory: Option<JetUiGrantedPath>,
    pub filters: Vec<JetUiFileFilter>,
    pub allow_multiple: bool,
}

impl JetUiFileDialogRequest {
    pub fn open(grant: JetUiFsGrant) -> Self {
        Self {
            kind: JetUiFileDialogKind::Open,
            title: "Open File".to_string(),
            grant,
            initial_directory: None,
            filters: Vec::new(),
            allow_multiple: false,
        }
    }

    pub fn save(grant: JetUiFsGrant) -> Self {
        Self {
            kind: JetUiFileDialogKind::Save,
            title: "Save File".to_string(),
            grant,
            initial_directory: None,
            filters: Vec::new(),
            allow_multiple: false,
        }
    }

    pub fn validate(&self) -> Result<(), JetUiHostError> {
        let access = match self.kind {
            JetUiFileDialogKind::Open => JetUiFsAccess::Read,
            JetUiFileDialogKind::Save => JetUiFsAccess::Write,
        };
        if !self.grant.allows(access) {
            return Err(JetUiHostError::ResourceDenied(format!(
                "dialog {:?} requires {:?} access",
                self.kind, access
            )));
        }
        if let Some(directory) = &self.initial_directory {
            if directory.grant_root() != self.grant.root() || directory.access() != access {
                return Err(JetUiHostError::ResourceDenied(
                    "initial directory is outside the dialog grant".to_string(),
                ));
            }
        }
        if matches!(self.kind, JetUiFileDialogKind::Save) && self.allow_multiple {
            return Err(JetUiHostError::InvalidRequest(
                "save dialogs cannot select multiple files".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiFileDialogSelection {
    pub files: Vec<JetUiGrantedPath>,
}

impl JetUiFileDialogSelection {
    pub fn one(file: JetUiGrantedPath) -> Self {
        Self { files: vec![file] }
    }

    pub fn new(files: Vec<JetUiGrantedPath>) -> Result<Self, JetUiHostError> {
        if files.is_empty() {
            return Err(JetUiHostError::InvalidRequest(
                "a completed file dialog must select at least one file".to_string(),
            ));
        }
        Ok(Self { files })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiClipboardText {
    pub text: String,
    pub selection: Option<JetUiTextRange>,
}
/// Receipt for a successful clipboard write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JetUiClipboardWrite {
    pub characters: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetUiImeMode {
    Native,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetUiImePhase {
    Start,
    Update,
    Commit,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiImeComposition {
    pub text: String,
    pub selection: JetUiTextRange,
    pub marked: Option<JetUiTextRange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiImeEvent {
    pub target: JetUiNodeId,
    pub phase: JetUiImePhase,
    pub composition: Option<JetUiImeComposition>,
}

impl JetUiImeEvent {
    pub fn cancel(target: JetUiNodeId) -> Self {
        Self {
            target,
            phase: JetUiImePhase::Cancel,
            composition: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetUiDragOperation {
    Copy,
    Move,
    Link,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetUiDropItem {
    Text(String),
    Uri(String),
    File(JetUiGrantedPath),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetUiDragPhase {
    Enter,
    Over,
    Drop,
    Leave,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiDragEvent {
    pub target: JetUiNodeId,
    pub phase: JetUiDragPhase,
    pub operation: JetUiDragOperation,
    pub items: Vec<JetUiDropItem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetUiShortcutModifier {
    Control,
    Alt,
    Shift,
    Meta,
    Command,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JetUiShortcutModifiers {
    bits: u8,
}

impl JetUiShortcutModifiers {
    pub const fn none() -> Self {
        Self { bits: 0 }
    }
    /// The logical platform-command modifier. Native adapters normalize the
    /// physical Control key on Unix/Windows and Meta on macOS to this distinct
    /// bit; it is not the explicit `.meta` modifier.
    pub const fn command() -> Self {
        Self { bits: 1 << 4 }
    }

    pub const fn control() -> Self {
        Self { bits: 1 }
    }

    pub const fn alt() -> Self {
        Self { bits: 1 << 1 }
    }

    pub const fn shift() -> Self {
        Self { bits: 1 << 2 }
    }

    pub const fn with(self, modifier: JetUiShortcutModifier) -> Self {
        let bit = match modifier {
            JetUiShortcutModifier::Control => 1,
            JetUiShortcutModifier::Alt => 1 << 1,
            JetUiShortcutModifier::Shift => 1 << 2,
            JetUiShortcutModifier::Meta => 1 << 3,
            JetUiShortcutModifier::Command => 1 << 4,
        };
        Self {
            bits: self.bits | bit,
        }
    }

    pub const fn contains(self, modifier: JetUiShortcutModifier) -> bool {
        let bit = match modifier {
            JetUiShortcutModifier::Control => 1,
            JetUiShortcutModifier::Alt => 1 << 1,
            JetUiShortcutModifier::Shift => 1 << 2,
            JetUiShortcutModifier::Meta => 1 << 3,
            JetUiShortcutModifier::Command => 1 << 4,
        };
        self.bits & bit != 0
    }

    pub const fn bits(self) -> u8 {
        self.bits
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self { bits: bits & 0b1_1111 }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct JetUiShortcut {
    pub key: String,
    pub modifiers: JetUiShortcutModifiers,
}

impl JetUiShortcut {
    pub fn new(key: &str, modifiers: JetUiShortcutModifiers) -> Result<Self, JetUiHostError> {
        let key = key.trim().to_lowercase();
        if key.is_empty() {
            return Err(JetUiHostError::InvalidRequest(
                "shortcut key must not be empty".to_string(),
            ));
        }
        Ok(Self { key, modifiers })
    }

    /// D-FOUND-PLATFORM1=A: `.cmd("key")` is the checked contextual
    /// constructor. Sema admits only a non-empty literal, so this route cannot
    /// produce the dynamic host Result used by `core.ui.host.shortcut`.
    pub fn cmd(key: &str) -> Self {
        Self::new(key, JetUiShortcutModifiers::command())
            .unwrap_or_else(|_| panic!("checked .cmd literal must be valid"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiShortcutBinding {
    pub shortcut: JetUiShortcut,
    pub action: String,
    pub node: Option<JetUiNodeId>,
}

impl JetUiShortcutBinding {
    pub fn new(
        shortcut: JetUiShortcut,
        action: &str,
        node: Option<JetUiNodeId>,
    ) -> Result<Self, JetUiHostError> {
        if action.is_empty() {
            return Err(JetUiHostError::InvalidRequest(
                "shortcut action must not be empty".to_string(),
            ));
        }
        Ok(Self {
            shortcut,
            action: action.to_string(),
            node,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetUiShortcutDispatch {
    Dispatched(JetUiShortcutBinding),
    Unhandled,
}

/// Deterministic action table.  It stores action identities, not callbacks,
/// so every host dispatches the same typed event and the package decides what
/// the action means.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetUiShortcutRegistry {
    bindings: std::collections::BTreeMap<JetUiShortcut, JetUiShortcutBinding>,
}

impl JetUiShortcutRegistry {
    pub fn register(
        &mut self,
        binding: JetUiShortcutBinding,
    ) -> Result<JetUiShortcutBinding, JetUiHostError> {
        if let Some(existing) = self.bindings.get(&binding.shortcut) {
            return Err(JetUiHostError::ShortcutConflict {
                shortcut: binding.shortcut,
                existing_action: existing.action.clone(),
            });
        }
        self.bindings
            .insert(binding.shortcut.clone(), binding.clone());
        Ok(binding)
    }

    pub fn unregister(&mut self, shortcut: &JetUiShortcut) -> Option<JetUiShortcutBinding> {
        self.bindings.remove(shortcut)
    }

    pub fn dispatch(&self, shortcut: &JetUiShortcut) -> JetUiShortcutDispatch {
        self.bindings
            .get(shortcut)
            .cloned()
            .map(JetUiShortcutDispatch::Dispatched)
            .unwrap_or(JetUiShortcutDispatch::Unhandled)
    }

    pub fn bindings(&self) -> Vec<JetUiShortcutBinding> {
        self.bindings.values().cloned().collect()
    }
}

/// Typed accessibility facts carried by one canonical node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetUiAccessibilityState {
    Disabled,
    Busy,
    Expanded(bool),
    Checked(bool),
    Selected(bool),
    Required,
    Value(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JetUiAccessibility {
    pub name: Option<String>,
    pub description: Option<String>,
    pub states: Vec<JetUiAccessibilityState>,
}

impl JetUiAccessibility {
    pub fn new(name: Option<&str>, description: Option<&str>) -> Result<Self, JetUiHostError> {
        if name.is_some_and(str::is_empty) || description.is_some_and(str::is_empty) {
            return Err(JetUiHostError::InvalidRequest(
                "accessibility name and description must not be empty".to_string(),
            ));
        }
        Ok(Self {
            name: name.map(str::to_string),
            description: description.map(str::to_string),
            states: Vec::new(),
        })
    }

    pub fn named(name: &str) -> Result<Self, JetUiHostError> {
        Self::new(Some(name), None)
    }

    pub fn with_state(mut self, state: JetUiAccessibilityState) -> Self {
        self.states.push(state);
        self
    }
}

/// Ui.rs implements this trait once `JetUiNode` carries the metadata field.
/// The trait is the only storage seam: no registry or parallel node tree is
/// introduced here.
pub trait JetUiAccessibilityTarget {
    fn attach_jet_ui_accessibility(
        &mut self,
        accessibility: JetUiAccessibility,
    ) -> Result<(), JetUiHostError>;

    fn jet_ui_accessibility(&self) -> Option<&JetUiAccessibility>;
}

/// A host-facing projection of metadata already attached to one canonical
/// node.  It owns only the copied metadata, never children or a second tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiAccessibilityProjection {
    node: JetUiNodeId,
    metadata: JetUiAccessibility,
}

impl JetUiAccessibilityProjection {
    pub fn node(&self) -> &JetUiNodeId {
        &self.node
    }

    pub fn metadata(&self) -> &JetUiAccessibility {
        &self.metadata
    }
}

pub fn jet_ui_accessibility_attach<T: JetUiAccessibilityTarget + ?Sized>(
    node: &mut T,
    accessibility: JetUiAccessibility,
) -> Result<(), JetUiHostError> {
    node.attach_jet_ui_accessibility(accessibility)
}

pub fn jet_ui_accessibility_project<T: JetUiAccessibilityTarget + ?Sized>(
    node: &T,
    node_id: JetUiNodeId,
) -> Option<JetUiAccessibilityProjection> {
    node.jet_ui_accessibility()
        .cloned()
        .map(|metadata| JetUiAccessibilityProjection {
            node: node_id,
            metadata,
        })
}

/// The shared host contract.  Each method checks the corresponding capability
/// fact before using its typed request or event value.
pub trait JetUiHost {
    fn capability_facts(&self) -> JetUiCapabilityFacts;
    fn attach_accessibility(
        &self,
        node: &mut dyn JetUiAccessibilityTarget,
        accessibility: JetUiAccessibility,
    ) -> JetUiServiceResult<()>;

    fn project_accessibility(
        &self,
        node: &dyn JetUiAccessibilityTarget,
        node_id: JetUiNodeId,
    ) -> JetUiServiceResult<Option<JetUiAccessibilityProjection>>;


    fn open_file(
        &mut self,
        request: JetUiFileDialogRequest,
    ) -> JetUiServiceResult<JetUiFileDialogSelection>;

    fn save_file(
        &mut self,
        request: JetUiFileDialogRequest,
    ) -> JetUiServiceResult<JetUiFileDialogSelection>;

    fn read_clipboard_text(&mut self) -> JetUiServiceResult<JetUiClipboardText>;

    fn write_clipboard_text(&mut self, text: String) -> JetUiServiceResult<JetUiClipboardWrite>;

    fn poll_ime_event(&mut self) -> JetUiServiceResult<Option<JetUiImeEvent>>;

    fn poll_drag_event(&mut self) -> JetUiServiceResult<Option<JetUiDragEvent>>;

    fn register_shortcut(
        &mut self,
        binding: JetUiShortcutBinding,
    ) -> JetUiServiceResult<JetUiShortcutBinding>;

    fn dispatch_shortcut(
        &mut self,
        shortcut: JetUiShortcut,
    ) -> JetUiServiceResult<JetUiShortcutDispatch>;

    /// Hosts supply checked canonical font bytes; this shared kernel owns shaping.
    fn font_bytes(&mut self, face: &JetFontFace) -> Result<&[u8], JetUiHostError> {
        jet_font_vetted_bytes(face)
    }

    fn shape_text(
        &mut self,
        text: &str,
        face: &JetFontFace,
    ) -> Result<JetGlyphRun, JetUiHostError> {
        jet_font_shape_with_fallback(text, face)
    }
}

// JET_VETTED_UNSAFE_BEGIN: jet_ui_host_scope
// AUDIT: this narrow seam stores a borrowed trait object in a thread-local
// callback scope. Safe Rust cannot place that non-'static borrow in ambient
// storage; the transmute changes only the erased lifetime, not the data/vtable
// representation. The scope guard must restore the predecessor before the host
// borrow ends, callbacks must stay inside that scope, and thread-local access
// prevents cross-thread use. Violating those rules causes use-after-borrow or
// aliasing mutable host access.

thread_local! {
    /// The current host is scoped to one backend/event-loop call.  A raw
    /// pointer is safe here because the guard never outlives the borrow used
    /// to install it and restores the previous value on drop.
    static JET_UI_CURRENT_HOST: std::cell::RefCell<Option<*mut dyn JetUiHost>> =
        std::cell::RefCell::new(None);
    /// Non-native tiers share one deterministic host instance rather than
    /// constructing a new service state for every call.
    static JET_UI_DEFAULT_HEADLESS_HOST: std::cell::RefCell<JetUiHeadlessHost> =
        std::cell::RefCell::new(JetUiHeadlessHost::new());
}

pub struct JetUiHostScope {
    previous: Option<*mut dyn JetUiHost>,
}

impl Drop for JetUiHostScope {
    fn drop(&mut self) {
        JET_UI_CURRENT_HOST.with(|slot| {
            slot.replace(self.previous);
        });
    }
}

/// Install one host for the duration of the surrounding backend/event-loop
/// operation.  Nested scopes restore their predecessor.
pub fn jet_ui_host_scope(host: &mut dyn JetUiHost) -> JetUiHostScope {
    // SAFETY: the slot only erases the borrow's lifetime; the returned guard
    // restores the previous pointer on drop, and `jet_ui_with_current_host`
    // dereferences it only while a caller still holds that guard, so the
    // pointer never outlives the borrow it was made from.
    let pointer: *mut (dyn JetUiHost + 'static) =
        unsafe { std::mem::transmute(host as *mut dyn JetUiHost) };
    let previous = JET_UI_CURRENT_HOST.with(|slot| slot.replace(Some(pointer)));
    JetUiHostScope { previous }
}

pub fn jet_ui_with_host<R>(host: &mut dyn JetUiHost, body: impl FnOnce() -> R) -> R {
    let _scope = jet_ui_host_scope(host);
    body()
}

pub fn jet_ui_with_current_host<R>(body: impl FnOnce(&mut dyn JetUiHost) -> R) -> R {
    let current = JET_UI_CURRENT_HOST.with(|slot| *slot.borrow());
    if let Some(pointer) = current {
        // SAFETY: `pointer` is installed only by `jet_ui_host_scope`, whose
        // guard is held by the caller for the duration of this callback.
        unsafe { body(&mut *pointer) }
    } else {
        JET_UI_DEFAULT_HEADLESS_HOST.with(|host| body(&mut *host.borrow_mut()))
    }
}
// JET_VETTED_UNSAFE_END: jet_ui_host_scope

pub fn jet_ui_host_capabilities() -> JetUiCapabilityFacts {
    jet_ui_with_current_host(|host| host.capability_facts())
}
pub fn jet_ui_host_open_file(
    request: JetUiFileDialogRequest,
) -> JetUiServiceResult<JetUiFileDialogSelection> {
    jet_ui_with_current_host(|host| jet_ui_open_file(host, request))
}
pub fn jet_ui_host_save_file(
    request: JetUiFileDialogRequest,
) -> JetUiServiceResult<JetUiFileDialogSelection> {
    jet_ui_with_current_host(|host| jet_ui_save_file(host, request))
}

pub fn jet_ui_host_clipboard_read_text() -> JetUiServiceResult<JetUiClipboardText> {
    jet_ui_with_current_host(|host| jet_ui_read_clipboard_text(host))
}

pub fn jet_ui_host_clipboard_write_text(
    text: &str,
) -> JetUiServiceResult<JetUiClipboardWrite> {
    jet_ui_with_current_host(|host| host.write_clipboard_text(text.to_string()))
}


pub fn jet_ui_host_ime_poll() -> JetUiServiceResult<Option<JetUiImeEvent>> {
    jet_ui_with_current_host(|host| host.poll_ime_event())
}

pub fn jet_ui_host_drag_poll() -> JetUiServiceResult<Option<JetUiDragEvent>> {
    jet_ui_with_current_host(|host| host.poll_drag_event())
}

pub fn jet_ui_host_shortcuts_register(
    binding: JetUiShortcutBinding,
) -> JetUiServiceResult<JetUiShortcutBinding> {
    jet_ui_with_current_host(|host| host.register_shortcut(binding))
}

pub fn jet_ui_host_shortcuts_dispatch(
    shortcut: JetUiShortcut,
) -> JetUiServiceResult<JetUiShortcutDispatch> {
    jet_ui_with_current_host(|host| host.dispatch_shortcut(shortcut))
}

pub fn jet_font_shape(text: &str, face: &JetFontFace) -> JetGlyphRun {
    jet_ui_with_current_host(|host| {
        host.shape_text(text, face).unwrap_or_else(|error| {
            panic!("font shaping host contract failure: {error:?}")
        })
    })
}
#[cfg(target_arch = "wasm32")]
const JET_WASM_FONT_MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
#[cfg(target_arch = "wasm32")]
const JET_WASM_FONT_MAX_FAMILY_BYTES: usize = 1024;
#[cfg(target_arch = "wasm32")]
const JET_WASM_FONT_MAX_GLYPHS: usize = 1_048_576;

#[cfg(target_arch = "wasm32")]
#[repr(C)]
struct JetWasmGlyph {
    id: i64,
    cluster: i64,
    x: f64,
    y: f64,
    advance_x: f64,
    advance_y: f64,
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static JET_WASM_FONT_TEXT: std::cell::RefCell<Vec<u8>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static JET_WASM_FONT_FAMILY: std::cell::RefCell<Vec<u8>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static JET_WASM_FONT_OUTPUT: std::cell::RefCell<Vec<JetWasmGlyph>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static JET_WASM_FONT_ERROR: std::cell::RefCell<Vec<u8>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static JET_WASM_FONT_ADVANCE_X: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    static JET_WASM_FONT_ADVANCE_Y: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    // The imported HarfBuzz getters return pointers into app memory. Separate
    // u32-backed scratch buffers keep those pointers aligned for Rust's
    // repr(C) glyph records while letting the JS adapter copy raw bytes.
    static JET_WASM_HB_INFO: std::cell::RefCell<Vec<u32>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static JET_WASM_HB_POSITIONS: std::cell::RefCell<Vec<u32>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(target_arch = "wasm32")]
fn jet_wasm_font_alloc_bytes(
    cell: &'static std::thread::LocalKey<std::cell::RefCell<Vec<u8>>>,
    length: u32,
    limit: usize,
) -> u32 {
    let length = length as usize;
    if length > limit {
        return 0;
    }
    cell.with(|slot| {
        let mut bytes = slot.borrow_mut();
        bytes.resize(length, 0);
        bytes.as_mut_ptr() as usize as u32
    })
}

#[cfg(target_arch = "wasm32")]
fn jet_wasm_hb_scratch_alloc(
    cell: &'static std::thread::LocalKey<std::cell::RefCell<Vec<u32>>>,
    glyph_count: u32,
) -> u32 {
    let glyph_count = glyph_count as usize;
    if glyph_count > JET_WASM_FONT_MAX_GLYPHS {
        return 0;
    }
    let words = glyph_count.checked_mul(5).unwrap_or(usize::MAX);
    if words > u32::MAX as usize {
        return 0;
    }
    cell.with(|slot| {
        let mut values = slot.borrow_mut();
        values.resize(words, 0);
        values.as_mut_ptr() as usize as u32
    })
}

#[cfg(target_arch = "wasm32")]
fn jet_wasm_font_error(message: &str) -> i32 {
    JET_WASM_FONT_ERROR.with(|slot| {
        let mut error = slot.borrow_mut();
        error.clear();
        error.extend_from_slice(message.as_bytes());
    });
    -1
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_hb_wasm_info_alloc(glyph_count: u32) -> u32 {
    jet_wasm_hb_scratch_alloc(&JET_WASM_HB_INFO, glyph_count)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_hb_wasm_positions_alloc(glyph_count: u32) -> u32 {
    jet_wasm_hb_scratch_alloc(&JET_WASM_HB_POSITIONS, glyph_count)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_font_shape_wasm_text_alloc(length: u32) -> u32 {
    jet_wasm_font_alloc_bytes(
        &JET_WASM_FONT_TEXT,
        length,
        JET_WASM_FONT_MAX_TEXT_BYTES,
    )
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_font_shape_wasm_family_alloc(length: u32) -> u32 {
    jet_wasm_font_alloc_bytes(
        &JET_WASM_FONT_FAMILY,
        length,
        JET_WASM_FONT_MAX_FAMILY_BYTES,
    )
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_font_shape_wasm_shape(
    text_pointer: u32,
    text_length: u32,
    family_pointer: u32,
    family_length: u32,
    size: f64,
    style: u32,
) -> i32 {
    JET_WASM_FONT_ERROR.with(|slot| slot.borrow_mut().clear());
    if text_length as usize > JET_WASM_FONT_MAX_TEXT_BYTES
        || family_length as usize > JET_WASM_FONT_MAX_FAMILY_BYTES
    {
        return jet_wasm_font_error("font shape input exceeds the supported size");
    }
    let style = match style {
        0 => JetFontStyle::Body,
        1 => JetFontStyle::Title,
        2 => JetFontStyle::Monospace,
        _ => return jet_wasm_font_error("font shape style is invalid"),
    };
    let family = match JET_WASM_FONT_FAMILY.with(|slot| {
        let bytes = slot.borrow();
        let pointer = bytes.as_ptr() as usize as u32;
        if family_length != 0 && (family_pointer != pointer || family_pointer == 0) {
            return Err("font shape family pointer is invalid");
        }
        if family_length as usize > bytes.len() {
            return Err("font shape family length is invalid");
        }
        std::str::from_utf8(&bytes[..family_length as usize])
            .map(str::to_owned)
            .map_err(|_| "font shape family is not valid UTF-8")
    }) {
        Ok(family) => family,
        Err(message) => return jet_wasm_font_error(message),
    };
    let face = if family.is_empty() && size == 0.0 {
        jet_font_system(style)
    } else {
        if family.is_empty() {
            return jet_wasm_font_error("font family must not be empty");
        }
        JetFontFace {
            family,
            size,
            style,
        }
    };
    let shaped = JET_WASM_FONT_TEXT.with(|slot| {
        let bytes = slot.borrow();
        let pointer = bytes.as_ptr() as usize as u32;
        if text_length != 0 && (text_pointer != pointer || text_pointer == 0) {
            return Err("font shape text pointer is invalid".to_string());
        }
        if text_length as usize > bytes.len() {
            return Err("font shape text length is invalid".to_string());
        }
        let text = std::str::from_utf8(&bytes[..text_length as usize])
            .map_err(|_| "font shape text is not valid UTF-8".to_string())?;
        jet_font_shape_with_fallback(text, &face).map_err(|error| format!("{error:?}"))
    });
    let run = match shaped {
        Ok(run) => run,
        Err(message) => return jet_wasm_font_error(&message),
    };
    JET_WASM_FONT_ADVANCE_X.with(|value| value.set(run.advance_x));
    JET_WASM_FONT_ADVANCE_Y.with(|value| value.set(run.advance_y));
    JET_WASM_FONT_OUTPUT.with(|slot| {
        let mut output = slot.borrow_mut();
        output.clear();
        output.extend(run.glyphs.into_iter().map(|glyph| JetWasmGlyph {
            id: glyph.id,
            cluster: glyph.cluster,
            x: glyph.x,
            y: glyph.y,
            advance_x: glyph.advance_x,
            advance_y: glyph.advance_y,
        }));
        i32::try_from(output.len()).unwrap_or(-1)
    })
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_font_shape_wasm_output_ptr() -> u32 {
    JET_WASM_FONT_OUTPUT.with(|slot| slot.borrow().as_ptr() as usize as u32)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_font_shape_wasm_output_len() -> u32 {
    JET_WASM_FONT_OUTPUT.with(|slot| slot.borrow().len() as u32)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_font_shape_wasm_output_advance_x() -> f64 {
    JET_WASM_FONT_ADVANCE_X.with(std::cell::Cell::get)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_font_shape_wasm_output_advance_y() -> f64 {
    JET_WASM_FONT_ADVANCE_Y.with(std::cell::Cell::get)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_font_shape_wasm_error_ptr() -> u32 {
    JET_WASM_FONT_ERROR.with(|slot| slot.borrow().as_ptr() as usize as u32)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn jet_font_shape_wasm_error_len() -> u32 {
    JET_WASM_FONT_ERROR.with(|slot| slot.borrow().len() as u32)
}

pub fn jet_ui_host_file_filter(
    label: &str,
    extensions: &Vec<String>,
    mime_types: &Vec<String>,
) -> JetUiServiceResult<JetUiFileFilter> {
    match JetUiFileFilter::new(label, extensions.clone(), mime_types.clone()) {
        Ok(filter) => Ok(filter),
        Err(error) => Err(error),
    }
}

pub fn jet_ui_host_file_filter_text() -> JetUiFileFilter {
    JetUiFileFilter::text()
}

pub fn jet_ui_host_fs_rights_read() -> JetUiFsRights {
    JetUiFsRights::read()
}

pub fn jet_ui_host_fs_rights_write() -> JetUiFsRights {
    JetUiFsRights::write()
}

pub fn jet_ui_host_fs_rights_read_write() -> JetUiFsRights {
    JetUiFsRights::read_write()
}

pub fn jet_ui_host_fs_grant(
    root: &str,
    rights: JetUiFsRights,
) -> JetUiServiceResult<JetUiFsGrant> {
    match JetUiFsGrant::new(root, rights) {
        Ok(grant) => Ok(grant),
        Err(error) => Err(error),
    }
}

pub fn jet_ui_host_open_request(grant: JetUiFsGrant) -> JetUiFileDialogRequest {
    JetUiFileDialogRequest::open(grant)
}

pub fn jet_ui_host_save_request(grant: JetUiFsGrant) -> JetUiFileDialogRequest {
    JetUiFileDialogRequest::save(grant)
}

pub fn jet_ui_host_shortcut(
    key: &str,
    modifiers: JetUiShortcutModifiers,
) -> JetUiServiceResult<JetUiShortcut> {
    match JetUiShortcut::new(key, modifiers) {
        Ok(shortcut) => Ok(shortcut),
        Err(error) => Err(error),
    }
}

pub fn jet_ui_host_shortcut_binding(
    shortcut: JetUiShortcut,
    action: &str,
) -> JetUiServiceResult<JetUiShortcutBinding> {
    match JetUiShortcutBinding::new(shortcut, action, None) {
        Ok(binding) => Ok(binding),
        Err(error) => Err(error),
    }
}

pub fn jet_ui_host_accessibility(
    name: &str,
    description: &str,
) -> JetUiServiceResult<JetUiAccessibility> {
    let description = (!description.is_empty()).then_some(description);
    match JetUiAccessibility::new(Some(name), description) {
        Ok(accessibility) => Ok(accessibility),
        Err(error) => Err(error),
    }
}

/// A deterministic host for tests and non-native tiers.  Dialogs cancel with
/// a stable reason; clipboard and event queues are in-memory; shortcuts use a
/// BTreeMap; no clock, random value, or OS state is consulted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetUiHeadlessHost {
    facts: JetUiCapabilityFacts,
    clipboard: String,
    ime_events: std::collections::VecDeque<JetUiImeEvent>,
    drag_events: std::collections::VecDeque<JetUiDragEvent>,
    shortcuts: JetUiShortcutRegistry,
}

impl Default for JetUiHeadlessHost {
    fn default() -> Self {
        Self::new()
    }
}


impl JetUiHeadlessHost {
    pub fn new() -> Self {
        Self {
            facts: JetUiCapabilityFacts::all(),
            clipboard: String::new(),
            ime_events: std::collections::VecDeque::new(),
            drag_events: std::collections::VecDeque::new(),
            shortcuts: JetUiShortcutRegistry::default(),
        }
    }

    pub fn with_capabilities(facts: JetUiCapabilityFacts) -> Self {
        Self {
            facts,
            ..Self::new()
        }
    }

    pub fn queue_ime_event(&mut self, event: JetUiImeEvent) -> JetUiServiceResult<()> {
        if let Err(error) = self.facts.require(JetUiCapability::Ime) {
            return Err(error);
        }
        if self.ime_events.len() >= JET_UI_HOST_MAX_QUEUED_EVENTS {
            return Err(JetUiHostError::QueueFull { service: "IME" });
        }
        self.ime_events.push_back(event);
        Ok(())
    }

    pub fn queue_drag_event(&mut self, event: JetUiDragEvent) -> JetUiServiceResult<()> {
        if let Err(error) = self.facts.require(JetUiCapability::DragDrop) {
            return Err(error);
        }
        if self.drag_events.len() >= JET_UI_HOST_MAX_QUEUED_EVENTS {
            return Err(JetUiHostError::QueueFull {
                service: "drag/drop",
            });
        }
        self.drag_events.push_back(event);
        Ok(())
    }

    pub fn attach_accessibility<T: JetUiAccessibilityTarget>(
        &self,
        node: &mut T,
        accessibility: JetUiAccessibility,
    ) -> JetUiServiceResult<()> {
        if let Err(error) = self.facts.require(JetUiCapability::Accessibility) {
            return Err(error);
        }
        match jet_ui_accessibility_attach(node, accessibility) {
            Ok(()) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl JetUiHost for JetUiHeadlessHost {
    fn capability_facts(&self) -> JetUiCapabilityFacts {
        self.facts
    }

    fn open_file(
        &mut self,
        request: JetUiFileDialogRequest,
    ) -> JetUiServiceResult<JetUiFileDialogSelection> {
        if let Err(error) = self.facts.require(JetUiCapability::FileDialog) {
            return Err(error);
        }
        if !matches!(request.kind, JetUiFileDialogKind::Open) {
            return Err(JetUiHostError::InvalidRequest(
                "open_file requires an open dialog request".to_string(),
            ));
        }
        if let Err(error) = request.validate() {
            return Err(error);
        }
        Err(JetUiHostError::Cancelled(JetUiCancellation::Headless))
    }

    fn save_file(
        &mut self,
        request: JetUiFileDialogRequest,
    ) -> JetUiServiceResult<JetUiFileDialogSelection> {
        if let Err(error) = self.facts.require(JetUiCapability::FileDialog) {
            return Err(error);
        }
        if !matches!(request.kind, JetUiFileDialogKind::Save) {
            return Err(JetUiHostError::InvalidRequest(
                "save_file requires a save dialog request".to_string(),
            ));
        }
        if let Err(error) = request.validate() {
            return Err(error);
        }
        Err(JetUiHostError::Cancelled(JetUiCancellation::Headless))
    }

    fn read_clipboard_text(&mut self) -> JetUiServiceResult<JetUiClipboardText> {
        if let Err(error) = self.facts.require(JetUiCapability::Clipboard) {
            return Err(error);
        }
        Ok(JetUiClipboardText {
            text: self.clipboard.clone(),
            selection: None,
        })
    }

    fn write_clipboard_text(&mut self, text: String) -> JetUiServiceResult<JetUiClipboardWrite> {
        if let Err(error) = self.facts.require(JetUiCapability::Clipboard) {
            return Err(error);
        }
        let characters = text.chars().count();
        self.clipboard = text;
        Ok(JetUiClipboardWrite { characters })
    }

    fn poll_ime_event(&mut self) -> JetUiServiceResult<Option<JetUiImeEvent>> {
        if let Err(error) = self.facts.require(JetUiCapability::Ime) {
            return Err(error);
        }
        Ok(self.ime_events.pop_front())
    }

    fn poll_drag_event(&mut self) -> JetUiServiceResult<Option<JetUiDragEvent>> {
        if let Err(error) = self.facts.require(JetUiCapability::DragDrop) {
            return Err(error);
        }
        Ok(self.drag_events.pop_front())
    }

    fn register_shortcut(
        &mut self,
        binding: JetUiShortcutBinding,
    ) -> JetUiServiceResult<JetUiShortcutBinding> {
        if let Err(error) = self.facts.require(JetUiCapability::Shortcuts) {
            return Err(error);
        }
        match self.shortcuts.register(binding) {
            Ok(binding) => Ok(binding),
            Err(error) => Err(error),
        }
    }

    fn dispatch_shortcut(
        &mut self,
        shortcut: JetUiShortcut,
    ) -> JetUiServiceResult<JetUiShortcutDispatch> {
        if let Err(error) = self.facts.require(JetUiCapability::Shortcuts) {
            return Err(error);
        }
        Ok(self.shortcuts.dispatch(&shortcut))
    }
    fn attach_accessibility(
        &self,
        node: &mut dyn JetUiAccessibilityTarget,
        accessibility: JetUiAccessibility,
    ) -> JetUiServiceResult<()> {
        if let Err(error) = self.facts.require(JetUiCapability::Accessibility) {
            return Err(error);
        }
        match jet_ui_accessibility_attach(node, accessibility) {
            Ok(()) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn project_accessibility(
        &self,
        node: &dyn JetUiAccessibilityTarget,
        node_id: JetUiNodeId,
    ) -> JetUiServiceResult<Option<JetUiAccessibilityProjection>> {
        if let Err(error) = self.facts.require(JetUiCapability::Accessibility) {
            return Err(error);
        }
        Ok(jet_ui_accessibility_project(node, node_id))
    }

    fn shape_text(
        &mut self,
        text: &str,
        face: &JetFontFace,
    ) -> Result<JetGlyphRun, JetUiHostError> {
        let _ = self;
        jet_font_shape_with_fallback(text, face)
    }
}

fn jet_ui_host_normalize_path(path: &str) -> Result<std::path::PathBuf, JetUiHostError> {
    if path.trim().is_empty() {
        return Err(JetUiHostError::InvalidRequest(
            "filesystem grant path must not be empty".to_string(),
        ));
    }

    let mut normalized = std::path::PathBuf::new();
    let mut rooted = false;
    let mut normal_depth = 0usize;
    for component in std::path::Path::new(path).components() {
        match component {
            std::path::Component::Prefix(prefix) => {
                normalized.push(prefix.as_os_str());
                rooted = true;
            }
            std::path::Component::RootDir => {
                normalized.push(component.as_os_str());
                rooted = true;
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if normal_depth == 0 {
                    return Err(JetUiHostError::ResourceDenied(format!(
                        "path `{}` escapes its lexical root",
                        path
                    )));
                }
                normalized.pop();
                normal_depth -= 1;
            }
            std::path::Component::Normal(part) => {
                normalized.push(part);
                normal_depth += 1;
            }
        }
    }
    if normalized.as_os_str().is_empty() && rooted {
        normalized.push(std::path::MAIN_SEPARATOR.to_string());
    }
    Ok(normalized)
}

fn jet_ui_host_path_text(path: &std::path::Path) -> String {
    if path.as_os_str().is_empty() {
        ".".to_string()
    } else {
        path.to_string_lossy().into_owned()
    }
}
