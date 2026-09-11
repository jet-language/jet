//! D-RENDERTGT*: resident-JIT UI host — `include!` canonical Prelude/Ui.rs
//! and the dynamic native GTK adapter. Opaque handles only.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use jet_codegen::Comptime::DevSink;
use jet_foundation::AST::{CtFloat, CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::Prelude::jet_e0956_unsupported as unsupported;
use ui_rt::{JetTuiStatefulWidget, JetTuiWidget};
use jet_foundation::MIR::{
    MirCoreClosureKind, MirNominalRef, MirPreludeCallId, MirRuntimeValue, MirSiteId, MirType,
    MirTypeKind,
};

#[allow(dead_code, unused_imports)]
pub(crate) mod ui_rt {
    pub trait JetShow {
        fn jet_show(&self) -> String;
    }
    mod jet_std {
        pub use crate::Reactive::reactive_rt::jet_reactive_effect_rooted;
    }


    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    #[allow(unused_imports)]
    pub use jet_foundation::Devtools::*;
    use jet_codegen::Codegen::{
        JET_CANONICAL_ARABIC_FONT_BYTES, JET_CANONICAL_FONT_BYTES,
        JET_CANONICAL_SYMBOLS_FONT_BYTES,
    };
    pub mod jet_tui_kernel {
        include!("../../jet-foundation/src/Prelude/TuiKernel.rs");
    }
    include!("../../jet-codegen/src/Prelude/Core/HostServices.rs");
    include!("../../jet-codegen/src/Prelude/Ui.rs");
    include!("../../jet-codegen/src/Prelude/Core/Preview.rs");
    include!("../../jet-codegen/src/Prelude/UiGtk.rs");
}


#[derive(Clone)]
pub(crate) enum UiValue {
    FontStyle(ui_rt::JetFontStyle),
    FontFace(ui_rt::JetFontFace),
    GlyphShaper(ui_rt::JetGlyphShaper),
    Glyph(ui_rt::JetGlyph),
    GlyphRun(ui_rt::JetGlyphRun),
    CapabilityFacts(ui_rt::JetUiCapabilityFacts),
    FileFilter(ui_rt::JetUiFileFilter),
    FsRights(ui_rt::JetUiFsRights),
    FsGrant(ui_rt::JetUiFsGrant),
    GrantedPath(ui_rt::JetUiGrantedPath),
    FileDialogRequest(ui_rt::JetUiFileDialogRequest),
    FileDialogSelection(ui_rt::JetUiFileDialogSelection),
    ClipboardText(ui_rt::JetUiClipboardText),
    ClipboardWrite(ui_rt::JetUiClipboardWrite),
    TextRange(ui_rt::JetUiTextRange),
    ImeEvent(Option<ui_rt::JetUiImeEvent>),
    DragEvent(Option<ui_rt::JetUiDragEvent>),
    ShortcutModifiers(ui_rt::JetUiShortcutModifiers),
    Shortcut(ui_rt::JetUiShortcut),
    ShortcutBinding(ui_rt::JetUiShortcutBinding),
    ShortcutDispatch(ui_rt::JetUiShortcutDispatch),
    Accessibility(ui_rt::JetUiAccessibility),
    AccessibilityProjection(Option<ui_rt::JetUiAccessibilityProjection>),
    TuiEvent(ui_rt::JetTuiEvent),
    TuiCapabilities(ui_rt::JetTuiCapabilities),
    TuiColor(ui_rt::JetTuiColor),
    TuiStyle(ui_rt::JetTuiStyle),
    TuiConstraint(ui_rt::JetTuiConstraint),
    TuiDirection(ui_rt::JetTuiDirection),
    TuiListState(ui_rt::JetTuiListState),
}

pub(crate) struct UiState {
    pub(crate) backends: Vec<UiBackendSlot>,
    pub(crate) nodes: Vec<ui_rt::JetUiNode>,
    pub(crate) constraints: Vec<ui_rt::JetSizeConstraint>,
    pub(crate) rects: Vec<ui_rt::JetRect>,
    pub(crate) events: Vec<ui_rt::JetInputEvent>,
    pub(crate) roles: Vec<ui_rt::JetAriaRole>,
    pub(crate) sizes: Vec<ui_rt::JetSize>,
    pub(crate) gtk_widgets: Vec<i64>,
    /// Host selection is explicit: headless mode keeps its deterministic host,
    /// while the normal path owns the real GTK adapter. The adapter is cloned
    /// before calls that may enter a native event loop or invoke user code.
    pub(crate) host: UiHostSlot,
    /// Typed values retain their shared Prelude ownership; i64 handles only
    /// index this table and never become a second semantic implementation.
    pub(crate) values: std::collections::HashMap<i64, UiValue>,
}

impl Default for UiState {
    fn default() -> Self {
        let host = if std::env::var_os("JET_UI_HEADLESS").is_some() {
            UiHostSlot::Headless(std::sync::Arc::new(std::sync::Mutex::new(
                ui_rt::JetUiHeadlessHost::new(),
            )))
        } else {
            UiHostSlot::Gtk(ui_rt::jet_ui_gtk())
        };
        Self {
            backends: Vec::new(),
            nodes: Vec::new(),
            constraints: Vec::new(),
            rects: Vec::new(),
            events: Vec::new(),
            roles: Vec::new(),
            sizes: Vec::new(),
            gtk_widgets: Vec::new(),
            host,
            values: std::collections::HashMap::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) enum UiHostSlot {
    Gtk(ui_rt::JetGtkBackend),
    Headless(std::sync::Arc<std::sync::Mutex<ui_rt::JetUiHeadlessHost>>),
}

impl UiHostSlot {
    fn with_host<R>(self, body: impl FnOnce(&mut dyn ui_rt::JetUiHost) -> R) -> R {
        match self {
            Self::Gtk(mut host) => body(&mut host),
            Self::Headless(host) => {
                let mut host = host.lock().unwrap_or_else(|error| error.into_inner());
                body(&mut *host)
            }
        }
    }
}
fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::runtime_host::JitRuntime) -> R,
    R: Default,
{
    Concurrency::with_runtime_mut(f)
}


#[derive(Clone)]
pub(crate) enum UiBackendSlot {
    Null(ui_rt::JetNullBackend),
    Tui(ui_rt::JetTuiBackend),
    Gtk(ui_rt::JetGtkBackend),
}

fn with_ui_host<F, R>(body: F) -> R
where
    F: FnOnce() -> R,
{
    let host = with_rt(|rt| Some(rt.ui.host.clone()));
    match host {
        Some(host) => host.with_host(|host| ui_rt::jet_ui_with_host(host, body)),
        // Ambient calls can run before a resident runtime is installed. In
        // that case the canonical helper deliberately uses its default host.
        None => body(),
    }
}
fn push_struct_f64(fields: &[f64]) -> i64 {
    with_rt(|rt| {
        let h = rt.heap.alloc_record(fields.len());
        for (i, v) in fields.iter().enumerate() {
            let _ = rt.heap.record_set_float(h, i as i64, *v);
        }
        h
    })
}
fn ui_store_value(rt: &mut crate::runtime_host::JitRuntime, handle: i64, value: UiValue) -> i64 {
    rt.ui.values.insert(handle, value);
    handle
}

fn ui_value(rt: &crate::runtime_host::JitRuntime, handle: i64) -> Option<UiValue> {
    rt.ui.values.get(&handle).cloned()
}
fn ui_push_tui_color(
    rt: &mut crate::runtime_host::JitRuntime,
    color: ui_rt::JetTuiColor,
) -> i64 {
    let record = rt.heap.alloc_record(match color {
        ui_rt::JetTuiColor::Rgb(..) => 4,
        _ => 2,
    });
    match color {
        ui_rt::JetTuiColor::Ansi16(index) => {
            let _ = rt.heap.record_set_int(record, 0, 0);
            let _ = rt.heap.record_set_int(record, 1, i64::from(index));
        }
        ui_rt::JetTuiColor::Ansi256(index) => {
            let _ = rt.heap.record_set_int(record, 0, 1);
            let _ = rt.heap.record_set_int(record, 1, i64::from(index));
        }
        ui_rt::JetTuiColor::Rgb(red, green, blue) => {
            let _ = rt.heap.record_set_int(record, 0, 2);
            let _ = rt.heap.record_set_int(record, 1, i64::from(red));
            let _ = rt.heap.record_set_int(record, 2, i64::from(green));
            let _ = rt.heap.record_set_int(record, 3, i64::from(blue));
        }
    }
    ui_store_value(rt, record, UiValue::TuiColor(color))
}

fn ui_decode_tui_color(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetTuiColor> {
    if let Some(UiValue::TuiColor(color)) = ui_value(rt, handle) {
        return Some(color);
    }
    match rt.heap.record_get_int(handle, 0)? {
        0 => Some(ui_rt::JetTuiColor::Ansi16(
            rt.heap.record_get_int(handle, 1)?.clamp(0, 15) as u8,
        )),
        1 => Some(ui_rt::JetTuiColor::Ansi256(
            rt.heap.record_get_int(handle, 1)?.clamp(0, 255) as u8,
        )),
        2 => Some(ui_rt::JetTuiColor::Rgb(
            rt.heap.record_get_int(handle, 1)?.clamp(0, 255) as u8,
            rt.heap.record_get_int(handle, 2)?.clamp(0, 255) as u8,
            rt.heap.record_get_int(handle, 3)?.clamp(0, 255) as u8,
        )),
        _ => None,
    }
}

fn ui_push_tui_capabilities(
    rt: &mut crate::runtime_host::JitRuntime,
    capabilities: ui_rt::JetTuiCapabilities,
) -> i64 {
    let profile = rt.heap.alloc_record(1);
    let profile_discriminant = match capabilities.profile {
        ui_rt::JetTuiColorProfile::Ansi16 => 0,
        ui_rt::JetTuiColorProfile::Ansi256 => 1,
        ui_rt::JetTuiColorProfile::TrueColor => 2,
        ui_rt::JetTuiColorProfile::Ascii => 3,
    };
    let _ = rt.heap.record_set_int(profile, 0, profile_discriminant);
    let record = rt.heap.alloc_record(8);
    let _ = rt.heap.record_set_record(record, 0, profile);
    let _ = rt.heap.record_set_bool(record, 1, capabilities.color);
    let _ = rt.heap.record_set_bool(record, 2, capabilities.unicode);
    let _ = rt.heap.record_set_bool(record, 3, capabilities.mouse);
    let _ = rt.heap.record_set_bool(record, 4, capabilities.resize);
    let _ = rt.heap.record_set_bool(record, 5, capabilities.clipboard);
    let _ = rt.heap.record_set_int(record, 6, capabilities.width as i64);
    let _ = rt.heap.record_set_int(record, 7, capabilities.height as i64);
    ui_store_value(rt, record, UiValue::TuiCapabilities(capabilities))
}

fn ui_decode_tui_capabilities(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetTuiCapabilities> {
    if let Some(UiValue::TuiCapabilities(capabilities)) = ui_value(rt, handle) {
        return Some(capabilities);
    }
    let profile = match rt.heap.record_get_int(rt.heap.record_get_record(handle, 0)?, 0)? {
        0 => ui_rt::JetTuiColorProfile::Ansi16,
        1 => ui_rt::JetTuiColorProfile::Ansi256,
        2 => ui_rt::JetTuiColorProfile::TrueColor,
        3 => ui_rt::JetTuiColorProfile::Ascii,
        _ => return None,
    };
    Some(ui_rt::JetTuiCapabilities {
        profile,
        color: rt.heap.record_get_bool(handle, 1)?,
        unicode: rt.heap.record_get_bool(handle, 2)?,
        mouse: rt.heap.record_get_bool(handle, 3)?,
        resize: rt.heap.record_get_bool(handle, 4)?,
        clipboard: rt.heap.record_get_bool(handle, 5)?,
        width: rt.heap.record_get_int(handle, 6)?.max(1) as usize,
        height: rt.heap.record_get_int(handle, 7)?.max(1) as usize,
    })
}

fn ui_push_tui_event(
    rt: &mut crate::runtime_host::JitRuntime,
    event: ui_rt::JetTuiEvent,
) -> i64 {
    let record = match &event {
        ui_rt::JetTuiEvent::Key { .. } | ui_rt::JetTuiEvent::Timer { .. } | ui_rt::JetTuiEvent::Io { .. } => {
            rt.heap.alloc_record(3)
        }
        ui_rt::JetTuiEvent::Resize { .. } | ui_rt::JetTuiEvent::Focus { .. } => {
            rt.heap.alloc_record(2)
        }
        ui_rt::JetTuiEvent::Interrupt | ui_rt::JetTuiEvent::Close => rt.heap.alloc_record(1),
    };
    match &event {
        ui_rt::JetTuiEvent::Key { code, modifiers } => {
            let _ = rt.heap.record_set_int(record, 0, 0);
            let code = rt.heap.alloc_string(code.clone());
            let _ = rt.heap.record_set_string(record, 1, code);
            let _ = rt.heap.record_set_int(record, 2, i64::from(*modifiers));
        }
        ui_rt::JetTuiEvent::Resize { size } => {
            let size_record = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_float(size_record, 0, size.width);
            let _ = rt.heap.record_set_float(size_record, 1, size.height);
            let _ = rt.heap.record_set_int(record, 0, 1);
            let _ = rt.heap.record_set_record(record, 1, size_record);
        }
        ui_rt::JetTuiEvent::Timer { id, elapsed_ms } => {
            let _ = rt.heap.record_set_int(record, 0, 2);
            let id = rt.heap.alloc_string(id.clone());
            let _ = rt.heap.record_set_string(record, 1, id);
            let _ = rt.heap.record_set_int(record, 2, *elapsed_ms);
        }
        ui_rt::JetTuiEvent::Io { channel, payload } => {
            let bytes = rt.heap.alloc_empty_list();
            for byte in payload {
                let _ = rt.heap.list_push_int(bytes, i64::from(*byte));
            }
            let _ = rt.heap.record_set_int(record, 0, 3);
            let channel = rt.heap.alloc_string(channel.clone());
            let _ = rt.heap.record_set_string(record, 1, channel);
            let _ = rt.heap.record_set_int(record, 2, bytes);
        }
        ui_rt::JetTuiEvent::Focus { focused } => {
            let _ = rt.heap.record_set_int(record, 0, 4);
            let _ = rt.heap.record_set_bool(record, 1, *focused);
        }
        ui_rt::JetTuiEvent::Interrupt => {
            let _ = rt.heap.record_set_int(record, 0, 5);
        }
        ui_rt::JetTuiEvent::Close => {
            let _ = rt.heap.record_set_int(record, 0, 6);
        }
    }
    ui_store_value(rt, record, UiValue::TuiEvent(event))
}

fn ui_push_tui_style(
    rt: &mut crate::runtime_host::JitRuntime,
    style: ui_rt::JetTuiStyle,
) -> i64 {
    let mut optional_color = |color: Option<ui_rt::JetTuiColor>| match color {
        Some(color) => {
            let value = ui_push_tui_color(rt, color);
            crate::runtime_host::alloc_jit_result(rt, true, value as u64)
        }
        None => crate::runtime_host::alloc_jit_result(rt, false, 0),
    };
    let foreground = optional_color(style.foreground);
    let background = optional_color(style.background);
    let record = rt.heap.alloc_record(5);
    let _ = rt.heap.record_set_int(record, 0, foreground);
    let _ = rt.heap.record_set_int(record, 1, background);
    let _ = rt.heap.record_set_bool(record, 2, style.bold);
    let _ = rt.heap.record_set_bool(record, 3, style.dim);
    let _ = rt.heap.record_set_bool(record, 4, style.underline);
    ui_store_value(rt, record, UiValue::TuiStyle(style))
}

fn ui_decode_tui_optional_color(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<Option<ui_rt::JetTuiColor>> {
    let (present, value) = crate::runtime_host::jit_result_parts(rt, handle)?;
    if present {
        Some(Some(ui_decode_tui_color(rt, value as i64)?))
    } else {
        Some(None)
    }
}

fn ui_decode_tui_style(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetTuiStyle> {
    if let Some(UiValue::TuiStyle(style)) = ui_value(rt, handle) {
        return Some(style);
    }
    Some(ui_rt::JetTuiStyle {
        foreground: ui_decode_tui_optional_color(rt, rt.heap.record_get_int(handle, 0)?)?,
        background: ui_decode_tui_optional_color(rt, rt.heap.record_get_int(handle, 1)?)?,
        bold: rt.heap.record_get_bool(handle, 2)?,
        dim: rt.heap.record_get_bool(handle, 3)?,
        underline: rt.heap.record_get_bool(handle, 4)?,
    })
}

fn ui_push_tui_constraint(
    rt: &mut crate::runtime_host::JitRuntime,
    constraint: ui_rt::JetTuiConstraint,
) -> i64 {
    let record = rt.heap.alloc_record(2);
    let (discriminant, value) = match constraint {
        ui_rt::JetTuiConstraint::Length(value) => (0, value),
        ui_rt::JetTuiConstraint::Min(value) => (1, value),
        ui_rt::JetTuiConstraint::Max(value) => (2, value),
        ui_rt::JetTuiConstraint::Percent(value) => (3, value),
        ui_rt::JetTuiConstraint::Fill(value) => (4, f64::from(value)),
    };
    let _ = rt.heap.record_set_int(record, 0, discriminant);
    let _ = rt.heap.record_set_float(record, 1, value);
    ui_store_value(rt, record, UiValue::TuiConstraint(constraint))
}

fn ui_decode_tui_constraint(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetTuiConstraint> {
    if let Some(UiValue::TuiConstraint(constraint)) = ui_value(rt, handle) {
        return Some(constraint);
    }
    let value = rt.heap.record_get_float(handle, 1)?;
    match rt.heap.record_get_int(handle, 0)? {
        0 => Some(ui_rt::JetTuiConstraint::Length(value)),
        1 => Some(ui_rt::JetTuiConstraint::Min(value)),
        2 => Some(ui_rt::JetTuiConstraint::Max(value)),
        3 => Some(ui_rt::JetTuiConstraint::Percent(value)),
        4 => Some(ui_rt::JetTuiConstraint::Fill(value.max(0.0).round().min(u16::MAX as f64) as u16)),
        _ => None,
    }
}

fn ui_push_tui_direction(
    rt: &mut crate::runtime_host::JitRuntime,
    direction: ui_rt::JetTuiDirection,
) -> i64 {
    let record = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(
        record,
        0,
        match direction {
            ui_rt::JetTuiDirection::Horizontal => 0,
            ui_rt::JetTuiDirection::Vertical => 1,
        },
    );
    ui_store_value(rt, record, UiValue::TuiDirection(direction))
}

fn ui_decode_tui_direction(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetTuiDirection> {
    if let Some(UiValue::TuiDirection(direction)) = ui_value(rt, handle) {
        return Some(direction);
    }
    match rt.heap.record_get_int(handle, 0)? {
        0 => Some(ui_rt::JetTuiDirection::Horizontal),
        1 => Some(ui_rt::JetTuiDirection::Vertical),
        _ => None,
    }
}

fn ui_push_tui_list_state(
    rt: &mut crate::runtime_host::JitRuntime,
    state: ui_rt::JetTuiListState,
) -> i64 {
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(record, 0, state.selected() as i64);
    let _ = rt.heap.record_set_int(record, 1, state.offset() as i64);
    ui_store_value(rt, record, UiValue::TuiListState(state))
}

fn ui_decode_tui_list_state(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetTuiListState> {
    if let Some(UiValue::TuiListState(state)) = ui_value(rt, handle) {
        return Some(state);
    }
    let mut state = ui_rt::JetTuiListState::default();
    state.select(rt.heap.record_get_int(handle, 0)?.max(0) as usize);
    state.scroll_to(rt.heap.record_get_int(handle, 1)?.max(0) as usize);
    Some(state)
}

fn ui_push_font_style(
    rt: &mut crate::runtime_host::JitRuntime,
    style: ui_rt::JetFontStyle,
) -> i64 {
    let record = rt.heap.alloc_record(1);
    let discriminant = match style {
        ui_rt::JetFontStyle::Body => 0,
        ui_rt::JetFontStyle::Title => 1,
        ui_rt::JetFontStyle::Monospace => 2,
    };
    let _ = rt.heap.record_set_int(record, 0, discriminant);
    ui_store_value(rt, record, UiValue::FontStyle(style))
}

fn ui_decode_font_style(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetFontStyle> {
    if let Some(UiValue::FontStyle(style)) = ui_value(rt, handle) {
        return Some(style);
    }
    match rt.heap.record_get_int(handle, 0)? {
        0 => Some(ui_rt::JetFontStyle::Body),
        1 => Some(ui_rt::JetFontStyle::Title),
        2 => Some(ui_rt::JetFontStyle::Monospace),
        _ => None,
    }
}

fn ui_push_glyph_shaper(
    rt: &mut crate::runtime_host::JitRuntime,
    shaper: ui_rt::JetGlyphShaper,
) -> i64 {
    let record = rt.heap.alloc_record(1);
    let discriminant = match shaper {
        ui_rt::JetGlyphShaper::HarfBuzz => 0,
        ui_rt::JetGlyphShaper::HeadlessFallback => 1,
    };
    let _ = rt.heap.record_set_int(record, 0, discriminant);
    ui_store_value(rt, record, UiValue::GlyphShaper(shaper))
}

fn ui_decode_glyph_shaper(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetGlyphShaper> {
    if let Some(UiValue::GlyphShaper(shaper)) = ui_value(rt, handle) {
        return Some(shaper);
    }
    match rt.heap.record_get_int(handle, 0)? {
        0 => Some(ui_rt::JetGlyphShaper::HarfBuzz),
        1 => Some(ui_rt::JetGlyphShaper::HeadlessFallback),
        _ => None,
    }
}

fn ui_push_font_face(
    rt: &mut crate::runtime_host::JitRuntime,
    face: ui_rt::JetFontFace,
) -> i64 {
    let family = rt.heap.alloc_string(face.family.clone());
    let style = ui_push_font_style(rt, face.style);
    let record = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_string(record, 0, family);
    let _ = rt.heap.record_set_float(record, 1, face.size);
    let _ = rt.heap.record_set_record(record, 2, style);
    ui_store_value(rt, record, UiValue::FontFace(face))
}

fn ui_decode_font_face(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetFontFace> {
    if let Some(UiValue::FontFace(face)) = ui_value(rt, handle) {
        return Some(face);
    }
    let family = rt.heap.record_clone_string(handle, 0)?;
    let size = rt.heap.record_get_float(handle, 1)?;
    let style = ui_decode_font_style(rt, rt.heap.record_get_record(handle, 2)?)?;
    Some(ui_rt::JetFontFace { family, size, style })
}

fn ui_push_glyph_run(
    rt: &mut crate::runtime_host::JitRuntime,
    run: ui_rt::JetGlyphRun,
) -> i64 {
    let glyphs = rt.heap.alloc_empty_list();
    for glyph in &run.glyphs {
        let record = rt.heap.alloc_record(6);
        let _ = rt.heap.record_set_int(record, 0, glyph.id);
        let _ = rt.heap.record_set_int(record, 1, glyph.cluster);
        let _ = rt.heap.record_set_float(record, 2, glyph.x);
        let _ = rt.heap.record_set_float(record, 3, glyph.y);
        let _ = rt.heap.record_set_float(record, 4, glyph.advance_x);
        let _ = rt.heap.record_set_float(record, 5, glyph.advance_y);
        let _ = rt.heap.list_push_int(glyphs, record);
    }
    let shaper = ui_push_glyph_shaper(rt, run.shaper);
    let record = rt.heap.alloc_record(6);
    let _ = rt.heap.record_set_int(record, 0, glyphs);
    let _ = rt.heap.record_set_float(record, 1, run.advance_x);
    let _ = rt.heap.record_set_float(record, 2, run.advance_y);
    let _ = rt.heap.record_set_record(record, 3, shaper);
    let _ = rt.heap.record_set_bool(record, 4, run.deterministic);
    let _ = rt.heap.record_set_bool(record, 5, run.approximate);
    ui_store_value(rt, record, UiValue::GlyphRun(run))
}

fn jet_jit_font_system(style: i64) -> i64 {
    with_rt(|rt| {
        let Some(style) = ui_decode_font_style(rt, style) else {
            rt.set_host_fault("JIT font.system received an invalid FontStyle handle");
            return 0;
        };
        ui_push_font_face(rt, ui_rt::jet_font_system(style))
    })
}

fn jet_jit_font_shape(text: i64, face: i64) -> i64 {
    let Some((text, face)) = with_rt(|rt| {
        let Some(text) = rt.heap.clone_string(text) else {
            rt.set_host_fault("JIT font.shape received a non-string text handle");
            return None;
        };
        let Some(face) = ui_decode_font_face(rt, face) else {
            rt.set_host_fault("JIT font.shape received an invalid FontFace handle");
            return None;
        };
        Some((text, face))
    }) else {
        return 0;
    };
    let run = with_ui_host(|| {
        ui_rt::jet_ui_with_current_host(|host| host.shape_text(&text, &face))
    });
    let run = match run {
        Ok(run) => run,
        Err(error) => {
            with_rt(|rt| {
                rt.set_host_fault(&format!("font shaping host contract failure: {error:?}"));
            });
            return 0;
        }
    };
    with_rt(|rt| ui_push_glyph_run(rt, run))
}
fn ambient_field<'a>(
    value: &'a CtValue,
    type_name: &str,
    field: &str,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    let CtValue::Struct {
        type_name: actual,
        fields,
    } = value
    else {
        return Err(unsupported(
            &format!("{type_name} value expected for `{field}`"),
            span,
        ));
    };
    if actual != type_name {
        return Err(unsupported(
            &format!("expected {type_name}, got {actual}"),
            span,
        ));
    }
    fields
        .iter()
        .find_map(|(name, value)| (name == field).then_some(value))
        .ok_or_else(|| unsupported(&format!("malformed {type_name} value"), span))
}

fn ambient_font_style(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetFontStyle, Diagnostic> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(unsupported("FontStyle value expected", span));
    };
    if type_name != "FontStyle" || !args.is_empty() {
        return Err(unsupported("FontStyle value expected", span));
    }
    match variant.as_str() {
        "Body" => Ok(ui_rt::JetFontStyle::Body),
        "Title" => Ok(ui_rt::JetFontStyle::Title),
        "Monospace" => Ok(ui_rt::JetFontStyle::Monospace),
        _ => Err(unsupported("unknown FontStyle variant", span)),
    }
}

fn ambient_font_face(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetFontFace, Diagnostic> {
    let family = match ambient_field(value, "FontFace", "family", span)? {
        CtValue::Str(value) => value.clone(),
        _ => return Err(unsupported("FontFace.family must be String", span)),
    };
    let size = match ambient_field(value, "FontFace", "size", span)? {
        CtValue::Float(value) => value.as_f64(),
        _ => return Err(unsupported("FontFace.size must be Float", span)),
    };
    let style = ambient_font_style(ambient_field(value, "FontFace", "style", span)?, span)?;
    Ok(ui_rt::JetFontFace {
        family,
        size,
        style,
    })
}

fn ambient_font_style_value(style: ui_rt::JetFontStyle) -> CtValue {
    let variant = match style {
        ui_rt::JetFontStyle::Body => "Body",
        ui_rt::JetFontStyle::Title => "Title",
        ui_rt::JetFontStyle::Monospace => "Monospace",
    };
    CtValue::Enum {
        type_name: "FontStyle".to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn ambient_font_face_value(face: ui_rt::JetFontFace) -> CtValue {
    CtValue::Struct {
        type_name: "FontFace".to_string(),
        fields: vec![
            ("family".to_string(), CtValue::Str(face.family)),
            ("size".to_string(), CtValue::Float(CtFloat::f64(face.size))),
            ("style".to_string(), ambient_font_style_value(face.style)),
        ],
    }
}

fn ambient_glyph_value(glyph: ui_rt::JetGlyph) -> CtValue {
    CtValue::Struct {
        type_name: "Glyph".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Int(glyph.id)),
            ("cluster".to_string(), CtValue::Int(glyph.cluster)),
            ("x".to_string(), CtValue::Float(CtFloat::f64(glyph.x))),
            ("y".to_string(), CtValue::Float(CtFloat::f64(glyph.y))),
            (
                "advance_x".to_string(),
                CtValue::Float(CtFloat::f64(glyph.advance_x)),
            ),
            (
                "advance_y".to_string(),
                CtValue::Float(CtFloat::f64(glyph.advance_y)),
            ),
        ],
    }
}

fn ambient_glyph_shaper_value(shaper: ui_rt::JetGlyphShaper) -> CtValue {
    let variant = match shaper {
        ui_rt::JetGlyphShaper::HarfBuzz => "HarfBuzz",
        ui_rt::JetGlyphShaper::HeadlessFallback => "HeadlessFallback",
    };
    CtValue::Enum {
        type_name: "GlyphShaper".to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn ambient_glyph_run_value(run: ui_rt::JetGlyphRun) -> CtValue {
    CtValue::Struct {
        type_name: "GlyphRun".to_string(),
        fields: vec![
            (
                "glyphs".to_string(),
                CtValue::List(run.glyphs.into_iter().map(ambient_glyph_value).collect()),
            ),
            (
                "advance_x".to_string(),
                CtValue::Float(CtFloat::f64(run.advance_x)),
            ),
            (
                "advance_y".to_string(),
                CtValue::Float(CtFloat::f64(run.advance_y)),
            ),
            (
                "shaper".to_string(),
                ambient_glyph_shaper_value(run.shaper),
            ),
            ("deterministic".to_string(), CtValue::Bool(run.deterministic)),
            ("approximate".to_string(), CtValue::Bool(run.approximate)),
        ],
    }
}
fn ambient_absent(type_name: &str) -> CtValue {
    CtValue::absent(Box::new(Type::Named(type_name.to_string())))
}

fn ambient_optional<'a>(
    value: &'a CtValue,
    type_name: &str,
    span: Span,
) -> Result<Option<&'a CtValue>, Diagnostic> {
    match value {
        CtValue::Present(value) => Ok(Some(value)),
        value if value.is_clean_stop() => Ok(None),
        _ => Err(unsupported(
            &format!("expected optional {type_name} value"),
            span,
        )),
    }
}

fn ambient_string(value: &CtValue, type_name: &str, span: Span) -> Result<String, Diagnostic> {
    match value {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(unsupported(
            &format!("{type_name} value must be String"),
            span,
        )),
    }
}

fn ambient_int(value: &CtValue, type_name: &str, span: Span) -> Result<i64, Diagnostic> {
    match value {
        CtValue::Int(value) => Ok(*value),
        _ => Err(unsupported(
            &format!("{type_name} value must be Int"),
            span,
        )),
    }
}

fn ambient_float(value: &CtValue, type_name: &str, span: Span) -> Result<f64, Diagnostic> {
    match value {
        CtValue::Float(value) => Ok(value.as_f64()),
        _ => Err(unsupported(
            &format!("{type_name} value must be Float"),
            span,
        )),
    }
}

fn ambient_bool(value: &CtValue, type_name: &str, span: Span) -> Result<bool, Diagnostic> {
    match value {
        CtValue::Bool(value) => Ok(*value),
        _ => Err(unsupported(
            &format!("{type_name} value must be Bool"),
            span,
        )),
    }
}

fn ambient_list<'a>(
    value: &'a CtValue,
    type_name: &str,
    span: Span,
) -> Result<&'a [CtValue], Diagnostic> {
    match value {
        CtValue::List(values) => Ok(values),
        _ => Err(unsupported(
            &format!("{type_name} value must be List"),
            span,
        )),
    }
}

fn ambient_strings(
    value: &CtValue,
    type_name: &str,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    ambient_list(value, type_name, span)?
        .iter()
        .map(|value| ambient_string(value, type_name, span))
        .collect()
}

fn ambient_enum(
    value: &CtValue,
    type_name: &str,
    span: Span,
) -> Result<String, Diagnostic> {
    let CtValue::Enum {
        type_name: actual,
        variant,
        args,
    } = value
    else {
        return Err(unsupported(&format!("{type_name} value expected"), span));
    };
    if actual != type_name || !args.is_empty() {
        return Err(unsupported(&format!("{type_name} value expected"), span));
    }
    Ok(variant.clone())
}

fn ambient_enum_arg<'a>(
    value: &'a CtValue,
    type_name: &str,
    variant: &str,
    index: usize,
    span: Span,
) -> Result<&'a CtValue, Diagnostic> {
    let CtValue::Enum {
        type_name: actual,
        variant: actual_variant,
        args,
    } = value
    else {
        return Err(unsupported(&format!("{type_name} value expected"), span));
    };
    if actual != type_name || actual_variant != variant {
        return Err(unsupported(&format!("expected {type_name}::{variant}"), span));
    }
    args.get(index)
        .map(|(_, value)| value)
        .ok_or_else(|| unsupported(&format!("malformed {type_name}::{variant} value"), span))
}

fn ambient_struct(
    type_name: &str,
    fields: Vec<(String, CtValue)>,
) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields,
    }
}

fn ambient_enum_value(type_name: &str, variant: &str, args: Vec<(Option<String>, CtValue)>) -> CtValue {
    CtValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args,
    }
}

fn ambient_option_value(type_name: &str, value: Option<CtValue>) -> CtValue {
    value.map_or_else(
        || ambient_absent(type_name),
        |value| CtValue::Present(Box::new(value)),
    )
}
fn ambient_fs_access(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFsAccess, Diagnostic> {
    match ambient_enum(value, "UiFsAccess", span)?.as_str() {
        "Read" => Ok(ui_rt::JetUiFsAccess::Read),
        "Write" => Ok(ui_rt::JetUiFsAccess::Write),
        _ => Err(unsupported("unknown UiFsAccess variant", span)),
    }
}

fn ambient_fs_rights(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFsRights, Diagnostic> {
    let bits = ambient_int(ambient_field(value, "UiFsRights", "bits", span)?, "UiFsRights.bits", span)?;
    let bits = u8::try_from(bits).map_err(|_| unsupported("UiFsRights.bits is out of range", span))?;
    Ok(ui_rt::JetUiFsRights::from_bits(bits))
}

fn ambient_node_id(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiNodeId, Diagnostic> {
    let value = ambient_string(
        ambient_field(value, "UiNodeId", "value", span)?,
        "UiNodeId.value",
        span,
    )?;
    ui_rt::JetUiNodeId::new(&value)
        .map_err(|_| unsupported("UiNodeId value is invalid", span))
}

fn ambient_file_filter(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFileFilter, Diagnostic> {
    let label = ambient_string(
        ambient_field(value, "UiFileFilter", "label", span)?,
        "UiFileFilter.label",
        span,
    )?;
    let extensions = ambient_strings(
        ambient_field(value, "UiFileFilter", "extensions", span)?,
        "UiFileFilter.extensions",
        span,
    )?;
    let mime_types = ambient_strings(
        ambient_field(value, "UiFileFilter", "mime_types", span)?,
        "UiFileFilter.mime_types",
        span,
    )?;
    ui_rt::JetUiFileFilter::new(&label, extensions, mime_types)
        .map_err(|error| unsupported(&format!("invalid UiFileFilter: {error:?}"), span))
}

fn ambient_granted_path(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiGrantedPath, Diagnostic> {
    let path = ambient_string(
        ambient_field(value, "UiGrantedPath", "path", span)?,
        "UiGrantedPath.path",
        span,
    )?;
    let grant_root = ambient_string(
        ambient_field(value, "UiGrantedPath", "grant_root", span)?,
        "UiGrantedPath.grant_root",
        span,
    )?;
    let access = ambient_fs_access(
        ambient_field(value, "UiGrantedPath", "access", span)?,
        span,
    )?;
    let grant = ui_rt::JetUiFsGrant::new(&grant_root, match access {
        ui_rt::JetUiFsAccess::Read => ui_rt::JetUiFsRights::read(),
        ui_rt::JetUiFsAccess::Write => ui_rt::JetUiFsRights::write(),
    })
    .map_err(|error| unsupported(&format!("invalid UiGrantedPath grant: {error:?}"), span))?;
    grant.scope(&path, access)
        .map_err(|error| unsupported(&format!("invalid UiGrantedPath: {error:?}"), span))
}

fn ambient_fs_grant(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFsGrant, Diagnostic> {
    let root = ambient_string(
        ambient_field(value, "UiFsGrant", "root", span)?,
        "UiFsGrant.root",
        span,
    )?;
    let rights = ambient_fs_rights(
        ambient_field(value, "UiFsGrant", "rights", span)?,
        span,
    )?;
    ui_rt::JetUiFsGrant::new(&root, rights)
        .map_err(|error| unsupported(&format!("invalid UiFsGrant: {error:?}"), span))
}

fn ambient_file_request(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFileDialogRequest, Diagnostic> {
    let kind = match ambient_enum(
        ambient_field(value, "UiFileDialogRequest", "kind", span)?,
        "UiFileDialogKind",
        span,
    )?
    .as_str()
    {
        "Open" => ui_rt::JetUiFileDialogKind::Open,
        "Save" => ui_rt::JetUiFileDialogKind::Save,
        _ => return Err(unsupported("unknown UiFileDialogKind variant", span)),
    };
    let title = ambient_string(
        ambient_field(value, "UiFileDialogRequest", "title", span)?,
        "UiFileDialogRequest.title",
        span,
    )?;
    let grant = ambient_fs_grant(
        ambient_field(value, "UiFileDialogRequest", "grant", span)?,
        span,
    )?;
    let initial_directory = ambient_optional(
        ambient_field(value, "UiFileDialogRequest", "initial_directory", span)?,
        "UiGrantedPath",
        span,
    )?
    .map(|value| ambient_granted_path(value, span))
    .transpose()?;
    let filters = ambient_list(
        ambient_field(value, "UiFileDialogRequest", "filters", span)?,
        "UiFileDialogRequest.filters",
        span,
    )?
    .iter()
    .map(|value| ambient_file_filter(value, span))
    .collect::<Result<Vec<_>, _>>()?;
    let allow_multiple = ambient_bool(
        ambient_field(value, "UiFileDialogRequest", "allow_multiple", span)?,
        "UiFileDialogRequest.allow_multiple",
        span,
    )?;
    let request = match kind {
        ui_rt::JetUiFileDialogKind::Open => ui_rt::JetUiFileDialogRequest::open(grant),
        ui_rt::JetUiFileDialogKind::Save => ui_rt::JetUiFileDialogRequest::save(grant),
    };
    Ok(ui_rt::JetUiFileDialogRequest {
        kind,
        title,
        grant: request.grant,
        initial_directory,
        filters,
        allow_multiple,
    })
}

fn ambient_file_selection(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFileDialogSelection, Diagnostic> {
    let files = ambient_list(
        ambient_field(value, "UiFileDialogSelection", "files", span)?,
        "UiFileDialogSelection.files",
        span,
    )?
    .iter()
    .map(|value| ambient_granted_path(value, span))
    .collect::<Result<Vec<_>, _>>()?;
    ui_rt::JetUiFileDialogSelection::new(files)
        .map_err(|error| unsupported(&format!("invalid UiFileDialogSelection: {error:?}"), span))
}

fn ambient_text_range(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiTextRange, Diagnostic> {
    let start = ambient_int(
        ambient_field(value, "UiTextRange", "start", span)?,
        "UiTextRange.start",
        span,
    )?;
    let end = ambient_int(
        ambient_field(value, "UiTextRange", "end", span)?,
        "UiTextRange.end",
        span,
    )?;
    let start = usize::try_from(start).map_err(|_| unsupported("UiTextRange.start is negative", span))?;
    let end = usize::try_from(end).map_err(|_| unsupported("UiTextRange.end is negative", span))?;
    ui_rt::JetUiTextRange::new(start, end)
        .map_err(|error| unsupported(&format!("invalid UiTextRange: {error:?}"), span))
}
fn ambient_clipboard_text(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiClipboardText, Diagnostic> {
    let text = ambient_string(
        ambient_field(value, "UiClipboardText", "text", span)?,
        "UiClipboardText.text",
        span,
    )?;
    let selection = ambient_optional(
        ambient_field(value, "UiClipboardText", "selection", span)?,
        "UiTextRange",
        span,
    )?
    .map(|value| ambient_text_range(value, span))
    .transpose()?;
    Ok(ui_rt::JetUiClipboardText { text, selection })
}

fn ambient_shortcut_modifiers(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiShortcutModifiers, Diagnostic> {
    let bits = ambient_int(
        ambient_field(value, "UiShortcutModifiers", "bits", span)?,
        "UiShortcutModifiers.bits",
        span,
    )?;
    let bits = u8::try_from(bits)
        .map_err(|_| unsupported("UiShortcutModifiers.bits is out of range", span))?;
    Ok(ui_rt::JetUiShortcutModifiers::from_bits(bits))
}

fn ambient_shortcut(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiShortcut, Diagnostic> {
    let key = ambient_string(
        ambient_field(value, "UiShortcut", "key", span)?,
        "UiShortcut.key",
        span,
    )?;
    let modifiers = ambient_shortcut_modifiers(
        ambient_field(value, "UiShortcut", "modifiers", span)?,
        span,
    )?;
    ui_rt::JetUiShortcut::new(&key, modifiers)
        .map_err(|error| unsupported(&format!("invalid UiShortcut: {error:?}"), span))
}

fn ambient_shortcut_binding(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiShortcutBinding, Diagnostic> {
    let shortcut = ambient_shortcut(
        ambient_field(value, "UiShortcutBinding", "shortcut", span)?,
        span,
    )?;
    let action = ambient_string(
        ambient_field(value, "UiShortcutBinding", "action", span)?,
        "UiShortcutBinding.action",
        span,
    )?;
    let node = ambient_optional(
        ambient_field(value, "UiShortcutBinding", "node", span)?,
        "UiNodeId",
        span,
    )?
    .map(|value| ambient_node_id(value, span))
    .transpose()?;
    ui_rt::JetUiShortcutBinding::new(shortcut, &action, node)
        .map_err(|error| unsupported(&format!("invalid UiShortcutBinding: {error:?}"), span))
}

fn ambient_accessibility_state(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiAccessibilityState, Diagnostic> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(unsupported("UiAccessibilityState value expected", span));
    };
    if type_name != "UiAccessibilityState" {
        return Err(unsupported("UiAccessibilityState value expected", span));
    }
    match variant.as_str() {
        "Disabled" if args.is_empty() => Ok(ui_rt::JetUiAccessibilityState::Disabled),
        "Busy" if args.is_empty() => Ok(ui_rt::JetUiAccessibilityState::Busy),
        "Required" if args.is_empty() => Ok(ui_rt::JetUiAccessibilityState::Required),
        "Expanded" if args.len() == 1 => Ok(ui_rt::JetUiAccessibilityState::Expanded(
            ambient_bool(&args[0].1, "UiAccessibilityState.Expanded", span)?,
        )),
        "Checked" if args.len() == 1 => Ok(ui_rt::JetUiAccessibilityState::Checked(
            ambient_bool(&args[0].1, "UiAccessibilityState.Checked", span)?,
        )),
        "Selected" if args.len() == 1 => Ok(ui_rt::JetUiAccessibilityState::Selected(
            ambient_bool(&args[0].1, "UiAccessibilityState.Selected", span)?,
        )),
        "Value" if args.len() == 1 => Ok(ui_rt::JetUiAccessibilityState::Value(
            ambient_string(&args[0].1, "UiAccessibilityState.Value", span)?,
        )),
        _ => Err(unsupported(
            "unknown or malformed UiAccessibilityState variant",
            span,
        )),
    }
}

fn ambient_accessibility(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiAccessibility, Diagnostic> {
    let name = ambient_optional(
        ambient_field(value, "UiAccessibility", "name", span)?,
        "String",
        span,
    )?
    .map(|value| ambient_string(value, "UiAccessibility.name", span))
    .transpose()?;
    let description = ambient_optional(
        ambient_field(value, "UiAccessibility", "description", span)?,
        "String",
        span,
    )?
    .map(|value| ambient_string(value, "UiAccessibility.description", span))
    .transpose()?;
    let states = ambient_list(
        ambient_field(value, "UiAccessibility", "states", span)?,
        "UiAccessibility.states",
        span,
    )?
    .iter()
    .map(|value| ambient_accessibility_state(value, span))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(ui_rt::JetUiAccessibility {
        name,
        description,
        states,
    })
}
fn ambient_fs_access_value(access: ui_rt::JetUiFsAccess) -> CtValue {
    ambient_enum_value(
        "UiFsAccess",
        match access {
            ui_rt::JetUiFsAccess::Read => "Read",
            ui_rt::JetUiFsAccess::Write => "Write",
        },
        Vec::new(),
    )
}

fn ambient_fs_rights_value(rights: ui_rt::JetUiFsRights) -> CtValue {
    ambient_struct(
        "UiFsRights",
        vec![("bits".to_string(), CtValue::Int(i64::from(rights.bits())))],
    )
}

fn ambient_node_id_value(node_id: ui_rt::JetUiNodeId) -> CtValue {
    ambient_struct(
        "UiNodeId",
        vec![("value".to_string(), CtValue::Str(node_id.as_str().to_string()))],
    )
}

fn ambient_file_filter_value(filter: ui_rt::JetUiFileFilter) -> CtValue {
    ambient_struct(
        "UiFileFilter",
        vec![
            ("label".to_string(), CtValue::Str(filter.label)),
            (
                "extensions".to_string(),
                CtValue::List(filter.extensions.into_iter().map(CtValue::Str).collect()),
            ),
            (
                "mime_types".to_string(),
                CtValue::List(filter.mime_types.into_iter().map(CtValue::Str).collect()),
            ),
        ],
    )
}

fn ambient_granted_path_value(path: ui_rt::JetUiGrantedPath) -> CtValue {
    ambient_struct(
        "UiGrantedPath",
        vec![
            ("path".to_string(), CtValue::Str(path.path().to_string())),
            (
                "grant_root".to_string(),
                CtValue::Str(path.grant_root().to_string()),
            ),
            ("access".to_string(), ambient_fs_access_value(path.access())),
        ],
    )
}

fn ambient_fs_grant_value(grant: ui_rt::JetUiFsGrant) -> CtValue {
    ambient_struct(
        "UiFsGrant",
        vec![
            ("root".to_string(), CtValue::Str(grant.root().to_string())),
            ("rights".to_string(), ambient_fs_rights_value(grant.rights())),
        ],
    )
}

fn ambient_file_request_value(request: ui_rt::JetUiFileDialogRequest) -> CtValue {
    let kind = ambient_enum_value(
        "UiFileDialogKind",
        match request.kind {
            ui_rt::JetUiFileDialogKind::Open => "Open",
            ui_rt::JetUiFileDialogKind::Save => "Save",
        },
        Vec::new(),
    );
    let initial_directory = request
        .initial_directory
        .map(ambient_granted_path_value);
    ambient_struct(
        "UiFileDialogRequest",
        vec![
            ("kind".to_string(), kind),
            ("title".to_string(), CtValue::Str(request.title)),
            ("grant".to_string(), ambient_fs_grant_value(request.grant)),
            (
                "initial_directory".to_string(),
                ambient_option_value("UiGrantedPath", initial_directory),
            ),
            (
                "filters".to_string(),
                CtValue::List(
                    request
                        .filters
                        .into_iter()
                        .map(ambient_file_filter_value)
                        .collect(),
                ),
            ),
            ("allow_multiple".to_string(), CtValue::Bool(request.allow_multiple)),
        ],
    )
}

fn ambient_file_selection_value(selection: ui_rt::JetUiFileDialogSelection) -> CtValue {
    ambient_struct(
        "UiFileDialogSelection",
        vec![(
            "files".to_string(),
            CtValue::List(
                selection
                    .files
                    .into_iter()
                    .map(ambient_granted_path_value)
                    .collect(),
            ),
        )],
    )
}

fn ambient_text_range_value(range: ui_rt::JetUiTextRange) -> CtValue {
    ambient_struct(
        "UiTextRange",
        vec![
            ("start".to_string(), CtValue::Int(i64::try_from(range.start).unwrap_or(i64::MAX))),
            ("end".to_string(), CtValue::Int(i64::try_from(range.end).unwrap_or(i64::MAX))),
        ],
    )
}

fn ambient_clipboard_text_value(value: ui_rt::JetUiClipboardText) -> CtValue {
    ambient_struct(
        "UiClipboardText",
        vec![
            ("text".to_string(), CtValue::Str(value.text)),
            (
                "selection".to_string(),
                ambient_option_value("UiTextRange", value.selection.map(ambient_text_range_value)),
            ),
        ],
    )
}

fn ambient_clipboard_write_value(value: ui_rt::JetUiClipboardWrite) -> CtValue {
    ambient_struct(
        "UiClipboardWrite",
        vec![(
            "characters".to_string(),
            CtValue::Int(i64::try_from(value.characters).unwrap_or(i64::MAX)),
        )],
    )
}

fn ambient_shortcut_modifiers_value(
    modifiers: ui_rt::JetUiShortcutModifiers,
) -> CtValue {
    ambient_struct(
        "UiShortcutModifiers",
        vec![("bits".to_string(), CtValue::Int(i64::from(modifiers.bits())))],
    )
}

fn ambient_shortcut_value(shortcut: ui_rt::JetUiShortcut) -> CtValue {
    ambient_struct(
        "UiShortcut",
        vec![
            ("key".to_string(), CtValue::Str(shortcut.key)),
            (
                "modifiers".to_string(),
                ambient_shortcut_modifiers_value(shortcut.modifiers),
            ),
        ],
    )
}

fn ambient_shortcut_binding_value(binding: ui_rt::JetUiShortcutBinding) -> CtValue {
    ambient_struct(
        "UiShortcutBinding",
        vec![
            (
                "shortcut".to_string(),
                ambient_shortcut_value(binding.shortcut),
            ),
            ("action".to_string(), CtValue::Str(binding.action)),
            (
                "node".to_string(),
                ambient_option_value("UiNodeId", binding.node.map(ambient_node_id_value)),
            ),
        ],
    )
}

fn ambient_shortcut_dispatch_value(dispatch: ui_rt::JetUiShortcutDispatch) -> CtValue {
    match dispatch {
        ui_rt::JetUiShortcutDispatch::Dispatched(binding) => ambient_enum_value(
            "UiShortcutDispatch",
            "Dispatched",
            vec![(None, ambient_shortcut_binding_value(binding))],
        ),
        ui_rt::JetUiShortcutDispatch::Unhandled => {
            ambient_enum_value("UiShortcutDispatch", "Unhandled", Vec::new())
        }
    }
}
fn ambient_ime_mode(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiImeMode, Diagnostic> {
    match ambient_enum(value, "UiImeMode", span)?.as_str() {
        "Native" => Ok(ui_rt::JetUiImeMode::Native),
        "Disabled" => Ok(ui_rt::JetUiImeMode::Disabled),
        _ => Err(unsupported("unknown UiImeMode variant", span)),
    }
}
fn ambient_ime_mode_value(mode: ui_rt::JetUiImeMode) -> CtValue {
    ambient_enum_value(
        "UiImeMode",
        match mode {
            ui_rt::JetUiImeMode::Native => "Native",
            ui_rt::JetUiImeMode::Disabled => "Disabled",
        },
        Vec::new(),
    )
}

fn ambient_ime_phase(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiImePhase, Diagnostic> {
    match ambient_enum(value, "UiImePhase", span)?.as_str() {
        "Start" => Ok(ui_rt::JetUiImePhase::Start),
        "Update" => Ok(ui_rt::JetUiImePhase::Update),
        "Commit" => Ok(ui_rt::JetUiImePhase::Commit),
        "Cancel" => Ok(ui_rt::JetUiImePhase::Cancel),
        _ => Err(unsupported("unknown UiImePhase variant", span)),
    }
}

fn ambient_ime_composition(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiImeComposition, Diagnostic> {
    let text = ambient_string(
        ambient_field(value, "UiImeComposition", "text", span)?,
        "UiImeComposition.text",
        span,
    )?;
    let selection = ambient_text_range(
        ambient_field(value, "UiImeComposition", "selection", span)?,
        span,
    )?;
    let marked = ambient_optional(
        ambient_field(value, "UiImeComposition", "marked", span)?,
        "UiTextRange",
        span,
    )?
    .map(|value| ambient_text_range(value, span))
    .transpose()?;
    Ok(ui_rt::JetUiImeComposition {
        text,
        selection,
        marked,
    })
}

fn ambient_ime_event(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiImeEvent, Diagnostic> {
    let target = ambient_node_id(
        ambient_field(value, "UiImeEvent", "target", span)?,
        span,
    )?;
    let phase = ambient_ime_phase(
        ambient_field(value, "UiImeEvent", "phase", span)?,
        span,
    )?;
    let composition = ambient_optional(
        ambient_field(value, "UiImeEvent", "composition", span)?,
        "UiImeComposition",
        span,
    )?
    .map(|value| ambient_ime_composition(value, span))
    .transpose()?;
    Ok(ui_rt::JetUiImeEvent {
        target,
        phase,
        composition,
    })
}

fn ambient_drag_phase(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiDragPhase, Diagnostic> {
    match ambient_enum(value, "UiDragPhase", span)?.as_str() {
        "Enter" => Ok(ui_rt::JetUiDragPhase::Enter),
        "Over" => Ok(ui_rt::JetUiDragPhase::Over),
        "Drop" => Ok(ui_rt::JetUiDragPhase::Drop),
        "Leave" => Ok(ui_rt::JetUiDragPhase::Leave),
        "Cancel" => Ok(ui_rt::JetUiDragPhase::Cancel),
        _ => Err(unsupported("unknown UiDragPhase variant", span)),
    }
}

fn ambient_drag_operation(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiDragOperation, Diagnostic> {
    match ambient_enum(value, "UiDragOperation", span)?.as_str() {
        "Copy" => Ok(ui_rt::JetUiDragOperation::Copy),
        "Move" => Ok(ui_rt::JetUiDragOperation::Move),
        "Link" => Ok(ui_rt::JetUiDragOperation::Link),
        _ => Err(unsupported("unknown UiDragOperation variant", span)),
    }
}

fn ambient_drop_item(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiDropItem, Diagnostic> {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(unsupported("UiDropItem value expected", span));
    };
    if type_name != "UiDropItem" || args.len() != 1 {
        return Err(unsupported("malformed UiDropItem value", span));
    }
    match variant.as_str() {
        "Text" => Ok(ui_rt::JetUiDropItem::Text(ambient_string(
            &args[0].1,
            "UiDropItem.Text",
            span,
        )?)),
        "Uri" => Ok(ui_rt::JetUiDropItem::Uri(ambient_string(
            &args[0].1,
            "UiDropItem.Uri",
            span,
        )?)),
        "File" => Ok(ui_rt::JetUiDropItem::File(ambient_granted_path(
            &args[0].1,
            span,
        )?)),
        _ => Err(unsupported("unknown UiDropItem variant", span)),
    }
}

fn ambient_drag_event(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiDragEvent, Diagnostic> {
    let target = ambient_node_id(
        ambient_field(value, "UiDragEvent", "target", span)?,
        span,
    )?;
    let phase = ambient_drag_phase(
        ambient_field(value, "UiDragEvent", "phase", span)?,
        span,
    )?;
    let operation = ambient_drag_operation(
        ambient_field(value, "UiDragEvent", "operation", span)?,
        span,
    )?;
    let items = ambient_list(
        ambient_field(value, "UiDragEvent", "items", span)?,
        "UiDragEvent.items",
        span,
    )?
    .iter()
    .map(|value| ambient_drop_item(value, span))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(ui_rt::JetUiDragEvent {
        target,
        phase,
        operation,
        items,
    })
}

fn ambient_aria_role(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetAriaRole, Diagnostic> {
    match ambient_enum(value, "UiAriaRole", span)?.as_str() {
        "Button" => Ok(ui_rt::JetAriaRole::Button),
        "TextInput" => Ok(ui_rt::JetAriaRole::TextInput),
        "Label" => Ok(ui_rt::JetAriaRole::Label),
        "Container" => Ok(ui_rt::JetAriaRole::Container),
        _ => Err(unsupported("unknown UiAriaRole variant", span)),
    }
}

fn ambient_node(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiNode, Diagnostic> {
    let label = ambient_string(
        ambient_field(value, "UiNode", "label", span)?,
        "UiNode.label",
        span,
    )?;
    let width = ambient_float(
        ambient_field(value, "UiNode", "width", span)?,
        "UiNode.width",
        span,
    )?;
    let height = ambient_float(
        ambient_field(value, "UiNode", "height", span)?,
        "UiNode.height",
        span,
    )?;
    let role = ambient_optional(
        ambient_field(value, "UiNode", "role", span)?,
        "UiAriaRole",
        span,
    )?
    .map(|value| ambient_aria_role(value, span))
    .transpose()?;
    let accessibility = ambient_optional(
        ambient_field(value, "UiNode", "accessibility", span)?,
        "UiAccessibility",
        span,
    )?
    .map(|value| ambient_accessibility(value, span))
    .transpose()?;
    let ime = ambient_optional(
        ambient_field(value, "UiNode", "ime", span)?,
        "UiImeMode",
        span,
    )?
    .map(|value| ambient_ime_mode(value, span))
    .transpose()?;
    let color = ambient_optional(
        ambient_field(value, "UiNode", "color", span)?,
        "String",
        span,
    )?
    .map(|value| ambient_string(value, "UiNode.color", span))
    .transpose()?;
    let kind = match ambient_enum(
        ambient_field(value, "UiNode", "kind", span)?,
        "UiNodeKind",
        span,
    )?
    .as_str()
    {
        "Custom" => ui_rt::JetUiNodeKind::Custom,
        "Text" => ui_rt::JetUiNodeKind::Text,
        "Box" => ui_rt::JetUiNodeKind::Box,
        "Button" => ui_rt::JetUiNodeKind::Button,
        "TextInput" => ui_rt::JetUiNodeKind::TextInput,
        _ => return Err(unsupported("unknown UiNodeKind variant", span)),
    };
    let children = ambient_list(
        ambient_field(value, "UiNode", "children", span)?,
        "UiNode.children",
        span,
    )?
    .iter()
    .map(|value| ambient_node(value, span))
    .collect::<Result<Vec<_>, _>>()?;
    let shortcut = ambient_optional(
        ambient_field(value, "UiNode", "shortcut", span)?,
        "UiShortcut",
        span,
    )?
    .map(|value| ambient_shortcut(value, span))
    .transpose()?;
    Ok(ui_rt::JetUiNode {
        label,
        width,
        height,
        role,
        accessibility,
        ime,
        color,
        style: None,
        kind,
        children,
        key: None,
        on_click: None,
        on_drop: None,
        shortcut,
    })
}
fn ambient_accessibility_state_value(
    state: ui_rt::JetUiAccessibilityState,
) -> CtValue {
    let (variant, args) = match state {
        ui_rt::JetUiAccessibilityState::Disabled => ("Disabled", Vec::new()),
        ui_rt::JetUiAccessibilityState::Busy => ("Busy", Vec::new()),
        ui_rt::JetUiAccessibilityState::Required => ("Required", Vec::new()),
        ui_rt::JetUiAccessibilityState::Expanded(value) => {
            ("Expanded", vec![(None, CtValue::Bool(value))])
        }
        ui_rt::JetUiAccessibilityState::Checked(value) => {
            ("Checked", vec![(None, CtValue::Bool(value))])
        }
        ui_rt::JetUiAccessibilityState::Selected(value) => {
            ("Selected", vec![(None, CtValue::Bool(value))])
        }
        ui_rt::JetUiAccessibilityState::Value(value) => {
            ("Value", vec![(None, CtValue::Str(value))])
        }
    };
    ambient_enum_value("UiAccessibilityState", variant, args)
}

fn ambient_accessibility_value(value: ui_rt::JetUiAccessibility) -> CtValue {
    ambient_struct(
        "UiAccessibility",
        vec![
            (
                "name".to_string(),
                ambient_option_value("String", value.name.map(CtValue::Str)),
            ),
            (
                "description".to_string(),
                ambient_option_value("String", value.description.map(CtValue::Str)),
            ),
            (
                "states".to_string(),
                CtValue::List(
                    value
                        .states
                        .into_iter()
                        .map(ambient_accessibility_state_value)
                        .collect(),
                ),
            ),
        ],
    )
}

fn ambient_capability_value(capability: ui_rt::JetUiCapability) -> CtValue {
    ambient_enum_value(
        "UiCapability",
        match capability {
            ui_rt::JetUiCapability::FileDialog => "FileDialog",
            ui_rt::JetUiCapability::Clipboard => "Clipboard",
            ui_rt::JetUiCapability::Ime => "Ime",
            ui_rt::JetUiCapability::DragDrop => "DragDrop",
            ui_rt::JetUiCapability::Shortcuts => "Shortcuts",
            ui_rt::JetUiCapability::Accessibility => "Accessibility",
        },
        Vec::new(),
    )
}

fn ambient_capability_facts_value(facts: ui_rt::JetUiCapabilityFacts) -> CtValue {
    let facts = facts
        .to_facts()
        .into_iter()
        .map(|fact| {
            ambient_struct(
                "UiCapabilityFact",
                vec![
                    ("capability".to_string(), ambient_capability_value(fact.capability)),
                    ("granted".to_string(), CtValue::Bool(fact.granted)),
                ],
            )
        })
        .collect();
    ambient_struct("UiCapabilityFacts", vec![("facts".to_string(), CtValue::List(facts))])
}

fn ambient_node_value(node: ui_rt::JetUiNode) -> CtValue {
    let children = node
        .children
        .into_iter()
        .map(ambient_node_value)
        .collect();
    let role = node.role.map(|role| {
        ambient_enum_value(
            "UiAriaRole",
            match role {
                ui_rt::JetAriaRole::Button => "Button",
                ui_rt::JetAriaRole::TextInput => "TextInput",
                ui_rt::JetAriaRole::Label => "Label",
                ui_rt::JetAriaRole::Container => "Container",
            },
            Vec::new(),
        )
    });
    let kind = ambient_enum_value(
        "UiNodeKind",
        match node.kind {
            ui_rt::JetUiNodeKind::Custom => "Custom",
            ui_rt::JetUiNodeKind::Text => "Text",
            ui_rt::JetUiNodeKind::Box => "Box",
            ui_rt::JetUiNodeKind::Button => "Button",
            ui_rt::JetUiNodeKind::TextInput => "TextInput",
        },
        Vec::new(),
    );
    ambient_struct(
        "UiNode",
        vec![
            ("label".to_string(), CtValue::Str(node.label)),
            ("width".to_string(), CtValue::Float(CtFloat::f64(node.width))),
            ("height".to_string(), CtValue::Float(CtFloat::f64(node.height))),
            (
                "role".to_string(),
                ambient_option_value("UiAriaRole", role),
            ),
            (
                "accessibility".to_string(),
                ambient_option_value(
                    "UiAccessibility",
                    node.accessibility.map(ambient_accessibility_value),
                ),
            ),
            (
                "ime".to_string(),
                ambient_option_value("UiImeMode", node.ime.map(ambient_ime_mode_value)),
            ),
            (
                "color".to_string(),
                ambient_option_value("String", node.color.map(CtValue::Str)),
            ),
            ("kind".to_string(), kind),
            ("children".to_string(), CtValue::List(children)),
            (
                "shortcut".to_string(),
                ambient_option_value("UiShortcut", node.shortcut.map(ambient_shortcut_value)),
            ),
        ],
    )
}

fn ambient_ime_phase_value(phase: ui_rt::JetUiImePhase) -> CtValue {
    ambient_enum_value(
        "UiImePhase",
        match phase {
            ui_rt::JetUiImePhase::Start => "Start",
            ui_rt::JetUiImePhase::Update => "Update",
            ui_rt::JetUiImePhase::Commit => "Commit",
            ui_rt::JetUiImePhase::Cancel => "Cancel",
        },
        Vec::new(),
    )
}

fn ambient_ime_composition_value(
    value: ui_rt::JetUiImeComposition,
) -> CtValue {
    ambient_struct(
        "UiImeComposition",
        vec![
            ("text".to_string(), CtValue::Str(value.text)),
            (
                "selection".to_string(),
                ambient_text_range_value(value.selection),
            ),
            (
                "marked".to_string(),
                ambient_option_value("UiTextRange", value.marked.map(ambient_text_range_value)),
            ),
        ],
    )
}

fn ambient_ime_event_value(value: ui_rt::JetUiImeEvent) -> CtValue {
    ambient_struct(
        "UiImeEvent",
        vec![
            ("target".to_string(), ambient_node_id_value(value.target)),
            ("phase".to_string(), ambient_ime_phase_value(value.phase)),
            (
                "composition".to_string(),
                ambient_option_value(
                    "UiImeComposition",
                    value.composition.map(ambient_ime_composition_value),
                ),
            ),
        ],
    )
}

fn ambient_drag_phase_value(phase: ui_rt::JetUiDragPhase) -> CtValue {
    ambient_enum_value(
        "UiDragPhase",
        match phase {
            ui_rt::JetUiDragPhase::Enter => "Enter",
            ui_rt::JetUiDragPhase::Over => "Over",
            ui_rt::JetUiDragPhase::Drop => "Drop",
            ui_rt::JetUiDragPhase::Leave => "Leave",
            ui_rt::JetUiDragPhase::Cancel => "Cancel",
        },
        Vec::new(),
    )
}

fn ambient_drag_operation_value(operation: ui_rt::JetUiDragOperation) -> CtValue {
    ambient_enum_value(
        "UiDragOperation",
        match operation {
            ui_rt::JetUiDragOperation::Copy => "Copy",
            ui_rt::JetUiDragOperation::Move => "Move",
            ui_rt::JetUiDragOperation::Link => "Link",
        },
        Vec::new(),
    )
}

fn ambient_drop_item_value(item: ui_rt::JetUiDropItem) -> CtValue {
    match item {
        ui_rt::JetUiDropItem::Text(value) => ambient_enum_value(
            "UiDropItem",
            "Text",
            vec![(None, CtValue::Str(value))],
        ),
        ui_rt::JetUiDropItem::Uri(value) => ambient_enum_value(
            "UiDropItem",
            "Uri",
            vec![(None, CtValue::Str(value))],
        ),
        ui_rt::JetUiDropItem::File(value) => ambient_enum_value(
            "UiDropItem",
            "File",
            vec![(None, ambient_granted_path_value(value))],
        ),
    }
}

fn ambient_drag_event_value(value: ui_rt::JetUiDragEvent) -> CtValue {
    ambient_struct(
        "UiDragEvent",
        vec![
            ("target".to_string(), ambient_node_id_value(value.target)),
            ("phase".to_string(), ambient_drag_phase_value(value.phase)),
            (
                "operation".to_string(),
                ambient_drag_operation_value(value.operation),
            ),
            (
                "items".to_string(),
                CtValue::List(value.items.into_iter().map(ambient_drop_item_value).collect()),
            ),
        ],
    )
}

fn ambient_projection_value(
    projection: ui_rt::JetUiAccessibilityProjection,
) -> CtValue {
    ambient_struct(
        "UiAccessibilityProjection",
        vec![
            ("node".to_string(), ambient_node_id_value(projection.node().clone())),
            (
                "metadata".to_string(),
                ambient_accessibility_value(projection.metadata().clone()),
            ),
        ],
    )
}

fn ambient_ui_cancellation_value(reason: ui_rt::JetUiCancellation) -> CtValue {
    ambient_enum_value(
        "UiCancellation",
        match reason {
            ui_rt::JetUiCancellation::User => "User",
            ui_rt::JetUiCancellation::Closed => "Closed",
            ui_rt::JetUiCancellation::Headless => "Headless",
            ui_rt::JetUiCancellation::Superseded => "Superseded",
            ui_rt::JetUiCancellation::Programmatic => "Programmatic",
        },
        Vec::new(),
    )
}

fn ambient_ui_host_error_value(error: ui_rt::JetUiHostError) -> CtValue {
    match error {
        ui_rt::JetUiHostError::Cancelled(reason) => ambient_enum_value(
            "UiHostError",
            "Cancelled",
            vec![(None, ambient_ui_cancellation_value(reason))],
        ),
        ui_rt::JetUiHostError::CapabilityUnavailable { capability } => ambient_enum_value(
            "UiHostError",
            "CapabilityUnavailable",
            vec![(None, CtValue::Str(capability.to_string()))],
        ),
        ui_rt::JetUiHostError::CapabilityDenied { capability } => ambient_enum_value(
            "UiHostError",
            "CapabilityDenied",
            vec![(None, ambient_capability_value(capability))],
        ),
        ui_rt::JetUiHostError::InvalidRequest(message) => ambient_enum_value(
            "UiHostError",
            "InvalidRequest",
            vec![(None, CtValue::Str(message))],
        ),
        ui_rt::JetUiHostError::HostFailure { service, message } => ambient_enum_value(
            "UiHostError",
            "HostFailure",
            vec![
                (None, CtValue::Str(service.to_string())),
                (None, CtValue::Str(message)),
            ],
        ),
        ui_rt::JetUiHostError::ResourceDenied(message) => ambient_enum_value(
            "UiHostError",
            "ResourceDenied",
            vec![(None, CtValue::Str(message))],
        ),
        ui_rt::JetUiHostError::ShortcutConflict {
            shortcut,
            existing_action,
        } => ambient_enum_value(
            "UiHostError",
            "ShortcutConflict",
            vec![
                (None, ambient_shortcut_value(shortcut)),
                (None, CtValue::Str(existing_action)),
            ],
        ),
        ui_rt::JetUiHostError::QueueFull { service } => ambient_enum_value(
            "UiHostError",
            "QueueFull",
            vec![(None, CtValue::Str(service.to_string()))],
        ),
    }
}

fn ambient_service_result<T>(
    result: ui_rt::JetUiServiceResult<T>,
    encode: impl FnOnce(T) -> CtValue,
) -> CtValue {
    match result {
        Ok(value) => CtValue::Present(Box::new(encode(value))),
        Err(error) => CtValue::failed(Box::new(ambient_ui_host_error_value(error))),
    }
}

fn ambient_option_result<T>(
    result: ui_rt::JetUiServiceResult<Option<T>>,
    type_name: &str,
    encode: impl FnOnce(T) -> CtValue,
) -> CtValue {
    ambient_service_result(result, |value| {
        value.map_or_else(
            || ambient_absent(type_name),
            |value| CtValue::Present(Box::new(encode(value))),
        )
    })
}
fn ui_mir_named(name: &str) -> MirType {
    MirType::from_kind(MirTypeKind::Apply { name: MirNominalRef::from_name(name), args: Vec::new() })
}

fn ui_mir_absent(name: &str) -> MirRuntimeValue {
    MirRuntimeValue::Absent {
        element: ui_mir_named(name),
    }
}

fn ui_mir_enum(type_name: &str, variant: &str) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn ui_mir_role(role: Option<ui_rt::JetAriaRole>) -> MirRuntimeValue {
    role.map_or_else(
        || ui_mir_absent("UiAriaRole"),
        |role| {
            let variant = match role {
                ui_rt::JetAriaRole::Button => "Button",
                ui_rt::JetAriaRole::TextInput => "TextInput",
                ui_rt::JetAriaRole::Label => "Label",
                ui_rt::JetAriaRole::Container => "Container",
            };
            MirRuntimeValue::Present(Box::new(ui_mir_enum("UiAriaRole", variant)))
        },
    )
}
fn ui_mir_ime(ime: Option<ui_rt::JetUiImeMode>) -> MirRuntimeValue {
    ime.map_or_else(
        || ui_mir_absent("UiImeMode"),
        |ime| {
            let variant = match ime {
                ui_rt::JetUiImeMode::Native => "Native",
                ui_rt::JetUiImeMode::Disabled => "Disabled",
            };
            MirRuntimeValue::Present(Box::new(ui_mir_enum("UiImeMode", variant)))
        },
    )
}

fn ui_mir_kind(kind: &ui_rt::JetUiNodeKind) -> MirRuntimeValue {
    let variant = match kind {
        ui_rt::JetUiNodeKind::Custom => "Custom",
        ui_rt::JetUiNodeKind::Text => "Text",
        ui_rt::JetUiNodeKind::Box => "Box",
        ui_rt::JetUiNodeKind::Button => "Button",
        ui_rt::JetUiNodeKind::TextInput => "TextInput",
    };
    ui_mir_enum("UiNodeKind", variant)
}

fn ui_mir_accessibility(
    accessibility: Option<ui_rt::JetUiAccessibility>,
) -> MirRuntimeValue {
    let Some(accessibility) = accessibility else {
        return ui_mir_absent("UiAccessibility");
    };
    let optional_string = |value: Option<String>| {
        value.map_or_else(
            || ui_mir_absent("String"),
            |value| MirRuntimeValue::Present(Box::new(MirRuntimeValue::String(value))),
        )
    };
    let states = accessibility
        .states
        .into_iter()
        .map(|state| {
            let (variant, args) = match state {
                ui_rt::JetUiAccessibilityState::Disabled => ("Disabled", Vec::new()),
                ui_rt::JetUiAccessibilityState::Busy => ("Busy", Vec::new()),
                ui_rt::JetUiAccessibilityState::Expanded(value) => (
                    "Expanded",
                    vec![(None, MirRuntimeValue::Bool(value))],
                ),
                ui_rt::JetUiAccessibilityState::Checked(value) => {
                    ("Checked", vec![(None, MirRuntimeValue::Bool(value))])
                }
                ui_rt::JetUiAccessibilityState::Selected(value) => {
                    ("Selected", vec![(None, MirRuntimeValue::Bool(value))])
                }
                ui_rt::JetUiAccessibilityState::Required => ("Required", Vec::new()),
                ui_rt::JetUiAccessibilityState::Value(value) => {
                    ("Value", vec![(None, MirRuntimeValue::String(value))])
                }
            };
            MirRuntimeValue::Enum {
                type_name: "UiAccessibilityState".to_string(),
                variant: variant.to_string(),
                args,
            }
        })
        .collect();
    MirRuntimeValue::Present(Box::new(MirRuntimeValue::Struct {
        type_name: "UiAccessibility".to_string(),
        fields: vec![
            ("name".to_string(), optional_string(accessibility.name)),
            (
                "description".to_string(),
                optional_string(accessibility.description),
            ),
            ("states".to_string(), MirRuntimeValue::List(states)),
        ],
    }))
}

fn ui_mir_node_value(
    node: ui_rt::JetUiNode,
    shortcut: MirRuntimeValue,
    callback: Option<(&str, MirRuntimeValue)>,
) -> MirRuntimeValue {
    let children = node
        .children
        .into_iter()
        .map(|child| ui_mir_node_value(child, ui_mir_absent("UiShortcut"), None))
        .collect();
    let mut fields = vec![
        ("label".to_string(), MirRuntimeValue::String(node.label)),
        (
            "width".to_string(),
            MirRuntimeValue::Float {
                value: node.width,
                f32: false,
            },
        ),
        (
            "height".to_string(),
            MirRuntimeValue::Float {
                value: node.height,
                f32: false,
            },
        ),
        ("role".to_string(), ui_mir_role(node.role)),
        (
            "accessibility".to_string(),
            ui_mir_accessibility(node.accessibility),
        ),
        ("ime".to_string(), ui_mir_ime(node.ime)),
        (
            "color".to_string(),
            node.color.map_or_else(
                || ui_mir_absent("String"),
                |value| MirRuntimeValue::Present(Box::new(MirRuntimeValue::String(value))),
            ),
        ),
        ("kind".to_string(), ui_mir_kind(&node.kind)),
        ("children".to_string(), MirRuntimeValue::List(children)),
        ("shortcut".to_string(), shortcut),
    ];
    if let Some((name, callback)) = callback {
        fields.push((name.to_string(), callback));
    }
    MirRuntimeValue::Struct {
        type_name: "UiNode".to_string(),
        fields,
    }
}

fn ui_mir_optional_shortcut(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<MirRuntimeValue, Diagnostic> {
    match value {
        MirRuntimeValue::Absent { .. } => Ok(value.clone()),
        MirRuntimeValue::Present(inner)
            if matches!(
                inner.as_ref(),
                MirRuntimeValue::Struct { type_name, .. } if type_name == "UiShortcut"
            ) =>
        {
            Ok(MirRuntimeValue::Present(inner.clone()))
        }
        _ => Err(unsupported(
            "core.ui.button() expects an optional UiShortcut",
            span,
        )),
    }
}

fn ui_mir_optional_label(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<Option<String>, Diagnostic> {
    match value {
        MirRuntimeValue::Absent { .. } => Ok(None),
        MirRuntimeValue::Present(value) => match value.as_ref() {
            MirRuntimeValue::String(value) => Ok(Some(value.clone())),
            _ => Err(unsupported(
                "core.ui.button() expects an optional accessible String label",
                span,
            )),
        },
        _ => Err(unsupported(
            "core.ui.button() expects an optional accessible String label",
            span,
        )),
    }
}

fn ui_mir_ime_mode(
    value: &MirRuntimeValue,
    span: Span,
) -> Result<ui_rt::JetUiImeMode, Diagnostic> {
    match value {
        MirRuntimeValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "UiImeMode" && args.is_empty() => match variant.as_str() {
            "Native" => Ok(ui_rt::JetUiImeMode::Native),
            "Disabled" => Ok(ui_rt::JetUiImeMode::Disabled),
            _ => Err(unsupported("unknown UiImeMode variant", span)),
        },
        _ => Err(unsupported("core.ui.text_input() expects UiImeMode", span)),
    }
}

fn ui_mir_preview_source(kind: &MirCoreClosureKind) -> Option<MirRuntimeValue> {
    let MirCoreClosureKind::UiPreview {
        source_file,
        source_start_line,
        source_start_column,
        source_end_line,
        source_end_column,
        build_id,
        revision,
        ..
    } = kind
    else {
        return None;
    };
    let source_id = source_file.clone();
    Some(MirRuntimeValue::Struct {
        type_name: "UiPreviewSource".to_string(),
        fields: vec![
            (
                "source_id".to_string(),
                MirRuntimeValue::String(source_id.clone()),
            ),
            (
                "build_id".to_string(),
                MirRuntimeValue::String(build_id.clone()),
            ),
            (
                "revision".to_string(),
                MirRuntimeValue::String(revision.clone()),
            ),
            (
                "span".to_string(),
                MirRuntimeValue::Struct {
                    type_name: "JetDevtoolsSourceSpan".to_string(),
                    fields: vec![
                        (
                            "source_id".to_string(),
                            MirRuntimeValue::String(source_id),
                        ),
                        (
                            "file".to_string(),
                            MirRuntimeValue::String(source_file.clone()),
                        ),
                        (
                            "start_line".to_string(),
                            MirRuntimeValue::Int(i64::from(*source_start_line)),
                        ),
                        (
                            "start_column".to_string(),
                            MirRuntimeValue::Int(i64::from(*source_start_column)),
                        ),
                        (
                            "end_line".to_string(),
                            MirRuntimeValue::Int(i64::from(*source_end_line)),
                        ),
                        (
                            "end_column".to_string(),
                            MirRuntimeValue::Int(i64::from(*source_end_column)),
                        ),
                    ],
                },
            ),
        ],
    })
}

fn jet_ui_core_closure_ambient_call(
    module: &str,
    method: &str,
    _call: MirPreludeCallId,
    kind: MirCoreClosureKind,
    args: Vec<MirRuntimeValue>,
    closure: Option<MirRuntimeValue>,
    _site: MirSiteId,
    _label: &str,
    span: Span,
) -> Option<Result<MirRuntimeValue, Diagnostic>> {
    let preview_source = ui_mir_preview_source(&kind);
    match (module, method, kind) {
        (
            "core.ui",
            "preview",
            MirCoreClosureKind::UiPreview {
                playground: false,
                ..
            },
        ) => Some((|| {
            let [name, viewport] = args.as_slice() else {
                return Err(unsupported(
                    "core.ui.preview() callback route has the wrong argument shape",
                    span,
                ));
            };
            let name = match name {
                MirRuntimeValue::String(value) if !value.trim().is_empty() => value.clone(),
                _ => {
                    return Err(unsupported(
                        "core.ui.preview() expects a non-empty String name",
                        span,
                    ))
                }
            };
            let callback = closure.ok_or_else(|| {
                unsupported(
                    "core.ui.preview() callback route has no callback value",
                    span,
                )
            })?;
            if !matches!(callback, MirRuntimeValue::Closure(_)) {
                return Err(unsupported(
                    "core.ui.preview() callback is not callable",
                    span,
                ));
            }
            Ok(MirRuntimeValue::Struct {
                type_name: "UiPreview".to_string(),
                fields: vec![
                    ("kind".to_string(), ui_mir_enum("UiPreviewKind", "Preview")),
                    ("name".to_string(), MirRuntimeValue::String(name)),
                    ("viewport".to_string(), viewport.clone()),
                    ("callback".to_string(), callback),
                    (
                        "source".to_string(),
                        preview_source
                            .clone()
                            .expect("UiPreview kind carries compiler source"),
                    ),
                ],
            })
        })()),
        (
            "core.ui",
            "playground",
            MirCoreClosureKind::UiPreview {
                playground: true,
                ..
            },
        ) => Some((|| {
            let [name, viewport] = args.as_slice() else {
                return Err(unsupported(
                    "core.ui.playground() callback route has the wrong argument shape",
                    span,
                ));
            };
            let name = match name {
                MirRuntimeValue::String(value) if !value.trim().is_empty() => value.clone(),
                _ => {
                    return Err(unsupported(
                        "core.ui.playground() expects a non-empty String name",
                        span,
                    ))
                }
            };
            let callback = closure.ok_or_else(|| {
                unsupported(
                    "core.ui.playground() callback route has no callback value",
                    span,
                )
            })?;
            if !matches!(callback, MirRuntimeValue::Closure(_)) {
                return Err(unsupported(
                    "core.ui.playground() callback is not callable",
                    span,
                ));
            }
            Ok(MirRuntimeValue::Struct {
                type_name: "UiPreview".to_string(),
                fields: vec![
                    (
                        "kind".to_string(),
                        ui_mir_enum("UiPreviewKind", "Playground"),
                    ),
                    ("name".to_string(), MirRuntimeValue::String(name)),
                    ("viewport".to_string(), viewport.clone()),
                    ("callback".to_string(), callback),
                    (
                        "source".to_string(),
                        preview_source
                            .expect("UiPreview kind carries compiler source"),
                    ),
                ],
            })
        })()),
        ("core.ui", "button", MirCoreClosureKind::UiAction) => Some((|| {
            let [display, shortcut, accessible_label] = args.as_slice() else {
                return Err(unsupported(
                    "core.ui.button() callback route has the wrong argument shape",
                    span,
                ));
            };
            let display = match display {
                MirRuntimeValue::String(value) => value,
                _ => {
                    return Err(unsupported(
                        "core.ui.button() expects a String display label",
                        span,
                    ))
                }
            };
            let shortcut = ui_mir_optional_shortcut(shortcut, span)?;
            let accessible_label = ui_mir_optional_label(accessible_label, span)?;
            let callback = closure.ok_or_else(|| {
                unsupported(
                    "core.ui.button() callback route has no callback value",
                    span,
                )
            })?;
            let accessible_label = accessible_label.map_or_else(
                || Err(ui_rt::JetAbsent),
                Ok,
            );
            let node = ui_rt::jet_ui_button_with_metadata(
                display,
                Err(ui_rt::JetAbsent),
                accessible_label,
            );
            Ok(ui_mir_node_value(
                node,
                shortcut,
                Some(("on_click", callback)),
            ))
        })()),
        ("core.ui", "text_input", MirCoreClosureKind::UiTextInputOnDrop) => {
            Some((|| {
                let [state, ime] = args.as_slice() else {
                    return Err(unsupported(
                        "core.ui.text_input() drop route has the wrong argument shape",
                        span,
                    ));
                };
                let state = match state {
                    MirRuntimeValue::String(value) => value,
                    _ => {
                        return Err(unsupported(
                            "core.ui.text_input() expects a String state",
                            span,
                        ))
                    }
                };
                let node = ui_rt::jet_ui_text_input(state, ui_mir_ime_mode(ime, span)?);
                let callback = closure.ok_or_else(|| {
                    unsupported(
                        "core.ui.text_input() drop route has no callback value",
                        span,
                    )
                })?;
                Ok(ui_mir_node_value(
                    node,
                    ui_mir_absent("UiShortcut"),
                    Some(("on_drop", callback)),
                ))
            })())
        }
        _ => None,
    }
}


fn ambient_expect_arity(
    args: &[CtValue],
    expected: usize,
    operation: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(unsupported(
            &format!("{operation} expects {expected} arguments"),
            span,
        ))
    }
}

fn jet_ui_host_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if matches!(module, "JetUiShortcut" | "::JetUiShortcut") && method == "cmd" {
        return Some((|| {
            ambient_expect_arity(&args, 1, "UiShortcut.cmd", span)?;
            let key = ambient_string(&args[0], "UiShortcut.cmd", span)?;
            Ok(ambient_shortcut_value(ui_rt::JetUiShortcut::cmd(&key)))
        })());
    }
    match (module, method) {
        ("core.ui", "node") => Some((|| {
            ambient_expect_arity(&args, 3, "core.ui.node", span)?;
            let label = ambient_string(&args[0], "core.ui.node", span)?;
            let width = ambient_float(&args[1], "core.ui.node", span)?;
            let height = ambient_float(&args[2], "core.ui.node", span)?;
            Ok(ambient_node_value(ui_rt::jet_ui_node(&label, width, height)))
        })()),
        ("core.ui", "text") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.text", span)?;
            let text = ambient_string(&args[0], "core.ui.text", span)?;
            Ok(ambient_node_value(ui_rt::jet_ui_text(&text)))
        })()),
        ("core.ui", "node_accessibility") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.node_accessibility", span)?;
            let node = ambient_node(&args[0], span)?;
            let accessibility = ambient_accessibility(&args[1], span)?;
            Ok(ambient_node_value(ui_rt::jet_ui_node_accessibility(
                node,
                accessibility,
            )))
        })()),
        ("core.ui", "node_shortcut") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.node_shortcut", span)?;
            let node = ambient_node(&args[0], span)?;
            let shortcut = ambient_shortcut(&args[1], span)?;
            Ok(ambient_node_value(ui_rt::jet_ui_node_shortcut(node, shortcut)))
        })()),
        ("core.ui", "text_input") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.text_input", span)?;
            let state = ambient_string(&args[0], "core.ui.text_input", span)?;
            let ime = ambient_ime_mode(&args[1], span)?;
            Ok(ambient_node_value(ui_rt::jet_ui_text_input(&state, ime)))
        })()),
        ("core.ui.host", "capabilities") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.capabilities", span)?;
            let facts = with_ui_host(|| ui_rt::jet_ui_host_capabilities());
            Ok(ambient_capability_facts_value(facts))
        })()),
        ("core.ui.host", "file_filter") => Some((|| {
            ambient_expect_arity(&args, 3, "core.ui.host.file_filter", span)?;
            let label = ambient_string(&args[0], "core.ui.host.file_filter", span)?;
            let extensions = ambient_strings(&args[1], "core.ui.host.file_filter", span)?;
            let mime_types = ambient_strings(&args[2], "core.ui.host.file_filter", span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_file_filter(&label, &extensions, &mime_types),
                ambient_file_filter_value,
            ))
        })()),
        ("core.ui.host", "file_filter_text") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.file_filter_text", span)?;
            Ok(ambient_file_filter_value(ui_rt::jet_ui_host_file_filter_text()))
        })()),
        ("core.ui.host", "fs_rights_read") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.fs_rights_read", span)?;
            Ok(ambient_fs_rights_value(ui_rt::jet_ui_host_fs_rights_read()))
        })()),
        ("core.ui.host", "fs_rights_write") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.fs_rights_write", span)?;
            Ok(ambient_fs_rights_value(ui_rt::jet_ui_host_fs_rights_write()))
        })()),
        ("core.ui.host", "fs_rights_read_write") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.fs_rights_read_write", span)?;
            Ok(ambient_fs_rights_value(
                ui_rt::jet_ui_host_fs_rights_read_write(),
            ))
        })()),
        ("core.ui.host", "fs_grant") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.host.fs_grant", span)?;
            let root = ambient_string(&args[0], "core.ui.host.fs_grant", span)?;
            let rights = ambient_fs_rights(&args[1], span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_fs_grant(&root, rights),
                ambient_fs_grant_value,
            ))
        })()),
        ("core.ui.host", "open_request") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.open_request", span)?;
            let grant = ambient_fs_grant(&args[0], span)?;
            Ok(ambient_file_request_value(ui_rt::jet_ui_host_open_request(
                grant,
            )))
        })()),
        ("core.ui.host", "save_request") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.save_request", span)?;
            let grant = ambient_fs_grant(&args[0], span)?;
            Ok(ambient_file_request_value(ui_rt::jet_ui_host_save_request(
                grant,
            )))
        })()),
        ("core.ui.host", "open_file") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.open_file", span)?;
            let request = ambient_file_request(&args[0], span)?;
            Ok(ambient_service_result(
                with_ui_host(|| ui_rt::jet_ui_host_open_file(request)),
                ambient_file_selection_value,
            ))
        })()),
        ("core.ui.host", "save_file") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.save_file", span)?;
            let request = ambient_file_request(&args[0], span)?;
            Ok(ambient_service_result(
                with_ui_host(|| ui_rt::jet_ui_host_save_file(request)),
                ambient_file_selection_value,
            ))
        })()),
        ("core.ui.host", "shortcut") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.host.shortcut", span)?;
            let key = ambient_string(&args[0], "core.ui.host.shortcut", span)?;
            let modifiers = ambient_shortcut_modifiers(&args[1], span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_shortcut(&key, modifiers),
                ambient_shortcut_value,
            ))
        })()),
        ("core.ui.host", "accessibility") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.host.accessibility", span)?;
            let name = ambient_string(&args[0], "core.ui.host.accessibility", span)?;
            let description = ambient_string(&args[1], "core.ui.host.accessibility", span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_accessibility(&name, &description),
                ambient_accessibility_value,
            ))
        })()),
        ("core.ui.host.clipboard", "read_text") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.clipboard.read_text", span)?;
            Ok(ambient_service_result(
                with_ui_host(|| ui_rt::jet_ui_host_clipboard_read_text()),
                ambient_clipboard_text_value,
            ))
        })()),
        ("core.ui.host.clipboard", "write_text") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.clipboard.write_text", span)?;
            let text = ambient_string(&args[0], "core.ui.host.clipboard.write_text", span)?;
            Ok(ambient_service_result(
                with_ui_host(|| ui_rt::jet_ui_host_clipboard_write_text(&text)),
                ambient_clipboard_write_value,
            ))
        })()),
        ("core.ui.host.ime", "poll") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.ime.poll", span)?;
            Ok(ambient_option_result(
                with_ui_host(|| ui_rt::jet_ui_host_ime_poll()),
                "UiImeEvent",
                ambient_ime_event_value,
            ))
        })()),
        ("core.ui.host.drag_drop", "poll") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.drag_drop.poll", span)?;
            Ok(ambient_option_result(
                with_ui_host(|| ui_rt::jet_ui_host_drag_poll()),
                "UiDragEvent",
                ambient_drag_event_value,
            ))
        })()),
        ("core.ui.host.shortcuts", "binding") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.host.shortcuts.binding", span)?;
            let shortcut = ambient_shortcut(&args[0], span)?;
            let action = ambient_string(&args[1], "core.ui.host.shortcuts.binding", span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_shortcut_binding(shortcut, &action),
                ambient_shortcut_binding_value,
            ))
        })()),
        ("core.ui.host.shortcuts", "register") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.shortcuts.register", span)?;
            let binding = ambient_shortcut_binding(&args[0], span)?;
            Ok(ambient_service_result(
                with_ui_host(|| ui_rt::jet_ui_host_shortcuts_register(binding)),
                ambient_shortcut_binding_value,
            ))
        })()),
        ("core.ui.host.shortcuts", "dispatch") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.shortcuts.dispatch", span)?;
            let shortcut = ambient_shortcut(&args[0], span)?;
            Ok(ambient_service_result(
                with_ui_host(|| ui_rt::jet_ui_host_shortcuts_dispatch(shortcut)),
                ambient_shortcut_dispatch_value,
            ))
        })()),
        ("core.ui.host.accessibility", "attach") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.host.accessibility.attach", span)?;
            let node = ambient_node(&args[0], span)?;
            let accessibility = ambient_accessibility(&args[1], span)?;
            Ok(ambient_service_result(
                with_ui_host(|| ui_rt::jet_ui_host_attach_accessibility(node, accessibility)),
                ambient_node_value,
            ))
        })()),
        ("core.ui.host.accessibility", "project") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.host.accessibility.project", span)?;
            let node = ambient_node(&args[0], span)?;
            let node_id = ambient_node_id(&args[1], span)?;
            Ok(ambient_option_result(
                with_ui_host(|| ui_rt::jet_ui_host_project_accessibility(node, node_id)),
                "UiAccessibilityProjection",
                ambient_projection_value,
            ))
        })()),
        _ => None,
    }
}

fn jet_ui_font_ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    _resolved_ret: Option<Type>,
    _sink: Option<&mut DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    match (module, method) {
        ("core.font", "system") => Some((|| {
            let style = args
                .first()
                .ok_or_else(|| unsupported("core.font.system() expects FontStyle", span))?;
            Ok(ambient_font_face_value(ui_rt::jet_font_system(
                ambient_font_style(style, span)?,
            )))
        })()),
        ("core.font", "shape") => Some((|| {
            let text = match args.first() {
                Some(CtValue::Str(value)) => value,
                _ => return Err(unsupported("core.font.shape() expects String text", span)),
            };
            let face = ambient_font_face(
                args.get(1)
                    .ok_or_else(|| unsupported("core.font.shape() expects FontFace", span))?,
                span,
            )?;
            let run = with_ui_host(|| {
                ui_rt::jet_ui_with_current_host(|host| host.shape_text(text, &face))
            })
            .map_err(|error| {
                Diagnostic::runtime_host_fault(
                    String::new(),
                    format!("font shaping host contract failure: {error:?}"),
                )
            })?;
            Ok(ambient_glyph_run_value(run))
        })()),
        _ => None,
    }
}

pub(crate) fn register_interpreter_ambient(
    context: &mut crate::ambient_interp::InterpreterAmbientContext,
) {
    context.register_core_call(jet_ui_font_ambient_core_call);
    context.register_core_call(jet_ui_host_ambient_core_call);
    context.register_core_closure_call(jet_ui_core_closure_ambient_call);
}

fn ui_result<T>(
    rt: &mut crate::runtime_host::JitRuntime,
    result: ui_rt::JetUiServiceResult<T>,
    store: impl FnOnce(&mut crate::runtime_host::JitRuntime, T) -> i64,
) -> i64 {
    match result {
        Ok(value) => {
            let stored = store(rt, value);
            crate::runtime_host::alloc_jit_result(rt, true, stored as u64)
        }
        Err(error) => {
            // Keep the typed host error on the canonical Err side. The resident
            // JIT's opaque error slot is rendered only at the observation edge;
            // cancellation is never collapsed into a bare integer.
            let error = rt.heap.alloc_string(format!("{error:?}"));
            crate::runtime_host::alloc_jit_result(rt, false, error as u64)
        }
    }
}

fn ui_list_strings(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<Vec<String>> {
    let length = rt.heap.list_len(handle)?;
    let mut strings = Vec::with_capacity(usize::try_from(length).ok()?);
    for index in 0..length {
        strings.push(rt.heap.clone_string(rt.heap.list_get_int(handle, index)?)?);
    }
    Some(strings)
}

fn ui_push_strings(rt: &mut crate::runtime_host::JitRuntime, values: &[String]) -> i64 {
    let list = rt.heap.alloc_empty_list();
    for value in values {
        let string = rt.heap.alloc_string(value.clone());
        let _ = rt.heap.list_push_int(list, string);
    }
    list
}

fn ui_push_capability_facts(
    rt: &mut crate::runtime_host::JitRuntime,
    facts: ui_rt::JetUiCapabilityFacts,
) -> i64 {
    let record = rt.heap.alloc_record(0);
    ui_store_value(rt, record, UiValue::CapabilityFacts(facts))
}

fn ui_decode_fs_rights(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetUiFsRights> {
    match ui_value(rt, handle)? {
        UiValue::FsRights(rights) => Some(rights),
        _ => None,
    }
}

fn ui_push_fs_rights(
    rt: &mut crate::runtime_host::JitRuntime,
    rights: ui_rt::JetUiFsRights,
) -> i64 {
    let record = rt.heap.alloc_record(0);
    ui_store_value(rt, record, UiValue::FsRights(rights))
}

fn ui_push_file_filter(
    rt: &mut crate::runtime_host::JitRuntime,
    filter: ui_rt::JetUiFileFilter,
) -> i64 {
    let label = rt.heap.alloc_string(filter.label.clone());
    let extensions = ui_push_strings(rt, &filter.extensions);
    let mime_types = ui_push_strings(rt, &filter.mime_types);
    let record = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_string(record, 0, label);
    let _ = rt.heap.record_set_int(record, 1, extensions);
    let _ = rt.heap.record_set_int(record, 2, mime_types);
    ui_store_value(rt, record, UiValue::FileFilter(filter))
}

fn ui_decode_file_filter(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetUiFileFilter> {
    match ui_value(rt, handle)? {
        UiValue::FileFilter(filter) => Some(filter),
        _ => None,
    }
}

fn ui_push_fs_grant(
    rt: &mut crate::runtime_host::JitRuntime,
    grant: ui_rt::JetUiFsGrant,
) -> i64 {
    let root = rt.heap.alloc_string(grant.root().to_string());
    let rights = ui_push_fs_rights(rt, grant.rights());
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_string(record, 0, root);
    let _ = rt.heap.record_set_record(record, 1, rights);
    ui_store_value(rt, record, UiValue::FsGrant(grant))
}

fn ui_decode_fs_grant(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetUiFsGrant> {
    match ui_value(rt, handle)? {
        UiValue::FsGrant(grant) => Some(grant),
        _ => None,
    }
}

fn ui_push_file_request(
    rt: &mut crate::runtime_host::JitRuntime,
    request: ui_rt::JetUiFileDialogRequest,
) -> i64 {
    let title = rt.heap.alloc_string(request.title.clone());
    let kind = rt.heap.alloc_record(1);
    let kind_discriminant = match request.kind {
        ui_rt::JetUiFileDialogKind::Open => 0,
        ui_rt::JetUiFileDialogKind::Save => 1,
    };
    let _ = rt.heap.record_set_int(kind, 0, kind_discriminant);
    let grant = ui_push_fs_grant(rt, request.grant.clone());
    let initial_directory = request
        .initial_directory
        .as_ref()
        .map(|directory| ui_push_granted_path(rt, directory.clone()).wrapping_add(1))
        .unwrap_or(0);
    let filters = rt.heap.alloc_empty_list();
    for filter in &request.filters {
        let handle = ui_push_file_filter(rt, filter.clone());
        let _ = rt.heap.list_push_int(filters, handle);
    }
    let record = rt.heap.alloc_record(6);
    let _ = rt.heap.record_set_string(record, 0, title);
    let _ = rt.heap.record_set_record(record, 1, kind);
    let _ = rt.heap.record_set_record(record, 2, grant);
    let _ = rt.heap.record_set_int(record, 3, initial_directory);
    let _ = rt.heap.record_set_int(record, 4, filters);
    let _ = rt.heap.record_set_bool(record, 5, request.allow_multiple);
    ui_store_value(rt, record, UiValue::FileDialogRequest(request))
}

fn ui_decode_file_request(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetUiFileDialogRequest> {
    match ui_value(rt, handle)? {
        UiValue::FileDialogRequest(request) => Some(request),
        _ => None,
    }
}

fn ui_push_granted_path(
    rt: &mut crate::runtime_host::JitRuntime,
    path: ui_rt::JetUiGrantedPath,
) -> i64 {
    let value = rt.heap.alloc_string(path.path().to_string());
    let grant_root = rt.heap.alloc_string(path.grant_root().to_string());
    let access = rt.heap.alloc_record(1);
    let access_discriminant = match path.access() {
        ui_rt::JetUiFsAccess::Read => 0,
        ui_rt::JetUiFsAccess::Write => 1,
    };
    let _ = rt.heap.record_set_int(access, 0, access_discriminant);
    let record = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_string(record, 0, value);
    let _ = rt.heap.record_set_string(record, 1, grant_root);
    let _ = rt.heap.record_set_record(record, 2, access);
    ui_store_value(rt, record, UiValue::GrantedPath(path))
}

fn ui_push_file_selection(
    rt: &mut crate::runtime_host::JitRuntime,
    selection: ui_rt::JetUiFileDialogSelection,
) -> i64 {
    let files = rt.heap.alloc_empty_list();
    for file in &selection.files {
        let handle = ui_push_granted_path(rt, file.clone());
        let _ = rt.heap.list_push_int(files, handle);
    }
    let record = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(record, 0, files);
    ui_store_value(rt, record, UiValue::FileDialogSelection(selection))
}

fn ui_push_text_range(
    rt: &mut crate::runtime_host::JitRuntime,
    range: ui_rt::JetUiTextRange,
) -> i64 {
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(record, 0, i64::try_from(range.start).unwrap_or(i64::MAX));
    let _ = rt.heap.record_set_int(record, 1, i64::try_from(range.end).unwrap_or(i64::MAX));
    ui_store_value(rt, record, UiValue::TextRange(range))
}

fn ui_push_clipboard_text(
    rt: &mut crate::runtime_host::JitRuntime,
    value: ui_rt::JetUiClipboardText,
) -> i64 {
    let text = rt.heap.alloc_string(value.text.clone());
    let selection = value
        .selection
        .map(|range| ui_push_text_range(rt, range).wrapping_add(1))
        .unwrap_or(0);
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_string(record, 0, text);
    let _ = rt.heap.record_set_int(record, 1, selection);
    ui_store_value(rt, record, UiValue::ClipboardText(value))
}

/// `UiClipboardWrite { characters }` — the same one-field record the
/// interpreter's `ambient_clipboard_write_value` projects.
fn ui_push_clipboard_write(
    rt: &mut crate::runtime_host::JitRuntime,
    value: ui_rt::JetUiClipboardWrite,
) -> i64 {
    let record = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(
        record,
        0,
        i64::try_from(value.characters).unwrap_or(i64::MAX),
    );
    ui_store_value(rt, record, UiValue::ClipboardWrite(value))
}

fn ui_decode_shortcut_modifiers(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetUiShortcutModifiers> {
    if let Some(UiValue::ShortcutModifiers(modifiers)) = ui_value(rt, handle) {
        return Some(modifiers);
    }
    Some(ui_rt::JetUiShortcutModifiers::from_bits(
        u8::try_from(handle & 0x1f).ok()?,
    ))
}

fn ui_push_shortcut(
    rt: &mut crate::runtime_host::JitRuntime,
    shortcut: ui_rt::JetUiShortcut,
) -> i64 {
    let key = rt.heap.alloc_string(shortcut.key.clone());
    let modifiers = rt.heap.alloc_record(1);
    let _ = rt
        .heap
        .record_set_int(modifiers, 0, i64::from(shortcut.modifiers.bits()));
    let record = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_string(record, 0, key);
    let _ = rt.heap.record_set_record(record, 1, modifiers);
    ui_store_value(rt, record, UiValue::Shortcut(shortcut))
}

fn ui_decode_shortcut(
    rt: &crate::runtime_host::JitRuntime,
    handle: i64,
) -> Option<ui_rt::JetUiShortcut> {
    match ui_value(rt, handle)? {
        UiValue::Shortcut(shortcut) => Some(shortcut),
        _ => None,
    }
}

fn ui_push_shortcut_binding(
    rt: &mut crate::runtime_host::JitRuntime,
    binding: ui_rt::JetUiShortcutBinding,
) -> i64 {
    let shortcut = ui_push_shortcut(rt, binding.shortcut.clone());
    let action = rt.heap.alloc_string(binding.action.clone());
    let node = i64::from(binding.node.is_some());
    let record = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_record(record, 0, shortcut);
    let _ = rt.heap.record_set_string(record, 1, action);
    let _ = rt.heap.record_set_int(record, 2, node);
    ui_store_value(rt, record, UiValue::ShortcutBinding(binding))
}

fn ui_push_accessibility(
    rt: &mut crate::runtime_host::JitRuntime,
    accessibility: ui_rt::JetUiAccessibility,
) -> i64 {
    let name = accessibility
        .name
        .as_ref()
        .map(|value| rt.heap.alloc_string(value.clone()).wrapping_add(1))
        .unwrap_or(0);
    let description = accessibility
        .description
        .as_ref()
        .map(|value| rt.heap.alloc_string(value.clone()).wrapping_add(1))
        .unwrap_or(0);
    let states = rt.heap.alloc_empty_list();
    let record = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_int(record, 0, name);
    let _ = rt.heap.record_set_int(record, 1, description);
    let _ = rt.heap.record_set_int(record, 2, states);
    ui_store_value(rt, record, UiValue::Accessibility(accessibility))
}

fn jet_jit_ui_node_accessibility(node: i64, accessibility: i64) -> i64 {
    let Some((node_value, accessibility)) = with_rt(|rt| {
        let Some(node_value) = rt.ui.nodes.get(node.saturating_sub(1) as usize).cloned() else {
            rt.set_host_fault("JIT UI node_accessibility received an invalid node handle");
            return None;
        };
        let Some(UiValue::Accessibility(accessibility)) = ui_value(rt, accessibility) else {
            rt.set_host_fault("JIT UI node_accessibility received an invalid accessibility handle");
            return None;
        };
        Some((node_value, accessibility))
    }) else {
        return 0;
    };
    let result = with_ui_host(|| {
        ui_rt::jet_ui_host_attach_accessibility(node_value, accessibility)
    });
    with_rt(|rt| {
        ui_result(rt, result, |rt, node| {
            rt.ui.nodes.push(node);
            rt.ui.nodes.len() as i64
        })
    })
}

fn jet_jit_ui_node_shortcut(node: i64, shortcut: i64) -> i64 {
    with_rt(|rt| {
        let Some(node_value) = rt.ui.nodes.get(node.saturating_sub(1) as usize).cloned() else {
            rt.set_host_fault("JIT UI node_shortcut received an invalid node handle");
            return 0;
        };
        let Some(shortcut) = ui_decode_shortcut(rt, shortcut) else {
            rt.set_host_fault("JIT UI node_shortcut received an invalid shortcut handle");
            return 0;
        };
        rt.ui.nodes.push(ui_rt::jet_ui_node_shortcut(node_value, shortcut));
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_ui_text_input(state: i64, ime: i64) -> i64 {
    with_rt(|rt| {
        let Some(state) = rt.heap.clone_string(state) else {
            rt.set_host_fault("JIT UI text_input received a non-string state handle");
            return 0;
        };
        let Some(ime) = ui_decode_ime_mode(rt, ime) else {
            return 0;
        };
        rt.ui.nodes.push(ui_rt::jet_ui_text_input(&state, ime));
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_ui_node_label(node: i64) -> i64 {
    with_rt(|rt| {
        let label = rt
            .ui
            .nodes
            .get(node.saturating_sub(1) as usize)
            .map(|n| n.label.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(label)
    })
}
fn jet_jit_ui_host_capabilities() -> i64 {
    let facts = with_ui_host(|| ui_rt::jet_ui_host_capabilities());
    with_rt(|rt| ui_push_capability_facts(rt, facts))
}

fn jet_jit_ui_host_file_filter(label: i64, extensions: i64, mime_types: i64) -> i64 {
    with_rt(|rt| {
        let Some(label) = rt.heap.clone_string(label) else {
            rt.set_host_fault("JIT ui.host.file_filter received a non-string label");
            return 0;
        };
        let Some(extensions) = ui_list_strings(rt, extensions) else {
            rt.set_host_fault("JIT ui.host.file_filter received an invalid extension list");
            return 0;
        };
        let Some(mime_types) = ui_list_strings(rt, mime_types) else {
            rt.set_host_fault("JIT ui.host.file_filter received an invalid MIME list");
            return 0;
        };
        let result = ui_rt::jet_ui_host_file_filter(&label, &extensions, &mime_types);
        ui_result(rt, result, ui_push_file_filter)
    })
}

fn jet_jit_ui_host_file_filter_text() -> i64 {
    with_rt(|rt| ui_push_file_filter(rt, ui_rt::jet_ui_host_file_filter_text()))
}

fn jet_jit_ui_host_fs_rights_read() -> i64 {
    with_rt(|rt| ui_push_fs_rights(rt, ui_rt::jet_ui_host_fs_rights_read()))
}

fn jet_jit_ui_host_fs_rights_write() -> i64 {
    with_rt(|rt| ui_push_fs_rights(rt, ui_rt::jet_ui_host_fs_rights_write()))
}

fn jet_jit_ui_host_fs_rights_read_write() -> i64 {
    with_rt(|rt| ui_push_fs_rights(rt, ui_rt::jet_ui_host_fs_rights_read_write()))
}

fn jet_jit_ui_host_fs_grant(root: i64, rights: i64) -> i64 {
    with_rt(|rt| {
        let Some(root) = rt.heap.clone_string(root) else {
            rt.set_host_fault("JIT ui.host.fs_grant received a non-string root");
            return 0;
        };
        let Some(rights) = ui_decode_fs_rights(rt, rights) else {
            rt.set_host_fault("JIT ui.host.fs_grant received invalid rights");
            return 0;
        };
        ui_result(
            rt,
            ui_rt::jet_ui_host_fs_grant(&root, rights),
            ui_push_fs_grant,
        )
    })
}

fn jet_jit_ui_host_open_request(grant: i64) -> i64 {
    with_rt(|rt| {
        let Some(grant) = ui_decode_fs_grant(rt, grant) else {
            rt.set_host_fault("JIT ui.host.open_request received invalid grant");
            return 0;
        };
        ui_push_file_request(rt, ui_rt::jet_ui_host_open_request(grant))
    })
}

fn jet_jit_ui_host_save_request(grant: i64) -> i64 {
    with_rt(|rt| {
        let Some(grant) = ui_decode_fs_grant(rt, grant) else {
            rt.set_host_fault("JIT ui.host.save_request received invalid grant");
            return 0;
        };
        ui_push_file_request(rt, ui_rt::jet_ui_host_save_request(grant))
    })
}

fn jet_jit_ui_host_open_file(request: i64) -> i64 {
    let Some(request) = with_rt(|rt| {
        let Some(request) = ui_decode_file_request(rt, request) else {
            rt.set_host_fault("JIT ui.host.open_file received invalid request");
            return None;
        };
        Some(request)
    }) else {
        return 0;
    };
    let result = with_ui_host(|| ui_rt::jet_ui_host_open_file(request));
    with_rt(|rt| ui_result(rt, result, ui_push_file_selection))
}
fn jet_jit_ui_host_save_file(request: i64) -> i64 {
    let Some(request) = with_rt(|rt| {
        let Some(request) = ui_decode_file_request(rt, request) else {
            rt.set_host_fault("JIT ui.host.save_file received invalid request");
            return None;
        };
        Some(request)
    }) else {
        return 0;
    };
    let result = with_ui_host(|| ui_rt::jet_ui_host_save_file(request));
    with_rt(|rt| ui_result(rt, result, ui_push_file_selection))
}


fn jet_jit_ui_host_shortcut(key: i64, modifiers: i64) -> i64 {
    with_rt(|rt| {
        let Some(key) = rt.heap.clone_string(key) else {
            rt.set_host_fault("JIT ui.host.shortcut received a non-string key");
            return 0;
        };
        let Some(modifiers) = ui_decode_shortcut_modifiers(rt, modifiers) else {
            rt.set_host_fault("JIT ui.host.shortcut received invalid modifiers");
            return 0;
        };
        ui_result(
            rt,
            ui_rt::jet_ui_host_shortcut(&key, modifiers),
            ui_push_shortcut,
        )
    })
}

fn jet_jit_ui_host_accessibility(name: i64, description: i64) -> i64 {
    with_rt(|rt| {
        let Some(name) = rt.heap.clone_string(name) else {
            rt.set_host_fault("JIT ui.host.accessibility received a non-string name");
            return 0;
        };
        let Some(description) = rt.heap.clone_string(description) else {
            rt.set_host_fault("JIT ui.host.accessibility received a non-string description");
            return 0;
        };
        ui_result(
            rt,
            ui_rt::jet_ui_host_accessibility(&name, &description),
            ui_push_accessibility,
        )
    })
}

fn ui_push_ime_phase(
    rt: &mut crate::runtime_host::JitRuntime,
    phase: ui_rt::JetUiImePhase,
) -> i64 {
    let record = rt.heap.alloc_record(1);
    let discriminant = match phase {
        ui_rt::JetUiImePhase::Start => 0,
        ui_rt::JetUiImePhase::Update => 1,
        ui_rt::JetUiImePhase::Commit => 2,
        ui_rt::JetUiImePhase::Cancel => 3,
    };
    let _ = rt.heap.record_set_int(record, 0, discriminant);
    record
}

fn ui_push_ime_event_value(
    rt: &mut crate::runtime_host::JitRuntime,
    event: ui_rt::JetUiImeEvent,
) -> i64 {
    let target = rt.heap.alloc_string(event.target.as_str().to_string());
    let phase = ui_push_ime_phase(rt, event.phase);
    let composition = event
        .composition
        .as_ref()
        .map(|composition| {
            let text = rt.heap.alloc_string(composition.text.clone());
            let selection = ui_push_text_range(rt, composition.selection);
            let marked = composition
                .marked
                .map(|range| ui_push_text_range(rt, range).wrapping_add(1))
                .unwrap_or(0);
            let record = rt.heap.alloc_record(3);
            let _ = rt.heap.record_set_string(record, 0, text);
            let _ = rt.heap.record_set_record(record, 1, selection);
            let _ = rt.heap.record_set_int(record, 2, marked);
            record
        })
        .map(|record| record.wrapping_add(1))
        .unwrap_or(0);
    let record = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_string(record, 0, target);
    let _ = rt.heap.record_set_record(record, 1, phase);
    let _ = rt.heap.record_set_int(record, 2, composition);
    record
}

fn ui_push_ime_event(
    rt: &mut crate::runtime_host::JitRuntime,
    event: Option<ui_rt::JetUiImeEvent>,
) -> i64 {
    let payload = event
        .as_ref()
        .map(|event| ui_push_ime_event_value(rt, event.clone()).wrapping_add(1))
        .unwrap_or(0);
    let record = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(record, 0, payload);
    ui_store_value(rt, record, UiValue::ImeEvent(event))
}

fn ui_push_drag_phase(
    rt: &mut crate::runtime_host::JitRuntime,
    phase: ui_rt::JetUiDragPhase,
) -> i64 {
    let record = rt.heap.alloc_record(1);
    let discriminant = match phase {
        ui_rt::JetUiDragPhase::Enter => 0,
        ui_rt::JetUiDragPhase::Over => 1,
        ui_rt::JetUiDragPhase::Drop => 2,
        ui_rt::JetUiDragPhase::Leave => 3,
        ui_rt::JetUiDragPhase::Cancel => 4,
    };
    let _ = rt.heap.record_set_int(record, 0, discriminant);
    record
}

fn ui_push_drag_operation(
    rt: &mut crate::runtime_host::JitRuntime,
    operation: ui_rt::JetUiDragOperation,
) -> i64 {
    let record = rt.heap.alloc_record(1);
    let discriminant = match operation {
        ui_rt::JetUiDragOperation::Copy => 0,
        ui_rt::JetUiDragOperation::Move => 1,
        ui_rt::JetUiDragOperation::Link => 2,
    };
    let _ = rt.heap.record_set_int(record, 0, discriminant);
    record
}

fn ui_push_drop_item(
    rt: &mut crate::runtime_host::JitRuntime,
    item: &ui_rt::JetUiDropItem,
) -> i64 {
    let record = rt.heap.alloc_record(2);
    match item {
        ui_rt::JetUiDropItem::Text(text) => {
            let text = rt.heap.alloc_string(text.clone());
            let _ = rt.heap.record_set_int(record, 0, 0);
            let _ = rt.heap.record_set_string(record, 1, text);
        }
        ui_rt::JetUiDropItem::Uri(uri) => {
            let uri = rt.heap.alloc_string(uri.clone());
            let _ = rt.heap.record_set_int(record, 0, 1);
            let _ = rt.heap.record_set_string(record, 1, uri);
        }
        ui_rt::JetUiDropItem::File(file) => {
            let file = ui_push_granted_path(rt, file.clone());
            let _ = rt.heap.record_set_int(record, 0, 2);
            let _ = rt.heap.record_set_record(record, 1, file);
        }
    }
    record
}

fn ui_push_drag_event_value(
    rt: &mut crate::runtime_host::JitRuntime,
    event: ui_rt::JetUiDragEvent,
) -> i64 {
    let target = rt.heap.alloc_string(event.target.as_str().to_string());
    let phase = ui_push_drag_phase(rt, event.phase);
    let operation = ui_push_drag_operation(rt, event.operation);
    let items = rt.heap.alloc_empty_list();
    for item in &event.items {
        let item = ui_push_drop_item(rt, item);
        let _ = rt.heap.list_push_int(items, item);
    }
    let record = rt.heap.alloc_record(4);
    let _ = rt.heap.record_set_string(record, 0, target);
    let _ = rt.heap.record_set_record(record, 1, phase);
    let _ = rt.heap.record_set_record(record, 2, operation);
    let _ = rt.heap.record_set_int(record, 3, items);
    record
}

fn ui_push_drag_event(
    rt: &mut crate::runtime_host::JitRuntime,
    event: Option<ui_rt::JetUiDragEvent>,
) -> i64 {
    let payload = event
        .as_ref()
        .map(|event| ui_push_drag_event_value(rt, event.clone()).wrapping_add(1))
        .unwrap_or(0);
    let record = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(record, 0, payload);
    ui_store_value(rt, record, UiValue::DragEvent(event))
}

fn jet_jit_ui_shortcut_command(key: i64) -> i64 {
    with_rt(|rt| {
        let Some(key) = rt.heap.clone_string(key) else {
            rt.set_host_fault("JIT UiShortcut.cmd received a non-string key handle");
            return 0;
        };
        ui_push_shortcut(rt, ui_rt::JetUiShortcut::cmd(&key))
    })
}

fn jet_jit_ui_host_clipboard_read_text() -> i64 {
    let result = with_ui_host(|| ui_rt::jet_ui_host_clipboard_read_text());
    with_rt(|rt| ui_result(rt, result, ui_push_clipboard_text))
}

fn jet_jit_ui_host_clipboard_write_text(text: i64) -> i64 {
    let Some(text) = with_rt(|rt| {
        let Some(text) = rt.heap.clone_string(text) else {
            rt.set_host_fault("JIT ui.host.clipboard.write_text received a non-string handle");
            return None;
        };
        Some(text)
    }) else {
        return 0;
    };
    let result = with_ui_host(|| ui_rt::jet_ui_host_clipboard_write_text(&text));
    with_rt(|rt| ui_result(rt, result, ui_push_clipboard_write))
}

fn jet_jit_ui_host_ime_poll() -> i64 {
    let result = with_ui_host(|| ui_rt::jet_ui_host_ime_poll());
    with_rt(|rt| ui_result(rt, result, ui_push_ime_event))
}

fn jet_jit_ui_host_drag_poll() -> i64 {
    let result = with_ui_host(|| ui_rt::jet_ui_host_drag_poll());
    with_rt(|rt| ui_result(rt, result, ui_push_drag_event))
}


fn jet_jit_ui_host_shortcut_binding(shortcut: i64, action: i64) -> i64 {
    with_rt(|rt| {
        let Some(shortcut) = ui_decode_shortcut(rt, shortcut) else {
            rt.set_host_fault("JIT ui.host.shortcuts.binding received invalid shortcut");
            return 0;
        };
        let Some(action) = rt.heap.clone_string(action) else {
            rt.set_host_fault("JIT ui.host.shortcuts.binding received invalid action");
            return 0;
        };
        ui_result(
            rt,
            ui_rt::jet_ui_host_shortcut_binding(shortcut, &action),
            ui_push_shortcut_binding,
        )
    })
}

fn ui_push_shortcut_dispatch(
    rt: &mut crate::runtime_host::JitRuntime,
    dispatch: ui_rt::JetUiShortcutDispatch,
) -> i64 {
    let record = rt.heap.alloc_record(2);
    match &dispatch {
        ui_rt::JetUiShortcutDispatch::Dispatched(binding) => {
            let binding = ui_push_shortcut_binding(rt, binding.clone());
            let _ = rt.heap.record_set_int(record, 0, 0);
            let _ = rt.heap.record_set_record(record, 1, binding);
        }
        ui_rt::JetUiShortcutDispatch::Unhandled => {
            let _ = rt.heap.record_set_int(record, 0, 1);
            let _ = rt.heap.record_set_int(record, 1, 0);
        }
    }
    ui_store_value(rt, record, UiValue::ShortcutDispatch(dispatch))
}

fn jet_jit_ui_host_shortcuts_register(binding: i64) -> i64 {
    let Some(binding) = with_rt(|rt| {
        let binding = match ui_value(rt, binding) {
            Some(UiValue::ShortcutBinding(binding)) => binding,
            _ => {
                rt.set_host_fault("JIT ui.host.shortcuts.register received invalid binding");
                return None;
            }
        };
        Some(binding)
    }) else {
        return 0;
    };
    let result = with_ui_host(|| ui_rt::jet_ui_host_shortcuts_register(binding));
    with_rt(|rt| ui_result(rt, result, ui_push_shortcut_binding))
}

fn jet_jit_ui_host_shortcuts_dispatch(shortcut: i64) -> i64 {
    let Some(shortcut) = with_rt(|rt| {
        let Some(shortcut) = ui_decode_shortcut(rt, shortcut) else {
            rt.set_host_fault("JIT ui.host.shortcuts.dispatch received invalid shortcut");
            return None;
        };
        Some(shortcut)
    }) else {
        return 0;
    };
    let result = with_ui_host(|| ui_rt::jet_ui_host_shortcuts_dispatch(shortcut));
    with_rt(|rt| ui_result(rt, result, ui_push_shortcut_dispatch))
}

fn jet_jit_ui_host_attach_accessibility(node: i64, accessibility: i64) -> i64 {
    let Some((node_value, accessibility)) = with_rt(|rt| {
        let Some(node_value) = rt.ui.nodes.get(node.saturating_sub(1) as usize).cloned() else {
            rt.set_host_fault("JIT ui.host.accessibility.attach received invalid node");
            return None;
        };
        let Some(UiValue::Accessibility(accessibility)) = ui_value(rt, accessibility) else {
            rt.set_host_fault("JIT ui.host.accessibility.attach received invalid metadata");
            return None;
        };
        Some((node_value, accessibility))
    }) else {
        return 0;
    };
    let result = with_ui_host(|| {
        ui_rt::jet_ui_host_attach_accessibility(node_value, accessibility)
    });
    with_rt(|rt| {
        ui_result(rt, result, |rt, node| {
            rt.ui.nodes.push(node);
            rt.ui.nodes.len() as i64
        })
    })
}

fn ui_push_accessibility_projection(
    rt: &mut crate::runtime_host::JitRuntime,
    projection: Option<ui_rt::JetUiAccessibilityProjection>,
) -> i64 {
    let payload = projection
        .as_ref()
        .map(|projection| {
            let node = rt.heap.alloc_string(projection.node().as_str().to_string());
            let metadata = ui_push_accessibility(rt, projection.metadata().clone());
            let record = rt.heap.alloc_record(2);
            let _ = rt.heap.record_set_string(record, 0, node);
            let _ = rt.heap.record_set_record(record, 1, metadata);
            record.wrapping_add(1)
        })
        .unwrap_or(0);
    let record = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(record, 0, payload);
    ui_store_value(rt, record, UiValue::AccessibilityProjection(projection))
}

fn jet_jit_ui_host_project_accessibility(node: i64, node_id: i64) -> i64 {
    let Some((node_value, node_id)) = with_rt(|rt| {
        let Some(node_value) = rt.ui.nodes.get(node.saturating_sub(1) as usize).cloned() else {
            rt.set_host_fault("JIT ui.host.accessibility.project received invalid node");
            return None;
        };
        let Some(node_id) = rt
            .heap
            .clone_string(node_id)
            .and_then(|value| ui_rt::JetUiNodeId::new(&value).ok())
        else {
            rt.set_host_fault("JIT ui.host.accessibility.project received invalid node ID");
            return None;
        };
        Some((node_value, node_id))
    }) else {
        return 0;
    };
    let result = with_ui_host(|| {
        ui_rt::jet_ui_host_project_accessibility(node_value, node_id)
    });
    with_rt(|rt| ui_result(rt, result, ui_push_accessibility_projection))
}

fn jet_jit_ui_node_dim(node: i64, which: i64) -> f64 {
    with_rt(|rt| {
        let Some(n) = rt.ui.nodes.get(node.saturating_sub(1) as usize) else {
            return 0.0;
        };
        if which == 0 {
            n.width
        } else {
            n.height
        }
    })
}

fn jet_jit_ui_null_backend() -> i64 {
    with_rt(|rt| {
        rt.ui
            .backends
            .push(UiBackendSlot::Null(ui_rt::jet_ui_null()));
        rt.ui.backends.len() as i64
    })
}

fn jet_jit_ui_tui_backend() -> i64 {
    with_rt(|rt| {
        rt.ui.backends.push(UiBackendSlot::Tui(ui_rt::jet_ui_tui()));
        rt.ui.backends.len() as i64
    })
}

fn jet_jit_ui_gtk_backend() -> i64 {
    let backend = ui_rt::jet_ui_gtk();
    with_rt(|rt| {
        if std::env::var_os("JET_UI_HEADLESS").is_none() {
            // Keep host services and the selected render backend on the same
            // native state so callbacks observe one GTK tree.
            rt.ui.host = UiHostSlot::Gtk(backend.clone());
        }
        rt.ui.backends.push(UiBackendSlot::Gtk(backend));
        rt.ui.backends.len() as i64
    })
}

fn jet_jit_ui_node(label: i64, w: f64, h: f64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        rt.ui.nodes.push(ui_rt::jet_ui_node(&label, w, h));
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_ui_text(label: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        rt.ui.nodes.push(ui_rt::jet_ui_text(&label));
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_ui_button(label: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        rt.ui.nodes.push(ui_rt::jet_ui_button(&label));
        rt.ui.nodes.len() as i64
    })
}
fn jet_jit_ui_phone() -> i64 {
    jet_jit_ui_preview_viewport(0)
}

fn jet_jit_ui_tablet() -> i64 {
    jet_jit_ui_preview_viewport(1)
}

fn jet_jit_ui_desktop() -> i64 {
    jet_jit_ui_preview_viewport(2)
}

fn jet_jit_ui_preview_viewport(kind: i64) -> i64 {
    with_rt(|rt| {
        let record = rt.heap.alloc_record(1);
        let _ = rt.heap.record_set_int(record, 0, kind);
        record
    })
}

fn jet_jit_ui_previews(values: i64) -> i64 {
    with_rt(|rt| {
        let record = rt.heap.alloc_record(1);
        let _ = rt.heap.record_set_int(record, 0, values);
        record
    })
}

fn jet_jit_ui_playgrounds(values: i64) -> i64 {
    jet_jit_ui_previews(values)
}


/// The canonical Core closure route carries a checked callable handle, not
/// the legacy callback expansion used by older UI rows.
fn core_ui_callable(callable: i64) -> Option<crate::runtime_host::JitCallableSlot> {
    let slot = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::jit_callable_parts(rt, callable)
    });
    if slot.is_none() {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault("MIR UI closure has an invalid callable handle");
            true
        });
    }
    slot
}

fn invoke_core_ui(slot: crate::runtime_host::JitCallableSlot) {
    unsafe {
        if slot.has_env {
            let callback: unsafe extern "C" fn(i64) =
                std::mem::transmute(slot.fn_ptr as usize);
            callback(slot.env);
        } else {
            let callback: unsafe extern "C" fn() =
                std::mem::transmute(slot.fn_ptr as usize);
            callback();
        }
    }
}

fn invoke_core_ui_drop(
    slot: crate::runtime_host::JitCallableSlot,
    items: Vec<ui_rt::JetUiDropItem>,
) {
    let items_handle = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for item in &items {
            let item = ui_push_drop_item(rt, item);
            let _ = rt.heap.list_push_int(list, item);
        }
        list
    });
    unsafe {
        if slot.has_env {
            let callback: unsafe extern "C" fn(i64, i64) =
                std::mem::transmute(slot.fn_ptr as usize);
            callback(slot.env, items_handle);
        } else {
            let callback: unsafe extern "C" fn(i64) = std::mem::transmute(slot.fn_ptr as usize);
            callback(items_handle);
        }
    }
}

fn ui_decode_ime_mode(
    rt: &mut crate::runtime_host::JitRuntime,
    raw: i64,
) -> Option<ui_rt::JetUiImeMode> {
    match raw {
        0 => Some(ui_rt::JetUiImeMode::Native),
        1 => Some(ui_rt::JetUiImeMode::Disabled),
        _ => {
            rt.set_host_fault("MIR UI text_input received an invalid IME mode");
            None
        }
    }
}

fn ui_decode_optional_shortcut(
    rt: &crate::runtime_host::JitRuntime,
    result: i64,
) -> Option<Option<ui_rt::JetUiShortcut>> {
    let (ok, bits) = crate::runtime_host::jit_result_parts(rt, result)?;
    if !ok {
        return Some(None);
    }
    ui_decode_shortcut(rt, bits as i64).map(Some)
}

fn ui_decode_optional_string(
    rt: &crate::runtime_host::JitRuntime,
    result: i64,
) -> Option<Option<String>> {
    let (ok, bits) = crate::runtime_host::jit_result_parts(rt, result)?;
    if !ok {
        return Some(None);
    }
    rt.heap.clone_string(bits as i64).map(Some)
}

#[allow(clippy::too_many_arguments)]
fn jet_jit_core_ui_preview_attach_compiler_source(
    preview: i64,
    source_id: i64,
    source_file: i64,
    build_id: i64,
    revision: i64,
    start_line: i64,
    start_column: i64,
    end_line: i64,
    end_column: i64,
) -> i64 {
    with_rt(|rt| {
        if rt.heap.record_get_int(preview, 4).is_none() {
            rt.set_host_fault("JIT UI preview has an invalid preview record");
            return 0;
        }
        let source = rt.heap.alloc_record(8);
        let _ = rt.heap.record_set_string(source, 0, source_id);
        let _ = rt.heap.record_set_string(source, 1, source_file);
        let _ = rt.heap.record_set_string(source, 2, build_id);
        let _ = rt.heap.record_set_string(source, 3, revision);
        let _ = rt.heap.record_set_int(source, 4, start_line);
        let _ = rt.heap.record_set_int(source, 5, start_column);
        let _ = rt.heap.record_set_int(source, 6, end_line);
        let _ = rt.heap.record_set_int(source, 7, end_column);
        if rt.heap.record_set_record(preview, 4, source).is_none() {
            rt.set_host_fault("JIT UI preview source attachment failed");
            0
        } else {
            preview
        }
    })
}

fn jet_jit_core_ui_preview_with_viewport(
    name: i64,
    viewport: i64,
    callable: i64,
) -> i64 {
    jet_jit_core_ui_preview_with_viewport_kind(name, viewport, callable, false)
}

fn jet_jit_core_ui_playground_with_viewport(
    name: i64,
    viewport: i64,
    callable: i64,
) -> i64 {
    jet_jit_core_ui_preview_with_viewport_kind(name, viewport, callable, true)
}

fn jet_jit_core_ui_preview_with_viewport_kind(
    name: i64,
    viewport: i64,
    callable: i64,
    playground: bool,
) -> i64 {
    let Some(_) = core_ui_callable(callable) else {
        return 0;
    };
    with_rt(|rt| {
        let Some(name) = rt.heap.clone_string(name) else {
            rt.set_host_fault("JIT UI preview received an invalid name handle");
            return 0;
        };
        if name.trim().is_empty() {
            rt.set_host_fault("JIT UI preview received an empty name");
            return 0;
        }
        let record = rt.heap.alloc_record(5);
        let name = rt.heap.alloc_string(name);
        let _ = rt.heap.record_set_string(record, 0, name);
        let _ = rt.heap.record_set_int(record, 1, viewport);
        let _ = rt.heap.record_set_int(record, 2, callable);
        let _ = rt.heap.record_set_bool(record, 3, playground);
        let _ = rt.heap.record_set_int(record, 4, 0);
        record
    })
}

fn jet_jit_core_ui_button_on_click(
    display: i64,
    shortcut: i64,
    accessible_label: i64,
    callable: i64,
) -> i64 {
    let Some(slot) = core_ui_callable(callable) else {
        return 0;
    };
    with_rt(|rt| {
        let Some(display) = rt.heap.clone_string(display) else {
            rt.set_host_fault("MIR UI button display text is not a string handle");
            return 0;
        };
        let Some(shortcut) = ui_decode_optional_shortcut(rt, shortcut) else {
            rt.set_host_fault("MIR UI button shortcut is not an option handle");
            return 0;
        };
        let Some(accessible_label) = ui_decode_optional_string(rt, accessible_label) else {
            rt.set_host_fault("MIR UI button accessible label is not an option handle");
            return 0;
        };
        let shortcut = shortcut.map_or(Err(ui_rt::JetAbsent), Ok);
        let accessible_label = accessible_label.map_or(Err(ui_rt::JetAbsent), Ok);
        rt.ui.nodes.push(ui_rt::jet_ui_button_on_click(
            &display,
            shortcut,
            accessible_label,
            move || invoke_core_ui(slot),
        ));
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_core_ui_text_input_on_drop(state: i64, ime: i64, callable: i64) -> i64 {
    let Some(slot) = core_ui_callable(callable) else {
        return 0;
    };
    with_rt(|rt| {
        let Some(state) = rt.heap.clone_string(state) else {
            rt.set_host_fault("MIR UI text_input state is not a string handle");
            return 0;
        };
        let Some(ime) = ui_decode_ime_mode(rt, ime) else {
            return 0;
        };
        rt.ui.nodes.push(ui_rt::jet_ui_text_input_on_drop(
            &state,
            ime,
            move |items| invoke_core_ui_drop(slot, items),
        ));
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_core_ui_reactive_render(callable: i64) -> i64 {
    let Some(slot) = core_ui_callable(callable) else {
        return 0;
    };
    ui_rt::jet_ui_reactive_render(move || invoke_core_ui(slot));
    0
}


fn jet_jit_ui_node_color(label: i64, w: f64, h: f64, color: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        let color = rt.heap.clone_string(color).unwrap_or_default();
        rt.ui
            .nodes
            .push(ui_rt::jet_ui_node_color(&label, w, h, &color));
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_ui_node_role(label: i64, w: f64, h: f64, role: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_default();
        let role = *rt
            .ui
            .roles
            .get(role.saturating_sub(1) as usize)
            .unwrap_or(&ui_rt::JetAriaRole::Label);
        rt.ui
            .nodes
            .push(ui_rt::jet_ui_node_role(&label, w, h, role));
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_ui_box(children: i64) -> i64 {
    with_rt(|rt| {
        let n = rt.heap.list_len(children).unwrap_or(0);
        let mut kids = Vec::new();
        for i in 0..n {
            let id = rt.heap.list_get_int(children, i).unwrap_or(0);
            if let Some(node) = rt.ui.nodes.get(id.saturating_sub(1) as usize) {
                kids.push(node.clone());
            }
        }
        rt.ui.nodes.push(ui_rt::jet_ui_box(kids));
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_ui_constraint(a: f64, b: f64, c: f64, d: f64) -> i64 {
    with_rt(|rt| {
        rt.ui.constraints.push(ui_rt::jet_ui_constraint(a, b, c, d));
        rt.ui.constraints.len() as i64
    })
}

fn jet_jit_ui_rect(x: f64, y: f64, w: f64, h: f64) -> i64 {
    with_rt(|rt| {
        rt.ui.rects.push(ui_rt::jet_ui_rect(x, y, w, h));
        rt.ui.rects.len() as i64
    })
}

fn jet_jit_ui_key_event(code: i64) -> i64 {
    with_rt(|rt| {
        let code = rt.heap.clone_string(code).unwrap_or_default();
        rt.ui.events.push(ui_rt::jet_ui_key_event(&code));
        rt.ui.events.len() as i64
    })
}
fn jet_jit_ui_resize_event(width: f64, height: f64) -> i64 {
    with_rt(|rt| {
        rt.ui.events.push(ui_rt::jet_ui_resize_event(width, height));
        rt.ui.events.len() as i64
    })
}
fn jet_jit_tui_capabilities() -> i64 {
    with_rt(|rt| ui_push_tui_capabilities(rt, ui_rt::jet_tui_capabilities()))
}

fn jet_jit_tui_key_event(code: i64) -> i64 {
    with_rt(|rt| {
        let code = rt.heap.clone_string(code).unwrap_or_default();
        ui_push_tui_event(rt, ui_rt::jet_tui_key_event(&code))
    })
}

fn jet_jit_tui_key_event_modifiers(code: i64, modifiers: i64) -> i64 {
    with_rt(|rt| {
        let code = rt.heap.clone_string(code).unwrap_or_default();
        ui_push_tui_event(rt, ui_rt::jet_tui_key_event_modifiers(&code, modifiers))
    })
}

fn jet_jit_tui_resize_event(width: f64, height: f64) -> i64 {
    with_rt(|rt| ui_push_tui_event(rt, ui_rt::jet_tui_resize_event(width, height)))
}

fn jet_jit_tui_timer_event(id: i64, elapsed_ms: i64) -> i64 {
    with_rt(|rt| {
        let id = rt.heap.clone_string(id).unwrap_or_default();
        ui_push_tui_event(rt, ui_rt::jet_tui_timer_event(&id, elapsed_ms))
    })
}

fn jet_jit_tui_io_event(channel: i64, payload: i64) -> i64 {
    with_rt(|rt| {
        let channel = rt.heap.clone_string(channel).unwrap_or_default();
        let payload = (0..rt.heap.list_len(payload).unwrap_or(0))
            .filter_map(|index| rt.heap.list_get_int(payload, index))
            .map(|value| value.clamp(0, 255) as u8)
            .collect();
        ui_push_tui_event(rt, ui_rt::jet_tui_io_event(&channel, payload))
    })
}

fn jet_jit_tui_focus_event(focused: i64) -> i64 {
    with_rt(|rt| ui_push_tui_event(rt, ui_rt::jet_tui_focus_event(focused != 0)))
}

fn jet_jit_tui_interrupt_event() -> i64 {
    with_rt(|rt| ui_push_tui_event(rt, ui_rt::jet_tui_interrupt_event()))
}

fn jet_jit_tui_close_event() -> i64 {
    with_rt(|rt| ui_push_tui_event(rt, ui_rt::jet_tui_close_event()))
}

fn jet_jit_tui_color_ansi16(index: i64) -> i64 {
    with_rt(|rt| ui_push_tui_color(rt, ui_rt::jet_tui_color_ansi16(index)))
}

fn jet_jit_tui_color_ansi256(index: i64) -> i64 {
    with_rt(|rt| ui_push_tui_color(rt, ui_rt::jet_tui_color_ansi256(index)))
}

fn jet_jit_tui_color_rgb(red: i64, green: i64, blue: i64) -> i64 {
    with_rt(|rt| ui_push_tui_color(rt, ui_rt::jet_tui_color_rgb(red, green, blue)))
}

fn jet_jit_tui_style() -> i64 {
    with_rt(|rt| ui_push_tui_style(rt, ui_rt::jet_tui_style()))
}

fn jet_jit_tui_style_foreground(style: i64, color: i64) -> i64 {
    with_rt(|rt| {
        let Some(style) = ui_decode_tui_style(rt, style) else {
            return 0;
        };
        let Some(color) = ui_decode_tui_color(rt, color) else {
            return 0;
        };
        ui_push_tui_style(rt, ui_rt::jet_tui_style_foreground(style, color))
    })
}

fn jet_jit_tui_style_background(style: i64, color: i64) -> i64 {
    with_rt(|rt| {
        let Some(style) = ui_decode_tui_style(rt, style) else {
            return 0;
        };
        let Some(color) = ui_decode_tui_color(rt, color) else {
            return 0;
        };
        ui_push_tui_style(rt, ui_rt::jet_tui_style_background(style, color))
    })
}

fn jet_jit_tui_style_bold(style: i64, enabled: i64) -> i64 {
    with_rt(|rt| {
        let Some(style) = ui_decode_tui_style(rt, style) else {
            return 0;
        };
        ui_push_tui_style(rt, ui_rt::jet_tui_style_bold(style, enabled != 0))
    })
}

fn jet_jit_tui_style_dim(style: i64, enabled: i64) -> i64 {
    with_rt(|rt| {
        let Some(style) = ui_decode_tui_style(rt, style) else {
            return 0;
        };
        ui_push_tui_style(rt, ui_rt::jet_tui_style_dim(style, enabled != 0))
    })
}

fn jet_jit_tui_style_underline(style: i64, enabled: i64) -> i64 {
    with_rt(|rt| {
        let Some(style) = ui_decode_tui_style(rt, style) else {
            return 0;
        };
        ui_push_tui_style(rt, ui_rt::jet_tui_style_underline(style, enabled != 0))
    })
}

fn jet_jit_tui_style_text(text: i64, style: i64, capabilities: i64) -> i64 {
    with_rt(|rt| {
        let text = rt.heap.clone_string(text).unwrap_or_default();
        let Some(style) = ui_decode_tui_style(rt, style) else {
            return 0;
        };
        let Some(capabilities) = ui_decode_tui_capabilities(rt, capabilities) else {
            return 0;
        };
        rt.heap
            .alloc_string(ui_rt::jet_tui_style_text(&text, style, capabilities))
    })
}

fn jet_jit_tui_ascii(text: i64) -> i64 {
    with_rt(|rt| {
        let text = rt.heap.clone_string(text).unwrap_or_default();
        rt.heap.alloc_string(ui_rt::jet_tui_ascii(&text))
    })
}

fn jet_jit_tui_display_width(text: i64) -> i64 {
    with_rt(|rt| {
        let text = rt.heap.clone_string(text).unwrap_or_default();
        ui_rt::jet_tui_display_width_int(&text)
    })
}


fn jet_jit_tui_length(value: f64) -> i64 {
    with_rt(|rt| ui_push_tui_constraint(rt, ui_rt::jet_tui_length(value)))
}

fn jet_jit_tui_min(value: f64) -> i64 {
    with_rt(|rt| ui_push_tui_constraint(rt, ui_rt::jet_tui_min(value)))
}

fn jet_jit_tui_max(value: f64) -> i64 {
    with_rt(|rt| ui_push_tui_constraint(rt, ui_rt::jet_tui_max(value)))
}

fn jet_jit_tui_percent(value: f64) -> i64 {
    with_rt(|rt| ui_push_tui_constraint(rt, ui_rt::jet_tui_percent(value)))
}

fn jet_jit_tui_fill(value: f64) -> i64 {
    with_rt(|rt| ui_push_tui_constraint(rt, ui_rt::jet_tui_fill(value)))
}

fn jet_jit_tui_horizontal() -> i64 {
    with_rt(|rt| ui_push_tui_direction(rt, ui_rt::jet_tui_horizontal()))
}

fn jet_jit_tui_vertical() -> i64 {
    with_rt(|rt| ui_push_tui_direction(rt, ui_rt::jet_tui_vertical()))
}

fn jet_jit_tui_layout(area: i64, direction: i64, constraints: i64) -> i64 {
    with_rt(|rt| {
        let Some(area) = rt.ui.rects.get(area.saturating_sub(1) as usize).copied() else {
            return 0;
        };
        let Some(direction) = ui_decode_tui_direction(rt, direction) else {
            return 0;
        };
        let mut values = Vec::new();
        for index in 0..rt.heap.list_len(constraints).unwrap_or(0) {
            let Some(handle) = rt.heap.list_get_int(constraints, index) else {
                return 0;
            };
            let Some(value) = ui_decode_tui_constraint(rt, handle) else {
                return 0;
            };
            values.push(value);
        }
        let frames = ui_rt::jet_tui_layout(area, direction, values);
        let output = rt.heap.alloc_empty_list();
        for frame in frames {
            rt.ui.rects.push(frame);
            let _ = rt.heap.list_push_int(output, rt.ui.rects.len() as i64);
        }
        output
    })
}

fn jet_jit_tui_list(items: i64) -> i64 {
    with_rt(|rt| {
        let Some(items) = ui_list_strings(rt, items) else {
            return 0;
        };
        let area = rt.ui.rects.last().copied().unwrap_or_else(|| {
            let capabilities = ui_rt::jet_tui_capabilities();
            ui_rt::jet_ui_rect(
                0.0,
                0.0,
                capabilities.width as f64,
                capabilities.height as f64,
            )
        });
        let node = ui_rt::JetTuiList::new(items)
            .render(area, &mut ui_rt::JetTuiListState::default());
        rt.ui.nodes.push(node);
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_tui_table(headers: i64, rows: i64) -> i64 {
    with_rt(|rt| {
        let Some(headers) = ui_list_strings(rt, headers) else {
            return 0;
        };
        let mut table_rows = Vec::new();
        for index in 0..rt.heap.list_len(rows).unwrap_or(0) {
            let Some(row) = rt.heap.list_get_int(rows, index) else {
                return 0;
            };
            let Some(row) = ui_list_strings(rt, row) else {
                return 0;
            };
            table_rows.push(row);
        }
        let area = rt.ui.rects.last().copied().unwrap_or_else(|| {
            let capabilities = ui_rt::jet_tui_capabilities();
            ui_rt::jet_ui_rect(
                0.0,
                0.0,
                capabilities.width as f64,
                capabilities.height as f64,
            )
        });
        let node = ui_rt::JetTuiTable::new(headers, table_rows).render(area);
        rt.ui.nodes.push(node);
        rt.ui.nodes.len() as i64
    })
}

fn jet_jit_tui_list_state() -> i64 {
    with_rt(|rt| ui_push_tui_list_state(rt, ui_rt::jet_tui_list_state()))
}

fn jet_jit_tui_list_state_select(state: i64, index: i64) -> i64 {
    with_rt(|rt| {
        let Some(state) = ui_decode_tui_list_state(rt, state) else {
            return 0;
        };
        let state = ui_rt::jet_tui_list_state_select(state, index);
        ui_push_tui_list_state(rt, state)
    })
}

fn jet_jit_tui_list_state_offset(state: i64, offset: i64) -> i64 {
    with_rt(|rt| {
        let Some(state) = ui_decode_tui_list_state(rt, state) else {
            return 0;
        };
        ui_push_tui_list_state(rt, ui_rt::jet_tui_list_state_offset(state, offset))
    })
}

fn jet_jit_tui_list_state_selected(state: i64) -> i64 {
    with_rt(|rt| {
        ui_decode_tui_list_state(rt, state)
            .map(|state| ui_rt::jet_tui_list_state_selected(state))
            .unwrap_or(0)
    })
}
fn jet_jit_ui_point(x: f64, y: f64) -> i64 {
    push_struct_f64(&[x, y])
}

fn jet_jit_ui_size(width: f64, height: f64) -> i64 {
    push_struct_f64(&[width, height])
}

fn jet_jit_ui_aria_role(kind: i64) -> i64 {
    with_rt(|rt| {
        let role = match kind {
            0 => ui_rt::jet_ui_aria_role_button(),
            1 => ui_rt::jet_ui_aria_role_text_input(),
            2 => ui_rt::jet_ui_aria_role_label(),
            _ => ui_rt::jet_ui_aria_role_container(),
        };
        rt.ui.roles.push(role);
        rt.ui.roles.len() as i64
    })
}

fn jet_jit_core_ui_aria_role_button() -> i64 {
    jet_jit_ui_aria_role(0)
}

fn jet_jit_core_ui_aria_role_text_input() -> i64 {
    jet_jit_ui_aria_role(1)
}

fn jet_jit_core_ui_aria_role_label() -> i64 {
    jet_jit_ui_aria_role(2)
}

fn jet_jit_core_ui_aria_role_container() -> i64 {
    jet_jit_ui_aria_role(3)
}

fn ui_backend_clone(
    rt: &crate::runtime_host::JitRuntime,
    backend: i64,
) -> Option<UiBackendSlot> {
    rt.ui
        .backends
        .get(backend.saturating_sub(1) as usize)
        .cloned()
}

fn ui_backend_clone_from_runtime(backend: i64) -> Option<UiBackendSlot> {
    with_rt(|rt| ui_backend_clone(rt, backend))
}

fn jet_jit_ui_measure(backend: i64, node: i64, constraint: i64) -> i64 {
    let Some((backend, node, constraint)) = with_rt(|rt| {
        Some((
            ui_backend_clone(rt, backend)?,
            rt.ui
                .nodes
                .get(node.saturating_sub(1) as usize)?
                .clone(),
            *rt.ui
                .constraints
                .get(constraint.saturating_sub(1) as usize)?,
        ))
    }) else {
        panic!("jit ui measure: bad backend, node, or constraint");
    };
    let size = match backend {
        UiBackendSlot::Null(b) => b.measure_node(node, constraint),
        UiBackendSlot::Tui(b) => b.measure_node(node, constraint),
        UiBackendSlot::Gtk(b) => b.measure_node(node, constraint),
    };
    with_rt(|rt| {
        rt.ui.sizes.push(size);
        let h = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_float(h, 0, size.width);
        let _ = rt.heap.record_set_float(h, 1, size.height);
        h
    })
}

fn jet_jit_ui_layout(backend: i64, node: i64, rect: i64) {
    let Some((backend, node, rect)) = with_rt(|rt| {
        Some((
            ui_backend_clone(rt, backend)?,
            rt.ui
                .nodes
                .get(node.saturating_sub(1) as usize)?
                .clone(),
            *rt.ui.rects.get(rect.saturating_sub(1) as usize)?,
        ))
    }) else {
        panic!("jit ui layout: bad backend, node, or rect");
    };
    match backend {
        UiBackendSlot::Null(b) => b.layout_node(node, rect),
        UiBackendSlot::Tui(b) => b.layout_node(node, rect),
        UiBackendSlot::Gtk(b) => b.layout_node(node, rect),
    }
}

fn jet_jit_ui_paint(backend: i64, node: i64) {
    let Some((backend, node)) = with_rt(|rt| {
        Some((
            ui_backend_clone(rt, backend)?,
            rt.ui
                .nodes
                .get(node.saturating_sub(1) as usize)?
                .clone(),
        ))
    }) else {
        panic!("jit ui paint: bad backend or node");
    };
    match backend {
        UiBackendSlot::Null(b) => b.paint_node(node),
        UiBackendSlot::Tui(b) => b.paint_node(node),
        UiBackendSlot::Gtk(b) => b.paint_node(node),
    }
}

/// D-UI-MOUNT1=A: measure → layout → paint (I9: same Prelude methods AOT uses).
fn jet_jit_ui_mount(backend: i64, node: i64, constraint: i64) {
    let Some((backend, node, constraint)) = with_rt(|rt| {
        Some((
            ui_backend_clone(rt, backend)?,
            rt.ui
                .nodes
                .get(node.saturating_sub(1) as usize)?
                .clone(),
            *rt.ui
                .constraints
                .get(constraint.saturating_sub(1) as usize)?,
        ))
    }) else {
        panic!("jit ui mount: bad backend, node, or constraint");
    };
    match backend {
        UiBackendSlot::Null(b) => b.mount_node(node, constraint),
        UiBackendSlot::Tui(b) => b.mount_node(node, constraint),
        UiBackendSlot::Gtk(b) => b.mount_node(node, constraint),
    }
}

fn jet_jit_ui_mount_default(backend: i64, node: i64) {
    let Some((backend, node)) = with_rt(|rt| {
        Some((
            ui_backend_clone(rt, backend)?,
            rt.ui
                .nodes
                .get(node.saturating_sub(1) as usize)?
                .clone(),
        ))
    }) else {
        panic!("jit ui mount_default: bad backend or node");
    };
    match backend {
        UiBackendSlot::Null(b) => b.mount_node_default(node),
        UiBackendSlot::Tui(b) => b.mount_node_default(node),
        UiBackendSlot::Gtk(b) => b.mount_node_default(node),
    }
}

fn jet_jit_ui_on_event(backend: i64, event: i64) -> i64 {
    let Some((backend, event)) = with_rt(|rt| {
        Some((
            ui_backend_clone(rt, backend)?,
            rt.ui
                .events
                .get(event.saturating_sub(1) as usize)?
                .clone(),
        ))
    }) else {
        panic!("jit ui on_event: bad backend or event");
    };
    let result = match backend {
        UiBackendSlot::Null(b) => b.dispatch_event(event),
        UiBackendSlot::Tui(b) => b.dispatch_event(event),
        UiBackendSlot::Gtk(b) => b.dispatch_event(event),
    };
    // JetEventResult as packed unit enum: 0=Handled, 1=Ignored (JetShow names).
    match result {
        ui_rt::JetEventResult::Handled => 0,
        ui_rt::JetEventResult::Ignored => 1,
    }
}

fn jet_jit_ui_commands(backend: i64) -> i64 {
    let backend = ui_backend_clone_from_runtime(backend)
        .expect("jit ui commands: bad backend");
    let cmds = match backend {
        UiBackendSlot::Null(b) => b.paint_commands(),
        UiBackendSlot::Tui(_) | UiBackendSlot::Gtk(_) => Vec::new(),
    };
    with_rt(|rt| {
        let list = rt.heap.alloc_empty_list();
        for cmd in cmds {
            let sid = rt.heap.alloc_string(cmd);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    })
}

fn jet_jit_ui_frame_lines(backend: i64) -> i64 {
    let backend = ui_backend_clone_from_runtime(backend)
        .expect("jit ui frame_lines: bad backend");
    let lines = match backend {
        UiBackendSlot::Tui(b) => b.frame_lines(),
        _ => Vec::new(),
    };
    with_rt(|rt| {
        let list = rt.heap.alloc_empty_list();
        for line in lines {
            let sid = rt.heap.alloc_string(line);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    })
}

fn jet_jit_ui_render_count(backend: i64) -> i64 {
    let backend = ui_backend_clone_from_runtime(backend)
        .expect("jit ui render_count: bad backend");
    let count = match backend {
        UiBackendSlot::Tui(b) => b.render_count(),
        _ => 0,
    };
    with_rt(|rt| rt.heap.int_from_i64(count))
}

fn jet_jit_ui_set_focus_group(backend: i64, nodes: i64) {
    let Some((backend, group)) = with_rt(|rt| {
        let n = rt.heap.list_len(nodes).unwrap_or(0);
        let mut group = Vec::new();
        for i in 0..n {
            let id = rt.heap.list_get_int(nodes, i).unwrap_or(0);
            if let Some(node) = rt.ui.nodes.get(id.saturating_sub(1) as usize) {
                group.push(node.clone());
            }
        }
        Some((ui_backend_clone(rt, backend)?, group))
    }) else {
        panic!("jit ui set_focus_group: bad backend");
    };
    match backend {
        UiBackendSlot::Null(b) => b.set_focus_group(group),
        UiBackendSlot::Tui(b) => b.set_focus_group(group),
        UiBackendSlot::Gtk(b) => b.set_focus_group(group),
    }
}

fn jet_jit_ui_focused_label(backend: i64) -> i64 {
    let backend = ui_backend_clone_from_runtime(backend)
        .expect("jit ui focused_label: bad backend");
    let label = match backend {
        UiBackendSlot::Null(b) => b.focused_label(),
        UiBackendSlot::Tui(b) => b.focused_label(),
        UiBackendSlot::Gtk(b) => b.focused_label(),
    };
    with_rt(|rt| rt.heap.alloc_string(label))
}

fn jet_jit_ui_gtk_button(backend: i64, label: i64) -> i64 {
    let Some((backend, label)) = with_rt(|rt| {
        Some((
            ui_backend_clone(rt, backend)?,
            rt.heap.clone_string(label).unwrap_or_default(),
        ))
    }) else {
        panic!("jit ui gtk button: bad backend");
    };
    let widget = match backend {
        UiBackendSlot::Gtk(b) => b.button(&label),
        _ => 0,
    };
    with_rt(|rt| {
        rt.ui.gtk_widgets.push(widget);
        rt.ui.gtk_widgets.len() as i64
    })
}

fn jet_jit_ui_gtk_on_click(
    backend: i64,
    widget: i64,
    fn_ptr: i64,
    n_caps: i64,
    c0: i64,
    c1: i64,
    c2: i64,
    c3: i64,
) {
    let cb = crate::Reactive::JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    let Some((backend, widget)) = with_rt(|rt| {
        Some((
            ui_backend_clone(rt, backend)?,
            *rt.ui
                .gtk_widgets
                .get(widget.saturating_sub(1) as usize)
                .unwrap_or(&0),
        ))
    }) else {
        panic!("jit ui gtk on_click: bad backend");
    };
    if let UiBackendSlot::Gtk(backend) = backend {
        // The backend clone owns the same native state, so GTK callbacks can
        // re-enter the runtime without holding its mutex.
        backend.on_click(widget, move || cb.invoke_void());
    }
}

fn jet_jit_ui_gtk_present(backend: i64, title: i64) {
    let Some((backend, title)) = with_rt(|rt| {
        Some((
            ui_backend_clone(rt, backend)?,
            rt.heap.clone_string(title).unwrap_or_default(),
        ))
    }) else {
        panic!("jit ui gtk present: bad backend");
    };
    if let UiBackendSlot::Gtk(backend) = backend {
        // `present` runs GTK's blocking loop. Do not hold the runtime mutex:
        // callbacks dispatched by that loop must be able to borrow it.
        backend.present(&title);
    }
}

fn jet_jit_ui_reactive_render(fn_ptr: i64, n_caps: i64, c0: i64, c1: i64, c2: i64, c3: i64) {
    let cb = crate::Reactive::JitCb {
        fn_ptr: fn_ptr as u64,
        caps: [c0, c1, c2, c3],
        n_caps: n_caps.clamp(0, 4) as u8,
    };
    // Use canonical reactive effect so signal deps re-run the render body.
    crate::Reactive::reactive_rt::jet_reactive_effect_rooted(move || cb.invoke_void());
}

host_fns! {
    struct UiHostFns;
    register: register_ui_symbols;
    declare: declare_ui_host_fns(module) {
        let cc = module.target_config().default_call_conv;

        let mut nullary = Signature::new(cc);
        nullary.returns.push(AbiParam::new(types::I64));
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut unary_void = Signature::new(cc);
        unary_void.params.push(AbiParam::new(types::I64));
        let mut binary = Signature::new(cc);
        binary.params.push(AbiParam::new(types::I64));
        binary.params.push(AbiParam::new(types::I64));
        binary.returns.push(AbiParam::new(types::I64));
        let mut preview_source = Signature::new(cc);
        for _ in 0..9 {
            preview_source.params.push(AbiParam::new(types::I64));
        }
        preview_source.returns.push(AbiParam::new(types::I64));
        let mut binary_void = Signature::new(cc);
        binary_void.params.push(AbiParam::new(types::I64));
        binary_void.params.push(AbiParam::new(types::I64));
        let mut node3 = Signature::new(cc);
        node3.params.push(AbiParam::new(types::I64));
        node3.params.push(AbiParam::new(types::F64));
        node3.params.push(AbiParam::new(types::F64));
        node3.returns.push(AbiParam::new(types::I64));
        let mut node4 = Signature::new(cc);
        node4.params.push(AbiParam::new(types::I64));
        node4.params.push(AbiParam::new(types::F64));
        node4.params.push(AbiParam::new(types::F64));
        node4.params.push(AbiParam::new(types::I64));
        node4.returns.push(AbiParam::new(types::I64));
        let mut f4 = Signature::new(cc);
        for _ in 0..4 {
            f4.params.push(AbiParam::new(types::F64));
        }
        f4.returns.push(AbiParam::new(types::I64));
        let mut f2 = Signature::new(cc);
        f2.params.push(AbiParam::new(types::F64));
        f2.params.push(AbiParam::new(types::F64));
        f2.returns.push(AbiParam::new(types::I64));
        let mut ternary = Signature::new(cc);
        for _ in 0..3 {
            ternary.params.push(AbiParam::new(types::I64));
        }
        ternary.returns.push(AbiParam::new(types::I64));
        let mut f1 = Signature::new(cc);
        f1.params.push(AbiParam::new(types::F64));
        f1.returns.push(AbiParam::new(types::I64));
        let mut i3 = Signature::new(cc);
        for _ in 0..3 {
            i3.params.push(AbiParam::new(types::I64));
        }
        i3.returns.push(AbiParam::new(types::I64));
        let mut triary = Signature::new(cc);
        for _ in 0..3 {
            triary.params.push(AbiParam::new(types::I64));
        }
        triary.returns.push(AbiParam::new(types::I64));

        let mut measure = Signature::new(cc);
        measure.params.push(AbiParam::new(types::I64));
        measure.params.push(AbiParam::new(types::I64));
        measure.params.push(AbiParam::new(types::I64));
        measure.returns.push(AbiParam::new(types::I64));
        let mut layout = Signature::new(cc);
        layout.params.push(AbiParam::new(types::I64));
        layout.params.push(AbiParam::new(types::I64));
        layout.params.push(AbiParam::new(types::I64));
        let mut paint = Signature::new(cc);
        paint.params.push(AbiParam::new(types::I64));
        paint.params.push(AbiParam::new(types::I64));
        let mut cb6 = Signature::new(cc);
        for _ in 0..6 {
            cb6.params.push(AbiParam::new(types::I64));
        }
        let mut core_button = Signature::new(cc);
        for _ in 0..4 {
            core_button.params.push(AbiParam::new(types::I64));
        }
        core_button.returns.push(AbiParam::new(types::I64));
        let mut gtk_click = Signature::new(cc);
        for _ in 0..8 {
            gtk_click.params.push(AbiParam::new(types::I64));
        }
        let mut node_dim = Signature::new(cc);
        node_dim.params.push(AbiParam::new(types::I64));
        node_dim.params.push(AbiParam::new(types::I64));
        node_dim.returns.push(AbiParam::new(types::F64));
    }
    tui_capabilities: "jet_tui_capabilities" => jet_jit_tui_capabilities: nullary;
    tui_ascii: "jet_tui_ascii" => jet_jit_tui_ascii: unary;
    tui_close_event: "jet_tui_close_event" => jet_jit_tui_close_event: nullary;
    tui_color_ansi16: "jet_tui_color_ansi16" => jet_jit_tui_color_ansi16: unary;
    tui_color_ansi256: "jet_tui_color_ansi256" => jet_jit_tui_color_ansi256: unary;
    tui_color_rgb: "jet_tui_color_rgb" => jet_jit_tui_color_rgb: i3;
    tui_display_width: "jet_tui_display_width_int" => jet_jit_tui_display_width: unary;
    tui_fill: "jet_tui_fill" => jet_jit_tui_fill: f1;
    tui_focus_event: "jet_tui_focus_event" => jet_jit_tui_focus_event: unary;
    tui_horizontal: "jet_tui_horizontal" => jet_jit_tui_horizontal: nullary;
    tui_interrupt_event: "jet_tui_interrupt_event" => jet_jit_tui_interrupt_event: nullary;
    tui_io_event: "jet_tui_io_event" => jet_jit_tui_io_event: binary;
    tui_key_event: "jet_tui_key_event" => jet_jit_tui_key_event: unary;
    tui_key_event_modifiers: "jet_tui_key_event_modifiers" => jet_jit_tui_key_event_modifiers: binary;
    tui_layout: "jet_tui_layout" => jet_jit_tui_layout: triary;
    tui_length: "jet_tui_length" => jet_jit_tui_length: f1;
    tui_list: "jet_tui_list" => jet_jit_tui_list: unary;
    tui_list_state: "jet_tui_list_state" => jet_jit_tui_list_state: nullary;
    tui_list_state_offset: "jet_tui_list_state_offset" => jet_jit_tui_list_state_offset: binary;
    tui_list_state_select: "jet_tui_list_state_select" => jet_jit_tui_list_state_select: binary;
    tui_list_state_selected: "jet_tui_list_state_selected" => jet_jit_tui_list_state_selected: unary;
    tui_max: "jet_tui_max" => jet_jit_tui_max: f1;
    tui_min: "jet_tui_min" => jet_jit_tui_min: f1;
    tui_percent: "jet_tui_percent" => jet_jit_tui_percent: f1;
    tui_resize_event: "jet_tui_resize_event" => jet_jit_tui_resize_event: f2;
    tui_style: "jet_tui_style" => jet_jit_tui_style: nullary;
    tui_style_background: "jet_tui_style_background" => jet_jit_tui_style_background: binary;
    tui_style_bold: "jet_tui_style_bold" => jet_jit_tui_style_bold: binary;
    tui_style_dim: "jet_tui_style_dim" => jet_jit_tui_style_dim: binary;
    tui_style_foreground: "jet_tui_style_foreground" => jet_jit_tui_style_foreground: binary;
    tui_style_text: "jet_tui_style_text" => jet_jit_tui_style_text: ternary;
    tui_style_underline: "jet_tui_style_underline" => jet_jit_tui_style_underline: binary;
    tui_table: "jet_tui_table" => jet_jit_tui_table: binary;
    tui_timer_event: "jet_tui_timer_event" => jet_jit_tui_timer_event: binary;
    tui_vertical: "jet_tui_vertical" => jet_jit_tui_vertical: nullary;
    null_backend: "jet_jit_ui_null_backend" => jet_jit_ui_null_backend: nullary;
    tui_backend: "jet_jit_ui_tui_backend" => jet_jit_ui_tui_backend: nullary;
    gtk_backend: "jet_jit_ui_gtk_backend" => jet_jit_ui_gtk_backend: nullary;
    font_system: "jet_font_system" => jet_jit_font_system: unary;
    font_shape: "jet_font_shape" => jet_jit_font_shape: binary;
    shortcut_command: "JetUiShortcut::cmd" => jet_jit_ui_shortcut_command: unary;
    node_accessibility: "jet_ui_node_accessibility" => jet_jit_ui_node_accessibility: binary;
    node_shortcut: "jet_ui_node_shortcut" => jet_jit_ui_node_shortcut: binary;
    text_input: "jet_ui_text_input" => jet_jit_ui_text_input: binary;
    host_capabilities: "jet_ui_host_capabilities" => jet_jit_ui_host_capabilities: nullary;
    host_file_filter: "jet_ui_host_file_filter" => jet_jit_ui_host_file_filter: ternary;
    host_file_filter_text: "jet_ui_host_file_filter_text" => jet_jit_ui_host_file_filter_text: nullary;
    host_fs_rights_read: "jet_ui_host_fs_rights_read" => jet_jit_ui_host_fs_rights_read: nullary;
    host_fs_rights_write: "jet_ui_host_fs_rights_write" => jet_jit_ui_host_fs_rights_write: nullary;
    host_fs_rights_read_write: "jet_ui_host_fs_rights_read_write" => jet_jit_ui_host_fs_rights_read_write: nullary;
    host_fs_grant: "jet_ui_host_fs_grant" => jet_jit_ui_host_fs_grant: binary;
    host_open_request: "jet_ui_host_open_request" => jet_jit_ui_host_open_request: unary;
    host_save_request: "jet_ui_host_save_request" => jet_jit_ui_host_save_request: unary;
    host_open_file: "jet_ui_host_open_file" => jet_jit_ui_host_open_file: unary;
    host_save_file: "jet_ui_host_save_file" => jet_jit_ui_host_save_file: unary;
    host_shortcut: "jet_ui_host_shortcut" => jet_jit_ui_host_shortcut: binary;
    core_preview_source_attach: "jet_ui_preview_attach_compiler_source" => jet_jit_core_ui_preview_attach_compiler_source: preview_source;
    host_accessibility: "jet_ui_host_accessibility" => jet_jit_ui_host_accessibility: binary;
    host_clipboard_read_text: "jet_ui_host_clipboard_read_text" => jet_jit_ui_host_clipboard_read_text: nullary;
    host_clipboard_write_text: "jet_ui_host_clipboard_write_text" => jet_jit_ui_host_clipboard_write_text: unary;
    host_ime_poll: "jet_ui_host_ime_poll" => jet_jit_ui_host_ime_poll: nullary;
    host_drag_poll: "jet_ui_host_drag_poll" => jet_jit_ui_host_drag_poll: nullary;
    host_shortcut_binding: "jet_ui_host_shortcut_binding" => jet_jit_ui_host_shortcut_binding: binary;
    host_shortcuts_register: "jet_ui_host_shortcuts_register" => jet_jit_ui_host_shortcuts_register: unary;
    host_shortcuts_dispatch: "jet_ui_host_shortcuts_dispatch" => jet_jit_ui_host_shortcuts_dispatch: unary;
    core_button_on_click: "jet_ui_button_on_click" => jet_jit_core_ui_button_on_click: core_button;
    core_preview_with_viewport: "jet_ui_preview_with_viewport" => jet_jit_core_ui_preview_with_viewport: ternary;
    core_playground_with_viewport: "jet_ui_playground_with_viewport" => jet_jit_core_ui_playground_with_viewport: ternary;
    core_text_input_on_drop: "jet_ui_text_input_on_drop" => jet_jit_core_ui_text_input_on_drop: ternary;
    text: "jet_jit_ui_text" => jet_jit_ui_text: unary;
    node: "jet_ui_node" => jet_jit_ui_node: node3;
    button: "jet_jit_ui_button" => jet_jit_ui_button: unary;
    preview_phone: "jet_ui_phone" => jet_jit_ui_phone: nullary;
    preview_tablet: "jet_ui_tablet" => jet_jit_ui_tablet: nullary;
    preview_desktop: "jet_ui_desktop" => jet_jit_ui_desktop: nullary;
    preview_registry: "jet_ui_previews" => jet_jit_ui_previews: unary;
    playground_registry: "jet_ui_playgrounds" => jet_jit_ui_playgrounds: unary;
    core_reactive_render: "jet_ui_reactive_render" => jet_jit_core_ui_reactive_render: unary;
    node_color: "jet_jit_ui_node_color" => jet_jit_ui_node_color: node4;
    point: "jet_jit_ui_point" => jet_jit_ui_point: f2;
    size: "jet_jit_ui_size" => jet_jit_ui_size: f2;
    node_role: "jet_jit_ui_node_role" => jet_jit_ui_node_role: node4;
    box_node: "jet_jit_ui_box" => jet_jit_ui_box: unary;
    constraint: "jet_jit_ui_constraint" => jet_jit_ui_constraint: f4;
    rect: "jet_jit_ui_rect" => jet_jit_ui_rect: f4;
    key_event: "jet_jit_ui_key_event" => jet_jit_ui_key_event: unary;
    resize_event: "jet_jit_ui_resize_event" => jet_jit_ui_resize_event: f2;
    aria_role: "jet_jit_ui_aria_role" => jet_jit_ui_aria_role: unary;
    core_aria_role_button: "jet_ui_aria_role_button" => jet_jit_core_ui_aria_role_button: nullary;
    core_aria_role_text_input: "jet_ui_aria_role_text_input" => jet_jit_core_ui_aria_role_text_input: nullary;
    core_aria_role_label: "jet_ui_aria_role_label" => jet_jit_core_ui_aria_role_label: nullary;
    core_aria_role_container: "jet_ui_aria_role_container" => jet_jit_core_ui_aria_role_container: nullary;
    node_label: "jet_jit_ui_node_label" => jet_jit_ui_node_label: unary;
    node_dim: "jet_jit_ui_node_dim" => jet_jit_ui_node_dim: node_dim;
    measure: "jet_jit_ui_measure" => jet_jit_ui_measure: measure;
    layout: "jet_jit_ui_layout" => jet_jit_ui_layout: layout;
    paint: "jet_jit_ui_paint" => jet_jit_ui_paint: paint;
    mount: "jet_jit_ui_mount" => jet_jit_ui_mount: layout;
    mount_default: "jet_jit_ui_mount_default" => jet_jit_ui_mount_default: paint;
    on_event: "jet_jit_ui_on_event" => jet_jit_ui_on_event: binary;
    commands: "jet_jit_ui_commands" => jet_jit_ui_commands: unary;
    frame_lines: "jet_jit_ui_frame_lines" => jet_jit_ui_frame_lines: unary;
    render_count: "jet_jit_ui_render_count" => jet_jit_ui_render_count: unary;
    set_focus_group: "jet_jit_ui_set_focus_group" => jet_jit_ui_set_focus_group: binary_void;
    focused_label: "jet_jit_ui_focused_label" => jet_jit_ui_focused_label: unary;
    gtk_button: "jet_jit_ui_gtk_button" => jet_jit_ui_gtk_button: binary;
    gtk_on_click: "jet_jit_ui_gtk_on_click" => jet_jit_ui_gtk_on_click: gtk_click;
    gtk_present: "jet_jit_ui_gtk_present" => jet_jit_ui_gtk_present: binary_void;
    reactive_render: "jet_jit_ui_reactive_render" => jet_jit_ui_reactive_render: cb6;
}
