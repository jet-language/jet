//! D-RENDERTGT*: resident-JIT UI host — `include!` canonical Prelude/Ui.rs
//! and the dynamic native GTK adapter. Opaque handles only.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use jet_codegen::Comptime::DevSink;
use jet_foundation::AST::{CtFloat, CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::Prelude::jet_e0956_unsupported as unsupported;
use jet_foundation::MIR::{
    MirCoreClosureKind, MirNominalRef, MirPreludeCallId, MirRuntimeValue, MirSiteId, MirType,
    MirTypeKind,
};

#[allow(unused_imports)]
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
    match ambient_enum(value, "UIFSAccess", span)?.as_str() {
        "Read" => Ok(ui_rt::JetUiFsAccess::Read),
        "Write" => Ok(ui_rt::JetUiFsAccess::Write),
        _ => Err(unsupported("unknown UIFSAccess variant", span)),
    }
}

fn ambient_fs_rights(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFsRights, Diagnostic> {
    let bits = ambient_int(ambient_field(value, "UIFSRights", "bits", span)?, "UIFSRights.bits", span)?;
    let bits = u8::try_from(bits).map_err(|_| unsupported("UIFSRights.bits is out of range", span))?;
    Ok(ui_rt::JetUiFsRights::from_bits(bits))
}

fn ambient_node_id(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiNodeId, Diagnostic> {
    let value = ambient_string(
        ambient_field(value, "UINodeID", "value", span)?,
        "UINodeID.value",
        span,
    )?;
    ui_rt::JetUiNodeId::new(&value)
        .map_err(|_| unsupported("UINodeID value is invalid", span))
}

fn ambient_file_filter(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFileFilter, Diagnostic> {
    let label = ambient_string(
        ambient_field(value, "UIFileFilter", "label", span)?,
        "UIFileFilter.label",
        span,
    )?;
    let extensions = ambient_strings(
        ambient_field(value, "UIFileFilter", "extensions", span)?,
        "UIFileFilter.extensions",
        span,
    )?;
    let mime_types = ambient_strings(
        ambient_field(value, "UIFileFilter", "mime_types", span)?,
        "UIFileFilter.mime_types",
        span,
    )?;
    ui_rt::JetUiFileFilter::new(&label, extensions, mime_types)
        .map_err(|error| unsupported(&format!("invalid UIFileFilter: {error:?}"), span))
}

fn ambient_granted_path(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiGrantedPath, Diagnostic> {
    let path = ambient_string(
        ambient_field(value, "UIGrantedPath", "path", span)?,
        "UIGrantedPath.path",
        span,
    )?;
    let grant_root = ambient_string(
        ambient_field(value, "UIGrantedPath", "grant_root", span)?,
        "UIGrantedPath.grant_root",
        span,
    )?;
    let access = ambient_fs_access(
        ambient_field(value, "UIGrantedPath", "access", span)?,
        span,
    )?;
    let grant = ui_rt::JetUiFsGrant::new(&grant_root, match access {
        ui_rt::JetUiFsAccess::Read => ui_rt::JetUiFsRights::read(),
        ui_rt::JetUiFsAccess::Write => ui_rt::JetUiFsRights::write(),
    })
    .map_err(|error| unsupported(&format!("invalid UIGrantedPath grant: {error:?}"), span))?;
    grant.scope(&path, access)
        .map_err(|error| unsupported(&format!("invalid UIGrantedPath: {error:?}"), span))
}

fn ambient_fs_grant(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFsGrant, Diagnostic> {
    let root = ambient_string(
        ambient_field(value, "UIFSGrant", "root", span)?,
        "UIFSGrant.root",
        span,
    )?;
    let rights = ambient_fs_rights(
        ambient_field(value, "UIFSGrant", "rights", span)?,
        span,
    )?;
    ui_rt::JetUiFsGrant::new(&root, rights)
        .map_err(|error| unsupported(&format!("invalid UIFSGrant: {error:?}"), span))
}

fn ambient_file_request(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiFileDialogRequest, Diagnostic> {
    let kind = match ambient_enum(
        ambient_field(value, "UIFileDialogRequest", "kind", span)?,
        "UIFileDialogKind",
        span,
    )?
    .as_str()
    {
        "Open" => ui_rt::JetUiFileDialogKind::Open,
        "Save" => ui_rt::JetUiFileDialogKind::Save,
        _ => return Err(unsupported("unknown UIFileDialogKind variant", span)),
    };
    let title = ambient_string(
        ambient_field(value, "UIFileDialogRequest", "title", span)?,
        "UIFileDialogRequest.title",
        span,
    )?;
    let grant = ambient_fs_grant(
        ambient_field(value, "UIFileDialogRequest", "grant", span)?,
        span,
    )?;
    let initial_directory = ambient_optional(
        ambient_field(value, "UIFileDialogRequest", "initial_directory", span)?,
        "UIGrantedPath",
        span,
    )?
    .map(|value| ambient_granted_path(value, span))
    .transpose()?;
    let filters = ambient_list(
        ambient_field(value, "UIFileDialogRequest", "filters", span)?,
        "UIFileDialogRequest.filters",
        span,
    )?
    .iter()
    .map(|value| ambient_file_filter(value, span))
    .collect::<Result<Vec<_>, _>>()?;
    let allow_multiple = ambient_bool(
        ambient_field(value, "UIFileDialogRequest", "allow_multiple", span)?,
        "UIFileDialogRequest.allow_multiple",
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

fn ambient_shortcut_modifiers(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiShortcutModifiers, Diagnostic> {
    let bits = ambient_int(
        ambient_field(value, "UIShortcutModifiers", "bits", span)?,
        "UIShortcutModifiers.bits",
        span,
    )?;
    let bits = u8::try_from(bits)
        .map_err(|_| unsupported("UIShortcutModifiers.bits is out of range", span))?;
    Ok(ui_rt::JetUiShortcutModifiers::from_bits(bits))
}

fn ambient_shortcut(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiShortcut, Diagnostic> {
    let key = ambient_string(
        ambient_field(value, "UIShortcut", "key", span)?,
        "UIShortcut.key",
        span,
    )?;
    let modifiers = ambient_shortcut_modifiers(
        ambient_field(value, "UIShortcut", "modifiers", span)?,
        span,
    )?;
    ui_rt::JetUiShortcut::new(&key, modifiers)
        .map_err(|error| unsupported(&format!("invalid UIShortcut: {error:?}"), span))
}

fn ambient_shortcut_binding(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiShortcutBinding, Diagnostic> {
    let shortcut = ambient_shortcut(
        ambient_field(value, "UIShortcutBinding", "shortcut", span)?,
        span,
    )?;
    let action = ambient_string(
        ambient_field(value, "UIShortcutBinding", "action", span)?,
        "UIShortcutBinding.action",
        span,
    )?;
    let node = ambient_optional(
        ambient_field(value, "UIShortcutBinding", "node", span)?,
        "UINodeID",
        span,
    )?
    .map(|value| ambient_node_id(value, span))
    .transpose()?;
    ui_rt::JetUiShortcutBinding::new(shortcut, &action, node)
        .map_err(|error| unsupported(&format!("invalid UIShortcutBinding: {error:?}"), span))
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
        return Err(unsupported("UIAccessibilityState value expected", span));
    };
    if type_name != "UIAccessibilityState" {
        return Err(unsupported("UIAccessibilityState value expected", span));
    }
    match variant.as_str() {
        "Disabled" if args.is_empty() => Ok(ui_rt::JetUiAccessibilityState::Disabled),
        "Busy" if args.is_empty() => Ok(ui_rt::JetUiAccessibilityState::Busy),
        "Required" if args.is_empty() => Ok(ui_rt::JetUiAccessibilityState::Required),
        "Expanded" if args.len() == 1 => Ok(ui_rt::JetUiAccessibilityState::Expanded(
            ambient_bool(&args[0].1, "UIAccessibilityState.Expanded", span)?,
        )),
        "Checked" if args.len() == 1 => Ok(ui_rt::JetUiAccessibilityState::Checked(
            ambient_bool(&args[0].1, "UIAccessibilityState.Checked", span)?,
        )),
        "Selected" if args.len() == 1 => Ok(ui_rt::JetUiAccessibilityState::Selected(
            ambient_bool(&args[0].1, "UIAccessibilityState.Selected", span)?,
        )),
        "Value" if args.len() == 1 => Ok(ui_rt::JetUiAccessibilityState::Value(
            ambient_string(&args[0].1, "UIAccessibilityState.Value", span)?,
        )),
        _ => Err(unsupported(
            "unknown or malformed UIAccessibilityState variant",
            span,
        )),
    }
}

fn ambient_accessibility(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiAccessibility, Diagnostic> {
    let name = ambient_optional(
        ambient_field(value, "UIAccessibility", "name", span)?,
        "String",
        span,
    )?
    .map(|value| ambient_string(value, "UIAccessibility.name", span))
    .transpose()?;
    let description = ambient_optional(
        ambient_field(value, "UIAccessibility", "description", span)?,
        "String",
        span,
    )?
    .map(|value| ambient_string(value, "UIAccessibility.description", span))
    .transpose()?;
    let states = ambient_list(
        ambient_field(value, "UIAccessibility", "states", span)?,
        "UIAccessibility.states",
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
        "UIFSAccess",
        match access {
            ui_rt::JetUiFsAccess::Read => "Read",
            ui_rt::JetUiFsAccess::Write => "Write",
        },
        Vec::new(),
    )
}

fn ambient_fs_rights_value(rights: ui_rt::JetUiFsRights) -> CtValue {
    ambient_struct(
        "UIFSRights",
        vec![("bits".to_string(), CtValue::Int(i64::from(rights.bits())))],
    )
}

fn ambient_node_id_value(node_id: ui_rt::JetUiNodeId) -> CtValue {
    ambient_struct(
        "UINodeID",
        vec![("value".to_string(), CtValue::Str(node_id.as_str().to_string()))],
    )
}

fn ambient_file_filter_value(filter: ui_rt::JetUiFileFilter) -> CtValue {
    ambient_struct(
        "UIFileFilter",
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
        "UIGrantedPath",
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
        "UIFSGrant",
        vec![
            ("root".to_string(), CtValue::Str(grant.root().to_string())),
            ("rights".to_string(), ambient_fs_rights_value(grant.rights())),
        ],
    )
}


fn ambient_file_selection_value(selection: ui_rt::JetUiFileDialogSelection) -> CtValue {
    ambient_struct(
        "UIFileDialogSelection",
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
        "UITextRange",
        vec![
            ("start".to_string(), CtValue::Int(i64::try_from(range.start).unwrap_or(i64::MAX))),
            ("end".to_string(), CtValue::Int(i64::try_from(range.end).unwrap_or(i64::MAX))),
        ],
    )
}

fn ambient_clipboard_text_value(value: ui_rt::JetUiClipboardText) -> CtValue {
    ambient_struct(
        "UIClipboardText",
        vec![
            ("text".to_string(), CtValue::Str(value.text)),
            (
                "selection".to_string(),
                ambient_option_value("UITextRange", value.selection.map(ambient_text_range_value)),
            ),
        ],
    )
}

fn ambient_clipboard_write_value(value: ui_rt::JetUiClipboardWrite) -> CtValue {
    ambient_struct(
        "UIClipboardWrite",
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
        "UIShortcutModifiers",
        vec![("bits".to_string(), CtValue::Int(i64::from(modifiers.bits())))],
    )
}

fn ambient_shortcut_value(shortcut: ui_rt::JetUiShortcut) -> CtValue {
    ambient_struct(
        "UIShortcut",
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
        "UIShortcutBinding",
        vec![
            (
                "shortcut".to_string(),
                ambient_shortcut_value(binding.shortcut),
            ),
            ("action".to_string(), CtValue::Str(binding.action)),
            (
                "node".to_string(),
                ambient_option_value("UINodeID", binding.node.map(ambient_node_id_value)),
            ),
        ],
    )
}

fn ambient_shortcut_dispatch_value(dispatch: ui_rt::JetUiShortcutDispatch) -> CtValue {
    match dispatch {
        ui_rt::JetUiShortcutDispatch::Dispatched(binding) => ambient_enum_value(
            "UIShortcutDispatch",
            "Dispatched",
            vec![(None, ambient_shortcut_binding_value(binding))],
        ),
        ui_rt::JetUiShortcutDispatch::Unhandled => {
            ambient_enum_value("UIShortcutDispatch", "Unhandled", Vec::new())
        }
    }
}
fn ambient_ime_mode(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiImeMode, Diagnostic> {
    match ambient_enum(value, "UIIMEMode", span)?.as_str() {
        "Native" => Ok(ui_rt::JetUiImeMode::Native),
        "Disabled" => Ok(ui_rt::JetUiImeMode::Disabled),
        _ => Err(unsupported("unknown UIIMEMode variant", span)),
    }
}
fn ambient_ime_mode_value(mode: ui_rt::JetUiImeMode) -> CtValue {
    ambient_enum_value(
        "UIIMEMode",
        match mode {
            ui_rt::JetUiImeMode::Native => "Native",
            ui_rt::JetUiImeMode::Disabled => "Disabled",
        },
        Vec::new(),
    )
}

fn ambient_aria_role(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiAriaRole, Diagnostic> {
    match ambient_enum(value, "UIAriaRole", span)?.as_str() {
        "Button" => Ok(ui_rt::JetUiAriaRole::Button),
        "TextInput" => Ok(ui_rt::JetUiAriaRole::TextInput),
        "Label" => Ok(ui_rt::JetUiAriaRole::Label),
        "Container" => Ok(ui_rt::JetUiAriaRole::Container),
        _ => Err(unsupported("unknown UIAriaRole variant", span)),
    }
}

fn ambient_node(
    value: &CtValue,
    span: Span,
) -> Result<ui_rt::JetUiNode, Diagnostic> {
    let label = ambient_string(
        ambient_field(value, "UINode", "label", span)?,
        "UINode.label",
        span,
    )?;
    let width = ambient_float(
        ambient_field(value, "UINode", "width", span)?,
        "UINode.width",
        span,
    )?;
    let height = ambient_float(
        ambient_field(value, "UINode", "height", span)?,
        "UINode.height",
        span,
    )?;
    let role = ambient_optional(
        ambient_field(value, "UINode", "role", span)?,
        "UIAriaRole",
        span,
    )?
    .map(|value| ambient_aria_role(value, span))
    .transpose()?;
    let accessibility = ambient_optional(
        ambient_field(value, "UINode", "accessibility", span)?,
        "UIAccessibility",
        span,
    )?
    .map(|value| ambient_accessibility(value, span))
    .transpose()?;
    let ime = ambient_optional(
        ambient_field(value, "UINode", "ime", span)?,
        "UIIMEMode",
        span,
    )?
    .map(|value| ambient_ime_mode(value, span))
    .transpose()?;
    let color = ambient_optional(
        ambient_field(value, "UINode", "color", span)?,
        "String",
        span,
    )?
    .map(|value| ambient_string(value, "UINode.color", span))
    .transpose()?;
    let kind = match ambient_enum(
        ambient_field(value, "UINode", "kind", span)?,
        "UINodeKind",
        span,
    )?
    .as_str()
    {
        "Custom" => ui_rt::JetUiNodeKind::Custom,
        "Text" => ui_rt::JetUiNodeKind::Text,
        "Box" => ui_rt::JetUiNodeKind::Box,
        "Button" => ui_rt::JetUiNodeKind::Button,
        "TextInput" => ui_rt::JetUiNodeKind::TextInput,
        _ => return Err(unsupported("unknown UINodeKind variant", span)),
    };
    let children = ambient_list(
        ambient_field(value, "UINode", "children", span)?,
        "UINode.children",
        span,
    )?
    .iter()
    .map(|value| ambient_node(value, span))
    .collect::<Result<Vec<_>, _>>()?;
    let shortcut = ambient_optional(
        ambient_field(value, "UINode", "shortcut", span)?,
        "UIShortcut",
        span,
    )?
    .map(|value| ambient_shortcut(value, span))
    .transpose()?;
    let style = ambient_optional(ambient_field(value, "UINode", "style", span)?, "UI style wire", span)?
        .map(|wire| {
            let CtValue::Struct { type_name, fields: words } = wire else {
                return Err(unsupported("UI style wire must be a five-word tuple", span));
            };
            if type_name != "tuple" || words.len() != 5 {
                return Err(unsupported("UI style wire must be a five-word tuple", span));
            }
            let color = |kind: &CtValue, word: &CtValue| -> Result<Option<ui_rt::JetTuiColor>, Diagnostic> {
                let word = ambient_int(word, "UI style word", span)?;
                Ok(match ambient_int(kind, "UI style tag", span)? {
                    0 => None,
                    1 => Some(ui_rt::JetTuiColor::ANSI16(word as u8)),
                    2 => Some(ui_rt::JetTuiColor::ANSI256(word as u8)),
                    3 => Some(ui_rt::JetTuiColor::RGB((word >> 16) as u8, (word >> 8) as u8, word as u8)),
                    _ => return Err(unsupported("invalid audited UI color carrier", span)),
                })
            };
            let flags = ambient_int(&words[4].1, "UI style flags", span)?;
            Ok(ui_rt::JetTuiStyle {
                foreground: color(&words[0].1, &words[1].1)?,
                background: color(&words[2].1, &words[3].1)?,
                bold: flags & 1 != 0,
                dim: flags & 2 != 0,
                underline: flags & 4 != 0,
            })
        })
        .transpose()?;
    Ok(ui_rt::JetUiNode {
        label,
        width,
        height,
        role,
        accessibility,
        ime,
        color,
        style,
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
    ambient_enum_value("UIAccessibilityState", variant, args)
}

fn ambient_accessibility_value(value: ui_rt::JetUiAccessibility) -> CtValue {
    ambient_struct(
        "UIAccessibility",
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
        "UICapability",
        match capability {
            ui_rt::JetUiCapability::FileDialog => "FileDialog",
            ui_rt::JetUiCapability::Clipboard => "Clipboard",
            ui_rt::JetUiCapability::IME => "IME",
            ui_rt::JetUiCapability::DragDrop => "DragDrop",
            ui_rt::JetUiCapability::Shortcuts => "Shortcuts",
            ui_rt::JetUiCapability::Accessibility => "Accessibility",
            ui_rt::JetUiCapability::FontShaping => "FontShaping",
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
                "UICapabilityFact",
                vec![
                    ("capability".to_string(), ambient_capability_value(fact.capability)),
                    ("granted".to_string(), CtValue::Bool(fact.granted)),
                ],
            )
        })
        .collect();
    ambient_struct("UICapabilityFacts", vec![("facts".to_string(), CtValue::List(facts))])
}

fn ambient_node_value(node: ui_rt::JetUiNode) -> CtValue {
    let children = node
        .children
        .into_iter()
        .map(ambient_node_value)
        .collect();
    let role = node.role.map(|role| {
        ambient_enum_value(
            "UIAriaRole",
            match role {
                ui_rt::JetUiAriaRole::Button => "Button",
                ui_rt::JetUiAriaRole::TextInput => "TextInput",
                ui_rt::JetUiAriaRole::Label => "Label",
                ui_rt::JetUiAriaRole::Container => "Container",
            },
            Vec::new(),
        )
    });
    let kind = ambient_enum_value(
        "UINodeKind",
        match node.kind {
            ui_rt::JetUiNodeKind::Custom => "Custom",
            ui_rt::JetUiNodeKind::Text => "Text",
            ui_rt::JetUiNodeKind::Box => "Box",
            ui_rt::JetUiNodeKind::Button => "Button",
            ui_rt::JetUiNodeKind::TextInput => "TextInput",
        },
        Vec::new(),
    );
    let style = node.style.map(|style| {
        let color = |color: Option<ui_rt::JetTuiColor>| -> (i64, i64) {
            match color {
                None => (0, 0),
                Some(ui_rt::JetTuiColor::ANSI16(index)) => (1, i64::from(index)),
                Some(ui_rt::JetTuiColor::ANSI256(index)) => (2, i64::from(index)),
                Some(ui_rt::JetTuiColor::RGB(red, green, blue)) =>
                    (3, (i64::from(red) << 16) | (i64::from(green) << 8) | i64::from(blue)),
            }
        };
        let (fg_kind, fg_word) = color(style.foreground);
        let (bg_kind, bg_word) = color(style.background);
        let flags = i64::from(style.bold) | (i64::from(style.dim) << 1) | (i64::from(style.underline) << 2);
        ambient_struct("tuple", vec![
            ("0".to_string(), CtValue::Int(fg_kind)),
            ("1".to_string(), CtValue::Int(fg_word)),
            ("2".to_string(), CtValue::Int(bg_kind)),
            ("3".to_string(), CtValue::Int(bg_word)),
            ("4".to_string(), CtValue::Int(flags)),
        ])
    });
    ambient_struct(
        "UINode",
        vec![
            ("label".to_string(), CtValue::Str(node.label)),
            ("width".to_string(), CtValue::Float(CtFloat::f64(node.width))),
            ("height".to_string(), CtValue::Float(CtFloat::f64(node.height))),
            (
                "role".to_string(),
                ambient_option_value("UIAriaRole", role),
            ),
            (
                "accessibility".to_string(),
                ambient_option_value(
                    "UIAccessibility",
                    node.accessibility.map(ambient_accessibility_value),
                ),
            ),
            (
                "ime".to_string(),
                ambient_option_value("UIIMEMode", node.ime.map(ambient_ime_mode_value)),
            ),
            (
                "color".to_string(),
                ambient_option_value("String", node.color.map(CtValue::Str)),
            ),
            ("kind".to_string(), kind),
            ("children".to_string(), CtValue::List(children)),
            ("style".to_string(), ambient_option_value("UI style wire", style)),
            (
                "shortcut".to_string(),
                ambient_option_value("UIShortcut", node.shortcut.map(ambient_shortcut_value)),
            ),
        ],
    )
}

fn ambient_ime_phase_value(phase: ui_rt::JetUiImePhase) -> CtValue {
    ambient_enum_value(
        "UIIMEPhase",
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
        "UIIMEComposition",
        vec![
            ("text".to_string(), CtValue::Str(value.text)),
            (
                "selection".to_string(),
                ambient_text_range_value(value.selection),
            ),
            (
                "marked".to_string(),
                ambient_option_value("UITextRange", value.marked.map(ambient_text_range_value)),
            ),
        ],
    )
}

fn ambient_ime_event_value(value: ui_rt::JetUiImeEvent) -> CtValue {
    ambient_struct(
        "UIIMEEvent",
        vec![
            ("target".to_string(), ambient_node_id_value(value.target)),
            ("phase".to_string(), ambient_ime_phase_value(value.phase)),
            (
                "composition".to_string(),
                ambient_option_value(
                    "UIIMEComposition",
                    value.composition.map(ambient_ime_composition_value),
                ),
            ),
        ],
    )
}

fn ambient_drag_phase_value(phase: ui_rt::JetUiDragPhase) -> CtValue {
    ambient_enum_value(
        "UIDragPhase",
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
        "UIDragOperation",
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
            "UIDropItem",
            "Text",
            vec![(None, CtValue::Str(value))],
        ),
        ui_rt::JetUiDropItem::URI(value) => ambient_enum_value(
            "UIDropItem",
            "URI",
            vec![(None, CtValue::Str(value))],
        ),
        ui_rt::JetUiDropItem::File(value) => ambient_enum_value(
            "UIDropItem",
            "File",
            vec![(None, ambient_granted_path_value(value))],
        ),
    }
}

fn ambient_drag_event_value(value: ui_rt::JetUiDragEvent) -> CtValue {
    ambient_struct(
        "UIDragEvent",
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
        "UIAccessibilityProjection",
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
        "UICancellation",
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
            "UIHostError",
            "Cancelled",
            vec![(None, ambient_ui_cancellation_value(reason))],
        ),
        ui_rt::JetUiHostError::CapabilityUnavailable { capability } => ambient_enum_value(
            "UIHostError",
            "CapabilityUnavailable",
            vec![(None, CtValue::Str(capability.to_string()))],
        ),
        ui_rt::JetUiHostError::CapabilityDenied { capability } => ambient_enum_value(
            "UIHostError",
            "CapabilityDenied",
            vec![(None, ambient_capability_value(capability))],
        ),
        ui_rt::JetUiHostError::InvalidRequest(message) => ambient_enum_value(
            "UIHostError",
            "InvalidRequest",
            vec![(None, CtValue::Str(message))],
        ),
        ui_rt::JetUiHostError::HostFailure { service, message } => ambient_enum_value(
            "UIHostError",
            "HostFailure",
            vec![
                (None, CtValue::Str(service.to_string())),
                (None, CtValue::Str(message)),
            ],
        ),
        ui_rt::JetUiHostError::ResourceDenied(message) => ambient_enum_value(
            "UIHostError",
            "ResourceDenied",
            vec![(None, CtValue::Str(message))],
        ),
        ui_rt::JetUiHostError::ShortcutConflict {
            shortcut,
            existing_action,
        } => ambient_enum_value(
            "UIHostError",
            "ShortcutConflict",
            vec![
                (None, ambient_shortcut_value(shortcut)),
                (None, CtValue::Str(existing_action)),
            ],
        ),
        ui_rt::JetUiHostError::QueueFull { service } => ambient_enum_value(
            "UIHostError",
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

fn ui_mir_role(role: Option<ui_rt::JetUiAriaRole>) -> MirRuntimeValue {
    role.map_or_else(
        || ui_mir_absent("UIAriaRole"),
        |role| {
            let variant = match role {
                ui_rt::JetUiAriaRole::Button => "Button",
                ui_rt::JetUiAriaRole::TextInput => "TextInput",
                ui_rt::JetUiAriaRole::Label => "Label",
                ui_rt::JetUiAriaRole::Container => "Container",
            };
            MirRuntimeValue::Present(Box::new(ui_mir_enum("UIAriaRole", variant)))
        },
    )
}
fn ui_mir_ime(ime: Option<ui_rt::JetUiImeMode>) -> MirRuntimeValue {
    ime.map_or_else(
        || ui_mir_absent("UIIMEMode"),
        |ime| {
            let variant = match ime {
                ui_rt::JetUiImeMode::Native => "Native",
                ui_rt::JetUiImeMode::Disabled => "Disabled",
            };
            MirRuntimeValue::Present(Box::new(ui_mir_enum("UIIMEMode", variant)))
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
    ui_mir_enum("UINodeKind", variant)
}

fn ui_mir_accessibility(
    accessibility: Option<ui_rt::JetUiAccessibility>,
) -> MirRuntimeValue {
    let Some(accessibility) = accessibility else {
        return ui_mir_absent("UIAccessibility");
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
                type_name: "UIAccessibilityState".to_string(),
                variant: variant.to_string(),
                args,
            }
        })
        .collect();
    MirRuntimeValue::Present(Box::new(MirRuntimeValue::Struct {
        type_name: "UIAccessibility".to_string(),
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
        .map(|child| ui_mir_node_value(child, ui_mir_absent("UIShortcut"), None))
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
        type_name: "UINode".to_string(),
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
                MirRuntimeValue::Struct { type_name, .. } if type_name == "UIShortcut"
            ) =>
        {
            Ok(MirRuntimeValue::Present(inner.clone()))
        }
        _ => Err(unsupported(
            "core.ui.button() expects an optional UIShortcut",
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
        } if type_name == "UIIMEMode" && args.is_empty() => match variant.as_str() {
            "Native" => Ok(ui_rt::JetUiImeMode::Native),
            "Disabled" => Ok(ui_rt::JetUiImeMode::Disabled),
            _ => Err(unsupported("unknown UIIMEMode variant", span)),
        },
        _ => Err(unsupported("core.ui.text_input() expects UIIMEMode", span)),
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
        type_name: "UIPreviewSource".to_string(),
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
                type_name: "UIPreview".to_string(),
                fields: vec![
                    ("kind".to_string(), ui_mir_enum("UIPreviewKind", "Preview")),
                    ("name".to_string(), MirRuntimeValue::String(name)),
                    ("viewport".to_string(), viewport.clone()),
                    ("callback".to_string(), callback),
                    (
                        "source".to_string(),
                        preview_source
                            .clone()
                            .expect("UIPreview kind carries compiler source"),
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
                type_name: "UIPreview".to_string(),
                fields: vec![
                    (
                        "kind".to_string(),
                        ui_mir_enum("UIPreviewKind", "Playground"),
                    ),
                    ("name".to_string(), MirRuntimeValue::String(name)),
                    ("viewport".to_string(), viewport.clone()),
                    ("callback".to_string(), callback),
                    (
                        "source".to_string(),
                        preview_source
                            .expect("UIPreview kind carries compiler source"),
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
                    ui_mir_absent("UIShortcut"),
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
            ambient_expect_arity(&args, 1, "UIShortcut.cmd", span)?;
            let key = ambient_string(&args[0], "UIShortcut.cmd", span)?;
            Ok(ambient_shortcut_value(ui_rt::JetUiShortcut::cmd(&key)))
        })());
    }
    match (module, method) {
        ("core.tui", "__cap_env_bytes") => Some((|| {
            ambient_expect_arity(&args, 1, "core.tui.__cap_env_bytes", span)?;
            let CtValue::Str(name) = &args[0] else {
                return Err(unsupported("core.tui.__cap_env_bytes value must be String", span));
            };
            Ok(ui_rt::jet_tui_cap_env_bytes(name).map_or_else(
                || CtValue::absent(Type::List(Box::new(Type::IntN { signed: false, bits: 8 }))),
                |bytes| CtValue::Present(Box::new(CtValue::Bytes(bytes))),
            ))
        })()),
        ("core.term", "__stdout_terminal") => Some((|| {
            ambient_expect_arity(&args, 0, "core.term.__stdout_terminal", span)?;
            Ok(CtValue::Bool(crate::IO::term_prelude::jet_term_stdout_is_terminal()))
        })()),
        ("core.ui", "__node_transfer") => Some((|| {
            let call = "core.ui.__node_transfer";
            ambient_expect_arity(&args, 15, call, span)?;
            let label = ambient_string(&args[0], call, span)?;
            let color = ambient_string(&args[7], call, span)?;
            let children = ambient_list(&args[5], call, span)?.iter()
                .map(|child| ambient_node(child, span)).collect::<Result<Vec<_>, _>>()?;
            Ok(ambient_node_value(ui_rt::jet_ui_raw_node_transfer(
                &label, ambient_float(&args[1], call, span)?, ambient_float(&args[2], call, span)?,
                ambient_int(&args[3], call, span)?.into(), ambient_int(&args[4], call, span)?.into(), children,
                ambient_bool(&args[6], call, span)?, &color, ambient_int(&args[8], call, span)?.into(),
                ambient_bool(&args[9], call, span)?, ambient_int(&args[10], call, span)?.into(),
                ambient_int(&args[11], call, span)?.into(), ambient_int(&args[12], call, span)?.into(),
                ambient_int(&args[13], call, span)?.into(), ambient_int(&args[14], call, span)?.into(),
            )))
        })()),
        ("core.ui", "text_input") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.text_input", span)?;
            let state = ambient_string(&args[0], "core.ui.text_input", span)?;
            let ime = ambient_ime_mode(&args[1], span)?;
            Ok(ambient_node_value(ui_rt::jet_ui_text_input(&state, ime)))
        })()),
        ("core.ui.host", "__capabilities") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.capabilities", span)?;
            let facts = ui_rt::jet_ui_host_capabilities();
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
        ("core.ui.host", "__open_file") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.open_file", span)?;
            let request = ambient_file_request(&args[0], span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_open_file(request),
                ambient_file_selection_value,
            ))
        })()),
        ("core.ui.host", "__save_file") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.save_file", span)?;
            let request = ambient_file_request(&args[0], span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_save_file(request),
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
        ("core.ui.host.clipboard", "__read_text") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.clipboard.read_text", span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_clipboard_read_text(),
                ambient_clipboard_text_value,
            ))
        })()),
        ("core.ui.host.clipboard", "__write_text") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.clipboard.write_text", span)?;
            let text = ambient_string(&args[0], "core.ui.host.clipboard.write_text", span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_clipboard_write_text(&text),
                ambient_clipboard_write_value,
            ))
        })()),
        ("core.ui.host.ime", "__poll") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.ime.poll", span)?;
            Ok(ambient_option_result(
                ui_rt::jet_ui_host_ime_poll(),
                "UIIMEEvent",
                ambient_ime_event_value,
            ))
        })()),
        ("core.ui.host.drag_drop", "__poll") => Some((|| {
            ambient_expect_arity(&args, 0, "core.ui.host.drag_drop.poll", span)?;
            Ok(ambient_option_result(
                ui_rt::jet_ui_host_drag_poll(),
                "UIDragEvent",
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
        ("core.ui.host.shortcuts", "__register") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.shortcuts.register", span)?;
            let binding = ambient_shortcut_binding(&args[0], span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_shortcuts_register(binding),
                ambient_shortcut_binding_value,
            ))
        })()),
        ("core.ui.host.shortcuts", "__dispatch") => Some((|| {
            ambient_expect_arity(&args, 1, "core.ui.host.shortcuts.dispatch", span)?;
            let shortcut = ambient_shortcut(&args[0], span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_shortcuts_dispatch(shortcut),
                ambient_shortcut_dispatch_value,
            ))
        })()),
        ("core.ui.host.accessibility", "__attach") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.host.accessibility.attach", span)?;
            let node = ambient_node(&args[0], span)?;
            let accessibility = ambient_accessibility(&args[1], span)?;
            Ok(ambient_service_result(
                ui_rt::jet_ui_host_attach_accessibility(node, accessibility),
                ambient_node_value,
            ))
        })()),
        ("core.ui.host.accessibility", "__project") => Some((|| {
            ambient_expect_arity(&args, 2, "core.ui.host.accessibility.project", span)?;
            let node = ambient_node(&args[0], span)?;
            let node_id = ambient_node_id(&args[1], span)?;
            Ok(ambient_option_result(
                ui_rt::jet_ui_host_project_accessibility(node, node_id),
                "UIAccessibilityProjection",
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
        ("core.font", "shape" | "__shape_text") => Some((|| {
            let text = match args.first() {
                Some(CtValue::Str(value)) => value,
                _ => return Err(unsupported("core.font.shape() expects String text", span)),
            };
            let face = ambient_font_face(
                args.get(1)
                    .ok_or_else(|| unsupported("core.font.shape() expects FontFace", span))?,
                span,
            )?;
            let run = ui_rt::jet_font_shape_raw(text, &face);
            if method == "__shape_text" {
                return Ok(match run {
                    Ok(run) => CtValue::Present(Box::new(ambient_glyph_run_value(run))),
                    Err(cause) => CtValue::failed(Box::new(CtValue::Str(cause))),
                });
            }
            Ok(ambient_glyph_run_value(run.map_err(|cause| {
                Diagnostic::runtime_host_fault(String::new(), format!("font shaping host contract failure: {cause}"))
            })?))
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

